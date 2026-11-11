use crate::error::{Result, SpectreError};
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

pub const DEFAULT_LOCK_TIMEOUT_MS: u64 = 30_000;
const POLL_INTERVAL: Duration = Duration::from_millis(100);

// Paths currently locked by this process. A lock file carrying our own PID is
// only stale if we are not holding that path (e.g. PID 1 after a container
// restart); this avoids deleting an active same-process lock.
static HELD: LazyLock<Mutex<HashSet<PathBuf>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

// Written on the line after the PID by engines that also hold an OS lock on
// the file. If we obtain the OS lock while the marker is present, the previous
// owner is gone even if its PID was reused by another process. Older versions
// and the JS fallback only parse the first line, so they still see the PID.
const OS_LOCK_MARKER: &str = "os-lock";

pub struct FileLock {
    path: PathBuf,
    file: Option<File>,
}

#[cfg(unix)]
fn pid_alive(pid: i32) -> bool {
    let rc = unsafe { libc::kill(pid, 0) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
fn pid_alive(pid: i32) -> bool {
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;
    const ERROR_INVALID_PARAMETER: u32 = 87;

    type Handle = *mut core::ffi::c_void;

    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn GetExitCodeProcess(handle: Handle, code: *mut u32) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
        fn GetLastError() -> u32;
    }

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid as u32);
        if handle.is_null() {
            return GetLastError() != ERROR_INVALID_PARAMETER;
        }
        let mut code: u32 = 0;
        let ok = GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        ok != 0 && code == STILL_ACTIVE
    }
}

#[cfg(not(any(unix, windows)))]
fn pid_alive(_pid: i32) -> bool {
    true
}

fn open_lock_file(path: &std::path::Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // No FILE_SHARE_DELETE: nobody can delete the lock file while a
        // handle is open on it, so a waiter can never end up locking a file
        // that has already been replaced by a new one.
        const FILE_SHARE_READ: u32 = 0x1;
        const FILE_SHARE_WRITE: u32 = 0x2;
        options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    }
    options.open(path)
}

/// Try to take an exclusive, non-blocking OS lock. `false` means another
/// handle holds it. Filesystems without lock support report `true` so the PID
/// protocol alone decides, as in earlier versions.
#[cfg(unix)]
fn try_os_lock(file: &File) -> bool {
    use std::os::unix::io::AsRawFd;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::EWOULDBLOCK)
}

#[cfg(unix)]
fn os_unlock(_file: &File) {
    // Released when the descriptor is closed.
}

#[cfg(windows)]
mod win {
    #[repr(C)]
    pub struct Overlapped {
        pub internal: usize,
        pub internal_high: usize,
        pub offset: u32,
        pub offset_high: u32,
        pub h_event: *mut core::ffi::c_void,
    }

    extern "system" {
        pub fn LockFileEx(
            file: *mut core::ffi::c_void,
            flags: u32,
            reserved: u32,
            len_low: u32,
            len_high: u32,
            overlapped: *mut Overlapped,
        ) -> i32;
        pub fn UnlockFileEx(
            file: *mut core::ffi::c_void,
            reserved: u32,
            len_low: u32,
            len_high: u32,
            overlapped: *mut Overlapped,
        ) -> i32;
    }

    pub const LOCKFILE_FAIL_IMMEDIATELY: u32 = 0x1;
    pub const LOCKFILE_EXCLUSIVE_LOCK: u32 = 0x2;
    pub const ERROR_LOCK_VIOLATION: i32 = 33;

    // Lock one byte far past the end of the file: Windows locks are
    // mandatory, and locking the PID bytes would stop other processes (and
    // the JS fallback) from reading who owns the database.
    pub fn overlapped() -> Overlapped {
        Overlapped {
            internal: 0,
            internal_high: 0,
            offset: 0,
            offset_high: 0x100,
            h_event: core::ptr::null_mut(),
        }
    }
}

#[cfg(windows)]
fn try_os_lock(file: &File) -> bool {
    use std::os::windows::io::AsRawHandle;
    let mut ov = win::overlapped();
    let ok = unsafe {
        win::LockFileEx(
            file.as_raw_handle() as *mut _,
            win::LOCKFILE_EXCLUSIVE_LOCK | win::LOCKFILE_FAIL_IMMEDIATELY,
            0,
            1,
            0,
            &mut ov,
        )
    };
    ok != 0 || std::io::Error::last_os_error().raw_os_error() != Some(win::ERROR_LOCK_VIOLATION)
}

#[cfg(windows)]
fn os_unlock(file: &File) {
    use std::os::windows::io::AsRawHandle;
    let mut ov = win::overlapped();
    unsafe {
        win::UnlockFileEx(file.as_raw_handle() as *mut _, 0, 1, 0, &mut ov);
    }
}

#[cfg(not(any(unix, windows)))]
fn try_os_lock(_file: &File) -> bool {
    true
}

#[cfg(not(any(unix, windows)))]
fn os_unlock(_file: &File) {}

/// On Unix the lock file is unlinked on release while still locked; a waiter
/// that opened the old inode must notice it is no longer the file at `path`.
#[cfg(unix)]
fn still_at_path(file: &File, path: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (file.metadata(), std::fs::metadata(path)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn still_at_path(_file: &File, _path: &std::path::Path) -> bool {
    true
}

impl FileLock {
    pub fn new(path: PathBuf) -> Self {
        Self { path, file: None }
    }

    /// Acquire the database lock.
    ///
    /// Two layers: an OS lock (`flock` / `LockFileEx`) that the kernel drops
    /// when the owner dies, so stale locks need no guessing and two processes
    /// can never both "take over" the same stale file; and the PID written in
    /// the file, still honoured so the JS fallback engine and older versions
    /// (which only know the PID protocol) keep excluding each other with us.
    pub fn acquire(&mut self, timeout_ms: u64) -> Result<()> {
        let start = Instant::now();
        let own_pid = std::process::id() as i32;
        loop {
            let mut held = HELD.lock().unwrap_or_else(|p| p.into_inner());
            match open_lock_file(&self.path) {
                Ok(mut file) => {
                    if try_os_lock(&file) {
                        if still_at_path(&file, &self.path) {
                            let mut content = String::new();
                            let _ = file.read_to_string(&mut content);
                            let mut lines = content.lines();
                            let owner = lines.next().and_then(|l| l.trim().parse::<i32>().ok());
                            let os_locked_owner = lines.any(|l| l.trim() == OS_LOCK_MARKER);
                            let foreign_owner = match owner {
                                // We hold the OS lock, so an owner that also
                                // used one has exited, whatever its PID now
                                // points to.
                                Some(_) if os_locked_owner => false,
                                Some(pid) if pid == own_pid => held.contains(&self.path),
                                Some(pid) => pid_alive(pid),
                                None => false,
                            };
                            if !foreign_owner {
                                let stamp = format!("{}\n{}\n", own_pid, OS_LOCK_MARKER);
                                let written = file
                                    .set_len(0)
                                    .and_then(|_| file.seek(SeekFrom::Start(0)))
                                    .and_then(|_| file.write_all(stamp.as_bytes()))
                                    .and_then(|_| file.sync_all());
                                if let Err(e) = written {
                                    os_unlock(&file);
                                    return Err(SpectreError::lock_failed(format!(
                                        "Failed to acquire lock: {}",
                                        e
                                    )));
                                }
                                held.insert(self.path.clone());
                                self.file = Some(file);
                                return Ok(());
                            }
                        }
                        os_unlock(&file);
                    }
                    drop(file);
                }
                // Windows: the file is being deleted by its previous owner.
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {}
                Err(e) => {
                    return Err(SpectreError::lock_failed(format!("Failed to acquire lock: {}", e)));
                }
            }
            drop(held);
            if start.elapsed().as_millis() as u64 > timeout_ms {
                return Err(SpectreError::lock_timeout(format!(
                    "Lock acquisition timeout after {}ms",
                    timeout_ms
                )));
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    pub fn release(&mut self) {
        if let Some(file) = self.file.take() {
            let mut held = HELD.lock().unwrap_or_else(|p| p.into_inner());
            held.remove(&self.path);
            // Unix: unlink while still locked; waiters re-check the inode.
            #[cfg(unix)]
            let _ = std::fs::remove_file(&self.path);
            // Clear the owner so a waiter that keeps this file (Windows, when
            // the delete below loses the race with its open) sees it as free.
            let _ = file.set_len(0);
            os_unlock(&file);
            drop(file);
            #[cfg(not(unix))]
            let _ = std::fs::remove_file(&self.path);
        }
    }

    /// Test helper: drop the OS lock and the file handle the way a crashed
    /// process would, leaving the lock file and its contents behind.
    #[cfg(test)]
    pub fn abandon(&mut self) {
        if let Some(file) = self.file.take() {
            HELD.lock().unwrap_or_else(|p| p.into_inner()).remove(&self.path);
            os_unlock(&file);
        }
    }

    pub fn is_locked(&self) -> bool {
        self.file.is_some()
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    const LIVE_FOREIGN_PID: &str = "1";
    #[cfg(windows)]
    const LIVE_FOREIGN_PID: &str = "4";

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("spectre-lock-{}-{}", name, std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn acquire_release() {
        let dir = test_dir("basic");
        let path = dir.join("db.lock");
        let mut a = FileLock::new(path.clone());
        a.acquire(1000).unwrap();
        assert!(a.is_locked());
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content.lines().next().unwrap(), std::process::id().to_string());
        a.release();
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_lock_taken_over() {
        let dir = test_dir("stale");
        let path = dir.join("db.lock");
        std::fs::write(&path, "999999999\n").unwrap();
        let mut a = FileLock::new(path.clone());
        a.acquire(1000).unwrap();
        a.release();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_own_pid_is_taken_over() {
        let dir = test_dir("ownpid");
        let path = dir.join("db.lock");
        std::fs::write(&path, format!("{}\n", std::process::id())).unwrap();
        let mut a = FileLock::new(path.clone());
        a.acquire(1000).unwrap();
        a.release();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn live_lock_times_out() {
        let dir = test_dir("live");
        let path = dir.join("db.lock");
        let mut holder = FileLock::new(path.clone());
        holder.acquire(1000).unwrap();
        let mut a = FileLock::new(path.clone());
        let err = a.acquire(150).unwrap_err();
        assert_eq!(err.code, crate::error::codes::LOCK_TIMEOUT);
        holder.release();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn live_pid_without_os_lock_marker_is_respected() {
        // e.g. the JS fallback engine, which only writes its PID.
        let dir = test_dir("pidonly");
        let path = dir.join("db.lock");
        std::fs::write(&path, format!("{}\n", LIVE_FOREIGN_PID)).unwrap();
        let mut a = FileLock::new(path.clone());
        let err = a.acquire(150).unwrap_err();
        assert_eq!(err.code, crate::error::codes::LOCK_TIMEOUT);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reused_pid_of_crashed_os_locked_owner_is_taken_over() {
        let dir = test_dir("reused");
        let path = dir.join("db.lock");
        std::fs::write(&path, format!("{}\n{}\n", LIVE_FOREIGN_PID, OS_LOCK_MARKER)).unwrap();
        let mut a = FileLock::new(path.clone());
        a.acquire(1000).unwrap();
        a.release();
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_acquirers_never_both_win() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let dir = test_dir("race");
        let path = dir.join("db.lock");
        std::fs::write(&path, "999999999\n").unwrap(); // stale owner
        let inside = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let (path, inside, max_seen) = (path.clone(), Arc::clone(&inside), Arc::clone(&max_seen));
                std::thread::spawn(move || {
                    for _ in 0..5 {
                        let mut lock = FileLock::new(path.clone());
                        lock.acquire(20_000).unwrap();
                        let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                        max_seen.fetch_max(now, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(2));
                        inside.fetch_sub(1, Ordering::SeqCst);
                        lock.release();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(max_seen.load(Ordering::SeqCst), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
