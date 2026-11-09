//! Windows console allocation and attachment.

#![cfg(windows)]

    use std::ffi::c_void;

    #[link(name = "kernel32")]
    extern "system" {
        fn AllocConsole() -> i32;
        fn AttachConsole(pid: u32) -> i32;
        fn FreeConsole() -> i32;
        fn SetStdHandle(which: u32, handle: *mut c_void) -> i32;
        fn SetConsoleTitleW(title: *const u16) -> i32;
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *mut c_void,
            disposition: u32,
            flags: u32,
            template: *mut c_void,
        ) -> *mut c_void;
    }

    const ATTACH_PARENT_PROCESS: u32 = 0xFFFF_FFFF;
    const STD_INPUT_HANDLE: u32 = (-10i32) as u32;
    const STD_OUTPUT_HANDLE: u32 = (-11i32) as u32;
    const STD_ERROR_HANDLE: u32 = (-12i32) as u32;
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ: u32 = 1;
    const FILE_SHARE_WRITE: u32 = 2;
    const OPEN_EXISTING: u32 = 3;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// A console for the terminal UI.
    ///
    /// Its own window, normally: this is a GUI-subsystem exe, so the shell that
    /// started it has already gone back to its prompt, and sharing that console
    /// would hand every other keystroke to the shell.
    ///
    /// The console it was started from, when the caller says it waits
    /// (`--inline`: `tui.ps1` runs `Start-Process -Wait -NoNewWindow`, cmd
    /// `start /b /wait`), and over SSH, where there is no desktop for a window
    /// to appear on at all — there, too, the shell has to be told to wait.
    pub fn open() -> Result<(), String> {
        let over_ssh = std::env::var_os("SSH_CONNECTION").is_some()
            || std::env::var_os("SSH_CLIENT").is_some();
        let inline = over_ssh
            || std::env::args()
                .skip(1)
                .any(|a| a.trim_start_matches('-').eq_ignore_ascii_case("inline"));
        unsafe {
            let attached = inline && AttachConsole(ATTACH_PARENT_PROCESS) != 0;
            if !attached {
                FreeConsole();
                if AllocConsole() == 0 {
                    return Err("не удалось открыть консоль для терминального режима".into());
                }
            }
            // Whatever handles we inherited (none, for a GUI process, or a pipe
            // someone redirected) are pointed at this console: Rust's stdout
            // writes through WriteConsoleW when its handle is a console, which is
            // what keeps Cyrillic intact whatever the code page.
            for (which, name) in [
                (STD_INPUT_HANDLE, "CONIN$"),
                (STD_OUTPUT_HANDLE, "CONOUT$"),
                (STD_ERROR_HANDLE, "CONOUT$"),
            ] {
                let name = wide(name);
                let h = CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    std::ptr::null_mut(),
                    OPEN_EXISTING,
                    0,
                    std::ptr::null_mut(),
                );
                if !h.is_null() && h != (-1isize) as *mut c_void {
                    SetStdHandle(which, h);
                }
            }
            let title = wide("Open Antigravity");
            SetConsoleTitleW(title.as_ptr());
        }
        Ok(())
    }
