use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::utils::is_admin;
use crate::{background, dns, endpoint, patch_binary, patch_ide, proxy, upstream};

use super::types::*;
use super::worker::Ctx;

pub(super) fn apply(ctx: &mut Ctx, cap: Cap, on: bool) {
    if cap.needs_admin() && !is_admin() {
        ctx.log(
            Level::Err,
            "Нужны права администратора. Нажмите «Перезапустить от имени администратора».",
        );
        return;
    }

    match (cap, on) {
        (Cap::ClientPatch, true) => {
            // From the click, unlike the line below: the user wants the patch
            // whether or not an install was found to take it, and auto-patch
            // puts it on whatever turns up later (D27).
            ctx.settings.patch_declined = false;
            // Set from the result, not from the click: a run that found no
            // install patched nothing, and persisting `true` there would leave
            // the switch reading On for ever on a machine with no Antigravity.
            ctx.settings.client_patch = enable_client_patch(ctx);
            // Switching the patch off took the watchdog task down with it. The
            // relay's own watchdog thread acts again the moment the decline is
            // gone, so without the task back the switch would read off while
            // patching went on. `set_watchdog` no longer starts the relay, so
            // this does not depend on the bypass being wanted.
            if is_admin() && ctx.settings.auto_patch_wanted() && !background::is_watchdog_enabled()
            {
                set_watchdog(ctx, true);
            }
        }
        (Cap::ClientPatch, false) => {
            ctx.settings.client_patch = false;
            // The one wish auto-patch has to respect (G38). Saved before
            // anything is unpatched, so a watchdog reading the file mid-run
            // already sees it.
            ctx.settings.patch_declined = true;
            if !ctx.settings.save() {
                ctx.log(
                    Level::Warn,
                    "Настройки не сохранились (файл занят?) — автопатч может вернуть патч. Выключите и «Автопатч».",
                );
            }
            disable_client_patch(ctx);
        }
        (Cap::Watchdog, on) => {
            ctx.settings.auto_patch = on;
            // Auto-patch switched on is the newer wish, and it means "keep
            // Antigravity patched" - a patch declined earlier would otherwise
            // leave the switch on and the watchdog doing nothing.
            if on {
                ctx.settings.patch_declined = false;
            }
            set_watchdog(ctx, on);
        }
        (Cap::Dns, on) => {
            ctx.settings.dns = on;
            if on {
                enable_dns(ctx)
            } else {
                disable_dns(ctx)
            }
        }
        (Cap::LocalProxy, on) => {
            ctx.settings.local_proxy = on;
            if on {
                enable_local_proxy(ctx)
            } else {
                disable_local_proxy(ctx)
            }
        }
        (Cap::OwnProxy, on) => {
            ctx.settings.own_proxy_enabled = on;
            if on {
                let text = ctx.settings.own_proxy.clone();
                if text.trim().is_empty() {
                    ctx.log(
                        Level::Warn,
                        "Впишите адрес прокси, затем включите переключатель.",
                    );
                    ctx.settings.own_proxy_enabled = false;
                } else {
                    set_own_proxy(ctx, &text);
                }
            } else {
                upstream::clear();
                ctx.log(Level::Ok, "Свой прокси отключён.");
            }
        }
        (Cap::DnsRotation, on) => {
            ctx.settings.rotate_providers = on;
            ctx.settings.save();
            reapply_dns_rules(ctx);
            ctx.log(
                Level::Ok,
                if on {
                    "Ротация включена — запрос идёт ко всем включённым серверам, ответ сверяется с эталонным."
                        .to_string()
                } else {
                    match first_enabled_provider(&ctx.settings) {
                        Some(name) => format!(
                            "Ротация выключена — используется только {}. Запасных серверов не будет.",
                            name
                        ),
                        None => "Ротация выключена, но все серверы отключены.".to_string(),
                    }
                },
            );
        }
        (Cap::VerifyTls, on) => {
            ctx.settings.verify_tls = on;
            ctx.log(
                Level::Ok,
                if on {
                    "Сверка TLS включена: адрес принимается, только если предъявил настоящий сертификат Google."
                } else {
                    "Сверка TLS выключена. Подменённый адрес больше не проверяется — остаётся только проверка, что он вообще отвечает."
                },
            );
        }
        (Cap::BuiltinExits, on) => {
            ctx.settings.builtin_exits = on;
            ctx.log(
                Level::Ok,
                if on {
                    "Встроенные выходы включены."
                } else {
                    "Встроенные выходы отключены."
                },
            );
        }
    }
}

// --- the two buttons --------------------------------------------------------

/// «Включить всё». Each part only if it is not on already, and in the order the
/// switches themselves need: the patch first (it closes Antigravity, so nothing
/// is holding a file), then the relay (I5 - the rules name it), the proxy
/// variable only once something answers on its port (I53), and auto-patch last,
/// because it runs out of the relay's install directory.
pub(super) fn enable_all(ctx: &mut Ctx) {
    let admin = is_admin() || !cfg!(target_os = "windows");
    // "Everything" includes the patch on installs that do not exist yet.
    ctx.settings.patch_declined = false;
    // Only when it is not already on everywhere: patching closes Antigravity,
    // and closing it for a no-op is exactly the kind of surprise this button
    // must not have.
    if !patch_is_on(ctx) || ctx.proxy_var_carried != Some(true) {
        apply(ctx, Cap::ClientPatch, true);
    }
    // Nothing but a line in settings.json, so it needs nobody's permission.
    if !ctx.settings.builtin_exits {
        apply(ctx, Cap::BuiltinExits, true);
    }
    let dns_on =
        dns::is_nrpt_applied() && background::is_running() && !background::relay_is_outdated();
    if !dns_on {
        if !admin {
            // Said only when it is actually missing: the card offers this button
            // for a patch that is off too, with the bypass already running.
            ctx.log(
                Level::Warn,
                "Обход ошибки 400 не включён: нужны права администратора. Нажмите «Перезапустить от имени администратора».",
            );
            return;
        }
        apply(ctx, Cap::Dns, true);
    }
    // A proxy the user set up themselves stays theirs (G33): `enable_local_proxy`
    // says so and leaves ours off, which also keeps the loopback door shut.
    if !endpoint::proxy_env_is_ours() {
        apply(ctx, Cap::LocalProxy, true);
    }
    // Whether or not the patch found anything to patch: auto-patch is what
    // patches the Antigravity installed after this click (D27).
    if admin && !background::is_watchdog_enabled() {
        apply(ctx, Cap::Watchdog, true);
    }
    ctx.log(
        Level::Ok,
        "Готово. Откройте Antigravity и напишите что-нибудь в чат — здесь появится подтверждение, что модель ответила.",
    );
}

pub(super) fn disable_all(ctx: &mut Ctx) {
    if endpoint::proxy_env_is_ours() {
        apply(ctx, Cap::LocalProxy, false);
    }
    if ctx.settings.builtin_exits {
        apply(ctx, Cap::BuiltinExits, false);
    }
    apply(ctx, Cap::Dns, false);
    if background::is_watchdog_enabled() {
        apply(ctx, Cap::Watchdog, false);
    }
    if patch_is_on(ctx) {
        apply(ctx, Cap::ClientPatch, false);
    }
    ctx.patched_seen.clear();
    ctx.log(Level::Ok, "Все компоненты обхода и патча отключены.");
}

pub(super) fn patch_is_on(ctx: &Ctx) -> bool {
    ctx.last
        .as_ref()
        .is_some_and(|s| s.client_patch == State::On)
}

/// «Перезапустить». The service is the part that can stop on its own - a crash the
/// task's restart policy gave up on, a relay older than this build, an antivirus
/// that ate the copy in `%ProgramData%` - and every one of those is fixed by
/// installing it again and putting its rules and variable back.
pub(super) fn repair(ctx: &mut Ctx) {
    if cfg!(target_os = "windows") && !is_admin() {
        ctx.log(
            Level::Err,
            "Нужны права администратора. Нажмите «Перезапустить от имени администратора».",
        );
        return;
    }
    ctx.settings.dns = true;
    // `ensure_running` reinstalls only a copy that differs from this build;
    // a relay that is merely stopped or outdated needs the full `enable`.
    if let Err(e) = background::enable() {
        ctx.log(Level::Err, format!("Служба обхода не запустилась: {}", e));
        return;
    }
    ctx.log(Level::Ok, "Служба обхода переустановлена и запущена.");
    enable_dns(ctx);
}

// --- client patch ----------------------------------------------------------

/// Settles `patch_declined` for a settings file an older build wrote (D27).
///
/// Before D27 a patch switched off by hand left two traces: `client_patch`
/// false, and the standalone watchdog task removed (`disable_client_patch`).
/// `parse` could only read the first and so took every patch-off file as
/// declined. The task is the second trace, and only this side can ask for it:
/// with it still registered the user saw auto-patch *on*, and it may act -
/// which is the owner's own machine (G55). Written back at once, so the relay
/// reads the settled answer and this runs once per machine.
pub(super) fn settle_decline(ctx: &mut Ctx) {
    if !ctx.settings.decline_unrecorded {
        return;
    }
    ctx.settings.patch_declined = !ctx.settings.client_patch && !background::is_watchdog_enabled();
    ctx.settings.decline_unrecorded = false;
    ctx.settings.save();
}

/// Auto-patch from the window (D27): every install the last deep scan found
/// unpatched is patched now, without a click, while auto-patch is on and the
/// patch was not switched off by hand. True when something was written, so the
/// caller knows the dots need a deep re-read.
///
/// The watchdog's own primitive, not `enable_client_patch`: that one closes
/// every Antigravity process up front, which is right for a button the user
/// pressed and wrong for something nobody asked for this second. This one
/// closes a process only when a binary it holds cannot be written otherwise -
/// and Desktop restarts its language server by itself and reloads the window,
/// which is what brings a black window back to life.
pub(super) fn auto_patch_found(ctx: &mut Ctx) -> bool {
    // Only while the switch is drawn on («при включенной опции»). Its state is
    // the standalone task, which a user without admin rights cannot have and
    // which `disable_dns` takes down with the relay («вернётся вместе с
    // обходом»); patching behind a switch that reads off is the surprise the
    // switch exists to rule out.
    let switch_on = ctx.last.as_ref().is_some_and(|s| s.watchdog.is_on());
    if !switch_on || !ctx.settings.auto_patch_wanted() {
        return false;
    }
    let found: Vec<(&'static str, PathBuf)> = ctx
        .last
        .as_ref()
        .map(|s| {
            s.installs
                .iter()
                .filter(|r| r.patched == Some(false))
                .filter_map(|r| Some((r.label, r.path.clone()?)))
                .collect()
        })
        .unwrap_or_default();
    if found.is_empty() {
        return false;
    }

    ctx.busy(Some("Автопатч"));
    let mut wrote = false;
    for (label, inst) in found {
        // Never a password prompt nobody asked for: the automatic pass leaves a
        // root-owned install to the button, which asks.
        if crate::elevate::needs_root(&inst) {
            ctx.log(
                Level::Warn,
                format!(
                    "Автопатч: {} — установка принадлежит root. Нажмите «Включить всё» и введите пароль.",
                    label
                ),
            );
            continue;
        }
        let mut patched = 0usize;
        let mut unknown = 0usize;
        let mut failed: Option<String> = None;
        for (_, outcome) in crate::watchdog::patch_now(&inst) {
            match outcome {
                patch_binary::RepatchOutcome::Repatched(_) => patched += 1,
                patch_binary::RepatchOutcome::AlreadyPatched => {}
                patch_binary::RepatchOutcome::SignatureMissing => unknown += 1,
                patch_binary::RepatchOutcome::Failed(e) => failed = Some(e),
            }
        }
        let where_ = crate::utils::mask_path(&inst.display().to_string());
        if patched > 0 {
            wrote = true;
            ctx.log(
                Level::Ok,
                format!("Автопатч: {} — патч наложен ({})", label, where_),
            );
        }
        if let Some(e) = failed {
            ctx.log(
                Level::Warn,
                format!(
                    "Автопатч: {} — не удалось записать ({}). Закройте Antigravity и нажмите «Включить всё».",
                    label, e
                ),
            );
        } else if unknown > 0 && patched == 0 {
            // The watchdog's rule (I19): a build this patcher does not know is
            // left exactly as it is, so Antigravity shows its own error.
            ctx.log(
                Level::Warn,
                format!(
                    "Автопатч: {} — эта сборка Antigravity анлокеру ещё не знакома, файлы не тронуты. Нужна новая версия анлокера.",
                    label
                ),
            );
        }
    }
    if wrote {
        // What the switch falls back to before the next deep read, and what an
        // older build reading this file takes as the user's wish.
        ctx.settings.client_patch = true;
        ctx.settings.save();
    }
    // Left raised: every caller re-reads the installs next and lowers it after.
    wrote
}

fn enable_client_patch(ctx: &mut Ctx) -> bool {
    ctx.log(Level::Step, "Патч клиента Antigravity");
    patch_binary::kill_affected_processes();

    let installs = crate::find_all_installs();
    let manual: Vec<PathBuf> = ctx.settings.manual_paths.clone();
    let mut targets = installs;
    for p in manual {
        if !targets.contains(&p) {
            targets.push(p);
        }
    }

    if targets.is_empty() {
        ctx.log(
            Level::Err,
            "Установки Antigravity не найдены. Укажите путь вручную (карандаш).",
        );
        return false;
    }

    // Recomputed every run: see Ctx::proxy_var_carried.
    ctx.proxy_var_carried = Some(false);
    ctx.proxy_var_retryable = false;

    let mut ok = 0usize;
    for inst in &targets {
        // A root-owned Linux install: only this write is done as root, behind
        // the desktop's password prompt (`elevate`).
        if crate::elevate::needs_root(inst) {
            let where_ = crate::utils::mask_path(&inst.display().to_string());
            ctx.log(
                Level::Step,
                format!("{where_} принадлежит root — запрашиваю пароль для записи патча"),
            );
            match crate::elevate::patch_as_root(inst) {
                Ok(done) => {
                    ok += 1;
                    if done.proxy_var {
                        ctx.proxy_var_carried = Some(true);
                    }
                    if done.proxy_var_retryable {
                        ctx.proxy_var_retryable = true;
                    }
                    ctx.log(Level::Ok, format!("{} — {}", done.label, where_));
                    for w in done.warnings {
                        ctx.log(Level::Warn, w);
                    }
                }
                Err(e) => ctx.log(Level::Err, format!("{where_} — {e}")),
            }
            continue;
        }
        match crate::process_install(inst) {
            Ok(outcome) => {
                ok += 1;
                if outcome.summary.proxy_var > 0 {
                    ctx.proxy_var_carried = Some(true);
                }
                if outcome.summary.proxy_var_retryable {
                    ctx.proxy_var_retryable = true;
                }
                ctx.log(
                    Level::Ok,
                    format!(
                        "{} — {}",
                        outcome.label,
                        crate::utils::mask_path(&inst.display().to_string())
                    ),
                );
                for w in outcome.warnings {
                    ctx.log(Level::Warn, w);
                }
            }
            Err(e) => ctx.log(
                Level::Err,
                format!(
                    "{} — {}",
                    crate::utils::mask_path(&inst.display().to_string()),
                    e
                ),
            ),
        }
    }

    if ok == 0 {
        ctx.log(Level::Err, "Ни одна установка не пропатчена.");
        return false;
    }

    // I39: the legacy CA comes out last, and `remove_legacy_ca` itself refuses
    // while the relay is still an older build.
    crate::remove_legacy_ca_quiet();

    ctx.log(Level::Ok, format!("Пропатчено установок: {}", ok));
    true
}

fn disable_client_patch(ctx: &mut Ctx) {
    ctx.log(Level::Step, "Снятие патча клиента");

    // I20: a live watchdog treats an unpatch as an update and puts the patch
    // straight back. Both of them stop first — the standalone task and the one
    // that runs inside the relay — and are restored below to whatever the user
    // still has switched on.
    // The relay comes back if the user still wants the DNS bypass. The watchdog
    // does NOT: a watchdog is a process whose whole job is to put the patch back.
    // Restoring it here undid the unpatch about four seconds later and flipped
    // the switch on again. The relay's own watchdog thread comes back with the
    // relay and stays idle, because `patch_declined` is already on disk - the
    // caller saved it before calling this, and the relay reads the file fresh
    // when it starts (a save after the relay was up would lose the race to its
    // first two polls). `settings.auto_patch` keeps its value, so the watchdog
    // returns the moment the patch does.
    let want_relay = ctx.settings.dns;
    background::disable_watchdog();
    if let Err(e) = background::disable() {
        ctx.log(Level::Warn, format!("Служба DNS не остановлена: {}", e));
    }

    patch_binary::kill_affected_processes();

    let mut targets = crate::find_all_installs();
    for p in ctx.settings.manual_paths.clone() {
        if !targets.contains(&p) {
            targets.push(p);
        }
    }

    // I45: every step runs, each judged on its own. No `?` between them.
    for inst in &targets {
        let where_ = crate::utils::mask_path(&inst.display().to_string());
        let mut reverted = 0usize;
        // Each file is judged on its own (I45), and a failure is said out loud:
        // a count alone turned "one of two files could not be written" into a
        // clean revert, and the user kept a half-patched install.
        for (label, res) in patch_binary::unpatch_all_binaries(inst) {
            match res {
                Ok(0) => {}
                Ok(_) => reverted += 1,
                Err(e) => ctx.log(Level::Err, format!("{} — {}: {}", where_, label, e)),
            }
        }
        for (label, res) in patch_ide::unpatch_ide_js(inst) {
            match res {
                Ok(true) => reverted += 1,
                Ok(false) => ctx.log(
                    Level::Warn,
                    format!("{} — {}: бэкапа нет, снят только маркер", where_, label),
                ),
                Err(e) => ctx.log(Level::Err, format!("{} — {}: {}", where_, label, e)),
            }
        }
        if let Err(e) = crate::restore_pristine_asar(&inst.join("resources")) {
            ctx.log(Level::Warn, format!("app.asar не восстановлен: {}", e));
        }
        if let Err(e) = endpoint::remove_ide(inst) {
            ctx.log(Level::Warn, format!("Оверрайд эндпоинта не снят: {}", e));
        }
        ctx.log(
            Level::Ok,
            format!("{} — возвращено файлов: {}", where_, reverted),
        );
    }
    if let Err(e) = endpoint::remove_cli() {
        ctx.log(Level::Warn, format!("Переменная CLI не снята: {}", e));
    }

    // The proxy variable is only read by a patched binary, so an unpatched
    // machine must not keep it: it would sit in the user environment forever,
    // naming a port nothing listens on.
    disable_local_proxy(ctx);
    ctx.proxy_var_carried = Some(false);

    if want_relay {
        if let Err(e) = background::ensure_running() {
            ctx.log(
                Level::Warn,
                format!("Служба DNS не поднялась обратно: {}", e),
            );
        }
    }
}

// --- watchdog --------------------------------------------------------------

fn set_watchdog(ctx: &mut Ctx, on: bool) {
    if !on {
        background::disable_watchdog();
        ctx.log(Level::Ok, "Автовосстановление патча отключено.");
        return;
    }
    // The task launches the copy in %ProgramData%, so it has to be there - but
    // only the file: starting the relay here switched the 400 bypass on for a
    // user who asked for auto-patch alone.
    if let Err(e) = background::ensure_installed() {
        ctx.log(
            Level::Warn,
            format!("Не удалось установить копию для автопатча: {}", e),
        );
    }
    match background::enable_watchdog() {
        Ok(()) => ctx.log(Level::Ok, "Автовосстановление патча включено."),
        Err(e) => ctx.log(Level::Err, format!("Не удалось включить: {}", e)),
    }
}

// --- DNS -------------------------------------------------------------------

fn enable_dns(ctx: &mut Ctx) {
    ctx.log(Level::Step, "Обход через DNS");

    // I5: the rules name the relay as their first nameserver, so it has to be
    // answering before they are written.
    if let Err(e) = background::ensure_running() {
        ctx.log(Level::Err, format!("Служба DNS не запустилась: {}", e));
        return;
    }
    ctx.log(Level::Ok, "Локальная служба DNS запущена.");

    match dns::setup_dns_nrpt() {
        Ok(outcome) => {
            dns::invalidate_cache();
            // The rules are in; what the client cached before them is not ours
            // to keep (G75). The relay does the same at its own start.
            dns::flush_client_cache();
            if outcome.stood_down_for_vpn {
                // Not a failure, and not "on" either: with the client measured
                // inside a tunnel the rules would override the resolver the user
                // turned on, and the substituted address is reached through the
                // tunnel anyway (D13, G26, G29).
                ctx.log(
                    Level::Warn,
                    "Обнаружен активный VPN, через который идёт Antigravity — \
                     правила DNS не создавались. Выключите VPN и включите переключатель заново.",
                );
            } else {
                ctx.log(Level::Ok, "Правила DNS установлены.");
            }
            if let Some(note) = dns::outcome_note(&outcome) {
                ctx.log(Level::Info, strip_ansi(&note));
            }
        }
        Err(e) => ctx.log(Level::Err, format!("Правила DNS не установлены: {}", e)),
    }

    // Both of these were taken down *by* `disable_dns` rather than by the user,
    // so they come back with it. Anything the user switched off themselves stays
    // off: these read the saved wish, not the previous system state.
    if ctx.settings.local_proxy && !endpoint::proxy_env_is_ours() {
        enable_local_proxy(ctx);
    }
    if ctx.settings.auto_patch_wanted() && !background::is_watchdog_enabled() {
        set_watchdog(ctx, true);
    }
}

fn disable_dns(ctx: &mut Ctx) {
    ctx.log(Level::Step, "Отключение обхода через DNS");

    // First, because the listener is about to stop existing: the local proxy
    // runs *inside* the relay, so stopping the relay leaves `OAG_GATEWAY`
    // naming a port nothing answers on, and Antigravity loses its sign-in with
    // `dial tcp 127.0.0.1: ... actively refused` (I53, G31). The user's wish for
    // the proxy is kept in settings, so it comes back when the relay does.
    if endpoint::proxy_env_is_ours() {
        disable_local_proxy(ctx);
    }

    // The standalone watchdog task launches the copy of this exe that
    // `background::disable` is about to delete. It is taken down first and put
    // back after, on a copy of its own: auto-patch is not part of the bypass,
    // and switching the bypass off must not switch it off too.
    let restore_watchdog = ctx.settings.auto_patch && background::is_watchdog_enabled();
    if restore_watchdog {
        background::disable_watchdog();
    }

    // I45: both steps run regardless of what the first one did.
    dns::remove_dns_nrpt();
    dns::invalidate_cache();
    ctx.log(Level::Ok, "Правила DNS сняты.");
    match background::disable() {
        Ok(()) => ctx.log(Level::Ok, "Локальная служба DNS остановлена."),
        Err(e) => ctx.log(
            Level::Warn,
            format!("Служба остановлена не полностью: {}", e),
        ),
    }
    if restore_watchdog {
        set_watchdog(ctx, true);
    }
}

/// Re-writes the NRPT rules when the pool they name has changed.
///
/// A rule lists one address per enabled provider, so switching a provider off —
/// or turning rotation off, which narrows the list to one — changes what Windows
/// is pointed at. Without this the switch looks like it did nothing: the pool
/// stops asking that provider, but the machine keeps it as a nameserver until
/// the DNS switch is toggled off and on again.
/// Returns whether it actually went near the system — the caller uses that to
/// decide whether the following refresh has anything new to read.
pub(super) fn reapply_dns_rules(ctx: &mut Ctx) -> bool {
    if !dns::is_nrpt_applied() {
        return false;
    }
    if !is_admin() && cfg!(target_os = "windows") {
        ctx.log(
            Level::Warn,
            "Список DNS сохранён, но правила не переписаны — нужны права администратора.",
        );
        return false;
    }
    match dns::setup_dns_nrpt() {
        Ok(_) => {
            dns::invalidate_cache();
            dns::flush_client_cache();
            ctx.log(Level::Ok, "Правила DNS переписаны под новый список.");
        }
        Err(e) => ctx.log(Level::Warn, format!("Правила DNS не переписаны: {}", e)),
    }
    true
}

// --- local proxy -----------------------------------------------------------

fn enable_local_proxy(ctx: &mut Ctx) {
    ctx.log(Level::Step, "Локальный прокси");
    let url = proxy::proxy_url();

    // 1. The user-wide pair that builds up to 2.11.0_4 wrote. It goes first:
    //    leaving it puts the whole machine through loopback (D19).
    match endpoint::remove_legacy_proxy_env(&url) {
        Ok(true) => ctx.log(Level::Ok, "Прежняя общесистемная HTTPS_PROXY снята."),
        Ok(false) => {}
        Err(e) => {
            ctx.log(Level::Err, format!("Прежняя HTTPS_PROXY не снята: {}", e));
            return;
        }
    }

    // 2. Measured (G33): ours outranks theirs inside the patched server. A user
    //    who configured their own proxy must keep it, so ours comes off — this
    //    is not a "skip".
    if endpoint::foreign_proxy(&url).is_some() {
        match endpoint::remove_proxy(&url, "") {
            Ok(()) => {
                ctx.log(
                    Level::Warn,
                    "В системе задан свой прокси — наш не включаем, чтобы не перехватывать чужой.",
                );
                ctx.settings.local_proxy = false;
            }
            // Swallowed by a `let _ =` until `2.15.1_2`, and that is one way the
            // two readers came apart: the setting went off while the variable
            // stayed set, so the relay stopped writing it and the window went on
            // drawing it as on (G74). A removal that did not happen must not be
            // recorded as a switch that did.
            Err(e) => ctx.log(
                Level::Err,
                format!(
                    "В системе задан свой прокси, но нашу переменную снять не удалось: {}",
                    e
                ),
            ),
        }
        return;
    }

    // 3. The variable has a private name that only a patched binary reads. If
    //    the rename did not land, writing it is writing into nothing.
    if ctx.proxy_var_carried == Some(false) {
        if patch_is_on(ctx) {
            enable_client_patch(ctx);
        }
        if ctx.proxy_var_carried == Some(false) {
            ctx.log(
                Level::Err,
                if ctx.proxy_var_retryable {
                    "Файл был занят — закройте Antigravity полностью и включите переключатель заново."
                } else {
                    "В этой сборке Antigravity нет места для переменной прокси — \
                     вероятно, вышла новая версия и нужен более новый анлокер."
                },
            );
            return;
        }
    }

    // 4. I53/G31: never name a listener that is not there. A variable pointing
    //    at a dead port takes the sign-in down with it.
    //    Named by the port that answered, not the one read above: the relay can
    //    move the proxy while this waits (P26), and a URL for the port it left
    //    would be a dead one no watchdog takes back off.
    let Some(port) = proxy::wait_for_our_listener(Duration::from_secs(3)) else {
        ctx.log(
            Level::Err,
            "Локальный прокси не отвечает — сначала включите «Обход через DNS».",
        );
        return;
    };
    let url = proxy::url_at(port);

    match endpoint::apply_proxy(&url, "") {
        Ok(_) => ctx.log(Level::Ok, "Локальный прокси включён."),
        Err(e) => ctx.log(Level::Err, format!("Не удалось включить: {}", e)),
    }
}

fn disable_local_proxy(ctx: &mut Ctx) {
    let url = proxy::proxy_url();
    // I45 again: `remove_proxy` collects its own failures and joins them, so a
    // half-failed removal still takes off everything it can.
    match endpoint::remove_proxy(&url, "") {
        Ok(()) => ctx.log(Level::Ok, "Локальный прокси отключён."),
        Err(e) => ctx.log(Level::Warn, format!("Снято не полностью: {}", e)),
    }
}

// --- own proxy -------------------------------------------------------------

pub(super) fn set_own_proxy(ctx: &mut Ctx, text: &str) {
    ctx.settings.own_proxy = text.trim().to_string();
    if ctx.settings.own_proxy.is_empty() {
        upstream::clear();
        ctx.settings.own_proxy_enabled = false;
        ctx.log(Level::Ok, "Свой прокси убран.");
        return;
    }

    let upstreams = match upstream::parse_multiple(&ctx.settings.own_proxy) {
        Ok(u) if !u.is_empty() => u,
        Ok(_) => {
            upstream::clear();
            ctx.settings.own_proxy_enabled = false;
            ctx.log(Level::Ok, "Свой прокси убран.");
            return;
        }
        Err(e) => {
            ctx.log(Level::Err, format!("Адрес не разобран: {}", e));
            ctx.settings.own_proxy_enabled = false;
            return;
        }
    };

    if let Err(e) = upstream::save_multiple(&upstreams) {
        ctx.log(Level::Err, format!("Не сохранён: {}", e));
        ctx.settings.own_proxy_enabled = false;
        return;
    }
    ctx.settings.own_proxy_enabled = true;
    if upstreams.len() == 1 {
        ctx.log(Level::Ok, format!("Свой прокси включён: {}", upstreams[0].display()));
    } else {
        ctx.log(
            Level::Ok,
            format!(
                "Включено прокси: {} шт. (основной: {}, резервный: {})",
                upstreams.len(),
                upstreams[0].display(),
                upstreams[1].display()
            ),
        );
    }

    ctx.log(Level::Info, "Проверяю прокси…");
    for (idx, up) in upstreams.iter().enumerate() {
        let label = if idx == 0 { "Основной" } else { "Резервный" };
        match upstream::probe(up) {
            Ok(()) => ctx.log(Level::Ok, format!("{} прокси ({}) отвечает.", label, up.display())),
            Err(e) => {
                ctx.log(
                    Level::Warn,
                    format!(
                        "{} прокси ({}) не отвечает: {} — сохранён; при сбоях запросы переключатся автоматически.",
                        label,
                        up.display(),
                        e
                    ),
                );
            }
        }
    }
}

/// Strips SGR escapes from a string built for a terminal.
///
/// `dns::outcome_note` returns text with colour codes baked in — it was written
/// to be printed. Rendering it in a window would show the raw `ESC[33m`.
pub(super) fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // Drop until the final byte of the sequence (a letter).
            for c2 in chars.by_ref() {
                if c2.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Whether a path looks like an install root, for the manual "pencil" dialog.
pub fn resolve_manual_path(raw: &Path) -> Option<PathBuf> {
    crate::resolve_install_root(raw)
}
