//! Single-instance guard to prevent running multiple concurrent copies of the UI.
//!
//! Prevents port collisions (41400), settings race conditions, and
//! redundant background tasks.

use std::path::{Path, PathBuf};

pub struct InstanceLock {
    #[cfg(target_os = "windows")]
    handle: *mut std::ffi::c_void,
    #[cfg(not(target_os = "windows"))]
    _file: std::fs::File,
    #[cfg(not(target_os = "windows"))]
    _path: PathBuf,
}

#[cfg(target_os = "windows")]
unsafe impl Send for InstanceLock {}
#[cfg(target_os = "windows")]
unsafe impl Sync for InstanceLock {}

#[cfg(target_os = "windows")]
impl Drop for InstanceLock {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            extern "system" {
                fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
            }
            unsafe { CloseHandle(self.handle) };
        }
    }
}

#[cfg(not(target_os = "windows"))]
impl Drop for InstanceLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self._path);
    }
}

#[cfg(target_os = "windows")]
pub fn try_acquire_named(name: &str) -> Result<InstanceLock, String> {
    extern "system" {
        fn CreateMutexW(
            lpMutexAttributes: *mut std::ffi::c_void,
            bInitialOwner: i32,
            lpName: *const u16,
        ) -> *mut std::ffi::c_void;
        fn GetLastError() -> u32;
        fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
    }
    const ERROR_ALREADY_EXISTS: u32 = 183;
    const ERROR_ACCESS_DENIED: u32 = 5;

    let mut wide: Vec<u16> = name.encode_utf16().collect();
    wide.push(0);

    let handle = unsafe { CreateMutexW(std::ptr::null_mut(), 1, wide.as_ptr()) };
    if handle.is_null() {
        let err = unsafe { GetLastError() };
        if err == ERROR_ACCESS_DENIED {
            return Err("Другой экземпляр приложения уже запущен в системе.".to_string());
        }
        return Err(format!("Не удалось инициализировать мьютекс процесса (ошибка {})", err));
    }

    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe { CloseHandle(handle) };
        return Err("Приложение уже запущено (в фоновом режиме или в трее).".to_string());
    }

    Ok(InstanceLock { handle })
}

#[cfg(not(target_os = "windows"))]
pub fn try_acquire_at(path: &Path) -> Result<InstanceLock, String> {
    use std::io::Write;
    use std::os::unix::io::AsRawFd;

    extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }
    const LOCK_EX: i32 = 2;
    const LOCK_NB: i32 = 4;

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| format!("Не удалось открыть файл блокировки: {e}"))?;

    let res = unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) };
    if res != 0 {
        return Err("Приложение уже запущено (в фоновом режиме или в трее).".to_string());
    }

    let _ = file.set_len(0);
    let _ = writeln!(file, "{}", std::process::id());

    Ok(InstanceLock {
        _file: file,
        _path: path.to_path_buf(),
    })
}

/// Attempts to acquire the global single-instance lock for Open Antigravity.
pub fn try_acquire() -> Result<InstanceLock, String> {
    #[cfg(target_os = "windows")]
    {
        try_acquire_named("Local\\OpenAntigravitySingleInstanceMutex")
    }

    #[cfg(not(target_os = "windows"))]
    {
        let path = if let Some(dir) = crate::portable::data_dir() {
            if std::fs::create_dir_all(&dir).is_ok() {
                dir.join("app.lock")
            } else {
                std::env::temp_dir().join("open_antigravity.lock")
            }
        } else {
            std::env::temp_dir().join("open_antigravity.lock")
        };
        try_acquire_at(&path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_instance_prevents_duplicate_locks() {
        #[cfg(target_os = "windows")]
        {
            let name = "Local\\OpenAntigravityTestMutex_UniqueTest";
            let lock1 = try_acquire_named(name);
            assert!(lock1.is_ok(), "First acquire should succeed");
            let lock2 = try_acquire_named(name);
            assert!(lock2.is_err(), "Second acquire while first is held must fail");
            drop(lock1);
            let lock3 = try_acquire_named(name);
            assert!(lock3.is_ok(), "Acquire after drop must succeed");
        }

        #[cfg(not(target_os = "windows"))]
        {
            let path = std::env::temp_dir().join(format!("oag_test_lock_{}.lock", std::process::id()));
            let lock1 = try_acquire_at(&path);
            assert!(lock1.is_ok(), "First acquire should succeed");
            let lock2 = try_acquire_at(&path);
            assert!(lock2.is_err(), "Second acquire while first is held must fail");
            drop(lock1);
            let lock3 = try_acquire_at(&path);
            assert!(lock3.is_ok(), "Acquire after drop must succeed");
            let _ = std::fs::remove_file(&path);
        }
    }
}
