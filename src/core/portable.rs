//! Портативный режим: всё состояние — рядом с exe, машина остаётся чистой.
//!
//! Обычная сборка раскладывает состояние по системе: настройки и логи в
//! `%LOCALAPPDATA%\OpenAntigravity`, копия релея и плановая задача в `%ProgramData%`.
//! Для запуска с флешки это ровно наоборот: пользователь кладёт exe на сменный
//! носитель, запускает на любой машине и ожидает, что после извлечения флешки
//! на ней не останется ничего.
//!
//! Поэтому в портативном режиме:
//!
//! * все файлы состояния (`settings.json`, `upstream.txt`, `gate.json`, логи)
//!   живут в `data\` рядом с exe — один каталог, который можно стереть целиком;
//! * релей не «устанавливается»: это тот же exe, запущенный из той же папки с
//!   `--dns-forwarder` (см. `background.rs`), без плановой задачи и без копии в
//!   `%ProgramData%`. Автозапуск после перезагрузки — единственное, что требует
//!   установки, и им портативный режим сознательно жертвует;
//! * включается режим одним из способов:
//!   1. явный аргумент `--portable`;
//!   2. файл-маркер `portable.flag` рядом с exe;
//!   3. наличие `data\` (если это не установленная версия);
//!   4. вшитая переменная `OAG_PORTABLE` (если не установленная версия).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Файл-маркер портативного режима рядом с exe. Пустого достаточно.
const MARKER: &str = "portable.flag";

/// Подкаталог рядом с exe, в котором портативная сборка держит всё состояние.
const DATA_SUBDIR: &str = "data";

/// Ответ решается один раз за жизнь процесса и больше не спрашивается:
/// от него зависит путь к настройкам, который никто не ожидает увидеть
/// меняющимся посреди работы.
static PORTABLE: OnceLock<bool> = OnceLock::new();

/// Запущена ли эта сборка в портативном режиме.
pub fn enabled() -> bool {
    *PORTABLE.get_or_init(detect)
}

/// Moves the files of a stray `data\` folder into the system folder.
///
/// The folder is *moved*, not removed. It used to be removed outright, which
/// threw away the relay's log, its record and the file naming the live
/// language server - and on Linux nothing was moved at all, the folder was
/// simply deleted.
///
/// Where both copies exist the newer one wins, so a state left over from an
/// older build never overwrites a newer one. A file that cannot be moved
/// stays where it is, and the folder is removed only when it is empty: what
/// is left in it was not ours to throw away.
fn migrate_data_into(stray: &Path, system: &Path) {
    if !stray.is_dir() {
        return;
    }
    if std::fs::create_dir_all(system).is_err() {
        // Nowhere to put it: stranded is better than gone.
        return;
    }
    let Ok(entries) = std::fs::read_dir(stray) else {
        return;
    };
    for entry in entries.flatten() {
        let from = entry.path();
        let to = system.join(entry.file_name());
        if !from.is_file() {
            // A subfolder: it stays for whoever put it there.
            continue;
        }
        let take = match (modified(&from), modified(&to)) {
            (Some(a), Some(b)) => a > b,
            (Some(_), None) => true,
            _ => false,
        };
        if take && std::fs::copy(&from, &to).is_err() {
            // The copy did not land: the original stays where it is.
            continue;
        }
        // Either the newer copy was just written over the older one, or the
        // one there was already newer - in both cases this one has nothing
        // left to say.
        let _ = std::fs::remove_file(&from);
    }
    // Only an empty folder goes. `remove_dir` refuses a folder that still
    // holds anything, which is exactly the point.
    let _ = std::fs::remove_dir(stray);
}

/// When a file was last written.
fn modified(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// Where the state belongs when this build is not portable.
///
/// Computed without asking `enabled()`: `detect()` runs inside it, and the
/// question would come back here.
pub fn system_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()).join("OpenAntigravity")
    }
    #[cfg(not(target_os = "windows"))]
    {
        let base = std::env::var("XDG_DATA_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share")
            });
        base.join("openantigravity")
    }
}

fn detect() -> bool {
    // 1. Флаг запуска из командной строки имеет наивысший приоритет.
    if std::env::args().any(|a| a == "--portable") {
        return true;
    }

    // 2. Явный файл-маркер portable.flag рядом с exe.
    if let Some(dir) = exe_dir() {
        if marker_present(Some(&dir)) {
            return true;
        }
    }

    // 3. Проверка на стандартную установленную версию:
    //    Если exe лежит в системном каталоге установки (%LOCALAPPDATA%\Programs\OpenAntigravity или Program Files)
    //    ИЛИ рядом есть unins000.exe / unins000.dat (деинсталлятор Inno Setup),
    //    то это ГАРАНТИРОВАННО установленная версия (даже если бинарник был скачан из zip-архива
    //    обновления и скомпилирован с OAG_PORTABLE=1).
    if is_installed_location() {
        clean_installed_spurious_data();
        return false;
    }

    // 4. Каталог data/ рядом с exe (для реальных портативных запусков).
    if let Some(dir) = exe_dir() {
        if dir.join(DATA_SUBDIR).is_dir() {
            return true;
        }
    }

    // 5. Вшито при сборке: OAG_PORTABLE.
    option_env!("OAG_PORTABLE").is_some()
}

/// Проверяет, является ли текущее расположение приложения системной установкой.
pub fn is_installed_location() -> bool {
    let Some(dir) = exe_dir() else { return false };

    // Признак 1: Деинсталлятор Inno Setup рядом с бинарником
    if dir.join("unins000.exe").is_file() || dir.join("unins000.dat").is_file() {
        return true;
    }

    // Признак 2: Стандартные системные папки установки Windows
    #[cfg(target_os = "windows")]
    {
        // setup.exe кладёт релей именно сюда. Проверки LOCALAPPDATA\Programs и
        // Program Files его не покрывают: релей видел рядом data\, решал, что
        // он портативный, и читал настройки из ProgramData\data, пока окно
        // писало их в LOCALAPPDATA. Отчего рабочий upstream-прокси выглядел
        // «не настроенным» и все запросы уходили в заблокированный регион.
        if let (Some(exe_path), Ok(program_data)) =
            (std::env::current_exe().ok(), std::env::var("PROGRAMDATA"))
        {
            let install_dir = PathBuf::from(program_data).join("OpenAntigravity");
            if under(&exe_path, &install_dir) {
                return true;
            }
        }
        if let (Some(exe_path), Ok(local_app_data)) = (std::env::current_exe().ok(), std::env::var("LOCALAPPDATA")) {
            let install_dir = PathBuf::from(local_app_data).join("Programs").join("OpenAntigravity");
            let exe_lower = exe_path.to_string_lossy().to_ascii_lowercase();
            let install_lower = install_dir.to_string_lossy().to_ascii_lowercase();
            if exe_lower.starts_with(&install_lower) {
                return true;
            }
        }
        if let (Some(exe_path), Ok(prog_files)) = (std::env::current_exe().ok(), std::env::var("ProgramFiles")) {
            let exe_lower = exe_path.to_string_lossy().to_ascii_lowercase();
            let pf_lower = prog_files.to_ascii_lowercase();
            if exe_lower.starts_with(&pf_lower) {
                return true;
            }
        }
        if let (Some(exe_path), Ok(prog_files_x86)) = (std::env::current_exe().ok(), std::env::var("ProgramFiles(x86)")) {
            let exe_lower = exe_path.to_string_lossy().to_ascii_lowercase();
            let pfx86_lower = prog_files_x86.to_ascii_lowercase();
            if exe_lower.starts_with(&pfx86_lower) {
                return true;
            }
        }
    }

    // Признак 3: Системные папки Linux
    #[cfg(not(target_os = "windows"))]
    {
        let dir_str = dir.to_string_lossy();
        if dir_str.starts_with("/usr/") || dir_str.starts_with("/opt/") {
            return true;
        }
    }

    false
}

/// Case-insensitive prefix check for Windows paths.
#[cfg(target_os = "windows")]
fn under(path: &Path, root: &Path) -> bool {
    path.to_string_lossy()
        .to_ascii_lowercase()
        .starts_with(&root.to_string_lossy().to_ascii_lowercase())
}

/// Для установленной версии: если при обновлении из zip была ошибочно создана папка data/,
/// переносим её содержимое в системную папку %LOCALAPPDATA%\OpenAntigravity.
///
/// Переносим, а не удаляем: удаление выбрасывало журнал релея, его запись
/// состояния и файл с пидом живого language server — и на Linux не переносилось
/// ничего вовсе, папка просто исчезала.
fn clean_installed_spurious_data() {
    let Some(dir) = exe_dir() else { return };
    migrate_data_into(&dir.join(DATA_SUBDIR), &system_data_dir());
}

/// Каталог, в котором лежит запущенный exe. На Windows это то же, что
/// `std::env::current_exe().parent()`, без раскрытия симлинков сверх того,
/// что уже делает сама функция.
/// В среде AppImage на Linux возвращает каталог самого файла `.AppImage`,
/// а не временную read-only точку монтирования squashfs.
pub fn exe_dir() -> Option<PathBuf> {
    if let Ok(appimage) = std::env::var("APPIMAGE") {
        let p = PathBuf::from(appimage);
        if let Some(parent) = p.parent() {
            return Some(parent.to_path_buf());
        }
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
}

/// Куда портативная сборка кладёт состояние. `None` — режим не активен, и
/// вызывающий должен пользоваться обычными системными путями.
pub fn data_dir() -> Option<PathBuf> {
    if enabled() {
        if let Ok(appimage) = std::env::var("APPIMAGE") {
            let p = PathBuf::from(appimage);
            if let Some(parent) = p.parent() {
                let candidate = parent.join(DATA_SUBDIR);
                if candidate.is_dir() || is_writable_dir(parent) {
                    return Some(candidate);
                }
                // Фолбэк для AppImage, запущенного из защищенного от записи каталога (/opt, CD-ROM)
                if let Some(xdg) = std::env::var("XDG_DATA_HOME").ok().filter(|s| !s.is_empty()) {
                    return Some(PathBuf::from(xdg).join("openantigravity").join(DATA_SUBDIR));
                }
                if let Some(home) = std::env::var("HOME").ok().filter(|s| !s.is_empty()) {
                    return Some(PathBuf::from(home).join(".local/share/openantigravity").join(DATA_SUBDIR));
                }
            }
        }
        exe_dir().map(|d| d.join(DATA_SUBDIR))
    } else {
        None
    }
}

fn is_writable_dir(dir: &Path) -> bool {
    let test_file = dir.join(format!(".oag_wprobe_{}", std::process::id()));
    if std::fs::write(&test_file, b"").is_ok() {
        let _ = std::fs::remove_file(test_file);
        true
    } else {
        false
    }
}

fn marker_present(dir: Option<&Path>) -> bool {
    dir.is_some_and(|d| d.join(MARKER).exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The state of an installed build is *moved* out of the folder next to
    /// its executable - not thrown away.
    ///
    /// The regression this pins: the folder was removed outright, which threw
    /// away the relay's log, its record and the file naming the live language
    /// server. On Linux nothing was moved at all - the folder was simply
    /// deleted.
    ///
    /// Measured on the machine this was written for: an installed copy with
    /// `data\` beside it lost that folder on the first run of the new build.
    #[test]
    fn the_state_of_an_installed_build_is_moved_and_not_thrown_away() {
        let root = std::env::temp_dir().join(format!("oag_migrate_{}", std::process::id()));
        let stray = root.join("data");
        let system = root.join("system");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&stray).unwrap();

        // What the relay keeps there.
        std::fs::write(stray.join("forwarder.log"), b"log").unwrap();
        std::fs::write(stray.join("gate.json"), b"{}").unwrap();
        std::fs::write(stray.join("settings.json"), b"{}").unwrap();

        migrate_data_into(&stray, &system);

        for name in ["forwarder.log", "gate.json", "settings.json"] {
            assert!(
                system.join(name).is_file(),
                "{name} must be moved, not deleted"
            );
            assert!(
                !stray.join(name).exists(),
                "{name} must not be left behind twice"
            );
        }
        assert!(
            !stray.exists(),
            "a folder left empty by the move is removed"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Where both copies exist, the newer one wins - and the older stray copy
    /// does not overwrite it.
    #[test]
    fn an_older_stray_file_does_not_overwrite_a_newer_one() {
        let root = std::env::temp_dir().join(format!("oag_migrate_older_{}", std::process::id()));
        let stray = root.join("data");
        let system = root.join("system");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&stray).unwrap();
        std::fs::create_dir_all(&system).unwrap();

        std::fs::write(stray.join("settings.json"), b"old").unwrap();
        std::fs::write(system.join("settings.json"), b"new").unwrap();
        let now = std::time::SystemTime::now();
        set_modified(&stray.join("settings.json"), now - std::time::Duration::from_secs(600));
        set_modified(&system.join("settings.json"), now);

        migrate_data_into(&stray, &system);

        assert_eq!(
            std::fs::read_to_string(system.join("settings.json")).unwrap(),
            "new",
            "the newer copy must win"
        );
        assert!(
            !stray.join("settings.json").exists(),
            "the superseded copy is not left behind"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// And the other way round: a newer stray copy replaces the older one in
    /// the system folder.
    #[test]
    fn a_newer_stray_file_replaces_an_older_one() {
        let root = std::env::temp_dir().join(format!("oag_migrate_newer_{}", std::process::id()));
        let stray = root.join("data");
        let system = root.join("system");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&stray).unwrap();
        std::fs::create_dir_all(&system).unwrap();

        std::fs::write(stray.join("settings.json"), b"new").unwrap();
        std::fs::write(system.join("settings.json"), b"old").unwrap();
        let now = std::time::SystemTime::now();
        set_modified(&stray.join("settings.json"), now);
        set_modified(&system.join("settings.json"), now - std::time::Duration::from_secs(600));

        migrate_data_into(&stray, &system);

        assert_eq!(
            std::fs::read_to_string(system.join("settings.json")).unwrap(),
            "new",
            "the newer copy must win"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A folder that cannot be emptied stays where it is.
    ///
    /// What is left in it was not ours to throw away: a subfolder, or a file
    /// that could not be moved.
    #[test]
    fn a_folder_that_cannot_be_emptied_stays() {
        let root = std::env::temp_dir().join(format!("oag_migrate_kept_{}", std::process::id()));
        let stray = root.join("data");
        let system = root.join("system");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(stray.join("nested")).unwrap();
        std::fs::write(stray.join("nested").join("keep.txt"), b"keep").unwrap();

        migrate_data_into(&stray, &system);

        assert!(
            stray.join("nested").join("keep.txt").is_file(),
            "what could not be moved must survive"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    fn set_modified(path: &Path, when: std::time::SystemTime) {
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(when).ok();
    }

    #[test]
    fn a_marker_next_to_the_exe_enables_portable_mode() {
        let dir = std::env::temp_dir().join("oag_portable_marker_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!marker_present(Some(dir.as_path())), "маркера ещё нет");
        std::fs::write(dir.join(MARKER), b"").unwrap();
        assert!(marker_present(Some(dir.as_path())));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn without_a_directory_there_is_no_marker() {
        assert!(!marker_present(None));
    }

    /// The installed relay lives in %ProgramData%\OpenAntigravity - the same
    /// exe the setup puts there and the watchdog task runs. Without this the
    /// relay saw data\ beside it and took the portable branch.
    #[cfg(target_os = "windows")]
    #[test]
    fn programdata_relay_is_an_installed_location() {
        let root = Path::new(r"C:\ProgramData\OpenAntigravity");
        assert!(under(
            &root.join("OpenAntigravityRelay.exe"),
            root
        ));
        assert!(!under(
            Path::new(r"C:\Users\me\AppData\Local\Programs\OpenAntigravity\OpenAntigravity.exe"),
            root
        ));
    }

    #[test]
    fn uninstaller_detects_as_installed_location() {
        let dir = std::env::temp_dir().join(format!("oag_test_unins_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let unins = dir.join("unins000.exe");
        let _ = std::fs::write(&unins, b"");

        assert!(dir.join("unins000.exe").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
