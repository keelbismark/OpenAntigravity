use std::collections::HashMap;
use std::path::PathBuf;

use crate::settings::Settings;
use crate::utils::is_admin;
use crate::{background, dns, endpoint, patch_binary, proxy, upstream};

use super::types::*;
use super::worker::Ctx;

pub(super) fn read_status(ctx: &mut Ctx, deep: bool) -> Status {
    let admin = is_admin();

    let settings = &ctx.settings;
    let patched_seen = &ctx.patched_seen;
    let (installs, watchdog, dns_probe, local_proxy, own_proxy, own_proxy_text, relay_outdated) =
        std::thread::scope(|scope| {
            let installs = scope.spawn(move || collect_installs(settings, deep, patched_seen));
            let watchdog = scope.spawn(move || read_watchdog(admin));
            let dns_probe = scope.spawn(probe_dns);
            let local_proxy = scope.spawn(move || read_local_proxy(settings));
            let own_proxy = scope.spawn(move || read_own_proxy(settings));
            let own_proxy_text = scope.spawn(move || {
                upstream::configured_display()
                    .unwrap_or_else(|| settings.own_proxy.clone())
            });
            let relay_outdated = scope.spawn(background::relay_is_outdated);
            (
                installs.join().unwrap_or_default(),
                watchdog.join().unwrap_or(State::Off),
                dns_probe.join().unwrap_or((false, false)),
                local_proxy.join().unwrap_or(State::Off),
                own_proxy.join().unwrap_or(State::Off),
                own_proxy_text.join().unwrap_or_default(),
                relay_outdated.join().unwrap_or(false),
            )
        });
    ctx.dns_probe = dns_probe;

    let patched: Vec<bool> = installs.iter().filter_map(|i| i.patched).collect();
    let client_patch = if patched.is_empty() {
        if ctx.settings.client_patch {
            State::Partial("состояние ещё не проверено".into())
        } else {
            State::Off
        }
    } else if patched.iter().all(|p| *p) {
        State::On
    } else if patched.iter().any(|p| *p) {
        State::Partial("пропатчены не все установки".into())
    } else {
        State::Off
    };

    Status {
        admin,
        client_patch,
        watchdog,
        dns: dns_state(
            admin,
            dns_probe.0,
            dns_probe.1,
            ctx.vpn,
            ctx.settings.vpn_detect,
        ),
        local_proxy,
        own_proxy,
        own_proxy_text,
        builtin_exits: State::Off,
        verify_tls: State::Off,
        vpn: ctx.vpn,
        dns_rotation: State::Off,
        relay_outdated,
        relay_running: dns_probe.1,
        rules: dns_probe.0,
        relay_egress: ctx.relay_egress,
        providers: Vec::new(),
        proxy_profiles: Vec::new(),
        installs,
    }
    .with_settings_switches(&ctx.settings)
}

pub(super) fn measure_vpn() -> (VpnSeen, Option<crate::egress::ClientEgress>) {
    use crate::egress::ClientEgress;
    let egress = crate::egress::detect();
    if !egress.as_ref().is_some_and(|e| e.vpn_active) {
        return (VpnSeen::None, None);
    }
    let reading = crate::egress::read();
    let client = match reading.client {
        ClientEgress::Tunnel => VpnSeen::CarryingClient,
        ClientEgress::Mixed => VpnSeen::PartlyCarryingClient,
        ClientEgress::Physical => VpnSeen::NotCarryingClient,
        ClientEgress::ViaLocalProxy => VpnSeen::ViaLocalProxy,
        ClientEgress::Unknown => VpnSeen::Unmeasured,
    };
    (client, Some(reading.relay))
}

pub(super) fn vpn_change_line(now: VpnSeen) -> Option<&'static str> {
    match now {
        VpnSeen::None => Some("VPN отключён — обход снова работает сам."),
        VpnSeen::CarryingClient => Some(
            "Antigravity пошёл через VPN — снятие ошибки 400 теперь зависит от вашего сервера.",
        ),
        VpnSeen::NotCarryingClient => Some("Трафик Antigravity идёт мимо VPN — обход применяется."),
        VpnSeen::ViaLocalProxy => Some(
            "Antigravity ходит через локальный прокси — до серверов Google \
             соединение открывает служба обхода, а не он сам.",
        ),
        VpnSeen::PartlyCarryingClient => {
            Some("Часть трафика Antigravity пошла через VPN, часть идёт мимо.")
        }
        VpnSeen::Unmeasured => None,
    }
}

pub(super) fn read_watchdog(admin: bool) -> State {
    if background::is_watchdog_enabled() {
        State::On
    } else if !admin && cfg!(target_os = "windows") {
        State::Blocked("нужны права администратора".into())
    } else {
        State::Off
    }
}

pub(super) fn probe_dns() -> (bool, bool) {
    let rules = dns::is_nrpt_applied();
    let relay = background::is_enabled() && background::is_running() && relay_reporting();
    (rules, relay)
}

pub(super) fn relay_reporting() -> bool {
    !cfg!(target_os = "windows") || crate::gate::read().is_some_and(|r| !r.is_stale())
}

pub(super) fn dns_state(
    admin: bool,
    rules: bool,
    relay: bool,
    vpn: Option<VpnSeen>,
    detect_on: bool,
) -> State {
    let _ = (vpn, detect_on);
    match (rules, relay) {
        (true, true) => State::On,
        (true, false) => State::Partial("правила стоят, служба не запущена".into()),
        (false, true) => State::Partial("служба работает, правил нет".into()),
        (false, false) => {
            if !admin && cfg!(target_os = "windows") {
                State::Blocked("нужны права администратора".into())
            } else {
                State::Off
            }
        }
    }
}

pub(super) fn read_local_proxy(settings: &Settings) -> State {
    let url = proxy::proxy_url();
    if let Some(foreign) = endpoint::foreign_proxy(&url) {
        let _ = foreign;
        return State::Blocked("в системе задан свой прокси".into());
    }
    local_proxy_state(settings.local_proxy, endpoint::proxy_env_is_ours())
}

pub(super) fn local_proxy_state(wanted: bool, ours: bool) -> State {
    match (wanted, ours) {
        (true, true) => State::On,
        (true, false) => State::Partial("переменная ещё не выставлена".into()),
        (false, true) => State::OffNote("переменная ещё не снята".into()),
        (false, false) => State::Off,
    }
}

pub(super) fn read_own_proxy(settings: &Settings) -> State {
    if settings.own_proxy_enabled && upstream::configured().is_some() {
        State::On
    } else {
        State::Off
    }
}

const INSTALL_KINDS: [&str; 3] = ["Antigravity 2.0", "Antigravity IDE", "Antigravity CLI"];

pub(super) fn collect_installs(
    settings: &Settings,
    deep: bool,
    seen: &HashMap<PathBuf, bool>,
) -> Vec<InstallRow> {
    let mut chosen: Vec<(PathBuf, bool)> = settings
        .manual_paths
        .iter()
        .map(|p| (p.clone(), true))
        .collect();
    for path in crate::discover_installs_fast() {
        if !chosen.iter().any(|(p, _)| p == &path) {
            chosen.push((path, false));
        }
    }

    let mut rows: Vec<InstallRow> = Vec::new();
    for label in INSTALL_KINDS {
        let hit = chosen
            .iter()
            .find(|(p, _)| crate::install_label(p) == label)
            .cloned();
        rows.push(match hit {
            Some((path, manual)) => InstallRow {
                label,
                patched: None,
                manual,
                path: Some(path),
            },
            None => InstallRow {
                label,
                patched: None,
                manual: false,
                path: None,
            },
        });
    }
    for (path, manual) in chosen {
        if !manual || rows.iter().any(|r| r.path.as_ref() == Some(&path)) {
            continue;
        }
        rows.push(InstallRow {
            label: crate::install_label(&path),
            patched: None,
            manual: true,
            path: Some(path),
        });
    }

    for row in &mut rows {
        let Some(path) = row.path.clone() else {
            continue;
        };
        if deep {
            let state = patch_binary::inspect_install(&path);
            row.patched = if state.is_empty() {
                None
            } else {
                Some(state.fully_patched())
            };
        } else {
            row.patched = seen.get(&path).copied();
        }
    }
    rows
}
