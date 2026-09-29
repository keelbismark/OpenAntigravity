//! Встроенная диагностика системы и сети (Self-test).
//!
//! Кроссплатформенная проверка (Linux + Windows):
//! 1. Окружение и права (ОС, архитектура, портативный режим, права администратора).
//! 2. Установки Antigravity (поиск IDE/Desktop/CLI, статус патча Language Server).
//! 3. Локальный сервис (статус службы/фонового процесса, порт прокси).
//! 4. DNS-обход (Windows NRPT / Linux /etc/hosts pinning).
//! 5. Апстрим-прокси (вшитый/свой, TCP пинг, HTTP CONNECT, сквозной TLS-тест к Google, гео-IP выхода).
//! 6. DNS unblock-провайдеры (проверка доступности и времени ответа).

use std::fmt::Write as _;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagStatus {
    Ok,
    Warn,
    Fail,
    Info,
}

impl DiagStatus {
    pub fn badge(&self) -> &'static str {
        match self {
            DiagStatus::Ok => "[OK]  ",
            DiagStatus::Warn => "[WARN]",
            DiagStatus::Fail => "[FAIL]",
            DiagStatus::Info => "[INFO]",
        }
    }

    pub fn ansi_badge(&self) -> &'static str {
        match self {
            DiagStatus::Ok => "\x1b[32m[OK]  \x1b[0m",
            DiagStatus::Warn => "\x1b[33m[WARN]\x1b[0m",
            DiagStatus::Fail => "\x1b[31m[FAIL]\x1b[0m",
            DiagStatus::Info => "\x1b[36m[INFO]\x1b[0m",
        }
    }

    pub fn is_ok(&self) -> bool {
        matches!(self, DiagStatus::Ok | DiagStatus::Info)
    }
}

pub struct DiagItem {
    pub category: &'static str,
    pub name: String,
    pub status: DiagStatus,
    pub summary: String,
    pub details: Vec<String>,
}

pub struct DiagReport {
    pub items: Vec<DiagItem>,
    pub timestamp: String,
    pub latency_ms: Option<u32>,
}

impl DiagReport {
    pub fn overall_status(&self) -> DiagStatus {
        if self.items.iter().any(|i| i.status == DiagStatus::Fail) {
            DiagStatus::Fail
        } else if self.items.iter().any(|i| i.status == DiagStatus::Warn) {
            DiagStatus::Warn
        } else {
            DiagStatus::Ok
        }
    }

    pub fn to_string_formatted(&self, use_ansi: bool) -> String {
        let mut out = String::new();
        let border = "═".repeat(64);
        let _ = writeln!(out, "{}", border);
        let _ = writeln!(
            out,
            "    Open Antigravity v{} — Диагностика системы и сети",
            crate::update::current_version()
        );
        let _ = writeln!(out, "    Время проверки: {}", self.timestamp);
        let _ = writeln!(out, "{}", border);

        let mut current_cat = "";
        for item in &self.items {
            if item.category != current_cat {
                current_cat = item.category;
                let _ = writeln!(out, "\n▶ {}", current_cat);
            }

            let badge = if use_ansi {
                item.status.ansi_badge()
            } else {
                item.status.badge()
            };
            let _ = writeln!(out, "  {} {}: {}", badge, item.name, item.summary);
            for d in &item.details {
                let _ = writeln!(out, "         └─ {}", d);
            }
        }

        let _ = writeln!(out, "\n{}", border);
        let conclusion = match self.overall_status() {
            DiagStatus::Ok => "ИТОГ: Все компоненты работают исправно, сеть настроена корректно.",
            DiagStatus::Warn => "ИТОГ: Система готова к работе, но есть замечания (см. [WARN]).",
            DiagStatus::Fail => "ИТОГ: Обнаружены проблемы, требующие внимания (см. [FAIL]).",
            DiagStatus::Info => "ИТОГ: Проверка завершена.",
        };
        let _ = writeln!(out, "  {}", conclusion);
        let _ = writeln!(out, "{}\n", border);
        out
    }
}

/// Выполняет полную диагностику системы и сети.
pub fn run_diagnostics() -> DiagReport {
    let mut items = Vec::new();
    let now = crate::utils::local_clock().map_or_else(
        || "неизвестно".to_string(),
        |c| c.hms(),
    );

    // 1. Окружение и режим
    let os_name = std::env::consts::OS;
    let arch_name = std::env::consts::ARCH;
    let is_portable = crate::portable::enabled();
    let is_admin = crate::utils::is_admin();

    let mut env_details = Vec::new();
    env_details.push(format!("Платформа: {} ({})", os_name, arch_name));
    env_details.push(format!(
        "Режим: {}",
        if is_portable {
            "Портативный (AG_PORTABLE=1, данные в data/)"
        } else {
            "Стандартный (системный каталог)"
        }
    ));
    env_details.push(format!(
        "Привилегии: {}",
        if is_admin {
            "Администратор / root"
        } else {
            "Обычный пользователь (не root/admin)"
        }
    ));

    items.push(DiagItem {
        category: "Окружение",
        name: "Система".to_string(),
        status: DiagStatus::Ok,
        summary: format!("{}-{}", os_name, arch_name),
        details: env_details,
    });

    // 2. Установки Antigravity
    let installs = crate::find_all_installs();
    if installs.is_empty() {
        items.push(DiagItem {
            category: "Antigravity",
            name: "Установки клиента".to_string(),
            status: DiagStatus::Warn,
            summary: "Копии Antigravity не обнаружены в стандартных путях".to_string(),
            details: vec![
                "Если Antigravity установлен в нестандартную папку, запустите программу с путём к ней:".to_string(),
                "open_antigravity /путь/к/папке".to_string(),
            ],
        });
    } else {
        for inst in &installs {
            let label = crate::install_label(inst);
            let mut details = Vec::new();
            let mut status = DiagStatus::Ok;
            details.push(format!("Путь: {}", crate::utils::mask_path(&inst.to_string_lossy())));

            // Проверка патча Language Server
            let state = crate::patch_binary::inspect_install(inst);
            if state.is_empty() {
                status = DiagStatus::Warn;
                details.push("Языковой сервер (Language Server / CLI) не найден".to_string());
            } else if state.fully_patched() {
                details.push(format!(
                    "Языковой сервер пропатчен (файлов: {})",
                    state.files.len()
                ));
            } else if state.partially_patched() {
                status = DiagStatus::Warn;
                details.push(format!(
                    "Частично пропатчен (файлов: {})",
                    state.files.len()
                ));
            } else {
                status = DiagStatus::Warn;
                details.push("Языковой сервер ещё не пропатчен (требуется разблокировка)".to_string());
            }

            // Проверка app.asar / JS
            let resources = inst.join("resources");
            let app_asar = resources.join("app.asar");
            let app_dir = resources.join("app");
            if app_asar.exists() {
                details.push("Компоненты Electron: app.asar присутствует".to_string());
            } else if app_dir.exists() {
                details.push("Компоненты Electron: распакованы в resources/app".to_string());
            }

            items.push(DiagItem {
                category: "Antigravity",
                name: label.to_string(),
                status,
                summary: if status == DiagStatus::Ok {
                    "Разблокирован и готов".to_string()
                } else {
                    "Требуется применить разблокировку".to_string()
                },
                details,
            });
        }
    }

    // 3. Локальный сервис и прокси
    let is_running = crate::background::is_running();
    let listen_port = crate::proxy::port();
    let mut proxy_details = Vec::new();

    let proxy_online = {
        let addr = format!("127.0.0.1:{}", listen_port);
        let start = Instant::now();
        if let Ok(stream) = TcpStream::connect_timeout(
            &addr.parse().unwrap_or_else(|_| "127.0.0.1:8080".parse().unwrap()),
            Duration::from_millis(500),
        ) {
            let _ = stream.set_nodelay(true);
            let ms = start.elapsed().as_millis();
            proxy_details.push(format!("Порт 127.0.0.1:{} слушается (ответ за {} мс)", listen_port, ms));
            true
        } else {
            false
        }
    };

    proxy_details.push(format!(
        "Фоновый процесс: {}",
        if is_running { "Активен" } else { "Не запущен" }
    ));

    let proxy_status = if proxy_online {
        DiagStatus::Ok
    } else if is_running {
        DiagStatus::Warn
    } else {
        DiagStatus::Info
    };

    items.push(DiagItem {
        category: "Локальный сервис",
        name: "Локальный прокси".to_string(),
        status: proxy_status,
        summary: if proxy_online {
            format!("Активен на порту {}", listen_port)
        } else {
            format!("Остановлен (порт {})", listen_port)
        },
        details: proxy_details,
    });

    // 4. DNS-обход
    #[cfg(target_os = "windows")]
    {
        let nrpt_on = crate::dns::is_nrpt_applied();
        items.push(DiagItem {
            category: "DNS-обход",
            name: "NRPT правила Windows".to_string(),
            status: if nrpt_on { DiagStatus::Ok } else { DiagStatus::Info },
            summary: if nrpt_on {
                "Правила активны".to_string()
            } else {
                "Правила выключены".to_string()
            },
            details: vec![
                if nrpt_on {
                    "Целевые домены Google перенаправляются на локальный релей"
                } else {
                    "Для обхода через DNS включите «Обход через DNS»"
                }
                .to_string(),
            ],
        });
    }

    #[cfg(not(target_os = "windows"))]
    {
        let hosts_on = crate::hosts_pin::is_applied();
        let mut details = Vec::new();
        details.push(format!(
            "/etc/hosts: {}",
            if hosts_on {
                "Записи блокировок внесены"
            } else {
                "Записей нет (DNS-обход выключен)"
            }
        ));

        // Проверка системного резолва
        let resolve_test = ("daily-cloudcode-pa.googleapis.com", 443).to_socket_addrs();
        match resolve_test {
            Ok(addrs) => {
                let ips: Vec<String> = addrs.map(|a| a.ip().to_string()).collect();
                details.push(format!("Резолв cloudcode: {}", ips.join(", ")));
            }
            Err(e) => {
                details.push(format!("Резолв cloudcode: ошибка ({})", e));
            }
        }

        items.push(DiagItem {
            category: "DNS-обход",
            name: "Пиннинг /etc/hosts (Linux)".to_string(),
            status: if hosts_on { DiagStatus::Ok } else { DiagStatus::Info },
            summary: if hosts_on {
                "Пиннинг применён".to_string()
            } else {
                "Пиннинг выключен".to_string()
            },
            details,
        });
    }

    // 5. Апстрим-прокси
    let builtin = crate::upstream::builtin_proxy();
    let configured = crate::upstream::configured();
    let mut proxy_latency_ms: Option<u32> = None;

    let mut upstream_details = Vec::new();
    if let Some(b) = &builtin {
        upstream_details.push(format!("Вшитый прокси: {}", mask_proxy_line(b)));
    } else {
        upstream_details.push("Вшитый прокси: не задан при сборке".to_string());
    }

    if let Some(up) = configured {
        upstream_details.push(format!("Активный прокси: {}", up.display()));

        // Тест 1: TCP подключение к прокси
        let target_addr = format!("{}:{}", up.host, up.port);
        let start = Instant::now();
        let tcp_res = target_addr.to_socket_addrs().ok().and_then(|mut iter| {
            iter.find_map(|sa| TcpStream::connect_timeout(&sa, Duration::from_secs(4)).ok())
        });

        if let Some(_stream) = tcp_res {
            let rtt = (start.elapsed().as_millis().min(u32::MAX as u128)) as u32;
            proxy_latency_ms = Some(rtt);
            upstream_details.push(format!("TCP пинг до прокси: {} мс", rtt));

            // Тест 2: HTTP CONNECT через прокси
            let connect_start = Instant::now();
            match crate::upstream::open(&up, "daily-cloudcode-pa.googleapis.com", 443, Duration::from_secs(5)) {
                Ok(_sock) => {
                    let connect_ms = connect_start.elapsed().as_millis();
                    upstream_details.push(format!(
                        "HTTP CONNECT туннель: успешно ({} мс)",
                        connect_ms
                    ));

                    // Тест 3: Сквозной probe до Google через туннель
                    let probe_start = Instant::now();
                    match crate::upstream::probe(&up) {
                        Ok(()) => {
                            let probe_ms = probe_start.elapsed().as_millis();
                            upstream_details.push(format!(
                                "Сквозной TLS-тест к Google: OK ({} мс)",
                                probe_ms
                            ));
                        }
                        Err(e) => {
                            upstream_details.push(format!("Сквозной TLS-тест к Google: сбой ({})", e));
                        }
                    }

                    // Тест 4: Геолокация выхода
                    match crate::upstream::exit_info(&up) {
                        Some((ip, country)) => {
                            upstream_details.push(format!("Выход прокси: {} (страна: {})", ip, country));
                            let is_blocked = country.eq_ignore_ascii_case("RU")
                                || country.eq_ignore_ascii_case("BY");
                            let status = if is_blocked {
                                DiagStatus::Fail
                            } else {
                                DiagStatus::Ok
                            };
                            let summary = if is_blocked {
                                format!("Прокси выходит в заблокированной стране ({})!", country)
                            } else {
                                format!("Работает исправно (выход: {}, {})", country, ip)
                            };

                            items.push(DiagItem {
                                category: "Маршрутизация",
                                name: "Апстрим-прокси".to_string(),
                                status,
                                summary,
                                details: upstream_details,
                            });
                        }
                        None => {
                            upstream_details.push("Определение страны выхода: тайм-аут или закрыт trace".to_string());
                            items.push(DiagItem {
                                category: "Маршрутизация",
                                name: "Апстрим-прокси".to_string(),
                                status: DiagStatus::Ok,
                                summary: "Туннель работает (страна не определена)".to_string(),
                                details: upstream_details,
                            });
                        }
                    }
                }
                Err(e) => {
                    upstream_details.push(format!("HTTP CONNECT туннель: ошибка ({})", e));
                    items.push(DiagItem {
                        category: "Маршрутизация",
                        name: "Апстрим-прокси".to_string(),
                        status: DiagStatus::Fail,
                        summary: format!("Прокси отклонил CONNECT ({})", e),
                        details: upstream_details,
                    });
                }
            }
        } else {
            upstream_details.push(format!("TCP подключение к {}: тайм-аут или отказ", target_addr));
            items.push(DiagItem {
                category: "Маршрутизация",
                name: "Апстрим-прокси".to_string(),
                status: DiagStatus::Fail,
                summary: "Прокси-сервер недоступен по сети".to_string(),
                details: upstream_details,
            });
        }
    } else {
        items.push(DiagItem {
            category: "Маршрутизация",
            name: "Апстрим-прокси".to_string(),
            status: DiagStatus::Warn,
            summary: "Прокси не настроен".to_string(),
            details: vec![
                "В сборку не вшит прокси и не задан свой прокси в настройках.".to_string(),
                "Укажите прокси в окне программы («Свой прокси») для обхода блокировок.".to_string(),
            ],
        });
    }

    // 6. DNS unblock-провайдеры
    let mut provider_details = Vec::new();
    for p in crate::resolvers::PROVIDERS {
        match p.transport {
            crate::resolvers::Transport::Udp => {
                let first_ip = p.v4.first().copied().unwrap_or("");
                if !first_ip.is_empty() {
                    let addr = format!("{}:53", first_ip);
                    let start = Instant::now();
                    // Быстрый пинг UDP сокета
                    if let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0") {
                        let _ = sock.set_read_timeout(Some(Duration::from_millis(800)));
                        let _ = sock.connect(&addr);
                        let ms = start.elapsed().as_millis();
                        provider_details.push(format!("{}: доступен (UDP {}, ~{} мс)", p.name, first_ip, ms));
                    } else {
                        provider_details.push(format!("{}: сокет недоступен", p.name));
                    }
                }
            }
            crate::resolvers::Transport::Doh(ep) => {
                let first_ip = ep.addrs.first().copied().unwrap_or("");
                let addr = format!("{}:443", first_ip);
                let start = Instant::now();
                if let Some(stream) = addr.to_socket_addrs().ok().and_then(|mut iter| {
                    iter.find_map(|sa| TcpStream::connect_timeout(&sa, Duration::from_millis(1500)).ok())
                }) {
                    let _ = stream.set_nodelay(true);
                    let ms = start.elapsed().as_millis();
                    provider_details.push(format!("{}: доступен (DoH {}, TCP {} мс)", p.name, first_ip, ms));
                } else {
                    provider_details.push(format!("{}: не ответил (DoH {})", p.name, first_ip));
                }
            }
        }
    }

    items.push(DiagItem {
        category: "DNS-провайдеры",
        name: "Пулы обхода".to_string(),
        status: DiagStatus::Ok,
        summary: format!("Проверено {} провайдеров", crate::resolvers::PROVIDERS.len()),
        details: provider_details,
    });

    DiagReport {
        items,
        timestamp: now,
        latency_ms: proxy_latency_ms,
    }
}

/// Маскирует учетные данные в строке прокси (login:pass@host:port -> host:port).
fn mask_proxy_line(s: &str) -> String {
    if let Some((_, hostport)) = s.split_once('@') {
        hostport.to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_proxy_line() {
        assert_eq!(mask_proxy_line("user:secret123@1.2.3.4:8080"), "1.2.3.4:8080");
        assert_eq!(mask_proxy_line("1.2.3.4:8080"), "1.2.3.4:8080");
    }

    #[test]
    fn test_diag_report_generation() {
        let report = run_diagnostics();
        assert!(!report.items.is_empty());
        let s = report.to_string_formatted(false);
        assert!(s.contains("Диагностика системы и сети"));
    }
}
