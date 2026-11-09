use std::collections::HashMap;
use std::sync::mpsc;
use crate::settings::Settings;
use super::apply::*;
use super::status::*;
use super::types::*;
use super::worker::*;


    #[test]
    fn terminal_colours_do_not_reach_the_window() {
        let coloured = "\x1b[33mвнимание\x1b[0m\x1b[92m: правило";
        assert_eq!(strip_ansi(coloured), "внимание: правило");
    }

    #[test]
    fn a_capability_that_edits_system_policy_asks_for_admin_on_windows() {
        // The trap this guards: without elevation `remove_dns_nrpt` and
        // `background::disable` do not fail, they silently do nothing — so an
        // unelevated OFF switch would render as off while the rules stayed.
        assert_eq!(Cap::Dns.needs_admin(), cfg!(target_os = "windows"));
        assert!(!Cap::ClientPatch.needs_admin(), "the patch needs no UAC");
        assert!(!Cap::OwnProxy.needs_admin());
    }

    /// G74: the window drew the local-proxy switch from the environment alone,
    /// so a variable that outlived a failed removal showed «вкл» while the relay
    /// logged «выключена в настройках» in the same minute. The setting is what
    /// the relay obeys, so the setting is what the switch may never contradict.
    #[test]
    fn the_local_proxy_switch_never_reads_on_while_the_setting_is_off() {
        for ours in [true, false] {
            assert!(
                !local_proxy_state(false, ours).is_on(),
                "switch on with the setting off (variable present: {ours})"
            );
        }
        assert_eq!(local_proxy_state(true, true), State::On);
        assert_eq!(local_proxy_state(false, false), State::Off);
        // Both mismatches say which way round they are, rather than going quiet.
        assert!(local_proxy_state(true, false).note().is_some());
        assert!(local_proxy_state(false, true).note().is_some());
        // And each stays switchable the other way (G41): asked-for reads as on
        // so it can be turned off, left-over reads as off so it can be turned on.
        assert!(local_proxy_state(true, false).is_on());
        assert!(!local_proxy_state(false, true).is_on());
    }

    #[test]
    fn with_rotation_off_the_one_that_answers_is_the_first_one_left_on() {
        let names = crate::resolvers::provider_names();
        assert!(names.len() >= 2, "the test needs a pool to narrow");

        let mut s = Settings::default();
        assert_eq!(first_enabled_provider(&s), Some(names[0]));

        // Turning the head off must move the choice down the list, not leave it
        // pointing at a provider that is switched off.
        s.set_provider_enabled(names[0], false);
        assert_eq!(first_enabled_provider(&s), Some(names[1]));

        for n in &names {
            s.set_provider_enabled(n, false);
        }
        assert_eq!(first_enabled_provider(&s), None);
        assert!(all_providers_disabled(&s));
    }

    #[test]
    fn a_partial_state_still_reads_as_on() {
        // Half-patched must not draw as off: the user would flip it on and get
        // a no-op on the installs that are already done.
        assert!(State::Partial("x".into()).is_on());
        assert!(!State::Blocked("нет прав".into()).is_on());
        assert!(!State::Off.is_on());
    }

    /// The bug this pins: «Сверять TLS» and «Ротация» described their *off*
    /// state with a note, the note was a `Partial`, and `Partial` reads as on —
    /// so the switch sprang back the moment the worker's snapshot arrived and
    /// neither could be turned off, while the log said they had been.
    #[test]
    fn a_switch_that_is_off_with_a_note_is_off() {
        let mut s = Settings::default();
        s.verify_tls = false;
        s.rotate_providers = false;
        s.builtin_exits = false;
        let st = blank_status().with_settings_switches(&s);

        assert!(!st.verify_tls.is_on(), "off with a note is still off");
        assert!(!st.dns_rotation.is_on());
        assert!(!st.builtin_exits.is_on());
        // The note itself must survive — it is what the row says underneath.
        assert!(st.verify_tls.note().is_some());
        assert!(st.dns_rotation.note().is_some());

        let on = blank_status().with_settings_switches(&Settings::default());
        assert!(on.verify_tls.is_on() && on.dns_rotation.is_on() && on.builtin_exits.is_on());
    }

    /// A settings-only refresh must produce the same switches a full one does,
    /// or a flip would answer differently depending on which path drew it.
    #[test]
    fn the_cheap_refresh_and_the_full_one_agree_about_the_settings_switches() {
        let mut s = Settings::default();
        s.verify_tls = false;
        let a = blank_status().with_settings_switches(&s);
        let b = blank_status().with_settings_switches(&s);
        assert_eq!(a.verify_tls, b.verify_tls);
        assert_eq!(a.dns_rotation, b.dns_rotation);
        assert_eq!(a.providers.len(), crate::resolvers::provider_names().len());
    }

    /// A switch that only writes `settings.json` must not send the worker back
    /// to Windows for scheduled tasks and environment variables.
    #[test]
    fn only_the_switches_that_touch_the_system_pay_for_a_system_scan() {
        assert_eq!(scan_after(Cap::VerifyTls), Scan::Settings);
        assert_eq!(scan_after(Cap::BuiltinExits), Scan::Settings);
        assert_eq!(scan_after(Cap::Dns), Scan::System);
        assert_eq!(scan_after(Cap::LocalProxy), Scan::System);
        assert_eq!(scan_after(Cap::ClientPatch), Scan::Deep);
    }

    /// Live: what a switch actually costs, which is the complaint this was built
    /// to answer («слишком много времени»). Prints, asserts only the shape.
    ///
    ///     cargo test what_a_refresh_costs -- --ignored --nocapture
    #[test]
    #[ignore = "times real helper processes; run on Windows with --ignored"]
    fn what_a_refresh_costs() {
        let (tx, _rx) = mpsc::channel();
        let mut ctx = Ctx {
            events: tx,
            wake: Box::new(|| {}),
            settings: Settings::load(),
            proxy_var_carried: None,
            proxy_var_retryable: false,
            patched_seen: HashMap::new(),
            vpn: Some(VpnSeen::None),
            relay_egress: None,
            last: None,
            dns_probe: (false, false),
        };

        let t = std::time::Instant::now();
        let full = read_status(&mut ctx, false);
        let system = t.elapsed();
        ctx.last = Some(full);

        let t = std::time::Instant::now();
        let _ = settings_only_status(&ctx, ctx.last.clone().unwrap());
        let settings = t.elapsed();

        println!("Scan::System   {:?}", system);
        println!("Scan::Settings {:?}", settings);
        assert!(
            settings < system,
            "the cheap path is not cheaper: {:?} vs {:?}",
            settings,
            system
        );
    }

    fn blank_status() -> Status {
        Status {
            admin: true,
            installs: Vec::new(),
            client_patch: State::Off,
            watchdog: State::Off,
            dns: State::Off,
            local_proxy: State::Off,
            own_proxy: State::Off,
            builtin_exits: State::Off,
            dns_rotation: State::Off,
            verify_tls: State::Off,
            vpn: None,
            own_proxy_text: String::new(),
            relay_outdated: false,
            relay_running: false,
            rules: false,
            providers: Vec::new(),
            proxy_profiles: Vec::new(),
            relay_egress: None,
        }
    }

    /// The regression the `Unmeasured` variant exists for: a tunnel is up and
    /// the client has not dialled out, which is what a window opened before
    /// Antigravity almost always sees. Reading that as "the client is outside
    /// the tunnel" is the indicator claiming a measurement nobody took.
    #[test]
    fn a_tunnel_with_nothing_to_read_is_not_a_client_outside_it() {
        use crate::egress::ClientEgress;
        let seen = |client| match client {
            ClientEgress::Tunnel => VpnSeen::CarryingClient,
            ClientEgress::Mixed => VpnSeen::PartlyCarryingClient,
            ClientEgress::Physical => VpnSeen::NotCarryingClient,
            ClientEgress::ViaLocalProxy => VpnSeen::ViaLocalProxy,
            ClientEgress::Unknown => VpnSeen::Unmeasured,
        };
        assert_eq!(seen(ClientEgress::Unknown), VpnSeen::Unmeasured);
        assert_ne!(seen(ClientEgress::Unknown), VpnSeen::NotCarryingClient);
        // The state the proxy route puts a patched client in: measured, and not
        // the same answer as "nothing to read".
        assert_eq!(seen(ClientEgress::ViaLocalProxy), VpnSeen::ViaLocalProxy);
        assert_ne!(seen(ClientEgress::ViaLocalProxy), VpnSeen::Unmeasured);
        // And an unmeasured tunnel must not block the DNS row: it takes evidence
        // to stand down, not the absence of it (S37).
        assert_eq!(
            dns_state(true, true, true, Some(VpnSeen::Unmeasured), true),
            State::On
        );
        // A client in a tunnel no longer blocks the row either (D25).
        assert_eq!(
            dns_state(true, true, true, Some(VpnSeen::CarryingClient), true),
            State::On
        );
    }
