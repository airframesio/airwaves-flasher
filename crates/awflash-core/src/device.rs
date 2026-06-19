//! Per-OS raw block-device access: unmount, open for writing, reopen for
//! cache-bypassing verification, and eject. The elevated helper is the only
//! caller. Regular-file targets are permitted for `--dry-run` testing.

use crate::error::{FlashError, Result};
use crate::model::DriveInfo;
use std::fs::{File, OpenOptions};
use std::path::Path;

/// Effective uid of this process (Some on Unix, None on Windows). 0 == root.
pub fn current_euid() -> Option<u32> {
    #[cfg(unix)]
    {
        Some(unsafe { libc::geteuid() })
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// The path we actually open for writing. On macOS this MUST be the raw
/// character device (/dev/rdiskN): the buffered /dev/diskN is ~100x slower.
pub fn write_path(drive: &DriveInfo) -> String {
    #[cfg(target_os = "macos")]
    {
        if let Some(raw) = drive.raw_device.as_deref() {
            if raw.starts_with("/dev/rdisk") {
                return raw.to_string();
            }
        }
        // derive the raw node even if the field is missing/stale
        if let Some(rest) = drive.device.strip_prefix("/dev/disk") {
            return format!("/dev/rdisk{rest}");
        }
        drive.device.clone()
    }
    #[cfg(not(target_os = "macos"))]
    {
        drive.device.clone()
    }
}

pub fn is_regular_file(path: &str) -> bool {
    std::fs::metadata(path).map(|m| m.is_file()).unwrap_or(false)
}

/// Unmount every volume on the target so the OS won't fight the write.
pub fn unmount(drive: &DriveInfo) -> Result<()> {
    if is_regular_file(&drive.device) {
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("diskutil")
            .args(["unmountDisk", "force", &drive.device])
            .output();
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        // unmount each mounted partition of this disk
        if let Ok(out) = std::process::Command::new("lsblk")
            .args(["-ln", "-o", "PATH,MOUNTPOINT", &drive.device])
            .output()
        {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                let mut it = line.split_whitespace();
                let path = it.next().unwrap_or("");
                let mp = it.next().unwrap_or("");
                if !mp.is_empty() {
                    let _ = std::process::Command::new("umount").arg(path).output();
                }
            }
        }
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        let _ = drive;
        // Volumes are locked/dismounted at open time (see open_for_write).
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = drive;
        Ok(())
    }
}

/// A device-access session. macOS re-probes/auto-mounts a disk right after we
/// write a partition table, which invalidates a held fd — so we open the device
/// fresh for each phase (write, verify, provision). To avoid re-prompting, the
/// macOS authorization is obtained ONCE here and reused for every open.
pub struct DeviceSession {
    /// path we open for I/O (raw /dev/rdiskN on macOS)
    pub write_path: String,
    /// whole-disk device, for unmount/eject
    pub device: String,
    is_file: bool,
    #[cfg(target_os = "macos")]
    auth: Option<[u8; 32]>,
}

impl DeviceSession {
    /// Acquire the session. On macOS this shows the single Touch-ID/password
    /// prompt (unless the target is a regular file, i.e. a dry-run).
    pub fn acquire(drive: &DriveInfo) -> Result<Self> {
        let write_path = write_path(drive);
        let is_file = is_regular_file(&drive.device);
        #[cfg(target_os = "macos")]
        let auth = if is_file {
            None
        } else {
            Some(macos_authopen::preauthorize(&write_path)?)
        };
        Ok(DeviceSession {
            write_path,
            device: drive.device.clone(),
            is_file,
            #[cfg(target_os = "macos")]
            auth,
        })
    }

    /// Open the device read+write. Retries briefly to ride out the macOS
    /// re-probe that follows writing a partition table (ENXIO/EBUSY).
    pub fn open(&self) -> Result<File> {
        let mut last = FlashError::Io("open failed".into());
        for attempt in 0..6 {
            // macOS auto-mounts freshly-written volumes; unmount before each try
            // so we can claim the whole raw disk.
            #[cfg(target_os = "macos")]
            if !self.is_file {
                let _ = std::process::Command::new("diskutil")
                    .args(["unmountDisk", "force", &self.device])
                    .output();
            }
            match self.open_once() {
                Ok(f) => return Ok(f),
                Err(e) => {
                    last = e;
                    std::thread::sleep(std::time::Duration::from_millis(250 * (attempt + 1)));
                }
            }
        }
        Err(last)
    }

    fn open_once(&self) -> Result<File> {
        #[cfg(target_os = "macos")]
        {
            if let Some(form) = &self.auth {
                return macos_authopen::authopen_with(&self.write_path, form);
            }
        }
        let _ = self.is_file;
        open_rw(&self.write_path, true)
    }
}

/// Hint the kernel to drop cached pages so a read-back actually hits the device
/// (Linux). macOS /dev/rdiskN is uncached; Windows uses write-through.
pub fn drop_cache(f: &File) {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::io::AsRawFd;
        unsafe {
            libc::posix_fadvise(f.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = f;
    }
}

#[cfg(target_os = "macos")]
mod macos_authopen {
    use super::{FlashError, File, Result};
    use nix::sys::socket::{
        recvmsg, socketpair, AddressFamily, ControlMessageOwned, MsgFlags, SockFlag, SockType,
    };
    use std::ffi::{c_char, c_void, CString};
    use std::io::{IoSliceMut, Write};
    use std::os::fd::{AsRawFd, FromRawFd, RawFd};
    use std::process::{Command, Stdio};

    // --- Security.framework Authorization Services ---------------------------
    #[repr(C)]
    struct AuthItem {
        name: *const c_char,
        value_length: usize,
        value: *mut c_void,
        flags: u32,
    }
    #[repr(C)]
    struct AuthRights {
        count: u32,
        items: *const AuthItem,
    }
    #[repr(C)]
    struct AuthExternalForm {
        bytes: [c_char; 32],
    }
    #[link(name = "Security", kind = "framework")]
    extern "C" {
        fn AuthorizationCreate(
            rights: *const AuthRights,
            environment: *const c_void,
            flags: u32,
            auth: *mut *mut c_void,
        ) -> i32;
        fn AuthorizationCopyRights(
            auth: *mut c_void,
            rights: *const AuthRights,
            environment: *const c_void,
            flags: u32,
            authorized: *mut *mut AuthRights,
        ) -> i32;
        fn AuthorizationMakeExternalForm(auth: *mut c_void, ext: *mut AuthExternalForm) -> i32;
        fn AuthorizationFree(auth: *mut c_void, flags: u32) -> i32;
    }
    const FLAG_INTERACTION_ALLOWED: u32 = 1 << 0;
    const FLAG_EXTEND_RIGHTS: u32 = 1 << 1;
    const FLAG_PREAUTHORIZE: u32 = 1 << 4;
    const ERR_CANCELED: i32 = -60006;

    /// Request the exact right `authopen` will need, FROM THIS (foreground) app,
    /// so the SecurityAgent prompt is associated with us and gets keyboard focus.
    /// Returns the externalized authorization to hand to `authopen -extauth`.
    pub fn preauthorize(path: &str) -> Result<[u8; 32]> {
        let canonical = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string());
        // authopen's right for an O_RDWR open: "sys.openfile.readwrite.<realpath>"
        let right = CString::new(format!("sys.openfile.readwrite.{canonical}"))
            .map_err(|_| FlashError::Other("bad device path".into()))?;

        unsafe {
            let mut auth: *mut c_void = std::ptr::null_mut();
            if AuthorizationCreate(std::ptr::null(), std::ptr::null(), 0, &mut auth) != 0 {
                return Err(FlashError::Other("AuthorizationCreate failed".into()));
            }
            let item = AuthItem {
                name: right.as_ptr(),
                value_length: 0,
                value: std::ptr::null_mut(),
                flags: 0,
            };
            let rights = AuthRights { count: 1, items: &item };
            let st = AuthorizationCopyRights(
                auth,
                &rights,
                std::ptr::null(),
                FLAG_INTERACTION_ALLOWED | FLAG_EXTEND_RIGHTS | FLAG_PREAUTHORIZE,
                std::ptr::null_mut(),
            );
            if st != 0 {
                AuthorizationFree(auth, 0);
                return Err(if st == ERR_CANCELED {
                    FlashError::Cancelled
                } else {
                    FlashError::PermissionDenied
                });
            }
            let mut ext = AuthExternalForm { bytes: [0; 32] };
            let mk = AuthorizationMakeExternalForm(auth, &mut ext);
            AuthorizationFree(auth, 0); // releases our local ref; external form stays valid
            if mk != 0 {
                return Err(FlashError::Other("AuthorizationMakeExternalForm failed".into()));
            }
            let mut out = [0u8; 32];
            for (o, b) in out.iter_mut().zip(ext.bytes.iter()) {
                *o = *b as u8;
            }
            Ok(out)
        }
    }

    /// Open `path` O_RDWR with admin rights and receive the fd over a socketpair,
    /// using a previously-obtained authorization token (no prompt).
    pub fn authopen_with(path: &str, ext_form: &[u8; 32]) -> Result<File> {
        let (parent, child) =
            socketpair(AddressFamily::Unix, SockType::Stream, None, SockFlag::empty())
                .map_err(|e| FlashError::Other(format!("socketpair: {e}")))?;

        let mut proc = Command::new("/usr/libexec/authopen")
            .arg("-stdoutpipe")
            .arg("-extauth")
            .arg("-o")
            .arg(libc::O_RDWR.to_string())
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::from(child)) // authopen sends the fd over this socket
            .spawn()
            .map_err(|e| FlashError::Other(format!("authopen: {e}")))?;

        // hand the pre-authorized token to authopen
        if let Some(mut sin) = proc.stdin.take() {
            let _ = sin.write_all(ext_form);
            let _ = sin.flush();
            // sin dropped here -> stdin closed
        }

        let mut buf = [0u8; 16];
        let mut iov = [IoSliceMut::new(&mut buf)];
        let mut cmsg = nix::cmsg_space!([RawFd; 1]);
        let mut received: Option<RawFd> = None;
        if let Ok(msg) = recvmsg::<()>(
            parent.as_raw_fd(),
            &mut iov,
            Some(&mut cmsg),
            MsgFlags::empty(),
        ) {
            if let Ok(cmsgs) = msg.cmsgs() {
                for c in cmsgs {
                    if let ControlMessageOwned::ScmRights(fds) = c {
                        if let Some(&fd) = fds.first() {
                            received = Some(fd);
                        }
                    }
                }
            }
        }

        let status = proc.wait().ok();
        match received {
            Some(fd) => Ok(unsafe { File::from_raw_fd(fd) }),
            None => {
                if matches!(status, Some(s) if !s.success()) {
                    Err(FlashError::PermissionDenied)
                } else {
                    Err(FlashError::Other(
                        "authopen did not return a device descriptor".into(),
                    ))
                }
            }
        }
    }
}

fn open_rw(path: &str, write: bool) -> Result<File> {
    let mut opts = OpenOptions::new();
    opts.read(true).write(write);

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // O_EXCL on a block device fails if it is mounted/claimed — a free lock.
        if !is_regular_file(path) {
            opts.custom_flags(libc::O_EXCL);
        }
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x1;
        const FILE_SHARE_WRITE: u32 = 0x2;
        const FILE_FLAG_WRITE_THROUGH: u32 = 0x8000_0000;
        opts.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
        opts.custom_flags(FILE_FLAG_WRITE_THROUGH);
    }

    let f = opts.open(Path::new(path)).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => {
            // If we are already root, this isn't an elevation problem — on macOS
            // it's usually the OS blocking raw-disk access (TCC / Full Disk Access).
            if current_euid() == Some(0) {
                FlashError::Other(format!(
                    "the system blocked access to {path}. On macOS, grant the app \
                     Full Disk Access in System Settings › Privacy & Security, then retry."
                ))
            } else {
                FlashError::PermissionDenied
            }
        }
        std::io::ErrorKind::NotFound => FlashError::DeviceRemoved,
        _ if e.raw_os_error() == Some(16) => FlashError::DeviceBusy, // EBUSY
        _ => FlashError::Io(e.to_string()),
    })?;

    #[cfg(target_os = "macos")]
    {
        // F_NOCACHE: bypass the unified buffer cache so writes hit the device.
        use std::os::unix::io::AsRawFd;
        unsafe {
            libc::fcntl(f.as_raw_fd(), libc::F_NOCACHE, 1);
        }
    }
    Ok(f)
}

/// Best-effort eject after a successful write.
pub fn eject(drive: &DriveInfo) {
    if is_regular_file(&drive.device) {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("diskutil")
            .args(["eject", &drive.device])
            .output();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("eject").arg(&drive.device).output();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = drive;
    }
}

/// Eject by device path (used by the GUI's eject_drive command).
pub fn eject_path(device_path: &str) {
    if is_regular_file(device_path) {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("diskutil")
            .args(["eject", device_path])
            .output();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("eject").arg(device_path).output();
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = device_path;
    }
}

/// Independent safety gate run inside the elevated helper: never write a system
/// or non-removable disk. Regular files (dry-run) are always allowed.
pub fn assert_safe_target(drive: &DriveInfo) -> Result<()> {
    if is_regular_file(&drive.device) {
        return Ok(());
    }
    if drive.system {
        return Err(FlashError::UnsafeTarget);
    }
    if !drive.removable {
        return Err(FlashError::UnsafeTarget);
    }
    if drive.read_only {
        return Err(FlashError::UnsafeTarget);
    }
    Ok(())
}
