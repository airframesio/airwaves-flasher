//! awflash-helper — the ELEVATED one-shot writer.
//!
//! Usage:
//!   awflash-helper --request <req.json> --progress-file <p.ndjson>
//!                  [--cancel-file <c>] [--dry-run]
//!
//! Reads a `FlashRequest` JSON, runs the write/verify/customize pipeline, and
//! appends one `FlashProgress` JSON object per line to the progress file. Exits
//! 0 on success, non-zero on error (the final line carries phase="error").

use awflash_core::flash;
use awflash_core::model::{FlashPhase, FlashProgress, FlashRequest};
use awflash_core::progress::{new_cancel, Cancel};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::process::exit;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

struct Args {
    request: String,
    progress_file: String,
    cancel_file: Option<String>,
    dry_run: bool,
}

fn parse_args() -> Args {
    let mut request = None;
    let mut progress_file = None;
    let mut cancel_file = None;
    let mut dry_run = false;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--request" => request = it.next(),
            "--progress-file" => progress_file = it.next(),
            "--cancel-file" => cancel_file = it.next(),
            "--dry-run" => dry_run = true,
            other => eprintln!("awflash-helper: ignoring unknown arg {other}"),
        }
    }
    Args {
        request: request.unwrap_or_else(|| die("missing --request")),
        progress_file: progress_file.unwrap_or_else(|| die("missing --progress-file")),
        cancel_file,
        dry_run,
    }
}

fn die(msg: &str) -> ! {
    eprintln!("awflash-helper: {msg}");
    exit(2);
}

fn append_line(path: &str, p: &FlashProgress) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        if let Ok(line) = serde_json::to_string(p) {
            let _ = writeln!(f, "{line}");
            let _ = f.flush();
        }
    }
}

fn main() {
    let args = parse_args();
    let _ = args.dry_run; // dry-run is enforced by passing a regular-file device path

    let req: FlashRequest = match fs::read_to_string(&args.request) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(r) => r,
            Err(e) => {
                append_line(
                    &args.progress_file,
                    &FlashProgress::new(FlashPhase::Error, format!("invalid request JSON: {e}")),
                );
                exit(2);
            }
        },
        Err(e) => {
            append_line(
                &args.progress_file,
                &FlashProgress::new(FlashPhase::Error, format!("cannot read request: {e}")),
            );
            exit(2);
        }
    };

    // cancellation: poll for the existence of the cancel file
    let cancel: Cancel = new_cancel();
    if let Some(cf) = args.cancel_file.clone() {
        let flag = cancel.clone();
        thread::spawn(move || loop {
            if std::path::Path::new(&cf).exists() {
                flag.store(true, Ordering::Relaxed);
                break;
            }
            thread::sleep(Duration::from_millis(200));
        });
    }

    // diagnostic: record the privilege level we actually launched with
    let uid = awflash_core::device::current_euid()
        .map(|u| u.to_string())
        .unwrap_or_else(|| "n/a".into());
    append_line(
        &args.progress_file,
        &FlashProgress::new(FlashPhase::Preparing, format!("Writer ready (uid {uid})")),
    );

    let progress_path = args.progress_file.clone();
    let mut emit = move |p: FlashProgress| append_line(&progress_path, &p);

    match flash::run_flash(&req, &mut emit, &cancel) {
        Ok(()) => exit(0),
        Err(e) => {
            let mut p = FlashProgress::new(FlashPhase::Error, e.to_string());
            p.message = format!("[{}] {}", e.kind(), e);
            append_line(&args.progress_file, &p);
            exit(1);
        }
    }
}
