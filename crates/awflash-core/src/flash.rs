//! The end-to-end flash pipeline, executed inside the elevated helper:
//! acquire (download/open) → decompress-on-the-fly → write raw device →
//! flush → read-back verify → write AWCFG config → eject.

use crate::decompress::{open_stream, ImageReader};
use crate::device;
use crate::error::{FlashError, Result};
use crate::model::{Compression, FlashPhase, FlashProgress, FlashRequest, ImageSource};
use crate::progress::{is_cancelled, Cancel, CountingReader};
use crate::provision;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::PathBuf;
use std::collections::VecDeque;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::{Duration, Instant};

const BUF: usize = 4 * 1024 * 1024;
/// Raw character devices (macOS /dev/rdiskN) require block-aligned I/O. 4096
/// covers both 512- and 4096-byte sector devices. BUF is a multiple of it.
const ALIGN: usize = 4096;

/// Deletes its path on drop (temp downloads / extracted zip entries).
struct TempGuard(PathBuf);
impl Drop for TempGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

struct Acquired {
    path: String,
    compression: Compression,
    extract_size: u64,
    compressed_size: u64,
    catalog_sha: Option<String>,
    _temps: Vec<TempGuard>,
}

pub type Emit<'a> = dyn FnMut(FlashProgress) + 'a;

/// Run the full pipeline. `emit` receives throttled progress; `cancel` aborts.
pub fn run_flash(req: &FlashRequest, emit: &mut Emit, cancel: &Cancel) -> Result<()> {
    emit(FlashProgress::new(FlashPhase::Preparing, "Preparing…"));
    device::assert_safe_target(&req.device)?;
    device::unmount(&req.device)?;
    check_cancel(cancel)?;

    let acq = acquire_source(&req.source, emit, cancel)?;
    check_cancel(cancel)?;

    // open compressed source, count compressed bytes, decompress on the fly
    let compressed_counter = Arc::new(AtomicU64::new(0));
    let file = File::open(&acq.path).map_err(|e| FlashError::Image(e.to_string()))?;
    let raw_reader: ImageReader = if acq.compression == Compression::None {
        Box::new(CountingReader::new(file, compressed_counter.clone()))
    } else {
        let counted = CountingReader::new(file, compressed_counter.clone());
        open_stream(Box::new(counted), acq.compression)
            .map_err(|e| FlashError::Image(e.to_string()))?
    };

    // Acquire the device (single auth prompt on macOS) and unmount it.
    let wpath = device::write_path(&req.device);
    let session = device::DeviceSession::acquire(&req.device)?;
    device::unmount(&req.device)?;
    emit(FlashProgress::new(FlashPhase::Preparing, format!("Writing to {wpath}")));

    // ---- WRITE (own fd, then close) ----------------------------------------
    let (src_hash, written) = {
        let mut out = session.open()?;
        let r = write_loop(
            raw_reader,
            &mut out,
            acq.extract_size,
            acq.compressed_size,
            &compressed_counter,
            emit,
            cancel,
        )?;
        // Flush. macOS /dev/rdiskN is uncached and rejects sync_all's F_FULLFSYNC
        // with ENOTTY — harmless, the data is already on the device.
        #[cfg(target_os = "macos")]
        {
            let _ = out.sync_data();
        }
        #[cfg(not(target_os = "macos"))]
        {
            out.sync_all().map_err(|e| FlashError::Io(e.to_string()))?;
        }
        r
    };
    check_cancel(cancel)?;

    // macOS re-probes/auto-mounts the freshly-written volumes here; unmount so we
    // can reopen the raw device cleanly for read-back and provisioning.
    device::unmount(&req.device)?;

    // ---- VERIFY + PROVISION (fresh fd) -------------------------------------
    let mut config_warning: Option<String> = None;
    {
        let mut dev = session.open()?;
        device::drop_cache(&dev);

        if req.verify {
            verify_read(&mut dev, written, &src_hash, acq.catalog_sha.as_deref(), emit, cancel)?;
        }

        if req.customization.enabled {
            emit(FlashProgress::new(
                FlashPhase::Configuring,
                "Writing first-boot configuration…",
            ));
            match provision::add_awcfg(&mut dev, req.device.size_bytes, &req.customization) {
                Ok(true) => {}
                Ok(false) => {
                    let msg = "First-boot settings could not be written — couldn't add a config \
                        (AWCFG) partition to this card. You can still configure the device from \
                        its web dashboard on first boot."
                        .to_string();
                    config_warning = Some(msg.clone());
                    let mut p = FlashProgress::new(FlashPhase::Configuring, msg);
                    p.warning = config_warning.clone();
                    emit(p);
                }
                Err(e) => return Err(e),
            }
            #[cfg(target_os = "macos")]
            {
                let _ = dev.sync_data();
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = dev.sync_all();
            }
        }
    }

    // finalize
    emit(FlashProgress::new(FlashPhase::Finalizing, "Flushing and ejecting…"));
    if req.eject {
        device::eject(&req.device);
    }

    let mut done = FlashProgress::new(
        FlashPhase::Done,
        "Airwaves OS is ready. Insert the card and power on.",
    );
    done.fraction = 1.0;
    done.bytes_processed = written;
    done.bytes_total = written;
    done.warning = config_warning;
    emit(done);
    Ok(())
}

fn check_cancel(cancel: &Cancel) -> Result<()> {
    if is_cancelled(cancel) {
        Err(FlashError::Cancelled)
    } else {
        Ok(())
    }
}

fn acquire_source(source: &ImageSource, emit: &mut Emit, cancel: &Cancel) -> Result<Acquired> {
    match source {
        ImageSource::Local { file } => {
            let mut acq = Acquired {
                path: file.path.clone(),
                compression: file.compression,
                extract_size: file.extract_size.unwrap_or(0),
                compressed_size: file.size,
                catalog_sha: None,
                _temps: vec![],
            };
            if acq.compression == Compression::Zip {
                let (p, size, guard) = extract_zip(&file.path)?;
                acq.path = p;
                acq.compression = Compression::None;
                acq.extract_size = size;
                acq._temps.push(guard);
            }
            Ok(acq)
        }
        ImageSource::Catalog { image } => {
            let tmp = temp_path("airwaves-dl");
            download(&image.url, &tmp, image.download_size, image.download_sha256.as_deref(), emit, cancel)?;
            let guard = TempGuard(PathBuf::from(&tmp));
            let mut acq = Acquired {
                path: tmp.clone(),
                compression: image.compression,
                extract_size: image.extract_size,
                compressed_size: image.download_size,
                catalog_sha: image.sha256.clone(),
                _temps: vec![guard],
            };
            if acq.compression == Compression::Zip {
                let (p, size, g2) = extract_zip(&acq.path)?;
                acq.path = p;
                acq.compression = Compression::None;
                acq.extract_size = size;
                acq._temps.push(g2);
            }
            Ok(acq)
        }
    }
}

/// Download `url` to `dest`, emitting Downloading progress and verifying
/// `expected_sha` (of the compressed file) when provided. Used by the helper
/// for direct runs and by the GUI's unprivileged pre-download step.
pub fn download(
    url: &str,
    dest: &str,
    expected_total: u64,
    expected_sha: Option<&str>,
    emit: &mut Emit,
    cancel: &Cancel,
) -> Result<()> {
    emit(FlashProgress::new(FlashPhase::Downloading, "Downloading image…"));
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("airwaves-os-flasher/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(FlashError::from)?;
    let mut resp = client.get(url).send().map_err(FlashError::from)?;
    if !resp.status().is_success() {
        return Err(FlashError::Download(format!("HTTP {}", resp.status())));
    }
    let total = resp.content_length().unwrap_or(expected_total);
    let mut file = File::create(dest).map_err(|e| FlashError::Download(e.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 256];
    let mut got: u64 = 0;
    let mut rate = Rate::new();
    let phase_start = Instant::now();
    let mut last = Instant::now();
    loop {
        check_cancel(cancel)?;
        let n = resp.read(&mut buf).map_err(|e| FlashError::Download(e.to_string()))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| FlashError::Download(e.to_string()))?;
        if expected_sha.is_some() {
            hasher.update(&buf[..n]);
        }
        got += n as u64;
        if last.elapsed().as_millis() >= 1000 {
            last = Instant::now();
            let bps = rate.update(got);
            let pending = phase_start.elapsed().as_secs() < WARMUP_SECS;
            emit(progress(FlashPhase::Downloading, "Downloading image…", got, total, bps, pending));
        }
    }
    file.sync_all().ok();

    if let Some(want) = expected_sha {
        let have = to_hex(&hasher.finalize());
        if !want.eq_ignore_ascii_case(&have) {
            return Err(FlashError::Download(
                "downloaded image checksum mismatch (corrupt download)".into(),
            ));
        }
    }
    Ok(())
}

fn write_loop(
    mut reader: ImageReader,
    out: &mut File,
    extract_size: u64,
    compressed_size: u64,
    compressed_counter: &Arc<AtomicU64>,
    emit: &mut Emit,
    cancel: &Cancel,
) -> Result<(String, u64)> {
    emit(FlashProgress::new(FlashPhase::Writing, "Writing image to device…"));
    let mut buf = vec![0u8; BUF];
    let mut hasher = Sha256::new();
    let mut written: u64 = 0;
    let mut rate = Rate::new();
    let phase_start = Instant::now();
    let mut last = Instant::now();

    loop {
        check_cancel(cancel)?;
        // fill the whole buffer before writing so every write is block-aligned
        // (the decompressor returns arbitrary-sized reads).
        let mut fill = 0usize;
        while fill < BUF {
            match reader.read(&mut buf[fill..]) {
                Ok(0) => break,
                Ok(n) => fill += n,
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(FlashError::Image(e.to_string())),
            }
        }
        if fill == 0 {
            break;
        }
        // write a full (aligned) buffer, or zero-pad the final short read up to a
        // sector boundary — the device is larger than the image so trailing zeros
        // are harmless and keep raw-device writes aligned.
        let to_write = if fill == BUF {
            BUF
        } else {
            let padded = (fill.div_ceil(ALIGN) * ALIGN).min(BUF);
            buf[fill..padded].fill(0);
            padded
        };
        out.write_all(&buf[..to_write]).map_err(|e| FlashError::Io(e.to_string()))?;
        hasher.update(&buf[..fill]); // hash only the real image bytes
        written += fill as u64;

        if last.elapsed().as_millis() >= 1000 {
            last = Instant::now();
            let (proc, total) = if extract_size > 0 {
                (written, extract_size)
            } else {
                (
                    compressed_counter.load(std::sync::atomic::Ordering::Relaxed),
                    compressed_size,
                )
            };
            let bps = rate.update(proc);
            let pending = phase_start.elapsed().as_secs() < WARMUP_SECS;
            emit(progress(FlashPhase::Writing, "Writing image to device…", proc, total, bps, pending));
        }

        if fill < BUF {
            break; // reached EOF on this fill
        }
    }
    Ok((to_hex(&hasher.finalize()), written))
}

fn verify_read(
    dev: &mut File,
    written: u64,
    src_hash: &str,
    catalog_sha: Option<&str>,
    emit: &mut Emit,
    cancel: &Cancel,
) -> Result<()> {
    emit(FlashProgress::new(FlashPhase::Verifying, "Verifying written data…"));
    dev.seek(std::io::SeekFrom::Start(0))
        .map_err(|e| FlashError::Io(e.to_string()))?;

    // catalog hash sanity check first (cheap, decisive)
    if let Some(want) = catalog_sha {
        if !want.eq_ignore_ascii_case(src_hash) {
            return Err(FlashError::VerifyMismatch);
        }
    }

    let align = ALIGN as u64;
    let aligned_total = written.div_ceil(align) * align;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; BUF];
    let mut done: u64 = 0;
    let mut rate = Rate::new();
    let phase_start = Instant::now();
    let mut last = Instant::now();

    while done < aligned_total {
        check_cancel(cancel)?;
        // read in block-aligned chunks (required for raw devices)
        let want = std::cmp::min(BUF as u64, aligned_total - done) as usize;
        let n = read_full(dev, &mut buf[..want]).map_err(|e| FlashError::Io(e.to_string()))?;
        if n == 0 {
            break;
        }
        let real = std::cmp::min(n as u64, written - done) as usize;
        hasher.update(&buf[..real]);
        done += n as u64;
        if last.elapsed().as_millis() >= 1000 {
            last = Instant::now();
            let shown = done.min(written);
            let bps = rate.update(shown);
            let pending = phase_start.elapsed().as_secs() < WARMUP_SECS;
            emit(progress(FlashPhase::Verifying, "Verifying written data…", shown, written, bps, pending));
        }
    }

    let readback = to_hex(&hasher.finalize());
    if readback != src_hash {
        return Err(FlashError::VerifyMismatch);
    }
    Ok(())
}

fn extract_zip(zip_path: &str) -> Result<(String, u64, TempGuard)> {
    let file = File::open(zip_path).map_err(|e| FlashError::Image(e.to_string()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| FlashError::Image(format!("zip: {e}")))?;
    // pick the largest .img/.iso entry, else the largest entry
    let mut best: Option<(usize, u64)> = None;
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| FlashError::Image(format!("zip: {e}")))?;
        let name = entry.name().to_ascii_lowercase();
        let size = entry.size();
        let preferred = name.ends_with(".img") || name.ends_with(".iso");
        let score = if preferred { size + (1 << 60) } else { size };
        if best.map(|(_, b)| score > b).unwrap_or(true) {
            best = Some((i, score));
        }
    }
    let idx = best.map(|(i, _)| i).ok_or_else(|| FlashError::Image("empty zip".into()))?;
    let mut entry = archive.by_index(idx).map_err(|e| FlashError::Image(format!("zip: {e}")))?;
    let uncompressed = entry.size();
    let dest = temp_path("airwaves-img");
    let mut out = File::create(&dest).map_err(|e| FlashError::Image(e.to_string()))?;
    std::io::copy(&mut entry, &mut out).map_err(|e| FlashError::Image(e.to_string()))?;
    out.sync_all().ok();
    Ok((dest.clone(), uncompressed, TempGuard(PathBuf::from(dest))))
}

/// Trailing-window throughput meter. Decompress→write is bursty (a buffer fills,
/// then flushes), so an instantaneous rate swings wildly. Averaging over the
/// last few seconds gives a steady, believable number — and a stable ETA.
const RATE_WINDOW: Duration = Duration::from_secs(5);

struct Rate {
    samples: VecDeque<(Instant, u64)>,
}
impl Rate {
    fn new() -> Self {
        Rate { samples: VecDeque::new() }
    }
    fn update(&mut self, bytes: u64) -> f64 {
        let now = Instant::now();
        self.samples.push_back((now, bytes));
        // keep ≥2 samples, drop anything older than the window
        while self.samples.len() > 2 && now.duration_since(self.samples[0].0) > RATE_WINDOW {
            self.samples.pop_front();
        }
        let (t0, b0) = self.samples[0];
        let dt = now.duration_since(t0).as_secs_f64();
        if dt < 0.001 {
            return 0.0;
        }
        bytes.saturating_sub(b0) as f64 / dt
    }
}

/// Throughput is too noisy in the first stretch of a phase to extrapolate an
/// ETA from — hold it "Pending" until things settle.
const WARMUP_SECS: u64 = 45;

fn progress(
    phase: FlashPhase,
    msg: &str,
    processed: u64,
    total: u64,
    speed_bps: f64,
    pending: bool,
) -> FlashProgress {
    let fraction = if total > 0 {
        (processed as f64 / total as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let eta = if !pending && speed_bps > 0.0 && total > processed {
        (total - processed) as f64 / speed_bps
    } else {
        0.0
    };
    FlashProgress {
        phase,
        fraction,
        bytes_processed: processed,
        bytes_total: total,
        speed_bps,
        eta_secs: eta,
        eta_pending: pending,
        warning: None,
        message: msg.to_string(),
    }
}

fn read_full<R: Read>(r: &mut R, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match r.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

fn temp_path(prefix: &str) -> String {
    let mut dir = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    dir.push(format!("{prefix}-{pid}-{nanos}.tmp"));
    dir.to_string_lossy().to_string()
}

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
