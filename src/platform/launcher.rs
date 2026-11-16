//! Запуск и автоматический запуск компонентов Antigravity (2.0 / IDE / CLI).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Целевой компонент для автоматического запуска при старте Open Antigravity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AutoLaunchTarget {
    #[default]
    None,
    Antigravity,
    Ide,
    Cli,
    Any,
}

impl AutoLaunchTarget {
    pub fn label(&self) -> &'static str {
        match self {
            Self::None => "Отключено",
            Self::Antigravity => "Antigravity 2.0",
            Self::Ide => "Antigravity IDE",
            Self::Cli => "Antigravity CLI (agy)",
            Self::Any => "Любая найденная",
        }
    }

    pub fn all() -> &'static [AutoLaunchTarget] {
        &[
            Self::None,
            Self::Antigravity,
            Self::Ide,
            Self::Cli,
            Self::Any,
        ]
    }
}

/// Находит исполняемый файл для указанной папки установки.
pub fn find_executable(install: &Path) -> Option<PathBuf> {
    if install.is_file() {
        return Some(install.to_path_buf());
    }

    #[cfg(target_os = "windows")]
    {
        // 1. Antigravity IDE
        let ide_candidates = [
            install.join("Antigravity IDE.exe"),
            install.join("antigravity-ide.exe"),
            install.join("bin").join("antigravity.cmd"),
        ];
        for cand in ide_candidates {
            if cand.is_file() {
                return Some(cand);
            }
        }

        // 2. Antigravity Desktop / 2.0
        let desktop_candidates = [
            install.join("Antigravity.exe"),
            install.join("antigravity.exe"),
            install.join("Antigravity-x64.exe"),
        ];
        for cand in desktop_candidates {
            if cand.is_file() {
                return Some(cand);
            }
        }

        // 3. Antigravity CLI
        let cli_candidates = [
            install.join("agy.exe"),
            install.join("bin").join("agy.exe"),
        ];
        for cand in cli_candidates {
            if cand.is_file() {
                return Some(cand);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let candidates = [
            install.join("Antigravity IDE"),
            install.join("antigravity-ide"),
            install.join("Antigravity"),
            install.join("antigravity"),
            install.join("antigravity-x64"),
            install.join("agy"),
            install.join("bin").join("antigravity"),
            install.join("bin").join("agy"),
        ];
        for cand in candidates {
            if cand.is_file() {
                return Some(cand);
            }
        }
    }

    None
}

/// Находит исполняемый файл и текстовую метку для заданного AutoLaunchTarget.
pub fn find_target(target: AutoLaunchTarget, manual_paths: &[PathBuf]) -> Option<(PathBuf, &'static str)> {
    if target == AutoLaunchTarget::None {
        return None;
    }

    let mut installs: Vec<PathBuf> = manual_paths.to_vec();
    for p in crate::discover_installs_fast() {
        if !installs.contains(&p) {
            installs.push(p);
        }
    }

    match target {
        AutoLaunchTarget::None => None,
        AutoLaunchTarget::Antigravity => {
            for inst in &installs {
                if crate::install_label(inst) == "Antigravity 2.0" {
                    if let Some(exe) = find_executable(inst) {
                        return Some((exe, "Antigravity 2.0"));
                    }
                }
            }
            #[cfg(target_os = "windows")]
            {
                if let Ok(local) = std::env::var("LOCALAPPDATA") {
                    let cand = PathBuf::from(&local).join("Programs").join("Antigravity").join("Antigravity.exe");
                    if cand.is_file() {
                        return Some((cand, "Antigravity 2.0"));
                    }
                }
            }
            None
        }
        AutoLaunchTarget::Ide => {
            for inst in &installs {
                if crate::install_label(inst) == "Antigravity IDE" {
                    if let Some(exe) = find_executable(inst) {
                        return Some((exe, "Antigravity IDE"));
                    }
                }
            }
            #[cfg(target_os = "windows")]
            {
                if let Ok(local) = std::env::var("LOCALAPPDATA") {
                    let cand = PathBuf::from(&local).join("Programs").join("Antigravity IDE").join("Antigravity IDE.exe");
                    if cand.is_file() {
                        return Some((cand, "Antigravity IDE"));
                    }
                }
            }
            None
        }
        AutoLaunchTarget::Cli => {
            for inst in &installs {
                if crate::install_label(inst) == "Antigravity CLI" {
                    if let Some(exe) = find_executable(inst) {
                        return Some((exe, "Antigravity CLI"));
                    }
                }
            }
            #[cfg(target_os = "windows")]
            {
                if let Ok(userprofile) = std::env::var("USERPROFILE") {
                    let cand = PathBuf::from(&userprofile).join(".local").join("bin").join("agy.exe");
                    if cand.is_file() {
                        return Some((cand, "Antigravity CLI"));
                    }
                }
                if let Ok(local) = std::env::var("LOCALAPPDATA") {
                    let cand = PathBuf::from(&local).join("agy").join("bin").join("agy.exe");
                    if cand.is_file() {
                        return Some((cand, "Antigravity CLI"));
                    }
                    let cand2 = PathBuf::from(&local).join("agy").join("agy.exe");
                    if cand2.is_file() {
                        return Some((cand2, "Antigravity CLI"));
                    }
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                if let Some(agy_dir) = crate::find_agy_dir() {
                    let cand = agy_dir.join("agy");
                    if cand.is_file() {
                        return Some((cand, "Antigravity CLI"));
                    }
                }
            }
            None
        }
        AutoLaunchTarget::Any => {
            find_target(AutoLaunchTarget::Antigravity, manual_paths)
                .or_else(|| find_target(AutoLaunchTarget::Ide, manual_paths))
                .or_else(|| find_target(AutoLaunchTarget::Cli, manual_paths))
        }
    }
}

/// Запускает целевой компонент по настройкам автозапуска.
pub fn launch_target(target: AutoLaunchTarget, manual_paths: &[PathBuf]) -> Result<String, String> {
    if target == AutoLaunchTarget::None {
        return Ok("Автозапуск отключён".to_string());
    }
    let (exe, label) = find_target(target, manual_paths)
        .ok_or_else(|| format!("Не удалось найти установку для {}", target.label()))?;
    launch(&exe, label)
}

/// Запускает конкретную установку по её корневому каталогу.
pub fn launch_install(install: &Path) -> Result<String, String> {
    let label = crate::install_label(install);
    let exe = find_executable(install)
        .ok_or_else(|| format!("В папке «{}» не найден исполняемый файл для {}", install.display(), label))?;
    launch(&exe, label)
}

/// Запускает исполняемый файл (GUI-приложение либо консольный CLI в терминале).
pub fn launch(exe_path: &Path, label: &str) -> Result<String, String> {
    if !exe_path.is_file() {
        return Err(format!("Исполняемый файл не найден: {}", exe_path.display()));
    }

    let is_cli = label.contains("CLI")
        || exe_path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.to_lowercase().starts_with("agy"));

    let port = crate::proxy::port();

    if is_cli {
        launch_cli_terminal(exe_path, port)
    } else {
        launch_gui_app(exe_path, port)
    }
}

fn launch_gui_app(exe_path: &Path, port: u16) -> Result<String, String> {
    let mut cmd = Command::new(exe_path);
    if let Some(dir) = exe_path.parent() {
        cmd.current_dir(dir);
    }

    if port > 0 {
        let proxy_url = format!("http://127.0.0.1:{}", port);
        cmd.env("HTTP_PROXY", &proxy_url);
        cmd.env("HTTPS_PROXY", &proxy_url);
        cmd.env("ALL_PROXY", &proxy_url);
        cmd.env("http_proxy", &proxy_url);
        cmd.env("https_proxy", &proxy_url);
        cmd.env("all_proxy", &proxy_url);
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x00000008;
        cmd.creation_flags(DETACHED_PROCESS);
    }

    cmd.spawn()
        .map_err(|e| format!("Не удалось запустить {}: {e}", exe_path.display()))?;

    let name = exe_path.file_name().and_then(|n| n.to_str()).unwrap_or("приложение");
    Ok(format!("Запущен «{}»", name))
}

fn launch_cli_terminal(exe_path: &Path, port: u16) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        let exe_str = exe_path.display().to_string();
        let ps_init = format!(
            "$env:HTTP_PROXY='http://127.0.0.1:{port}'; $env:HTTPS_PROXY='http://127.0.0.1:{port}'; $env:ALL_PROXY='http://127.0.0.1:{port}'; \
             Write-Host '===================================================' -ForegroundColor Cyan; \
             Write-Host '  Open Antigravity: Сессия Antigravity CLI (agy)   ' -ForegroundColor Green; \
             Write-Host '  HTTP_PROXY / HTTPS_PROXY -> http://127.0.0.1:{port}' -ForegroundColor Yellow; \
             Write-Host '===================================================' -ForegroundColor Cyan; \
             & '{exe}'",
            port = port,
            exe = exe_str.replace('\'', "''"),
        );
        Command::new("powershell.exe")
            .args(["-NoExit", "-Command", &ps_init])
            .spawn()
            .map_err(|e| format!("Не удалось запустить PowerShell для CLI: {e}"))?;
        Ok("Запущена терминальная сессия Antigravity CLI".to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let exe_str = exe_path.display().to_string();
        let bash_init = format!(
            "export HTTP_PROXY='http://127.0.0.1:{port}' HTTPS_PROXY='http://127.0.0.1:{port}' ALL_PROXY='http://127.0.0.1:{port}'; \
             echo -e '\\033[1;36m===================================================\\033[0m'; \
             echo -e '\\033[1;32m  Open Antigravity: Сессия Antigravity CLI (agy)   \\033[0m'; \
             echo -e '\\033[1;33m  HTTP_PROXY / HTTPS_PROXY -> http://127.0.0.1:{port}\\033[0m'; \
             echo -e '\\033[1;36m===================================================\\033[0m'; \
             exec \"{exe}\"",
            port = port,
            exe = exe_str.replace('"', "\\\""),
        );
        let spawned = Command::new("x-terminal-emulator")
            .args(["-e", "bash", "-c", &bash_init])
            .spawn()
            .or_else(|_| {
                Command::new("gnome-terminal")
                    .args(["--", "bash", "-c", &bash_init])
                    .spawn()
            })
            .or_else(|_| {
                Command::new("konsole")
                    .args(["-e", "bash", "-c", &bash_init])
                    .spawn()
            });
        spawned
            .map(|_| "Запущена терминальная сессия Antigravity CLI".to_string())
            .map_err(|e| format!("Не удалось запустить эмулятор терминала: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_launch_target_serde_roundtrip() {
        for target in AutoLaunchTarget::all() {
            let json = serde_json::to_string(target).unwrap();
            let parsed: AutoLaunchTarget = serde_json::from_str(&json).unwrap();
            assert_eq!(*target, parsed);
            assert!(!target.label().is_empty());
        }
    }

    #[test]
    fn test_find_executable_dummy_dir() {
        let temp = std::env::temp_dir().join(format!("oag_test_launcher_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp);

        #[cfg(target_os = "windows")]
        {
            let exe = temp.join("Antigravity.exe");
            let _ = std::fs::write(&exe, b"test");
            assert_eq!(find_executable(&temp), Some(exe.clone()));
            let _ = std::fs::remove_file(exe);

            let ide = temp.join("Antigravity IDE.exe");
            let _ = std::fs::write(&ide, b"test");
            assert_eq!(find_executable(&temp), Some(ide.clone()));
            let _ = std::fs::remove_file(ide);

            let agy = temp.join("agy.exe");
            let _ = std::fs::write(&agy, b"test");
            assert_eq!(find_executable(&temp), Some(agy.clone()));
            let _ = std::fs::remove_file(agy);
        }

        #[cfg(not(target_os = "windows"))]
        {
            let exe = temp.join("antigravity");
            let _ = std::fs::write(&exe, b"test");
            assert_eq!(find_executable(&temp), Some(exe.clone()));
            let _ = std::fs::remove_file(exe);
        }

        let _ = std::fs::remove_dir_all(&temp);
    }
}
