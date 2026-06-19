//! Flash orchestration.
//!
//! The long, unprivileged work (download + cache) runs here in the app process,
//! so the admin/Touch-ID prompt only appears when the privileged write is about
//! to start. Only the write/verify/customize/eject step runs in the elevated
//! `awflash-helper`, whose NDJSON progress we tail and re-emit as Tauri events.
#![allow(dead_code, unused_imports)] // the helper path is compiled out on macOS

use awflash_core::escalation::elevated_command;
use awflash_core::flash;
use awflash_core::model::{FlashPhase, FlashProgress, FlashRequest, ImageSource, LocalImage};
use awflash_core::progress::{new_cancel, Cancel};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::SharedState;

fn helper_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("no exe dir")?;
    let name = if cfg!(windows) { "awflash-helper.exe" } else { "awflash-helper" };
    let p = dir.join(name);
    if p.exists() {
        Ok(p)
    } else {
        Err(format!("helper not found next to app at {}", p.display()))
    }
}

fn run_dir() -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("awflash-{}", std::process::id()));
    p
}

fn cache_dir() -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push("airwaves-flasher-cache");
    p
}

pub fn start(app: AppHandle, state: &SharedState, req: FlashRequest) -> Result<(), String> {
    let cancel = {
        let mut st = state.lock().map_err(|e| e.to_string())?;
        if st.active {
            return Err("a flash is already in progress".into());
        }
        st.active = true;
        let c = new_cancel();
        st.cancel_flag = Some(c.clone());
        st.cancel_file = None;
        c
    };
    thread::spawn(move || run(app, req, cancel));
    Ok(())
}

pub fn cancel(state: &SharedState) -> Result<(), String> {
    let st = state.lock().map_err(|e| e.to_string())?;
    if let Some(flag) = &st.cancel_flag {
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    if let Some(path) = &st.cancel_file {
        let _ = fs::write(path, b"1");
    }
    Ok(())
}

fn reset_state(app: &AppHandle) {
    if let Some(st) = app.try_state::<SharedState>() {
        if let Ok(mut s) = st.lock() {
            s.active = false;
            s.cancel_flag = None;
            s.cancel_file = None;
        }
    }
}

/// Emit progress to the UI, logging once per phase change.
fn emit(app: &AppHandle, last: &mut Option<FlashPhase>, p: FlashProgress) {
    if last.as_ref() != Some(&p.phase) {
        let _ = app.emit("flash://log", format!("{:?}: {}", p.phase, p.message));
        *last = Some(p.phase);
    }
    let _ = app.emit("flash://progress", p);
}

fn run(app: AppHandle, req: FlashRequest, cancel: Cancel) {
    let mut last: Option<FlashPhase> = None;
    emit(&app, &mut last, FlashProgress::new(FlashPhase::Preparing, "Preparing…"));

    // ---- 1. resolve a local image path (download remote sources here) -------
    let mut delete_after: Option<PathBuf> = None;
    let local_req = match &req.source {
        ImageSource::Local { .. } => req.clone(),
        ImageSource::Catalog { image } => {
            let persistent = req.cache_seconds > 0;
            let dir = if persistent { cache_dir() } else { run_dir() };
            let _ = fs::create_dir_all(&dir);
            if persistent {
                cleanup_cache(&dir, req.cache_seconds);
            }
            let fname = url_filename(&image.url);
            let dest = dir.join(&fname);
            let dest_str = dest.to_string_lossy().to_string();

            if persistent && is_fresh(&dest, req.cache_seconds) {
                let mut p = FlashProgress::new(
                    FlashPhase::Downloading,
                    format!("Using cached download ({fname})"),
                );
                p.fraction = 1.0;
                p.bytes_processed = image.download_size;
                p.bytes_total = image.download_size;
                emit(&app, &mut last, p);
            } else {
                let res = flash::download(
                    &image.url,
                    &dest_str,
                    image.download_size,
                    image.download_sha256.as_deref(),
                    &mut |p| emit(&app, &mut last, p),
                    &cancel,
                );
                if let Err(e) = res {
                    let _ = fs::remove_file(&dest); // drop any partial file
                    finish_error(&app, &mut last, e);
                    cleanup(None);
                    reset_state(&app);
                    return;
                }
                if !persistent {
                    delete_after = Some(dest.clone());
                }
            }

            let file = LocalImage {
                path: dest_str,
                name: fname,
                size: image.download_size,
                compression: image.compression,
                extract_size: Some(image.extract_size),
            };
            FlashRequest {
                source: ImageSource::Local { file },
                ..req.clone()
            }
        }
    };

    // ---- 2a. macOS: write in-process via authopen (single prompt, no root) --
    #[cfg(target_os = "macos")]
    {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            emit(&app, &mut last, FlashProgress::new(FlashPhase::Cancelled, "Cancelled"));
        } else {
            match flash::run_flash(&local_req, &mut |p| emit(&app, &mut last, p), &cancel) {
                Ok(()) => {} // run_flash emits the terminal Done itself
                Err(awflash_core::FlashError::Cancelled) => {
                    emit(&app, &mut last, FlashProgress::new(FlashPhase::Cancelled, "Cancelled"));
                }
                Err(e) => finish_error(&app, &mut last, e),
            }
        }
        cleanup(delete_after);
        reset_state(&app);
        return;
    }

    // ---- 2b. Linux/Windows: elevate + write in the helper -------------------
    #[cfg(not(target_os = "macos"))]
    {
    let helper = match helper_path() {
        Ok(h) => h,
        Err(e) => {
            finish_error_msg(&app, &mut last, &e);
            cleanup(delete_after);
            reset_state(&app);
            return;
        }
    };

    let rdir = run_dir();
    let _ = fs::create_dir_all(&rdir);
    let req_path = rdir.join("request.json");
    let progress_path = rdir.join("progress.ndjson");
    let cancel_path = rdir.join("cancel");
    let _ = fs::write(&req_path, serde_json::to_vec(&local_req).unwrap_or_default());
    let _ = fs::write(&progress_path, b"");

    if let Some(st) = app.try_state::<SharedState>() {
        if let Ok(mut s) = st.lock() {
            s.cancel_file = Some(cancel_path.to_string_lossy().to_string());
        }
    }
    // if the user already cancelled during download, don't launch the writer
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        emit(&app, &mut last, FlashProgress::new(FlashPhase::Cancelled, "Cancelled"));
        cleanup(delete_after);
        let _ = fs::remove_dir_all(&rdir);
        reset_state(&app);
        return;
    }

    let args = vec![
        "--request".into(),
        req_path.to_string_lossy().to_string(),
        "--progress-file".into(),
        progress_path.to_string_lossy().to_string(),
        "--cancel-file".into(),
        cancel_path.to_string_lossy().to_string(),
    ];
    let mut cmd = elevated_command(&helper.to_string_lossy(), &args);
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            finish_error_msg(&app, &mut last, &format!("failed to launch writer: {e}"));
            cleanup(delete_after);
            let _ = fs::remove_dir_all(&rdir);
            reset_state(&app);
            return;
        }
    };

    let progress_str = progress_path.to_string_lossy().to_string();
    let mut emitted = 0usize;
    let mut saw_terminal = false;
    loop {
        drain(&progress_str, &mut emitted, &app, &mut last, &mut saw_terminal);
        match child.try_wait() {
            Ok(Some(status)) => {
                drain(&progress_str, &mut emitted, &app, &mut last, &mut saw_terminal);
                if !saw_terminal {
                    let (phase, msg) = if status.success() {
                        (FlashPhase::Done, "Flash finished.".to_string())
                    } else {
                        (
                            FlashPhase::Error,
                            "The writer exited unexpectedly (the elevation prompt may have been cancelled)."
                                .to_string(),
                        )
                    };
                    let mut p = FlashProgress::new(phase, msg);
                    if status.success() {
                        p.fraction = 1.0;
                    }
                    emit(&app, &mut last, p);
                }
                break;
            }
            Ok(None) => {}
            Err(_) => break,
        }
        thread::sleep(Duration::from_millis(120));
    }

    cleanup(delete_after);
    let _ = fs::remove_dir_all(&rdir);
    reset_state(&app);
    } // end cfg(not macos)
}

fn finish_error(app: &AppHandle, last: &mut Option<FlashPhase>, e: awflash_core::FlashError) {
    let mut p = FlashProgress::new(FlashPhase::Error, format!("[{}] {}", e.kind(), e));
    p.fraction = 0.0;
    emit(app, last, p);
}

fn finish_error_msg(app: &AppHandle, last: &mut Option<FlashPhase>, msg: &str) {
    emit(app, last, FlashProgress::new(FlashPhase::Error, msg.to_string()));
}

fn cleanup(delete_after: Option<PathBuf>) {
    if let Some(p) = delete_after {
        let _ = fs::remove_file(p);
    }
}

fn drain(
    progress_path: &str,
    emitted: &mut usize,
    app: &AppHandle,
    last: &mut Option<FlashPhase>,
    saw_terminal: &mut bool,
) {
    let Ok(mut f) = fs::File::open(progress_path) else {
        return;
    };
    let _ = f.seek(SeekFrom::Start(0));
    let mut content = String::new();
    if f.read_to_string(&mut content).is_err() {
        return;
    }
    let parts: Vec<&str> = content.split('\n').collect();
    let complete = if parts.is_empty() { 0 } else { parts.len() - 1 };
    for line in parts.iter().take(complete).skip(*emitted) {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(p) = serde_json::from_str::<FlashProgress>(line) {
            if matches!(
                p.phase,
                FlashPhase::Done | FlashPhase::Error | FlashPhase::Cancelled
            ) {
                *saw_terminal = true;
            }
            emit(app, last, p);
        }
    }
    *emitted = complete;
}

fn url_filename(url: &str) -> String {
    let no_query = url.split(['?', '#']).next().unwrap_or(url);
    let name = no_query.rsplit('/').next().unwrap_or("");
    if name.is_empty() {
        "airwaves-os.img".to_string()
    } else {
        name.to_string()
    }
}

fn is_fresh(path: &Path, max_age_secs: u64) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() || meta.len() == 0 {
        return false;
    }
    match meta.modified().ok().and_then(|m| m.elapsed().ok()) {
        Some(age) => age.as_secs() <= max_age_secs,
        None => false,
    }
}

fn cleanup_cache(dir: &Path, max_age_secs: u64) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        if let Ok(meta) = entry.metadata() {
            if let Ok(modified) = meta.modified() {
                if now
                    .duration_since(modified)
                    .map(|d| d.as_secs() > max_age_secs)
                    .unwrap_or(false)
                {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }
}
