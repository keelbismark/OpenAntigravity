//! Кроссплатформенное управление автозапуском (Windows + Linux).
//!
//! Windows: ключ в `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
//! Linux: desktop-файл в `~/.config/autostart/open-antigravity.desktop`.

use std::path::{Path, PathBuf};

/// Проверяет, включён ли автозапуск в текущей системе.
pub fn is_enabled() -> bool {
    #[cfg(target_os = "windows")]
    {
        is_windows_enabled()
    }
    #[cfg(not(target_os = "windows"))]
    {
        is_linux_enabled()
    }
}

/// Включает или выключает автозапуск приложения при старте системы.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        set_windows_enabled(enabled)
    }
    #[cfg(not(target_os = "windows"))]
    {
        set_linux_enabled(enabled)
    }
}

#[cfg(not(target_os = "windows"))]
fn autostart_desktop_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir).join("autostart").join("open-antigravity.desktop"));
        }
    }
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".config").join("autostart").join("open-antigravity.desktop"))
}

#[cfg(not(target_os = "windows"))]
fn is_linux_enabled() -> bool {
    autostart_desktop_path().map_or(false, |p| p.is_file())
}

#[cfg(not(target_os = "windows"))]
fn set_linux_enabled(enabled: bool) -> Result<(), String> {
    let path = autostart_desktop_path().ok_or_else(|| "Не удалось определить домашнюю папку ($HOME)".to_string())?;
    if enabled {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("Не удалось создать папку autostart: {e}"))?;
        }
        let exe = crate::utils::app_executable()?;
        let exe_dir = exe.parent().unwrap_or_else(|| Path::new("."));
        let launch_sh = exe_dir.join("launch.sh");
        let exec_cmd = if launch_sh.exists() {
            format!("bash \"{}\" --minimized", launch_sh.display())
        } else {
            format!("\"{}\" --minimized", exe.display())
        };
        let icon_path = exe_dir.join("icon.png");
        let icon_line = if icon_path.exists() {
            format!("Icon={}", icon_path.display())
        } else {
            "Icon=open-antigravity".to_string()
        };

        let content = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Open Antigravity\n\
             Exec={exec_cmd}\n\
             {icon_line}\n\
             Terminal=false\n\
             Categories=Utility;Development;\n\
             Comment=Open Antigravity autostart\n\
             X-GNOME-Autostart-enabled=true\n"
        );
        std::fs::write(&path, content).map_err(|e| format!("Не удалось записать файл автозапуска: {e}"))?;
    } else if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("Не удалось удалить файл автозапуска: {e}"))?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn is_windows_enabled() -> bool {
    let script = "Get-ItemProperty -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run' -Name 'Open Antigravity' -ErrorAction SilentlyContinue";
    if let Some(out) = crate::utils::powershell_within(script, std::time::Duration::from_secs(5)) {
        out.status.success() && !out.stdout.is_empty()
    } else {
        false
    }
}

#[cfg(target_os = "windows")]
fn set_windows_enabled(enabled: bool) -> Result<(), String> {
    if enabled {
        let exe = std::env::current_exe().map_err(|e| format!("Не удалось определить путь к exe: {e}"))?;
        let script = format!(
            "Set-ItemProperty -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run' -Name 'Open Antigravity' -Value '\"{}\" --minimized'",
            exe.display()
        );
        if let Some(out) = crate::utils::powershell_within(&script, std::time::Duration::from_secs(10)) {
            if out.status.success() {
                Ok(())
            } else {
                Err(format!("PowerShell ошибка: {}", String::from_utf8_lossy(&out.stderr).trim()))
            }
        } else {
            Err("Не удалось запустить PowerShell для настройки автозапуска".to_string())
        }
    } else {
        let script = "Remove-ItemProperty -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run' -Name 'Open Antigravity' -ErrorAction SilentlyContinue";
        let _ = crate::utils::powershell_within(script, std::time::Duration::from_secs(10));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_autostart_query_does_not_panic() {
        let _ = is_enabled();
    }
}
