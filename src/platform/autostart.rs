//! Кроссплатформенное управление автозапуском (Windows + Linux).
//!
//! Windows: задача планировщика с повышенными правами (Highest) или ключ в HKCU\Run.
//! Linux: desktop-файл в `~/.config/autostart/open-antigravity.desktop`.

#[cfg(not(target_os = "windows"))]
use std::path::{Path, PathBuf};

/// Проверяет, включён ли автозапуск в текущей системе (планировщик задач либо реестр).
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

/// Проверяет, настроен ли автозапуск с правами администратора (через Планировщик Windows).
pub fn is_admin_enabled() -> bool {
    #[cfg(target_os = "windows")]
    {
        is_task_enabled()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// Включает или выключает автозапуск приложения при старте системы, сохраняя текущий режим прав.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    set_enabled_with_admin(enabled, is_admin_enabled() || crate::utils::is_admin())
}

/// Включает или выключает автозапуск приложения при старте системы с явным выбором прав администратора.
pub fn set_enabled_with_admin(enabled: bool, as_admin: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        set_windows_enabled_with_admin(enabled, as_admin)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = as_admin;
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
    crate::platform::fast_win::get_hkcu_run("Open Antigravity").is_some()
}

#[cfg(target_os = "windows")]
fn is_windows_enabled() -> bool {
    is_task_enabled() || is_registry_run_enabled()
}

#[cfg(target_os = "windows")]
fn set_windows_enabled_with_admin(enabled: bool, as_admin: bool) -> Result<(), String> {
    if enabled {
        let exe = std::env::current_exe().map_err(|e| format!("Не удалось определить путь к exe: {e}"))?;
        let exe_str = exe.display().to_string();

        if as_admin {
            // Регистрация в планировщике задач с RunLevel Highest для автозапуска от имени администратора без UAC
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
                    if !out.status.success() {
                        let err = String::from_utf8_lossy(&out.stderr);
                        return Err(format!("Ошибка создания задачи планировщика: {}", err.trim()));
                    }
                } else {
                    return Err("Не удалось выполнить PowerShell для создания задачи планировщика".to_string());
                }
            } else {
                // Если текущий процесс не админ, запрашиваем повышение привилегий один раз через UAC
                let inner_cmd = format!(
                    "$a = New-ScheduledTaskAction -Execute '{exe}' -Argument '--minimized'; \
                     $t = New-ScheduledTaskTrigger -AtLogOn; \
                     $s = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries \
                          -MultipleInstances IgnoreNew -ExecutionTimeLimit ([TimeSpan]::Zero); \
                     $p = New-ScheduledTaskPrincipal -UserId \"$env:USERDOMAIN\\$env:USERNAME\" \
                          -LogonType Interactive -RunLevel Highest; \
                     Register-ScheduledTask -TaskName '{task}' -Action $a -Trigger $t -Settings $s \
                          -Principal $p -Description 'Open Antigravity: автозапуск с повышенными правами' -Force; \
                     Remove-ItemProperty -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run' -Name 'Open Antigravity' -ErrorAction SilentlyContinue",
                    exe = exe_str.replace('\'', "''"),
                    task = AUTOSTART_TASK_NAME,
                );
                let elevate_script = format!(
                    "Start-Process powershell.exe -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', \"{}\"",
                    inner_cmd.replace('"', "`\"")
                );
                crate::utils::powershell_within(&elevate_script, std::time::Duration::from_secs(30));
                if !is_task_enabled() {
                    return Err("Для автозапуска от администратора требуется подтверждение UAC".to_string());
                }
            }

            // Очищаем HKCU Run, чтобы исключить двойной запуск
            let _ = crate::platform::fast_win::set_hkcu_run("Open Antigravity", None);
        } else {
            // Обычный автозапуск без прав администратора: удаляем задачу из планировщика и пишем в HKCU Run
            let mut cmd = std::process::Command::new("schtasks");
            cmd.args(["/Delete", "/TN", AUTOSTART_TASK_NAME, "/F"]);
            crate::utils::bounded_output(crate::utils::no_window(&mut cmd), std::time::Duration::from_secs(5));

            let run_val = format!("\"{}\" --minimized", exe_str);
            crate::platform::fast_win::set_hkcu_run("Open Antigravity", Some(&run_val))?;
        }
        Ok(())
    } else {
        // Выключение автозапуска: удаляем задачу из планировщика и ключ из реестра
        let mut cmd = std::process::Command::new("schtasks");
        cmd.args(["/Delete", "/TN", AUTOSTART_TASK_NAME, "/F"]);
        crate::utils::bounded_output(crate::utils::no_window(&mut cmd), std::time::Duration::from_secs(5));

        if is_task_enabled() && !crate::utils::is_admin() {
            let elevate_del = format!(
                "Start-Process powershell.exe -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList '-NoProfile', '-Command', 'Unregister-ScheduledTask -TaskName \"{}\" -Confirm:$false -ErrorAction SilentlyContinue'",
                AUTOSTART_TASK_NAME
            );
            crate::utils::powershell_within(&elevate_del, std::time::Duration::from_secs(20));
        }

        let _ = crate::platform::fast_win::set_hkcu_run("Open Antigravity", None);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_autostart_query_does_not_panic() {
        let _ = is_enabled();
        let _ = is_admin_enabled();
    }
}
