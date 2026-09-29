//! Кроссплатформенное создание ярлыков приложения (Windows + Linux).
//!
//! Windows: Создание ярлыка .lnk на Рабочем столе с указанием рабочей папки и иконки.
//! Linux: Создание файла .desktop на Рабочем столе (~/Desktop) и в меню приложений
//! (~/.local/share/applications/) с правами на запуск и доверием KDE/GNOME.

use std::path::{Path, PathBuf};

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

    // Поиск иконки (icon.png рядом, либо icon.ico)
    let icon_png = exe_dir.join("icon.png");
    let icon_ico = exe_dir.join("icon.ico");
    let icon_path = if icon_png.exists() {
        icon_png.display().to_string()
    } else if icon_ico.exists() {
        icon_ico.display().to_string()
    } else {
        "utilities-terminal".to_string()
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

    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
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

/// Добавляет Open Antigravity в библиотеку Steam (shortcuts.vdf).
/// Работает на Steam Deck (SteamOS), Linux ПК и Windows.
pub fn add_to_steam() -> Result<String, String> {
    let exe = crate::utils::app_executable()?;
    let exe_dir = exe.parent().unwrap_or_else(|| Path::new("."));

    #[cfg(target_os = "windows")]
    let (exec_path, start_dir) = (
        format!("\"{}\"", exe.display()),
        format!("\"{}\"", exe_dir.display()),
    );

    #[cfg(not(target_os = "windows"))]
    let (exec_path, start_dir) = {
        let launch_sh = exe_dir.join("launch.sh");
        if launch_sh.exists() {
            (
                format!("\"{}\"", launch_sh.display()),
                format!("\"{}\"", exe_dir.display()),
            )
        } else {
            (
                format!("\"{}\"", exe.display()),
                format!("\"{}\"", exe_dir.display()),
            )
        }
    };

    let icon_path = {
        let p_png = exe_dir.join("icon.png");
        let p_ico = exe_dir.join("icon.ico");
        if p_png.exists() {
            p_png.display().to_string()
        } else if p_ico.exists() {
            p_ico.display().to_string()
        } else {
            String::new()
        }
    };

    let vdf_paths = find_all_steam_shortcuts_files();
    if vdf_paths.is_empty() {
        return Err("Каталог Steam userdata не найден. Убедитесь, что Steam установлен и выполнен вход.".to_string());
    }

    let mut added_count = 0;
    for vdf_file in vdf_paths {
        if append_steam_shortcut(&vdf_file, "Open Antigravity", &exec_path, &start_dir, &icon_path)? {
            added_count += 1;
        }
    }

    if added_count > 0 {
        Ok(format!(
            "Ярлык успешно добавлен в Steam (профилей: {}). Перезапустите Steam, чтобы увидеть его в библиотеке.",
            added_count
        ))
    } else {
        Ok("Ярлык «Open Antigravity» уже присутствует в библиотеке Steam.".to_string())
    }
}

fn find_all_steam_shortcuts_files() -> Vec<PathBuf> {
    let mut out = Vec::new();

    #[cfg(target_os = "windows")]
    let base_dirs = {
        let mut dirs = Vec::new();
        if let Ok(prog_files) = std::env::var("ProgramFiles(x86)") {
            dirs.push(PathBuf::from(prog_files).join("Steam").join("userdata"));
        }
        if let Ok(prog_files) = std::env::var("ProgramFiles") {
            dirs.push(PathBuf::from(prog_files).join("Steam").join("userdata"));
        }
        dirs
    };

    #[cfg(not(target_os = "windows"))]
    let base_dirs = {
        let home = std::env::var("HOME").unwrap_or_default();
        vec![
            PathBuf::from(&home).join(".local").join("share").join("Steam").join("userdata"),
            PathBuf::from(&home).join(".steam").join("steam").join("userdata"),
        ]
    };

    for base in base_dirs {
        if let Ok(entries) = std::fs::read_dir(base) {
            for entry in entries.flatten() {
                let user_dir = entry.path();
                if user_dir.is_dir() {
                    let config_dir = user_dir.join("config");
                    let shortcuts_vdf = config_dir.join("shortcuts.vdf");
                    if config_dir.is_dir() && !out.contains(&shortcuts_vdf) {
                        out.push(shortcuts_vdf);
                    }
                }
            }
        }
    }

    out
}

fn append_steam_shortcut(
    vdf_path: &Path,
    app_name: &str,
    exe_cmd: &str,
    start_dir: &str,
    icon: &str,
) -> Result<bool, String> {
    let mut data = if vdf_path.exists() {
        std::fs::read(vdf_path).map_err(|e| format!("Не удалось прочитать {}: {}", vdf_path.display(), e))?
    } else {
        // Создаем пустой VDF
        let mut init = Vec::new();
        init.push(0x00);
        init.extend_from_slice(b"shortcuts\x00");
        init.extend_from_slice(b"\x08\x08");
        init
    };

    // Проверяем, нет ли уже такого ярлыка в shortcuts.vdf
    if let Some(pos) = data.windows(app_name.len()).position(|w| w == app_name.as_bytes()) {
        if pos > 0 && data[pos - 1] == 0 {
            return Ok(false); // Уже добавлен
        }
    }

    // Создаем бэкап перед изменением
    if vdf_path.exists() {
        let bak = vdf_path.with_extension("vdf.bak");
        let _ = std::fs::copy(vdf_path, bak);
    }

    // Находим последний индекс
    let mut max_index = -1i32;
    let mut idx = 0;
    while idx + 1 < data.len() {
        if data[idx] == 0x00 {
            let start = idx + 1;
            if let Some(end) = data[start..].iter().position(|&b| b == 0x00) {
                if let Ok(s) = std::str::from_utf8(&data[start..start + end]) {
                    if let Ok(num) = s.parse::<i32>() {
                        if num > max_index {
                            max_index = num;
                        }
                    }
                }
                idx = start + end + 1;
                continue;
            }
        }
        idx += 1;
    }

    let next_index = max_index + 1;

    // Ищем завершающие байты \x08\x08
    let insert_pos = if data.ends_with(b"\x08\x08") {
        data.len() - 2
    } else if data.ends_with(b"\x08") {
        data.len() - 1
    } else {
        data.len()
    };

    let mut entry = Vec::new();
    // \x00{next_index}\x00
    entry.push(0x00);
    entry.extend_from_slice(next_index.to_string().as_bytes());
    entry.push(0x00);

    // appid: int32 (простой стабильный hash)
    let appid = (simple_hash(exe_cmd) | 0x80000000) as i32;
    write_vdf_int(&mut entry, "appid", appid);
    write_vdf_string(&mut entry, "AppName", app_name);
    write_vdf_string(&mut entry, "Exe", exe_cmd);
    write_vdf_string(&mut entry, "StartDir", start_dir);
    write_vdf_string(&mut entry, "icon", icon);
    write_vdf_string(&mut entry, "ShortcutPath", "");
    write_vdf_string(&mut entry, "LaunchOptions", "");
    write_vdf_int(&mut entry, "IsHidden", 0);
    write_vdf_int(&mut entry, "AllowDesktopConfig", 1);
    write_vdf_int(&mut entry, "AllowOverlay", 1);
    write_vdf_int(&mut entry, "OpenVR", 0);
    write_vdf_int(&mut entry, "Devkit", 0);
    write_vdf_string(&mut entry, "DevkitGameID", "");
    write_vdf_int(&mut entry, "DevkitOverrideAppID", 0);
    write_vdf_int(&mut entry, "LastPlayTime", 0);
    write_vdf_string(&mut entry, "FlatpakAppID", "");

    // tags: \x00tags\x00\x08
    entry.push(0x00);
    entry.extend_from_slice(b"tags\x00\x08");

    // конец секции ярлыка: \x08
    entry.push(0x08);

    data.splice(insert_pos..insert_pos, entry);

    // Убеждаемся, что в конце стоят \x08\x08
    if !data.ends_with(b"\x08\x08") {
        data.push(0x08);
        data.push(0x08);
    }

    std::fs::write(vdf_path, data)
        .map_err(|e| format!("Не удалось сохранить {}: {}", vdf_path.display(), e))?;

    Ok(true)
}

fn write_vdf_string(buf: &mut Vec<u8>, key: &str, val: &str) {
    buf.push(0x01);
    buf.extend_from_slice(key.as_bytes());
    buf.push(0x00);
    buf.extend_from_slice(val.as_bytes());
    buf.push(0x00);
}

fn write_vdf_int(buf: &mut Vec<u8>, key: &str, val: i32) {
    buf.push(0x02);
    buf.extend_from_slice(key.as_bytes());
    buf.push(0x00);
    buf.extend_from_slice(&val.to_le_bytes());
}

fn simple_hash(s: &str) -> u32 {
    let mut h = 0x811c9dc5u32;
    for b in s.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}
