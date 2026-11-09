//! Command-line interface actions: headless batch operations for automation, scripts and CI/CD.

use std::env;
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use serde_json::json;

use crate::diag::{self, DiagStatus};
use crate::patch_binary;
use crate::utils::mask_path;

/// Returns true if the argument matches any headless CLI flag or subcommand.
pub fn is_cli_flag(arg: &str) -> bool {
    let clean = arg.trim_start_matches('-').to_ascii_lowercase();
    matches!(
        clean.as_str(),
        "status"
            | "patch"
            | "unpatch"
            | "revert"
            | "fix"
            | "repair"
            | "check"
            | "diagnose"
            | "diag"
            | "shortcut"
            | "create-shortcut"
            | "help"
            | "h"
            | "?"
            | "about"
            | "license"
            | "licence"
            | "version"
            | "v"
    )
}

/// Runs CLI dispatch. Returns `Some(exit_code)` if a CLI command was processed,
/// or `None` if the application should proceed to normal GUI/TUI mode.
pub fn dispatch(args: &[String]) -> Option<i32> {
    if args.len() <= 1 {
        return None;
    }

    let has_json = args.iter().any(|a| a.trim_start_matches('-').eq_ignore_ascii_case("json"));

    let cmd = args.iter().skip(1).find_map(|a| {
        let clean = a.trim_start_matches('-').to_ascii_lowercase();
        if clean == "json" {
            None
        } else if is_cli_flag(a) {
            Some(clean)
        } else {
            None
        }
    })?;

    match cmd.as_str() {
        "status" => Some(handle_status(has_json)),
        "patch" => Some(handle_patch()),
        "unpatch" | "revert" => Some(handle_unpatch()),
        "fix" | "repair" => Some(handle_fix()),
        "check" | "diagnose" | "diag" => Some(handle_check(has_json)),
        "shortcut" | "create-shortcut" => Some(handle_shortcut()),
        "help" | "h" | "?" => {
            crate::print_help();
            Some(0)
        }
        "about" | "license" | "licence" | "version" | "v" => {
            println!("{}", crate::canary::about_banner());
            Some(0)
        }
        _ => None,
    }
}

/// Discovers all Antigravity installations and applies unlocking patches.
pub fn handle_patch() -> i32 {
    println!("════════════════════════════════════════════════════════════════");
    println!("    Open Antigravity v{} — Пакетная разблокировка", crate::update::current_version());
    println!("════════════════════════════════════════════════════════════════\n");

    let installs = crate::find_all_installs();
    if installs.is_empty() {
        eprintln!("[FAIL] Не найдено установленных копий Antigravity (IDE / CLI / Desktop).");
        eprintln!("       Укажите путь к установке вручную или установите Antigravity.");
        return 1;
    }

    let mut has_failure = false;
    let mut patched_count = 0usize;

    for inst in &installs {
        let label = crate::install_label(inst);
        let masked = mask_path(&inst.display().to_string());
        println!("▶ Обработка: {} ({})", label, masked);

        match crate::process_install(inst) {
            Ok(outcome) => {
                patched_count += 1;
                println!(
                    "  [OK]   {}: успешно (бинарников: {}, прокси: {})",
                    outcome.label, outcome.summary.ok, outcome.summary.proxy_var
                );
                for warn in outcome.warnings {
                    println!("         [WARN] {}", warn);
                }
            }
            Err(e) => {
                has_failure = true;
                eprintln!("  [FAIL] {}: ошибка — {}", label, e);
                if e.contains("locked") || e.contains("отказано в доступе") {
                    eprintln!("         Подсказка: закройте работающие процессы Antigravity и повторите запуск.");
                }
            }
        }
    }

    println!("\n▶ Сетевая маршрутизация (DNS)");
    match crate::dns::setup_dns_nrpt() {
        Ok(_) => println!("  [OK]   Правила перенаправления доменов Google активны."),
        Err(e) => {
            println!("  [WARN] Настройка DNS-маршрутизации: {}", e);
        }
    }

    println!("\n════════════════════════════════════════════════════════════════");
    if has_failure {
        eprintln!("  ИТОГ: Разблокировка завершена с ошибками (пропатчено: {}/{})", patched_count, installs.len());
        1
    } else {
        println!("  ИТОГ: Все найденные копии ({}) успешно разблокированы.", patched_count);
        0
    }
}

/// Restores pristine original binaries and configuration from backups.
pub fn handle_unpatch() -> i32 {
    println!("════════════════════════════════════════════════════════════════");
    println!("    Open Antigravity v{} — Откат изменений (Unpatch)", crate::update::current_version());
    println!("════════════════════════════════════════════════════════════════\n");

    let installs = crate::find_all_installs();
    if installs.is_empty() {
        println!("Установленных копий Antigravity не обнаружено. Очистка глобальных настроек...");
    }

    for inst in &installs {
        let label = crate::install_label(inst);
        let masked = mask_path(&inst.display().to_string());
        println!("▶ Откат: {} ({})", label, masked);

        let mut reverted_files = 0usize;
        for (bin_label, res) in patch_binary::unpatch_all_binaries(inst) {
            match res {
                Ok(0) => {}
                Ok(n) => {
                    reverted_files += n;
                    println!("  [OK]   {}: восстановлен оригинал", bin_label);
                }
                Err(e) => eprintln!("  [WARN] {}: {}", bin_label, e),
            }
        }

        for (ide_label, res) in crate::patcher::ide::unpatch_ide_js(inst) {
            match res {
                Ok(true) => {
                    reverted_files += 1;
                    println!("  [OK]   {}: восстановлен из бэкапа", ide_label);
                }
                Ok(false) => println!("  [INFO] {}: бэкапа нет, маркер снят", ide_label),
                Err(e) => eprintln!("  [WARN] {}: {}", ide_label, e),
            }
        }

        let _ = crate::restore_pristine_asar(&inst.join("resources"));
        let _ = crate::endpoint::remove_ide(inst);
        println!("         Восстановлено компонентов: {}", reverted_files);
    }

    println!("\n▶ Снятие DNS-правил и переменных окружения");
    let _ = crate::endpoint::remove_cli();
    crate::dns::remove_dns_nrpt();
    println!("  [OK]   Правила DNS сняты, переменные окружения очищены.");

    println!("\n════════════════════════════════════════════════════════════════");
    println!("  ИТОГ: Исходное состояние компонентов успешно восстановлено.");
    0
}

/// Inspects and displays system readiness and patch status.
pub fn handle_status(json: bool) -> i32 {
    let installs = crate::discover_installs_fast();
    let dns_on = crate::dns::is_nrpt_applied();
    let proxy_port = crate::proxy::port();
    let proxy_addr = SocketAddr::from(([127, 0, 0, 1], proxy_port));
    let proxy_active = TcpStream::connect_timeout(&proxy_addr, Duration::from_millis(60)).is_ok();
    let service_active = crate::platform::service::is_running();

    let mut install_entries = Vec::new();
    let mut all_patched = !installs.is_empty();

    for inst in &installs {
        let label = crate::install_label(inst);
        let state = patch_binary::inspect_install(inst);
        let fully_patched = state.fully_patched();
        let partially_patched = state.partially_patched();

        if !fully_patched {
            all_patched = false;
        }

        install_entries.push((label, inst.clone(), fully_patched, partially_patched));
    }

    let ready = all_patched && dns_on;

    if json {
        let installs_json: Vec<serde_json::Value> = install_entries
            .iter()
            .map(|(label, path, fully, partial)| {
                json!({
                    "name": label,
                    "path": path.to_string_lossy(),
                    "fully_patched": fully,
                    "partially_patched": partial,
                })
            })
            .collect();

        let output = json!({
            "version": crate::update::current_version(),
            "platform": env::consts::OS,
            "architecture": env::consts::ARCH,
            "ready": ready,
            "dns_routing": dns_on,
            "local_proxy": {
                "port": proxy_port,
                "active": proxy_active,
            },
            "background_service": service_active,
            "installs": installs_json,
        });

        println!("{}", serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".into()));
    } else {
        println!("════════════════════════════════════════════════════════════════");
        println!("    Open Antigravity v{} — Статус готовности", crate::update::current_version());
        println!("════════════════════════════════════════════════════════════════\n");

        println!("▶ Компоненты Antigravity");
        if install_entries.is_empty() {
            println!("  [WARN] Установки не найдены (запустите Antigravity или укажите путь)");
        } else {
            for (label, path, fully, partial) in &install_entries {
                let badge = if *fully {
                    "[OK]  "
                } else if *partial {
                    "[WARN]"
                } else {
                    "[FAIL]"
                };
                let status_str = if *fully {
                    "разблокирован"
                } else if *partial {
                    "частично пропатчен (требуется повтор)"
                } else {
                    "не разблокирован (требуется --patch)"
                };
                println!("  {} {}: {} ({})", badge, label, status_str, mask_path(&path.display().to_string()));
            }
        }

        println!("\n▶ Сетевая подсистема");
        let dns_badge = if dns_on { "[OK]  " } else { "[WARN]" };
        println!("  {} DNS-маршрутизация: {}", dns_badge, if dns_on { "активна" } else { "не активна (требуется --fix)" });

        let proxy_badge = if proxy_active { "[OK]  " } else { "[INFO]" };
        println!(
            "  {} Локальный прокси: порт {} ({})",
            proxy_badge,
            proxy_port,
            if proxy_active { "слушает входящие подключения" } else { "остановлен" }
        );

        let service_badge = if service_active { "[OK]  " } else { "[INFO]" };
        println!("  {} Фоновый релей: {}", service_badge, if service_active { "активен" } else { "остановлен" });

        println!("\n════════════════════════════════════════════════════════════════");
        if ready {
            println!("  ИТОГ: Система готова к работе. Antigravity доступен.");
        } else {
            println!("  ИТОГ: Требуется настройка. Запустите: open_antigravity --patch");
        }
    }

    if ready { 0 } else { 1 }
}

/// Runs self-test diagnostics and automatically executes remediation actions for issues.
pub fn handle_fix() -> i32 {
    println!("════════════════════════════════════════════════════════════════");
    println!("    Open Antigravity v{} — Автоматическое исправление", crate::update::current_version());
    println!("════════════════════════════════════════════════════════════════\n");

    println!("Выполняется диагностика системы...");
    let report = diag::run_diagnostics();

    let mut actions: Vec<(String, diag::AutoFixAction)> = Vec::new();
    for item in &report.items {
        if let Some(fix) = item.fix {
            actions.push((item.name.clone(), fix));
        }
    }

    if actions.is_empty() {
        println!("\n[OK] Замечаний не обнаружено. Все компоненты системы работают штатно.");
        return 0;
    }

    println!("\nОбнаружено проблем, требующих исправления: {}", actions.len());
    let mut failed = false;

    for (name, action) in actions {
        println!("▶ Исправление для «{}»: {}...", name, action.label());
        match diag::execute_fix(action) {
            Ok(msg) => println!("  [OK]   {}", msg),
            Err(e) => {
                failed = true;
                eprintln!("  [FAIL] {}", e);
            }
        }
    }

    println!("\n════════════════════════════════════════════════════════════════");
    if failed {
        eprintln!("  ИТОГ: Часть исправлений завершилась с ошибкой. Проверьте права администратора.");
        1
    } else {
        println!("  ИТОГ: Все исправления успешно применены.");
        0
    }
}

/// Runs diagnostics and prints formatted report or JSON.
pub fn handle_check(json: bool) -> i32 {
    let report = diag::run_diagnostics();
    if json {
        println!("{}", report.to_json());
    } else {
        println!("{}", report.to_string_formatted(true));
    }

    if report.overall_status() == DiagStatus::Fail {
        1
    } else {
        0
    }
}

/// Creates desktop and menu application shortcuts.
pub fn handle_shortcut() -> i32 {
    match crate::shortcut::create_shortcuts() {
        Ok(msg) => {
            println!("{}", msg);
            0
        }
        Err(err) => {
            eprintln!("Ошибка создания ярлыков: {}", err);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_cli_flag() {
        assert!(is_cli_flag("--status"));
        assert!(is_cli_flag("status"));
        assert!(is_cli_flag("--patch"));
        assert!(!is_cli_flag("--json"));
        assert!(!is_cli_flag("--unknown"));
    }

    #[test]
    fn test_dispatch_empty_returns_none() {
        let args = vec!["open_antigravity".to_string()];
        assert_eq!(dispatch(&args), None);
    }

    #[test]
    fn test_dispatch_help_flag() {
        let args = vec!["open_antigravity".to_string(), "--help".to_string()];
        assert_eq!(dispatch(&args), Some(0));
    }

    #[test]
    fn test_dispatch_version_flag() {
        let args = vec!["open_antigravity".to_string(), "-v".to_string()];
        assert_eq!(dispatch(&args), Some(0));
    }

    #[test]
    fn test_dispatch_unknown_flag_returns_none() {
        let args = vec!["open_antigravity".to_string(), "--unknown-flag-xyz".to_string()];
        assert_eq!(dispatch(&args), None);
    }
}
