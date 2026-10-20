//! Fast Win32 registry and process helpers for Windows.
//! Bypasses slow PowerShell, tasklist, and schtasks processes for routine queries.

#![cfg(target_os = "windows")]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

#[link(name = "advapi32")]
extern "system" {
    fn RegOpenKeyExW(
        hKey: isize,
        lpSubKey: *const u16,
        ulOptions: u32,
        samDesired: u32,
        phkResult: *mut isize,
    ) -> i32;
    fn RegQueryValueExW(
        hKey: isize,
        lpValueName: *const u16,
        lpReserved: *mut u32,
        lpType: *mut u32,
        lpData: *mut u8,
        lpcbData: *mut u32,
    ) -> i32;
    fn RegSetValueExW(
        hKey: isize,
        lpValueName: *const u16,
        Reserved: u32,
        dwType: u32,
        lpData: *const u8,
        cbData: u32,
    ) -> i32;
    fn RegDeleteValueW(hKey: isize, lpValueName: *const u16) -> i32;
    fn RegEnumKeyExW(
        hKey: isize,
        dwIndex: u32,
        lpName: *mut u16,
        lpcchName: *mut u32,
        lpReserved: *mut u32,
        lpClass: *mut u16,
        lpcchClass: *mut u32,
        lpftLastWriteTime: *mut u64,
    ) -> i32;
    fn RegCloseKey(hKey: isize) -> i32;
}

#[link(name = "user32")]
extern "system" {
    fn SendMessageTimeoutW(
        hWnd: isize,
        Msg: u32,
        wParam: usize,
        lParam: *const u16,
        fuFlags: u32,
        uTimeout: u32,
        lpdwResult: *mut usize,
    ) -> isize;
}

#[link(name = "kernel32")]
extern "system" {
    fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> *mut std::ffi::c_void;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    fn GetExitCodeProcess(handle: *mut std::ffi::c_void, exit_code: *mut u32) -> i32;
    fn CreateToolhelp32Snapshot(dwFlags: u32, th32ProcessID: u32) -> *mut std::ffi::c_void;
    fn Process32FirstW(hSnapshot: *mut std::ffi::c_void, lppe: *mut PROCESSENTRY32W) -> i32;
    fn Process32NextW(hSnapshot: *mut std::ffi::c_void, lppe: *mut PROCESSENTRY32W) -> i32;
}

const HKEY_CURRENT_USER: isize = -2147483647; // 0x80000001
const HKEY_LOCAL_MACHINE: isize = -2147483646; // 0x80000002
const KEY_READ: u32 = 0x20019;
const KEY_WRITE: u32 = 0x20006;
const REG_SZ: u32 = 1;
const HWND_BROADCAST: isize = 0xffff;
const WM_SETTINGCHANGE: u32 = 0x001A;
const SMTO_ABORTIFHUNG: u32 = 0x0002;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const STILL_ACTIVE: u32 = 259;
const TH32CS_SNAPPROCESS: u32 = 0x00000002;

#[repr(C)]
#[allow(non_snake_case, dead_code)]
struct PROCESSENTRY32W {
    dwSize: u32,
    cntUsage: u32,
    th32ProcessID: u32,
    th32DefaultHeapID: usize,
    th32ModuleID: u32,
    cntThreads: u32,
    th32ParentProcessID: u32,
    pcPriClassBase: i32,
    dwFlags: u32,
    szExeFile: [u16; 260],
}

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

// ---------------------------------------------------------------------------
// Environment variables via Registry
// ---------------------------------------------------------------------------

/// Reads a persistent environment variable from the Windows registry.
pub fn get_env(name: &str, scope: &str) -> Option<String> {
    if scope.eq_ignore_ascii_case("machine") {
        get_reg_env(
            HKEY_LOCAL_MACHINE,
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
            name,
        )
    } else {
        get_reg_env(HKEY_CURRENT_USER, "Environment", name)
    }
}

/// Reads a persistent user environment variable from `HKCU\Environment`.
pub fn get_user_env(name: &str) -> Option<String> {
    get_reg_env(HKEY_CURRENT_USER, "Environment", name)
}

fn get_reg_env(root: isize, subkey: &str, name: &str) -> Option<String> {
    let subkey_w = to_wide(subkey);
    let name_w = to_wide(name);
    let mut key: isize = 0;
    unsafe {
        if RegOpenKeyExW(root, subkey_w.as_ptr(), 0, KEY_READ, &mut key) != 0 {
            return None;
        }
        let mut buf_size: u32 = 0;
        let mut val_type: u32 = 0;
        if RegQueryValueExW(
            key,
            name_w.as_ptr(),
            std::ptr::null_mut(),
            &mut val_type,
            std::ptr::null_mut(),
            &mut buf_size,
        ) != 0
        {
            RegCloseKey(key);
            return None;
        }
        let mut data = vec![0u8; buf_size as usize];
        if RegQueryValueExW(
            key,
            name_w.as_ptr(),
            std::ptr::null_mut(),
            &mut val_type,
            data.as_mut_ptr(),
            &mut buf_size,
        ) != 0
        {
            RegCloseKey(key);
            return None;
        }
        RegCloseKey(key);

        let u16_slice: &[u16] =
            std::slice::from_raw_parts(data.as_ptr() as *const u16, data.len() / 2);
        let s = String::from_utf16_lossy(u16_slice);
        let trimmed = s.trim_matches('\0').trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }
}

/// Sets or deletes a persistent user environment variable in `HKCU\Environment`
/// and broadcasts `WM_SETTINGCHANGE`.
pub fn set_user_env(name: &str, value: Option<&str>) -> Result<(), String> {
    let subkey_w = to_wide("Environment");
    let name_w = to_wide(name);
    let mut key: isize = 0;
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey_w.as_ptr(), 0, KEY_WRITE, &mut key) != 0 {
            return Err("Не удалось открыть HKCU\\Environment для записи".to_string());
        }
        let res = match value {
            Some(v) => {
                let v_w = to_wide(v);
                RegSetValueExW(
                    key,
                    name_w.as_ptr(),
                    0,
                    REG_SZ,
                    v_w.as_ptr() as *const u8,
                    (v_w.len() * 2) as u32,
                )
            }
            None => RegDeleteValueW(key, name_w.as_ptr()),
        };
        RegCloseKey(key);

        let env_w = to_wide("Environment");
        let mut res_out = 0;
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            env_w.as_ptr(),
            SMTO_ABORTIFHUNG,
            100,
            &mut res_out,
        );

        if res == 0 || (value.is_none() && (res == 2 || res == 0)) {
            Ok(())
        } else {
            Err(format!("Ошибка реестра при сохранении переменной: {res}"))
        }
    }
}

// ---------------------------------------------------------------------------
// Process liveness & snapshotting via Toolhelp32 & OpenProcess
// ---------------------------------------------------------------------------

/// Fast check whether a process with PID is currently alive without running `tasklist`.
pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code = 0u32;
        let res = GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        res != 0 && code == STILL_ACTIVE
    }
}

/// Finds all running process IDs matching `exe_name` (case-insensitive) using Toolhelp32.
pub fn find_pids_by_name(exe_name: &str) -> Vec<u32> {
    let mut pids = Vec::new();
    let invalid_handle = (-1isize) as *mut std::ffi::c_void;
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot.is_null() || snapshot == invalid_handle {
            return pids;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                if name.eq_ignore_ascii_case(exe_name) {
                    pids.push(entry.th32ProcessID);
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
    }
    pids
}

/// Returns true if any instance of `exe_name` is running other than `my_pid`.
pub fn has_other_instance(exe_name: &str, my_pid: u32) -> bool {
    find_pids_by_name(exe_name).into_iter().any(|p| p != my_pid)
}

// ---------------------------------------------------------------------------
// NRPT rules inspection via Registry
// ---------------------------------------------------------------------------

/// Reads all NRPT namespaces matching the given tags directly from Windows Registry.
/// Returns None if registry cannot be read so caller can fall back to PowerShell.
pub fn get_nrpt_namespaces(tags: &[&str]) -> Option<Vec<String>> {
    let subkey_w = to_wide(r"SYSTEM\CurrentControlSet\Services\Dnscache\Parameters\DnsPolicyConfig");
    let mut key: isize = 0;
    unsafe {
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey_w.as_ptr(), 0, KEY_READ, &mut key) != 0 {
            return None;
        }

        let mut index = 0u32;
        let mut namespaces = Vec::new();
        let comment_w = to_wide("Comment");
        let name_w = to_wide("Name");

        loop {
            let mut subkey_name = [0u16; 256];
            let mut name_len = subkey_name.len() as u32;
            if RegEnumKeyExW(
                key,
                index,
                subkey_name.as_mut_ptr(),
                &mut name_len,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) != 0
            {
                break;
            }
            index += 1;

            let mut sub_hkey: isize = 0;
            if RegOpenKeyExW(key, subkey_name.as_ptr(), 0, KEY_READ, &mut sub_hkey) == 0 {
                // Read Comment
                let mut val_type = 0u32;
                let mut buf_size = 0u32;
                if RegQueryValueExW(
                    sub_hkey,
                    comment_w.as_ptr(),
                    std::ptr::null_mut(),
                    &mut val_type,
                    std::ptr::null_mut(),
                    &mut buf_size,
                ) == 0
                    && buf_size > 0
                {
                    let mut data = vec![0u8; buf_size as usize];
                    if RegQueryValueExW(
                        sub_hkey,
                        comment_w.as_ptr(),
                        std::ptr::null_mut(),
                        &mut val_type,
                        data.as_mut_ptr(),
                        &mut buf_size,
                    ) == 0
                    {
                        let u16_slice: &[u16] = std::slice::from_raw_parts(
                            data.as_ptr() as *const u16,
                            data.len() / 2,
                        );
                        let comment = String::from_utf16_lossy(u16_slice)
                            .trim_matches('\0')
                            .trim()
                            .to_string();

                        if tags.iter().any(|t| t.eq_ignore_ascii_case(&comment)) {
                            // Read Name (REG_MULTI_SZ or REG_SZ)
                            let mut name_type = 0u32;
                            let mut name_size = 0u32;
                            if RegQueryValueExW(
                                sub_hkey,
                                name_w.as_ptr(),
                                std::ptr::null_mut(),
                                &mut name_type,
                                std::ptr::null_mut(),
                                &mut name_size,
                            ) == 0
                                && name_size > 0
                            {
                                let mut name_data = vec![0u8; name_size as usize];
                                if RegQueryValueExW(
                                    sub_hkey,
                                    name_w.as_ptr(),
                                    std::ptr::null_mut(),
                                    &mut name_type,
                                    name_data.as_mut_ptr(),
                                    &mut name_size,
                                ) == 0
                                {
                                    let u16_name: &[u16] = std::slice::from_raw_parts(
                                        name_data.as_ptr() as *const u16,
                                        name_data.len() / 2,
                                    );
                                    let name_str = String::from_utf16_lossy(u16_name);
                                    // Parse null-separated or newline-separated strings
                                    for part in name_str.split('\0') {
                                        let clean = part.trim().trim_end_matches('.').to_lowercase();
                                        if !clean.is_empty() && !namespaces.contains(&clean) {
                                            namespaces.push(clean);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                RegCloseKey(sub_hkey);
            }
        }
        RegCloseKey(key);
        Some(namespaces)
    }
}

/// Checks if any subkey with our tag exists in DnsPolicyConfig.
pub fn has_any_nrpt_rules(tags: &[&str]) -> Option<bool> {
    get_nrpt_namespaces(tags).map(|ns| !ns.is_empty())
}

/// Checks if any conflicting NRPT rule (Comment not matching our tag) exists for our namespaces.
pub fn has_conflicting_rules(namespaces: &[&str], our_tag: &str) -> Option<bool> {
    let subkey_w = to_wide(r"SYSTEM\CurrentControlSet\Services\Dnscache\Parameters\DnsPolicyConfig");
    let mut key: isize = 0;
    unsafe {
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey_w.as_ptr(), 0, KEY_READ, &mut key) != 0 {
            return None;
        }
        let mut index = 0u32;
        let mut has_conflict = false;
        let comment_w = to_wide("Comment");
        let name_w = to_wide("Name");

        loop {
            let mut subkey_name = [0u16; 256];
            let mut name_len = subkey_name.len() as u32;
            if RegEnumKeyExW(
                key,
                index,
                subkey_name.as_mut_ptr(),
                &mut name_len,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) != 0
            {
                break;
            }
            index += 1;

            let mut sub_hkey: isize = 0;
            if RegOpenKeyExW(key, subkey_name.as_ptr(), 0, KEY_READ, &mut sub_hkey) == 0 {
                let mut val_type = 0u32;
                let mut buf_size = 0u32;
                if RegQueryValueExW(
                    sub_hkey,
                    comment_w.as_ptr(),
                    std::ptr::null_mut(),
                    &mut val_type,
                    std::ptr::null_mut(),
                    &mut buf_size,
                ) == 0
                    && buf_size > 0
                {
                    let mut data = vec![0u8; buf_size as usize];
                    if RegQueryValueExW(
                        sub_hkey,
                        comment_w.as_ptr(),
                        std::ptr::null_mut(),
                        &mut val_type,
                        data.as_mut_ptr(),
                        &mut buf_size,
                    ) == 0
                    {
                        let u16_slice: &[u16] = std::slice::from_raw_parts(
                            data.as_ptr() as *const u16,
                            data.len() / 2,
                        );
                        let comment = String::from_utf16_lossy(u16_slice)
                            .trim_matches('\0')
                            .trim()
                            .to_string();

                        if !comment.eq_ignore_ascii_case(our_tag) {
                            let mut name_type = 0u32;
                            let mut name_size = 0u32;
                            if RegQueryValueExW(
                                sub_hkey,
                                name_w.as_ptr(),
                                std::ptr::null_mut(),
                                &mut name_type,
                                std::ptr::null_mut(),
                                &mut name_size,
                            ) == 0
                                && name_size > 0
                            {
                                let mut name_data = vec![0u8; name_size as usize];
                                if RegQueryValueExW(
                                    sub_hkey,
                                    name_w.as_ptr(),
                                    std::ptr::null_mut(),
                                    &mut name_type,
                                    name_data.as_mut_ptr(),
                                    &mut name_size,
                                ) == 0
                                {
                                    let u16_name: &[u16] = std::slice::from_raw_parts(
                                        name_data.as_ptr() as *const u16,
                                        name_data.len() / 2,
                                    );
                                    let name_str = String::from_utf16_lossy(u16_name);
                                    for part in name_str.split('\0') {
                                        let clean = part.trim().trim_end_matches('.').to_lowercase();
                                        if namespaces.iter().any(|ns| ns.eq_ignore_ascii_case(&clean)) {
                                            has_conflict = true;
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                RegCloseKey(sub_hkey);
            }
            if has_conflict {
                break;
            }
        }
        RegCloseKey(key);
        Some(has_conflict)
    }
}
