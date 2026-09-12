use crate::error::{Result, SpectreError};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const DEFAULT_LOCK_TIMEOUT_MS: u64 = 30_000;
const POLL_INTERVAL: Duration = Duration::from_millis(100);

pub struct FileLock {
    path: PathBuf,
    file: Option<File>,
}

fn pid_alive(pid: i32) -> bool {

    unsafe { libc::kill(pid, 0) == 0 }
}

impl FileLock {
    pub fn new(path: PathBuf) -> Self {
        Self { path, file: None }
    }

    pub fn acquire(&mut self, timeout_ms: u64) -> Result<()> {
        let start = Instant::now();
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&self.path) {
                Ok(mut f) => {
                    f.write_all(format!("{}\n", std::process::id()).as_bytes())
                        .and_then(|_| f.sync_all())
                        .map_err(|e| SpectreError::lock_failed(format!("Failed to acquire lock: {}", e)))?;
                    self.file = Some(f);
                    return Ok(());
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {

                    let stale = std::fs::read_to_string(&self.path)
                        .ok()
                        .and_then(|content| content.trim().parse::<i32>().ok())
                        .map(|pid| !pid_alive(pid))
                        .unwrap_or(false);
                    if stale {
                        let _ = std::fs::remove_file(&self.path);
                        continue;
                    }
                    if start.elapsed().as_millis() as u64 > timeout_ms {
                        return Err(SpectreError::lock_timeout(format!(
                            "Lock acquisition timeout after {}ms",
                            timeout_ms
                        )));
                    }
                    std::thread::sleep(POLL_INTERVAL);
                }
                Err(e) => {
                    return Err(SpectreError::lock_failed(format!("Failed to acquire lock: {}", e)));
                }
            }
        }
    }

    pub fn release(&mut self) {
        if self.file.take().is_some() {
            let _ = std::fs::remove_file(&self.path);
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

    #[test]
    fn acquire_release() {
        let dir = std::env::temp_dir().join(format!("spectre-lock-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("db.lock");
        let mut a = FileLock::new(path.clone());
        a.acquire(1000).unwrap();
        assert!(a.is_locked());
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content.trim(), std::process::id().to_string());
        a.release();
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_lock_taken_over() {
        let dir = std::env::temp_dir().join(format!("spectre-lock-stale-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("db.lock");
        std::fs::write(&path, "999999999\n").unwrap();
        let mut a = FileLock::new(path.clone());
        a.acquire(1000).unwrap();
        a.release();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn live_lock_times_out() {
        let dir = std::env::temp_dir().join(format!("spectre-lock-live-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("db.lock");
        std::fs::write(&path, format!("{}\n", std::process::id())).unwrap();
        let mut a = FileLock::new(path.clone());
        let err = a.acquire(150).unwrap_err();
        assert_eq!(err.code, crate::error::codes::LOCK_TIMEOUT);
        std::fs::remove_file(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
