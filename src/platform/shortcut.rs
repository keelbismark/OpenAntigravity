//! Кроссплатформенное создание ярлыков приложения (Windows + Linux).
//!
//! Windows: Создание ярлыка .lnk на Рабочем столе с указанием рабочей папки и иконки.
//! Linux: Создание файла .desktop на Рабочем столе (~/Desktop) и в меню приложений
//! (~/.local/share/applications/) с правами на запуск и доверием KDE/GNOME.

use std::path::Path;
#[cfg(not(target_os = "windows"))]
use std::path::PathBuf;

/// Создает ярлык запуска для текущей установки Open Antigravity.
pub fn create_shortcuts() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        create_windows_shortcut()
    }
    #[cfg(not(target_os = "windows"))]
    {
        create_linux_shortcuts()
    }
}

#[cfg(target_os = "windows")]
fn create_windows_shortcut() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Не удалось определить путь к exe: {}", e))?;
    let exe_dir = exe.parent().unwrap_or_else(|| Path::new("."));

    let script = format!(
        "$ws = New-Object -ComObject WScript.Shell; \
         $desk = [Environment]::GetFolderPath('Desktop'); \
         $s = $ws.CreateShortcut(\"$desk\\Open Antigravity.lnk\"); \
         $s.TargetPath = '{}'; \
         $s.WorkingDirectory = '{}'; \
         $s.IconLocation = '{},0'; \
         $s.Description = 'Open Antigravity'; \
         $s.Save()",
        exe.display(),
        exe_dir.display(),
        exe.display()
    );

    if let Some(out) = crate::utils::powershell_within(&script, std::time::Duration::from_secs(10)) {
        if out.status.success() {
            Ok("Ярлык «Open Antigravity» успешно создан на Рабочем столе.".to_string())
        } else {
            let err = String::from_utf8_lossy(&out.stderr);
            Err(format!("PowerShell ошибка создания ярлыка: {}", err.trim()))
        }
    } else {
        Err("Не удалось запустить PowerShell для создания ярлыка.".to_string())
    }
}

#[cfg(not(target_os = "windows"))]
fn create_linux_shortcuts() -> Result<String, String> {
    let exe = crate::utils::app_executable()?;
    let exe_dir = exe.parent().unwrap_or_else(|| Path::new("."));

    // Если рядом есть launch.sh, запускаем через него (там sudo-guard и noexec фолбэк)
    let launch_sh = exe_dir.join("launch.sh");
    let exec_cmd = if launch_sh.exists() {
        format!("bash \"{}\"", launch_sh.display())
    } else {
        format!("\"{}\"", exe.display())
    };

    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());

    // Поиск иконки (icon.png рядом, icon.ico, либо сохранение встроенной иконки)
    let icon_png = exe_dir.join("icon.png");
    let icon_ico = exe_dir.join("icon.ico");
    let fallback_icon = PathBuf::from(&home)
        .join(".local")
        .join("share")
        .join("openantigravity")
        .join("icon.png");

    let icon_path = if icon_png.exists() {
        icon_png.display().to_string()
    } else if icon_ico.exists() {
        icon_ico.display().to_string()
    } else {
        if !fallback_icon.exists() {
            if let Some(parent) = fallback_icon.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&fallback_icon, include_bytes!("../../assets/icon.png"));
        }
        if fallback_icon.exists() {
            fallback_icon.display().to_string()
        } else {
            "utilities-terminal".to_string()
        }
    };

    let desktop_content = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Open Antigravity\n\
         Comment=Open Antigravity 2.0 / IDE / CLI\n\
         Exec={}\n\
         Path={}\n\
         Icon={}\n\
         Terminal=false\n\
         Categories=Utility;Development;\n\
         StartupNotify=true\n",
        exec_cmd,
        exe_dir.display(),
        icon_path
    );

    let mut created_paths = Vec::new();

    // 1. Меню приложений (~/.local/share/applications)
    let apps_dir = PathBuf::from(&home).join(".local").join("share").join("applications");
    let _ = std::fs::create_dir_all(&apps_dir);
    let _ = std::fs::remove_file(apps_dir.join("ag-unlocker.desktop"));
    let app_shortcut = apps_dir.join("open-antigravity.desktop");
    if std::fs::write(&app_shortcut, &desktop_content).is_ok() {
        make_executable(&app_shortcut);
        created_paths.push("меню приложений".to_string());
    }

    // 2. Рабочий стол пользователя (Desktop)
    let desktop_dir = get_linux_desktop_dir(&home);
    if desktop_dir.is_dir() {
        let _ = std::fs::remove_file(desktop_dir.join("ag-unlocker.desktop"));
        let desk_shortcut = desktop_dir.join("open-antigravity.desktop");
        if std::fs::write(&desk_shortcut, &desktop_content).is_ok() {
            make_executable(&desk_shortcut);
            // Для KDE Plasma: пометить файл доверенным
            let _ = std::process::Command::new("gio")
                .args(["set", &desk_shortcut.to_string_lossy(), "metadata::trusted", "true"])
                .output();
            created_paths.push("Рабочий стол".to_string());
        }
    }

    // Обновляем базу данных .desktop файлов
    let _ = std::process::Command::new("update-desktop-database")
        .arg(&apps_dir)
        .output();

    if created_paths.is_empty() {
        Err("Не удалось сохранить файл .desktop ни в одну из папок.".to_string())
    } else {
        Ok(format!(
            "Ярлык «Open Antigravity» создан: {}.",
            created_paths.join(" и ")
        ))
    }
}

#[cfg(not(target_os = "windows"))]
fn get_linux_desktop_dir(home: &str) -> PathBuf {
    if let Ok(out) = std::process::Command::new("xdg-user-dir").arg("DESKTOP").output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                let p = PathBuf::from(s);
                if p.is_dir() {
                    return p;
                }
            }
        }
    }
    PathBuf::from(home).join("Desktop")
}

#[cfg(not(target_os = "windows"))]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(perms.mode() | 0o755);
        let _ = std::fs::set_permissions(path, perms);
    }
}

