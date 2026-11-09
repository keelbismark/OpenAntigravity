use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use crate::settings::Settings;
use crate::{background, dns, upstream};

use super::apply::*;
use super::status::*;
use super::types::*;

pub struct Worker {
    tx: Sender<Cmd>,
}

impl Worker {
    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }
}

pub fn spawn(events: Sender<Event>, wake: Box<dyn Fn() + Send>) -> Worker {
    let (tx, rx) = mpsc::channel::<Cmd>();
    let own = tx.clone();
    std::thread::Builder::new()
        .name("ops".to_string())
        .spawn(move || run_worker(rx, own, events, wake))
        .ok();
    Worker { tx }
}

pub(super) struct Ctx {
    pub(super) events: Sender<Event>,
    pub(super) wake: Box<dyn Fn() + Send>,
    pub(super) settings: Settings,
    pub(super) proxy_var_carried: Option<bool>,
    pub(super) proxy_var_retryable: bool,
    pub(super) patched_seen: HashMap<PathBuf, bool>,
    pub(super) vpn: Option<VpnSeen>,
    pub(super) relay_egress: Option<crate::egress::ClientEgress>,
    pub(super) last: Option<Status>,
    pub(super) dns_probe: (bool, bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Scan {
    Deep,
    System,
    Settings,
}

impl Ctx {
    pub(super) fn log(&self, level: Level, msg: impl Into<String>) {
        let _ = self.events.send(Event::Log(level, msg.into()));
        (self.wake)();
    }
    pub(super) fn busy(&self, what: Option<&str>) {
        let _ = self.events.send(Event::Busy(what.map(|s| s.to_string())));
        (self.wake)();
    }
}

static MEASURING_VPN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn run_worker(
    rx: Receiver<Cmd>,
    own: Sender<Cmd>,
    events: Sender<Event>,
    wake: Box<dyn Fn() + Send>,
) {
    let mut ctx = Ctx {
        events,
        wake,
        settings: Settings::load(),
        proxy_var_carried: None,
        proxy_var_retryable: false,
        patched_seen: HashMap::new(),
        vpn: None,
        relay_egress: None,
        last: None,
        dns_probe: (false, false),
    };

    settle_decline(&mut ctx);
    startup_housekeeping(&mut ctx);

    push_status(&mut ctx, Scan::System);

    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Stop => return,
            Cmd::Unlocked => {
                if auto_patch_found(&mut ctx) {
                    push_status(&mut ctx, Scan::Deep);
                } else {
                    push_status(&mut ctx, Scan::Deep);
                }
                ctx.busy(None);
            }
            Cmd::RemeasureVpn => {
                use std::sync::atomic::Ordering;
                if !MEASURING_VPN.swap(true, Ordering::SeqCst) {
                    let back = own.clone();
                    let spawned = std::thread::Builder::new()
                        .name("vpn-probe".to_string())
                        .spawn(move || {
                            let (vpn, relay_egress) = measure_vpn();
                            let relay = background::is_enabled() && background::is_running();
                            MEASURING_VPN.store(false, Ordering::SeqCst);
                            let _ = back.send(Cmd::Probed {
                                vpn,
                                relay,
                                relay_egress,
                            });
                        });
                    if spawned.is_err() {
                        MEASURING_VPN.store(false, Ordering::SeqCst);
                    }
                }
            }
            Cmd::Probed {
                vpn,
                relay,
                relay_egress,
            } => {
                let before = ctx.vpn;
                ctx.vpn = Some(vpn);
                ctx.relay_egress = relay_egress;
                if before.is_some() && before != ctx.vpn {
                    if let Some(line) = vpn_change_line(vpn) {
                        ctx.log(Level::Info, line);
                    }
                }
                ctx.dns_probe.1 = relay;
                push_status(&mut ctx, Scan::Settings);
            }
            Cmd::DeepRefresh => {
                ctx.busy(Some("Проверка установок"));
                push_status(&mut ctx, Scan::Deep);
                ctx.busy(None);
            }
            Cmd::Set(cap, on) => {
                ctx.busy(Some(cap.title()));
                apply(&mut ctx, cap, on);
                ctx.settings.save();
                push_status(&mut ctx, scan_after(cap));
                if cap == Cap::Watchdog && on && auto_patch_found(&mut ctx) {
                    push_status(&mut ctx, Scan::Deep);
                }
                ctx.busy(None);
            }
            Cmd::AddPath(p) => {
                if !ctx.settings.manual_paths.contains(&p) {
                    ctx.settings.manual_paths.push(p);
                    ctx.settings.save();
                }
                ctx.busy(Some("Проверка установок"));
                push_status(&mut ctx, Scan::Deep);
                if auto_patch_found(&mut ctx) {
                    push_status(&mut ctx, Scan::Deep);
                }
                ctx.busy(None);
            }
            Cmd::ForgetPath(p) => {
                ctx.settings.manual_paths.retain(|x| x != &p);
                ctx.settings.save();
                push_status(&mut ctx, Scan::System);
            }
            Cmd::SetOwnProxy(text) => {
                ctx.busy(Some("Проверка прокси"));
                set_own_proxy(&mut ctx, &text);
                ctx.settings.save();
                push_status(&mut ctx, Scan::System);
                ctx.busy(None);
            }
            Cmd::SaveProxyProfile { name, address } => {
                let name = name.trim().to_string();
                let address = address.trim().to_string();
                if !name.is_empty() && !address.is_empty() {
                    if let Some(pos) = ctx.settings.proxy_profiles.iter().position(|p| p.name == name) {
                        ctx.settings.proxy_profiles[pos].address = address;
                    } else {
                        ctx.settings.proxy_profiles.push(crate::settings::ProxyProfile { name, address });
                    }
                    ctx.settings.save();
                    push_status(&mut ctx, Scan::System);
                }
            }
            Cmd::DeleteProxyProfile(index) => {
                if index < ctx.settings.proxy_profiles.len() {
                    ctx.settings.proxy_profiles.remove(index);
                    ctx.settings.save();
                    push_status(&mut ctx, Scan::System);
                }
            }
            Cmd::SelectProxyProfile(index) => {
                if let Some(profile) = ctx.settings.proxy_profiles.get(index).cloned() {
                    ctx.busy(Some("Переключение прокси"));
                    set_own_proxy(&mut ctx, &profile.address);
                    ctx.settings.save();
                    push_status(&mut ctx, Scan::System);
                    ctx.busy(None);
                }
            }
            Cmd::EnableAll => {
                ctx.busy(Some("Включение компонентов"));
                enable_all(&mut ctx);
                ctx.settings.save();
                push_status(&mut ctx, Scan::Deep);
                ctx.busy(None);
            }
            Cmd::DisableAll => {
                ctx.busy(Some("Отключение компонентов"));
                disable_all(&mut ctx);
                ctx.settings.save();
                push_status(&mut ctx, Scan::Deep);
                ctx.busy(None);
            }
            Cmd::Repair => {
                ctx.busy(Some("Восстановление службы обхода"));
                repair(&mut ctx);
                ctx.settings.save();
                push_status(&mut ctx, Scan::System);
                ctx.busy(None);
            }
            Cmd::KillHolder => {
                ctx.busy(Some("Освобождение порта"));
                let target = crate::gate::read().and_then(|r| {
                    r.blockers
                        .into_iter()
                        .find(|b| b.cause == "held" && b.ours && !b.by.is_empty())
                });
                match target {
                    Some(b) => match crate::portcheck::kill_holding_process(b.pid, &b.by) {
                        Ok(pid) => ctx.log(
                            Level::Info,
                            format!(
                                "Порт {} освобождён: «{}» (pid {}) завершён.",
                                b.addr, b.by, pid
                            ),
                        ),
                        Err(e) => ctx.log(
                            Level::Warn,
                            format!(
                                "Не удалось завершить «{}» (порт {}): {}. Закройте её вручную \
                                 и нажмите «Освободить порт» ещё раз.",
                                b.by, b.addr, e
                            ),
                        ),
                    },
                    None => ctx.log(
                        Level::Info,
                        "Занимающая порт копия уже не найдена — просто пересобираю обход.",
                    ),
                }
                match background::quiet_relay_tasks() {
                    Ok(true) => ctx.log(
                        Level::Info,
                        "Задача старой копии отключена — порт больше не вернётся сам.",
                    ),
                    Ok(false) => {}
                    Err(e) => ctx.log(
                        Level::Warn,
                        format!(
                            "Задачу, которая запускает старую копию, отключить не удалось ({e}). \
                             Порт вернётся через минуту. Запустите программу от имени \
                             администратора и повторите, либо выполните от администратора: \
                             schtasks /Change /TN \"AG Unlocker DNS\" /Disable и \
                             schtasks /Change /TN \"Open Antigravity DNS\" /Disable"
                        ),
                    ),
                }
                repair(&mut ctx);
                ctx.settings.save();
                push_status(&mut ctx, Scan::System);
                ctx.busy(None);
            }
            Cmd::AutoFix(action) => {
                ctx.busy(Some(action.label()));
                match crate::platform::diag::execute_fix(action) {
                    Ok(msg) => {
                        ctx.log(Level::Ok, msg);
                    }
                    Err(e) => {
                        ctx.log(Level::Warn, format!("Сбой автоисправления: {e}"));
                    }
                }
                repair(&mut ctx);
                ctx.settings.save();
                push_status(&mut ctx, Scan::Deep);
                ctx.busy(None);
            }
            Cmd::ReorderProviders(order) => {
                ctx.settings.provider_order = order;
                ctx.settings.save();
                ctx.log(
                    Level::Info,
                    match first_enabled_provider(&ctx.settings) {
                        Some(n) => format!("Порядок DNS изменён, первый — {}", n),
                        None => "Порядок DNS изменён.".to_string(),
                    },
                );
                push_status(&mut ctx, Scan::Settings);
            }
            Cmd::SetProvider(name, on) => {
                ctx.settings.set_provider_enabled(&name, on);
                if all_providers_disabled(&ctx.settings) {
                    ctx.settings.set_provider_enabled(&name, true);
                    ctx.log(
                        Level::Warn,
                        "Нельзя отключить все DNS-серверы — хотя бы один должен остаться.",
                    );
                } else if !on && no_usable_nameserver(&ctx.settings) {
                    ctx.settings.set_provider_enabled(&name, true);
                    ctx.log(
                        Level::Warn,
                        "Этот сервер нельзя отключить: он остался единственным, который Windows может использовать как DNS-сервер напрямую.",
                    );
                } else {
                    ctx.log(
                        Level::Info,
                        format!(
                            "DNS-сервер {} {}",
                            name,
                            if on {
                                "включён"
                            } else {
                                "отключён"
                            }
                        ),
                    );
                }
                ctx.settings.save();
                let rewrote = reapply_dns_rules(&mut ctx);
                push_status(
                    &mut ctx,
                    if rewrote {
                        Scan::System
                    } else {
                        Scan::Settings
                    },
                );
            }
        }
    }
}

fn startup_housekeeping(ctx: &mut Ctx) {
    if !cfg!(target_os = "windows") || !crate::utils::is_admin() {
        return;
    }
    let before = dns::is_nrpt_applied();
    dns::refresh_pinned_hosts();
    dns::invalidate_cache();
    if before && !dns::is_nrpt_applied() {
        ctx.log(
            Level::Warn,
            "Antigravity идёт через активный VPN — правила DNS сняты, чтобы не перебивать \
             ваш туннель. Выключите VPN и включите «Обход через DNS» заново.",
        );
    }
    if ctx.settings.dns || before {
        if let Err(e) = background::ensure_running() {
            ctx.log(Level::Warn, format!("Автозапуск службы DNS: {}", e));
        }
    }
}

pub(super) fn scan_after(cap: Cap) -> Scan {
    match cap {
        Cap::ClientPatch => Scan::Deep,
        Cap::Watchdog | Cap::Dns | Cap::LocalProxy | Cap::OwnProxy | Cap::DnsRotation => {
            Scan::System
        }
        Cap::BuiltinExits | Cap::VerifyTls => Scan::Settings,
    }
}

pub(super) fn push_status(ctx: &mut Ctx, scan: Scan) {
    if scan == Scan::Deep {
        let (vpn, relay_egress) = measure_vpn();
        ctx.vpn = Some(vpn);
        ctx.relay_egress = relay_egress;
    } else if ctx.vpn.is_none() {
        ctx.vpn = Some(VpnSeen::None);
    }

    let status = match scan {
        Scan::Settings => match ctx.last.clone() {
            Some(prev) => settings_only_status(ctx, prev),
            None => read_status(ctx, false),
        },
        Scan::System => read_status(ctx, false),
        Scan::Deep => read_status(ctx, true),
    };

    if scan == Scan::Deep {
        ctx.patched_seen = status
            .installs
            .iter()
            .filter_map(|r| match (&r.path, r.patched) {
                (Some(p), Some(v)) => Some((p.clone(), v)),
                _ => None,
            })
            .collect();
    }
    ctx.last = Some(status.clone());
    let _ = ctx.events.send(Event::Status(Box::new(status)));
    (ctx.wake)();
}

pub(super) fn settings_only_status(ctx: &Ctx, prev: Status) -> Status {
    let (rules, relay) = ctx.dns_probe;
    Status {
        dns: dns_state(prev.admin, rules, relay, ctx.vpn, ctx.settings.vpn_detect),
        relay_running: relay,
        rules,
        own_proxy_text: upstream::configured_display()
            .unwrap_or_else(|| ctx.settings.own_proxy.clone()),
        vpn: ctx.vpn,
        relay_egress: ctx.relay_egress,
        ..prev
    }
    .with_settings_switches(&ctx.settings)
}
