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
//! * включается режим одним из трёх способов, по убыванию приоритета:
//!   1. при сборке была задана переменная `AG_PORTABLE` (вшито в билд);
//!   2. exe запущен с флагом `--portable`;
//!   3. рядом с exe лежит пустой файл-маркер `portable.flag`.

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

fn detect() -> bool {
    // 1. Вшито при сборке: само наличие переменной включает режим, значение
    //    не читается — так правило «есть значит да» не приходится объяснять.
    if option_env!("AG_PORTABLE").is_some() {
        return true;
    }
    // 2. Флаг запуска.
    if std::env::args().any(|a| a == "--portable") {
        return true;
    }
    // 3. Маркер рядом с exe.
    marker_present(exe_dir().as_deref())
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
    let test_file = dir.join(format!(".ag_wprobe_{}", std::process::id()));
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

    #[test]
    fn a_marker_next_to_the_exe_enables_portable_mode() {
        let dir = std::env::temp_dir().join("ag_portable_marker_test");
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
}
