// The GUI is the product; a console subsystem binary would put a black window
// on screen behind it. The CLI flags that still print (--about/--version) and
// the background modes attach to the parent console instead - see attach_console.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
// The Windows build is the shipping platform and is fully linted. The Linux port
// is partial (phase 1: the client patch; the NRPT/relay/systemd layer is stubbed,
// P7 phase 5), so a large Windows-only surface is legitimately dead code there.
// Silence exactly that noise on non-Windows targets, and only there, so a real
// dead symbol on Windows is still caught.
#![cfg_attr(
    not(target_os = "windows"),
    allow(dead_code, unused_imports, unused_variables, unused_mut)
)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub mod canary;
pub mod core;
pub mod gui;
pub mod network;
pub mod patcher;
pub mod platform;
pub mod tui;

// Domain-level re-exports for crate root resolution
pub use self::core::{ls_log, ops, portable, settings, update, utils};
pub use self::network::{
    dns, dns_client, dns_forwarder, doh, egress, endpoint, gate, hosts as hosts_pin,
    loopback, net, proxy, resolvers, routes, upstream,
};
pub use self::patcher::{asar, binary as patch_binary, ide as patch_ide, watchdog};
pub use self::platform::{
    autostart, diag, elevate, health, notify, portcheck, service as background, shortcut,
    single_instance,
};

use self::patcher::asar::extract_asar;
use self::patcher::binary::patch_all_binaries;
use self::patcher::ide::{is_new_desktop_architecture, patch_desktop, patch_extension_js, patch_ide};

pub fn clean_input_path(input: &str) -> String {
    let mut s = input.trim();
    if ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
        && s.len() >= 2
    {
        s = &s[1..s.len() - 1];
    }
    s = s.trim();
    s = s.trim_matches('"').trim_matches('\'').trim();
    s.to_string()
}

fn is_install_root(path: &Path) -> bool {
    if !path.exists() || !path.is_dir() {
        return false;
    }

    let path_str = path.to_string_lossy().to_lowercase();
    if path_str == "c:\\windows"
        || path_str.starts_with("c:\\windows\\")
        || path_str.contains("\\windows\\system32")
        || path_str.contains("\\windows\\syswow64")
    {
        return false;
    }

    let resources = path.join("resources");
    if resources.is_dir()
        && (resources.join("app.asar").exists()
            || resources.join("app").exists()
            || resources.join("bin").exists())
    {
        return true;
    }
    // `is_file`, not `exists`: `%LOCALAPPDATA%\agy` is the CLI's *directory*, and
    // an `exists()` check there made the parent-walk treat `%LOCALAPPDATA%` itself
    // as an install root. A launcher/CLI is always a file.
    if path.join("agy.exe").is_file() || path.join("agy").is_file() {
        return true;
    }
    if path.join("Antigravity.exe").is_file()
        || path.join("Antigravity IDE.exe").is_file()
        || path.join("antigravity.exe").is_file()
    {
        return true;
    }
    // Linux/macOS launcher names (no extension).
    #[cfg(not(target_os = "windows"))]
    if path.join("antigravity").is_file()
        || path.join("Antigravity").is_file()
        || path.join("antigravity-ide").is_file()
    {
        return true;
    }
    if path.join("out").join("main.js").exists() || path.join("dist").join("main.js").exists() {
        return true;
    }
    false
}

pub fn resolve_install_root(raw: &Path) -> Option<PathBuf> {
    let target = fs::canonicalize(raw).unwrap_or_else(|_| raw.to_path_buf());
    let mut p = target;

    if p.is_file() {
        if let Some(parent) = p.parent() {
            p = parent.to_path_buf();
        }
    }

    if !p.exists() {
        return None;
    }

    if is_install_root(&p) {
        return Some(p);
    }

    let mut current = p.clone();
    for _ in 0..4 {
        if let Some(parent) = current.parent() {
            if is_install_root(parent) {
                return Some(parent.to_path_buf());
            }
            current = parent.to_path_buf();
        } else {
            break;
        }
    }

    let subfolder_candidates = [
        "Antigravity IDE",
        "Antigravity",
        "Antigravity-x64",
        "antigravity-x64",
        "antigravity",
        "antigravity-ide",
        "agy",
        "Programs\\Antigravity IDE",
        "Programs\\Antigravity",
        "resources",
    ];
    for sub in subfolder_candidates {
        let candidate = p.join(sub);
        if is_install_root(&candidate) {
            return Some(candidate);
        }
    }

    None
}

/// The fixed install locations, before any resolution. Kept separate from
/// `find_all_installs` so the watchdog can enumerate installs without the
/// PowerShell registry scan - spawning PowerShell on a timer inside the
/// background relay would be both wasteful and a stray-window risk.
#[cfg(target_os = "windows")]
fn standard_install_candidates() -> Vec<PathBuf> {
    let local_appdata = env::var("LOCALAPPDATA").unwrap_or_default();
    let prog_files = env::var("PROGRAMFILES").unwrap_or_default();
    let prog_files_x86 = env::var("PROGRAMFILES(X86)").unwrap_or_default();
    let userprofile = env::var("USERPROFILE").unwrap_or_default();

    let mut v = vec![
        PathBuf::from(&local_appdata)
            .join("Programs")
            .join("Antigravity"),
        PathBuf::from(&local_appdata)
            .join("Programs")
            .join("Antigravity IDE"),
        PathBuf::from(&local_appdata)
            .join("Programs")
            .join("Antigravity-x64"),
        PathBuf::from(&prog_files).join("Antigravity"),
        PathBuf::from(&prog_files).join("Antigravity IDE"),
        PathBuf::from(&prog_files).join("Antigravity-x64"),
        PathBuf::from(&prog_files_x86).join("Antigravity"),
        PathBuf::from(&prog_files_x86).join("Antigravity IDE"),
        PathBuf::from(&local_appdata).join("Antigravity"),
        PathBuf::from(&local_appdata).join("Antigravity IDE"),
        PathBuf::from(&local_appdata).join("agy").join("bin"),
        PathBuf::from(&local_appdata).join("agy"),
    ];

    if !userprofile.is_empty() {
        let u = PathBuf::from(&userprofile);
        for folder in ["Downloads", "Desktop", "Applications", "Programs"] {
            for name in ["Antigravity", "Antigravity IDE", "Antigravity-x64"] {
                v.push(u.join(folder).join(name));
            }
        }
        v.push(u.join("Antigravity"));
        v.push(u.join("Antigravity IDE"));
        v.push(u.join("Antigravity-x64"));
    }

    for drive in ["C:\\", "D:\\", "E:\\"] {
        v.push(PathBuf::from(drive).join("Antigravity"));
        v.push(PathBuf::from(drive).join("Antigravity IDE"));
        v.push(PathBuf::from(drive).join("Antigravity-x64"));
    }

    v
}

/// The Linux equivalents. Antigravity ships as an Electron/VS Code fork, which on
/// Linux lands in one of the system prefixes (`.deb` → `/usr/share` or `/opt`;
/// tarball → `/opt`, `~/Downloads` or under the home dir) with the language server at
/// `resources/app/extensions/antigravity/bin/` or `resources/bin/`.
#[cfg(not(target_os = "windows"))]
fn standard_install_candidates() -> Vec<PathBuf> {
    let home = env::var("HOME").unwrap_or_default();
    let mut v: Vec<PathBuf> = Vec::new();
    // System-wide install roots, all common capitalisations and packagings.
    for base in ["/opt", "/usr/share", "/usr/lib", "/usr/local/share"] {
        for name in [
            "Antigravity",
            "Antigravity IDE",
            "Antigravity-x64",
            "antigravity",
            "antigravity-ide",
            "antigravity-x64",
        ] {
            v.push(PathBuf::from(base).join(name));
        }
    }
    // Per-user installs (tarballs, Downloads, Desktop, user applications, `agy` CLI).
    if !home.is_empty() {
        let h = PathBuf::from(&home);
        for folder in ["Downloads", "Desktop", "Applications", "Programs", "Apps", "Software"] {
            for name in [
                "Antigravity",
                "Antigravity IDE",
                "Antigravity-x64",
                "antigravity",
                "antigravity-ide",
                "antigravity-x64",
            ] {
                v.push(h.join(folder).join(name));
            }
        }
        v.push(h.join(".local/share/Antigravity"));
        v.push(h.join(".local/share/Antigravity IDE"));
        v.push(h.join(".local/share/antigravity"));
        v.push(h.join(".local/share/antigravity-ide"));
        v.push(h.join(".local/opt/Antigravity"));
        v.push(h.join(".local/opt/antigravity"));
        v.push(h.join(".local/Applications/Antigravity"));
        v.push(h.join("Antigravity"));
        v.push(h.join("Antigravity IDE"));
        v.push(h.join("Antigravity-x64"));
        v.push(h.join("antigravity"));
        // CLI locations
        v.push(h.join(".local/bin"));
        v.push(h.join(".agy/bin"));
        v.push(h.join(".agy"));
        v.push(h.join(".local/share/agy/bin"));
    }
    v
}

/// The directory holding the `agy` CLI, found via PATH first and then the common
/// per-user/system bin dirs. Linux only; returns the dir (an install root once
/// `agy` is in it), not the file.
#[cfg(not(target_os = "windows"))]
fn find_agy_dir() -> Option<PathBuf> {
    let home = env::var("HOME").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(path) = env::var("PATH") {
        dirs.extend(path.split(':').filter(|d| !d.is_empty()).map(PathBuf::from));
    }
    dirs.push(PathBuf::from(format!("{}/.local/bin", home)));
    dirs.push(PathBuf::from(format!("{}/.agy/bin", home)));
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs.push(PathBuf::from("/usr/bin"));
    dirs.into_iter()
        .find(|d| !is_snap_path(d) && d.join("agy").is_file())
}

/// True for a path served by snapd's read-only squashfs mounts, which cannot be
/// patched in place.
#[cfg(not(target_os = "windows"))]
fn is_snap_path(p: &Path) -> bool {
    p.starts_with("/snap") || p.starts_with("/var/lib/snapd")
}

/// Scans the usual Linux prefixes (including user Downloads, Desktop and Applications)
/// for any directory whose name contains "antigravity" or "agy".
#[cfg(not(target_os = "windows"))]
fn scan_antigravity_dirs() -> Vec<PathBuf> {
    let home = env::var("HOME").unwrap_or_default();
    let mut out = Vec::new();
    let mut bases = vec![
        "/opt".to_string(),
        "/usr/share".to_string(),
        "/usr/lib".to_string(),
        "/usr/local/share".to_string(),
    ];
    if !home.is_empty() {
        bases.push(format!("{}/.local/share", home));
        bases.push(format!("{}/.local/opt", home));
        bases.push(format!("{}/.local/Applications", home));
        bases.push(format!("{}/Downloads", home));
        bases.push(format!("{}/Desktop", home));
        bases.push(format!("{}/Applications", home));
        bases.push(format!("{}/Programs", home));
        bases.push(format!("{}/Apps", home));
        bases.push(format!("{}/Software", home));
        bases.push(home.clone());
    }
    for base in bases {
        if let Ok(entries) = fs::read_dir(&base) {
            for e in entries.flatten() {
                let p = e.path();
                let hit = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| {
                        let l = n.to_lowercase();
                        l.contains("antigravity") || l == "agy"
                    });
                if hit && p.is_dir() {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// Inspects `/proc` for running Antigravity IDE, CLI or Language Server processes.
/// Finding the running process yields the exact root directory instantaneously.
#[cfg(not(target_os = "windows"))]
fn scan_running_processes() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return out;
    };
    for e in entries.flatten() {
        let name = e.file_name();
        let s = name.to_string_lossy();
        if !s.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let exe_link = e.path().join("exe");
        if let Ok(target) = fs::read_link(&exe_link) {
            let t = target.to_string_lossy().to_lowercase();
            if t.contains("antigravity") || t.contains("agy") || t.contains("language_server") {
                if let Some(root) = resolve_install_root(&target) {
                    if !out.contains(&root) {
                        out.push(root);
                    }
                }
            }
        }
    }
    out
}

/// Inspects system and user `.desktop` application launchers to extract target binary paths.
#[cfg(not(target_os = "windows"))]
fn scan_desktop_files() -> Vec<PathBuf> {
    let home = env::var("HOME").unwrap_or_default();
    let mut out = Vec::new();
    let dirs = [
        format!("{}/.local/share/applications", home),
        format!("{}/Desktop", home),
        "/usr/share/applications".to_string(),
        "/usr/local/share/applications".to_string(),
        "/var/lib/flatpak/exports/share/applications".to_string(),
    ];
    for dir in dirs {
        let Ok(entries) = fs::read_dir(&dir) else { continue; };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(content) = fs::read_to_string(&p) else { continue; };
            let l = content.to_lowercase();
            if !l.contains("antigravity") && !l.contains("agy") {
                continue;
            }
            for line in content.lines() {
                let trimmed = line.trim();
                if let Some(cmd) = trimmed.strip_prefix("Exec=") {
                    let first_token = cmd.split_whitespace().next().unwrap_or_default();
                    let exec_path = PathBuf::from(first_token);
                    if let Some(root) = resolve_install_root(&exec_path) {
                        if !out.contains(&root) {
                            out.push(root);
                        }
                    }
                } else if let Some(path_val) = trimmed.strip_prefix("Path=") {
                    let dir_path = PathBuf::from(path_val.trim());
                    if let Some(root) = resolve_install_root(&dir_path) {
                        if !out.contains(&root) {
                            out.push(root);
                        }
                    }
                }
            }
        }
    }
    out
}

/// Resolves install locations using fast filesystem checks, directory scans,
/// running process detection, and desktop shortcuts. Fast and lightweight.
pub fn discover_installs_fast() -> Vec<PathBuf> {
    let mut installs = Vec::new();
    let mut candidates = standard_install_candidates();

    #[cfg(not(target_os = "windows"))]
    {
        candidates.extend(scan_antigravity_dirs());
        candidates.extend(scan_desktop_files());
        candidates.extend(scan_running_processes());
        if let Some(agy_dir) = find_agy_dir() {
            candidates.push(agy_dir);
        }
    }

    for cand in candidates {
        if let Some(resolved) = resolve_install_root(&cand) {
            if !installs.contains(&resolved) {
                installs.push(resolved);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    installs.retain(|p| !is_snap_path(p));

    installs
}

pub fn find_all_installs() -> Vec<PathBuf> {
    let mut installs = discover_installs_fast();

    #[cfg(target_os = "windows")]
    {
        let ps_cmd = r#"Get-ItemProperty HKLM:\Software\Wow6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*, HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*, HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\* | Where-Object { $_.DisplayName -like '*Antigravity*' -or $_.DisplayName -like '*agy*' -or $_.InstallLocation -like '*Antigravity*' } | ForEach-Object { $_.InstallLocation }"#;
        if let Some(output) = utils::powershell_within(ps_cmd, Duration::from_secs(20)) {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let cleaned = clean_input_path(line);
                    if !cleaned.is_empty() {
                        if let Some(resolved) = resolve_install_root(&PathBuf::from(&cleaned)) {
                            if !installs.contains(&resolved) {
                                installs.push(resolved);
                            }
                        }
                    }
                }
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    installs.retain(|p| !is_snap_path(p));

    installs
}

/// Puts a v2.4+ install back into its pristine shape: the extracted
/// `resources/app` from an older patch is removed and `app.asar` is restored.
/// Electron prefers `resources/app` over the archive, so the directory must be
/// gone before the archive is put back.
pub fn restore_pristine_asar(resources: &Path) -> Result<(), String> {
    let app_dir = resources.join("app");
    let app_asar = resources.join("app.asar");
    let asar_bak = resources.join("app.asar.bak");

    // Only touch resources/app when it demonstrably came from an archive.
    // Antigravity IDE ships resources/app as its real, unpacked layout - with
    // neither app.asar nor a backup present there is nothing to restore, and
    // deleting the directory would destroy the install.
    if !asar_bak.exists() && !app_asar.exists() {
        return Ok(());
    }

    if app_dir.exists() {
        fs::remove_dir_all(&app_dir)
            .map_err(|e| format!("не удалось удалить resources\\app: {}", e))?;
    }
    if asar_bak.exists() && !app_asar.exists() {
        fs::rename(&asar_bak, &app_asar)
            .map_err(|e| format!("не удалось восстановить app.asar: {}", e))?;
    }
    Ok(())
}

/// One install, patched. Carries what used to be printed.
///
/// `summary` is returned rather than folded into a process-wide flag on the way
/// past: the console build died after one run, so a sticky "the proxy rename
/// landed" atomic was harmless there. A window that patches twice would inherit
/// the first run's verdict and go on writing the proxy variable for a rename
/// that has since started failing. The caller owns the verdict for *its* run.
pub struct InstallOutcome {
    pub label: &'static str,
    /// Non-fatal notes — a leftover override that would not come off. The patch
    /// succeeded anyway, so these are not errors.
    pub warnings: Vec<String>,
    pub summary: patch_binary::BinarySummary,
}

pub fn process_install(install: &Path) -> Result<InstallOutcome, String> {
    // Patch all relevant binaries (Language Server / CLI).
    let bin_summary = patch_all_binaries(install);
    let mut warnings: Vec<String> = Vec::new();

    let resources = install.join("resources");
    let app_dir = resources.join("app");
    let app_asar = resources.join("app.asar");

    if app_asar.exists() {
        // Peek at dist/main.js straight out of the archive. On v2.4+ the shell
        // carries no auth code, so nothing is unpacked and the install stays
        // byte-identical to a fresh one.
        let is_new_arch = asar::read_asar_entry(&app_asar, "dist/main.js")
            .and_then(|b| String::from_utf8(b).ok())
            .is_some_and(|src| is_new_desktop_architecture(&src));

        if is_new_arch {
            // Clean up leftovers from a patch applied before v2.4.
            restore_pristine_asar(&resources)?;
            // The Language Server is the only thing being patched here, so if
            // it did not take there is nothing to report as success.
            if bin_summary.ok == 0 {
                return Err(binary_failure_message(&bin_summary));
            }
            return Ok(InstallOutcome {
                label: "Antigravity Desktop",
                warnings,
                summary: bin_summary,
            });
        }

        // Older layout: unpack so the JS can be patched.
        if app_dir.exists() {
            let _ = fs::remove_dir_all(&app_dir);
        }
        if !extract_asar(&app_asar, &app_dir) {
            return Err("Ошибка получения доступа к приложению".to_string());
        }
    }

    let ide_js = app_dir.join("out").join("main.js");
    let desktop_js = app_dir.join("dist").join("main.js");

    if ide_js.exists() {
        patch_ide(install, &ide_js)?;
        if let Err(e) = patch_extension_js(install) {
            // Not reported per install: the progress line is one row wide, and
            // the extension patch is cosmetic next to the Language Server one.
            let _ = e;
        }
        // The endpoint is deliberately NOT overridden any more (2.11.0).
        //
        // Pushing the client onto `daily-cloudcode-pa` was a workaround for one
        // fact: nobody substituted `cloudcode-pa`, so the host Antigravity picks
        // for itself had no route (S9/N15). A provider that substitutes it is now
        // measured and first in the pool, so the workaround costs more than it
        // buys - it is a user-visible setting in *their* settings.json that
        // survives our revert paths only if they run one, and it pins a host
        // choice Antigravity is entitled to make differently per build or
        // account. Leave the host alone; route whichever one it asks for.
        //
        // Upgrade duty, same shape as `remove_legacy_ca` (I27): a machine
        // patched by <= 2.10.x still carries the override, and leaving it would
        // silently keep that machine on the old host. Menu 1 takes it back out.
        if let Err(e) = endpoint::remove_ide(install) {
            warnings.push(format!("Прежний оверрайд эндпоинта не снят: {}", e));
        }
        return Ok(InstallOutcome {
            label: "Antigravity IDE",
            warnings,
            summary: bin_summary,
        });
    } else if desktop_js.exists() {
        let js_patched = patch_desktop(install, &desktop_js)?;
        if !js_patched {
            // v2.4+ unpacked by an older build of this tool: undo the unpack.
            restore_pristine_asar(&resources)?;
        }
        return Ok(InstallOutcome {
            label: "Antigravity Desktop",
            warnings,
            summary: bin_summary,
        });
    } else if install.join("agy.exe").is_file() || install.join("agy").is_file() {
        if bin_summary.ok == 0 {
            return Err(binary_failure_message(&bin_summary));
        }
        // Not overridden any more, for the reason spelled out in the IDE arm
        // above; here the leftover is an environment variable rather than a
        // settings key, and it outlives a reinstall, so taking it back out
        // matters more, not less.
        if let Err(e) = endpoint::remove_cli() {
            warnings.push(format!("Прежний {} не снят: {}", endpoint::CLI_ENV_VAR, e));
        }
        return Ok(InstallOutcome {
            label: "Antigravity CLI",
            warnings,
            summary: bin_summary,
        });
    }

    Err("Компоненты приложения не найдены".to_string())
}

fn binary_failure_message(summary: &patch_binary::BinarySummary) -> String {
    if summary.total() == 0 {
        "Бинарник Language Server / CLI не найден в этой установке".to_string()
    } else if let Some(err) = &summary.last_error {
        err.clone()
    } else {
        "Сигнатура в Language Server не найдена — вероятно, вышла новая версия Antigravity"
            .to_string()
    }
}

/// The same removal without a word of output, for the window.
///
/// Both guards are load-bearing and neither is cosmetic (I39): nothing to remove
/// is the common case, and pulling the root from under a relay that is still an
/// older build makes every gate request die as `BadCertificate`. Returns whether
/// anything was actually taken out.
pub fn remove_legacy_ca_quiet() -> bool {
    let cert = proxy::ca_cert_path();
    if !cert.exists() {
        return false;
    }
    if background::relay_is_outdated() {
        return false;
    }
    endpoint::clear_node_ca(&cert.to_string_lossy()).ok();
    proxy::untrust_ca();
    true
}

/// Which product an install directory is, for the progress line.
///
/// A guess from the layout rather than the name `process_install` returns,
/// because the line is printed before the work starts - that is the whole point
/// of it.
pub fn install_label(install: &Path) -> &'static str {
    if install.join("agy.exe").is_file() || install.join("agy").is_file() {
        "Antigravity CLI"
    } else if install.join("Antigravity IDE.exe").exists()
        || install.join("resources").join("app").join("out").exists()
    {
        "Antigravity IDE"
    } else {
        "Antigravity 2.0"
    }
}

fn print_help() {
    println!(
        "Open Antigravity v{} (портативная сборка)\n\
         Разблокировка и сетевая маршрутизация для Antigravity 2.0 (Google IDE).\n\
         https://github.com/keelbismark/OpenAntigravity\n\
         \n\
         Использование:\n\
           open_antigravity [ОПЦИИ]\n\
         \n\
         Опции:\n\
           -h, --help                 Показать эту справку и выйти\n\
           -v, --version              Показать версию сборки и происхождение\n\
           --tui, --cli               Запустить интерактивный текстовый интерфейс (TUI)\n\
           --check, --diagnose        Комплексная сетевая и системная диагностика (self-test)\n\
           --shortcut                 Создать ярлыки на Рабочем столе и в главном меню\n\
           --steam                    Добавить Open Antigravity в библиотеку Steam\n\
           --about, --license         Показать лицензионное соглашение и правовую информацию\n\
         \n\
         По умолчанию открывается графический интерфейс (GUI), либо TUI, если дисплей недоступен.",
        crate::update::current_version()
    );
}

fn main() {
    if env::args().skip(1).any(|a| {
        matches!(
            a.trim_start_matches('-').to_ascii_lowercase().as_str(),
            "help"
                | "h"
                | "?"
                | "about"
                | "license"
                | "licence"
                | "version"
                | "v"
                | "cli"
                | "check"
                | "diagnose"
                | "diag"
                | "shortcut"
                | "create-shortcut"
                | "steam"
                | "add-to-steam"
        )
    }) {
        utils::attach_parent_console();
    }

    if env::args().skip(1).any(|a| {
        matches!(
            a.trim_start_matches('-').to_ascii_lowercase().as_str(),
            "help" | "h" | "?"
        )
    }) {
        print_help();
        return;
    }

    if env::args().skip(1).any(|a| {
        matches!(
            a.trim_start_matches('-').to_ascii_lowercase().as_str(),
            "check" | "diagnose" | "diag"
        )
    }) {
        let report = diag::run_diagnostics();
        println!("{}", report.to_string_formatted(true));
        return;
    }

    if env::args().skip(1).any(|a| {
        matches!(
            a.trim_start_matches('-').to_ascii_lowercase().as_str(),
            "shortcut" | "create-shortcut"
        )
    }) {
        match shortcut::create_shortcuts() {
            Ok(msg) => println!("{}", msg),
            Err(err) => eprintln!("Ошибка: {}", err),
        }
        return;
    }

    if env::args().skip(1).any(|a| {
        matches!(
            a.trim_start_matches('-').to_ascii_lowercase().as_str(),
            "steam" | "add-to-steam"
        )
    }) {
        match shortcut::add_to_steam() {
            Ok(msg) => println!("{}", msg),
            Err(err) => eprintln!("Ошибка: {}", err),
        }
        return;
    }

    if env::args().any(|a| a == background::FORWARDER_FLAG) {
        dns_forwarder::detach_console();
        watchdog::start();
        if let Err(e) = dns_forwarder::run() {
            dns_forwarder::log_fatal(&e);
            std::process::exit(1);
        }
        return;
    }

    if env::args().any(|a| a == background::WATCHDOG_FLAG) {
        dns_forwarder::detach_console();
        watchdog::run_forever();
        return;
    }

    if env::args().any(|a| a == background::PROXY_FLAG) {
        dns_forwarder::detach_console();
        if let Err(e) = dns_forwarder::run_proxy_service() {
            dns_forwarder::log_fatal(&e);
            std::process::exit(1);
        }
        return;
    }

    // Root's half of patching a root-owned install (`elevate`): started by
    // pkexec from the window, patches the one install named and exits.
    {
        let args: Vec<String> = env::args().collect();
        if let Some(i) = args.iter().position(|a| a == elevate::PATCH_ROOT_FLAG) {
            elevate::run_helper(args.get(i + 1).cloned());
        }
        if let Some(i) = args.iter().position(|a| a == elevate::WRITE_HOSTS_FLAG) {
            elevate::run_write_hosts_helper(args.get(i + 1).cloned());
        }
        if args.iter().any(|a| a == elevate::REMOVE_HOSTS_FLAG) {
            elevate::run_remove_hosts_helper();
        }
    }

    canary::handle_cli_flags();

    // Prevent running multiple concurrent instances of the application.
    let _instance_lock = match single_instance::try_acquire() {
        Ok(lock) => lock,
        Err(err) => {
            utils::message_box("Open Antigravity", &err);
            return;
        }
    };

    if tui::requested() {
        run_tui(None);
        return;
    }
    // A server over SSH: a terminal, and nothing to draw a window into.
    #[cfg(not(target_os = "windows"))]
    if tui::only_terminal() {
        run_tui(None);
        return;
    }

    if let Err(e) = gui::run() {
        // No renderer opened a window. The terminal UI carries every switch the
        // window does, so it is offered instead of a dead end: on Windows in a
        // console of its own, on Linux only when started from a terminal (a
        // double-clicked launcher has none, and gets the message instead).
        let note = format!("Окно не открылось ({e}) — работаю в терминале.");
        #[cfg(target_os = "windows")]
        run_tui(Some(note));
        #[cfg(not(target_os = "windows"))]
        {
            use std::io::IsTerminal;
            if std::io::stdin().is_terminal() {
                run_tui(Some(note));
            } else {
                utils::message_box("Open Antigravity", &e);
                std::process::exit(1);
            }
        }
    }
}

fn run_tui(note: Option<String>) {
    if let Err(e) = tui::run(note) {
        utils::message_box("Open Antigravity", &e);
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: `%LOCALAPPDATA%` holds an `agy` *directory* (the CLI's folder).
    /// An `exists()` check matched that directory, so the parent-walk in
    /// `resolve_install_root` treated `%LOCALAPPDATA%` itself as an install root and
    /// menu 1 reported `%LOCALAPPDATA% - Компоненты приложения не найдены`. A
    /// launcher/CLI is a *file*, so a directory named `agy` must not qualify.
    #[test]
    fn a_directory_named_agy_does_not_make_its_parent_an_install() {
        let base = env::temp_dir().join("ag_isroot_dir_test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("agy").join("bin")).unwrap();
        assert!(
            !is_install_root(&base),
            "a subdirectory named agy must not make its parent an install root"
        );
        // The empty agy dir is not a root either, so resolving a path under it must
        // not walk up to `base`.
        assert_ne!(resolve_install_root(&base.join("agy")), Some(base.clone()));
        fs::remove_dir_all(&base).ok();
    }

    /// The real CLI directory - one that actually holds the `agy` binary as a
    /// file - still resolves as an install root.
    #[test]
    fn a_directory_holding_the_agy_binary_is_an_install() {
        let base = env::temp_dir().join("ag_isroot_file_test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("agy"), b"binary").unwrap();
        assert!(is_install_root(&base));
        fs::remove_dir_all(&base).ok();
    }
}
