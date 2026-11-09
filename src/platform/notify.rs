//! Кроссплатформенные системные уведомления (Desktop Notifications).
//!
//! Отправка всплывающих сообщений пользователю при работе приложения в фоне
//! (когда окно свернуто в системный трей):
//! - Обнаружено обновление Antigravity (автопатч Language Server)
//! - Доступна новая версия Open Antigravity
//! - Результаты проверки сети

/// Отправляет системное уведомление.
pub fn send(title: &str, message: &str) {
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("notify-send")
            .arg("-a")
            .arg("Open Antigravity")
            .arg(title)
            .arg(message)
            .spawn();
    }

    #[cfg(target_os = "windows")]
    {
        win32::send_notification(title, message);
    }
}

#[cfg(target_os = "windows")]
mod win32 {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    #[repr(C)]
    #[allow(non_snake_case)]
    struct NOTIFYICONDATAW {
        cbSize: u32,
        hWnd: isize,
        uID: u32,
        uFlags: u32,
        uCallbackMessage: u32,
        hIcon: isize,
        szTip: [u16; 128],
        dwState: u32,
        dwStateMask: u32,
        szInfo: [u16; 256],
        uTimeoutOrVersion: u32,
        szInfoTitle: [u16; 64],
        dwInfoFlags: u32,
        guidItem: [u8; 16],
        hBalloonIcon: isize,
    }

    const NIM_ADD: u32 = 0x00000000;
    const NIM_DELETE: u32 = 0x00000002;
    const NIF_ICON: u32 = 0x00000002;
    const NIF_INFO: u32 = 0x00000010;
    const NIIF_INFO: u32 = 0x00000001;

    #[link(name = "shell32")]
    extern "system" {
        fn Shell_NotifyIconW(dwMessage: u32, lpData: *const NOTIFYICONDATAW) -> i32;
    }

    #[link(name = "user32")]
    extern "system" {
        fn CreateWindowExW(
            dwExStyle: u32,
            lpClassName: *const u16,
            lpWindowName: *const u16,
            dwStyle: u32,
            X: i32,
            Y: i32,
            nWidth: i32,
            nHeight: i32,
            hWndParent: isize,
            hMenu: isize,
            hInstance: isize,
            lpParam: *mut std::ffi::c_void,
        ) -> isize;
        fn DestroyWindow(hWnd: isize) -> i32;
        fn LoadIconW(hInstance: isize, lpIconName: *const u16) -> isize;
    }

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    pub fn send_notification(title: &str, message: &str) {
        let title_s = title.to_string();
        let message_s = message.to_string();

        std::thread::spawn(move || unsafe {
            let class_name = to_wide("STATIC");
            let window_name = to_wide("OpenAntigravityNotify");
            let hwnd = CreateWindowExW(
                0,
                class_name.as_ptr(),
                window_name.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                -3, // HWND_MESSAGE
                0,
                0,
                std::ptr::null_mut(),
            );
            if hwnd == 0 {
                return;
            }

            let mut nid = std::mem::zeroed::<NOTIFYICONDATAW>();
            nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = hwnd;
            nid.uID = 0x4F41; // 'OA'
            nid.uFlags = NIF_ICON | NIF_INFO;
            nid.dwInfoFlags = NIIF_INFO;
            nid.hIcon = LoadIconW(0, 32516 as *const u16); // IDI_INFORMATION

            let msg_w = to_wide(&message_s);
            let msg_len = (nid.szInfo.len() - 1).min(msg_w.len());
            nid.szInfo[..msg_len].copy_from_slice(&msg_w[..msg_len]);

            let title_w = to_wide(&title_s);
            let title_len = (nid.szInfoTitle.len() - 1).min(title_w.len());
            nid.szInfoTitle[..title_len].copy_from_slice(&title_w[..title_len]);

            let res = Shell_NotifyIconW(NIM_ADD, &nid);
            if res != 0 {
                std::thread::sleep(std::time::Duration::from_millis(4000));
                Shell_NotifyIconW(NIM_DELETE, &nid);
            }
            DestroyWindow(hwnd);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notify_does_not_panic() {
        send("Open Antigravity Test", "Тестовое уведомление");
    }

    #[test]
    fn test_notify_with_long_strings_truncates_safely() {
        let long_title = "A".repeat(500);
        let long_body = "B".repeat(2000);
        send(&long_title, &long_body);
    }
}
