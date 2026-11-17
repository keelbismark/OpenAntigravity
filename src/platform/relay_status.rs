//! One-command view of the running bypass, for support and bug reports.
//!
//! `--relay-status` reads the same facts the report does but prints them on a
//! terminal, without taking the role and without waiting: unlike
//! `--relay-role`, a support question must not block the relay.

use std::net::{Ipv4Addr, SocketAddr};

/// The facts the command prints. Kept as a struct so the text is a pure
/// function and can be checked without a running relay.
#[derive(Debug, Clone, Default)]
pub struct RelayStatus {
    pub window_build: String,
    pub relay_build: String,
    pub record_age: Option<u64>,
    pub owner_pid: u32,
    pub loopback: bool,
    pub first_route: String,
    pub own_proxy: String,
    pub proxy_port: u16,
    pub proxy_answers: bool,
    pub door_holders: Vec<u32>,
    pub blockers: Vec<String>,
}

impl RelayStatus {
    /// What the command prints.
    pub fn text(&self) -> String {
        let mut out = String::new();
        let push = |out: &mut String, line: String| {
            out.push_str(&line);
            out.push('\n');
        };

        push(
            &mut out,
            format!("Open Antigravity: окно {}, служба {}", self.window_build, self.relay_build),
        );
        if !self.relay_build.is_empty() && self.relay_build != self.window_build {
            push(
                &mut out,
                "  ВНИМАНИЕ: сборки отличаются — обновление службы не применилось".into(),
            );
        }
        match self.record_age {
            Some(age) => push(&mut out, format!("  запись: {age} с назад, владелец роли: {}", self.owner_pid)),
            None => push(&mut out, "  записи службы нет — релей не запущен".into()),
        }
        push(
            &mut out,
            format!("  дверь 127.65.71.1:443: {}", if self.door_holders.is_empty() { "нет слушателя".into() } else { format!(
                "pid {}",
                self.door_holders
                    .iter()
                    .map(|pid| pid.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ) }),
        );
        push(
            &mut out,
            format!(
                "  прокси {}:{}: {}",
                crate::proxy::LISTEN_IP,
                self.proxy_port,
                if self.proxy_answers { "отвечает" } else { "не отвечает" }
            ),
        );
        push(
            &mut out,
            format!("  гейт-хосты через локальные адреса: {}", if self.loopback { "да" } else { "нет" }),
        );
        push(&mut out, format!("  свой прокси: {}", self.own_proxy));
        push(
            &mut out,
            format!("  первый маршрут: {}", if self.first_route.is_empty() { "нет" } else { &self.first_route }),
        );
        if self.blockers.is_empty() {
            push(&mut out, "  блокировки: нет".into());
        } else {
            push(&mut out, format!("  блокировки: {}", self.blockers.join("; ")));
        }
        out
    }
}

/// Reads the facts. Best effort throughout: a missing record is reported as
/// such rather than being an error.
pub fn collect() -> RelayStatus {
    let report = crate::gate::read();
    let port = crate::proxy::port();
    let proxy_answers = crate::proxy::our_listener_port().is_some();
    let door = SocketAddr::from((Ipv4Addr::new(127, 65, 71, 1), 443));
    let door_holders = crate::platform::portcheck::listener_pids(
        door,
        crate::platform::portcheck::Proto::Tcp,
    );

    RelayStatus {
        window_build: crate::update::current_version().to_string(),
        relay_build: report
            .as_ref()
            .map(|r| r.build.clone())
            .unwrap_or_default(),
        record_age: report.as_ref().map(|r| r.age().as_secs()),
        owner_pid: report.as_ref().map_or(0, |r| r.owner_pid),
        loopback: report.as_ref().is_some_and(|r| r.loopback),
        first_route: report
            .as_ref()
            .and_then(|r| r.routes.first())
            .map(|row| row.label.clone())
            .unwrap_or_default(),
        own_proxy: report
            .as_ref()
            .map(|r| r.own_route_reason.clone())
            .unwrap_or_default(),
        proxy_port: port,
        proxy_answers,
        door_holders,
        blockers: report
            .as_ref()
            .map(|r| {
                r.blockers
                    .iter()
                    .map(|b| format!("{} {} ({})", b.what, b.addr, b.cause))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_names_the_builds_the_record_and_the_routes() {
        let s = RelayStatus {
            window_build: "1.6.0".into(),
            relay_build: "1.6.0".into(),
            record_age: Some(12),
            owner_pid: 4242,
            loopback: true,
            first_route: "свой прокси".into(),
            own_proxy: "настроен (2.26.0.77)".into(),
            proxy_port: 53129,
            proxy_answers: true,
            door_holders: vec![4242],
            blockers: Vec::new(),
        };
        let text = s.text();
        assert!(text.contains("служба 1.6.0"), "{text}");
        assert!(text.contains("владелец роли: 4242"), "{text}");
        assert!(text.contains("pid 4242"), "{text}");
        assert!(text.contains("первый маршрут: свой прокси"), "{text}");
        assert!(text.contains("блокировки: нет"), "{text}");
        assert!(!text.contains("ВНИМАНИЕ"), "{text}");
    }

    #[test]
    fn status_says_when_the_service_is_a_different_build() {
        let s = RelayStatus {
            window_build: "1.6.0".into(),
            relay_build: "1.5.9".into(),
            ..RelayStatus::default()
        };
        let text = s.text();
        assert!(text.contains("ВНИМАНИЕ"), "{text}");
        assert!(text.contains("записи службы нет"), "{text}");
    }
}
