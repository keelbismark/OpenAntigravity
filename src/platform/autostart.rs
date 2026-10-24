//! Кроссплатформенное управление автозапуском (Windows + Linux).
//!
//! Windows: задача планировщика с повышенными правами (Highest) или ключ в HKCU\Run.
//! Linux: desktop-файл в `~/.config/autostart/open-antigravity.desktop`.

#[cfg(not(target_os = "windows"))]
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
const AUTOSTART_TASK_NAME: &str = "Open Antigravity Autostart";

#[cfg(target_os = "windows")]
fn is_task_enabled() -> bool {
    let mut cmd = std::process::Command::new("schtasks");
    cmd.args(["/Query", "/TN", AUTOSTART_TASK_NAME]);
    crate::utils::bounded_output(crate::utils::no_window(&mut cmd), std::time::Duration::from_secs(5))
        .is_some_and(|o| o.status.success())
}

#[cfg(target_os = "windows")]
fn is_registry_run_enabled() -> bool {
    let script = "Get-ItemProperty -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run' -Name 'Open Antigravity' -ErrorAction SilentlyContinue";
    if let Some(out) = crate::utils::powershell_within(script, std::time::Duration::from_secs(5)) {
        out.status.success() && !out.stdout.is_empty()
    } else {
        false
    }
}

#[cfg(target_os = "windows")]
fn is_windows_enabled() -> bool {
    is_task_enabled() || is_registry_run_enabled()
}

#[cfg(target_os = "windows")]
fn set_windows_enabled(enabled: bool) -> Result<(), String> {
    if enabled {
        let exe = std::env::current_exe().map_err(|e| format!("Не удалось определить путь к exe: {e}"))?;
        let exe_str = exe.display().to_string();

        // Если у процесса есть права администратора — регистрируем задачу в планировщике с RunLevel Highest.
        // Это позволяет приложению запускаться при входе в систему СРАЗУ с правами администратора
        // БЕЗ окна UAC, чего невозможно добиться через обычный HKCU\...\Run.
        let mut task_created = false;
        if crate::utils::is_admin() {
            let task_script = format!(
                "$a = New-ScheduledTaskAction -Execute '{exe}' -Argument '--minimized'; \
                 $t = New-ScheduledTaskTrigger -AtLogOn; \
                 $s = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries \
                      -MultipleInstances IgnoreNew -ExecutionTimeLimit ([TimeSpan]::Zero); \
                 $p = New-ScheduledTaskPrincipal -UserId \"$env:USERDOMAIN\\$env:USERNAME\" \
                      -LogonType Interactive -RunLevel Highest; \
                 Register-ScheduledTask -TaskName '{task}' -Action $a -Trigger $t -Settings $s \
                      -Principal $p -Description 'Open Antigravity: автозапуск с повышенными правами' -Force -ErrorAction Stop",
                exe = exe_str.replace('\'', "''"),
                task = AUTOSTART_TASK_NAME,
            );
            if let Some(out) = crate::utils::powershell_within(&task_script, std::time::Duration::from_secs(10)) {
                if out.status.success() {
                    task_created = true;
                    // Удаляем запись из HKCU\...\Run, чтобы исключить двойной запуск
                    let clean_reg = "Remove-ItemProperty -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run' -Name 'Open Antigravity' -ErrorAction SilentlyContinue";
                    let _ = crate::utils::powershell_within(clean_reg, std::time::Duration::from_secs(5));
                }
            }
        }

        if !task_created {
            // Фолбэк для работы без прав администратора — запись в реестр HKCU
            let reg_script = format!(
                "Set-ItemProperty -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run' -Name 'Open Antigravity' -Value '\"{}\" --minimized'",
                exe_str
            );
            if let Some(out) = crate::utils::powershell_within(&reg_script, std::time::Duration::from_secs(10)) {
                if !out.status.success() {
                    return Err(format!("PowerShell ошибка: {}", String::from_utf8_lossy(&out.stderr).trim()));
                }
            } else {
                return Err("Не удалось настроить автозапуск через реестр".to_string());
            }
        }
        Ok(())
    } else {
        // Выключение автозапуска: удаляем задачу из планировщика и ключ из реестра
        let mut cmd = std::process::Command::new("schtasks");
        cmd.args(["/Delete", "/TN", AUTOSTART_TASK_NAME, "/F"]);
        crate::utils::bounded_output(crate::utils::no_window(&mut cmd), std::time::Duration::from_secs(5));

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
