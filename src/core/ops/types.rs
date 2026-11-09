use std::path::PathBuf;
use crate::settings::Settings;
use crate::upstream;

/// Everything the UI can flip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cap {
    /// Switch 1: the client patch that lifts the account-region block.
    ClientPatch,
    /// Re-applies the patch after Antigravity updates itself.
    Watchdog,
    /// The DNS half of the 400 bypass: local relay + NRPT rules.
    Dns,
    /// Our own CONNECT proxy, named by the private env var the patch installs.
    LocalProxy,
    /// The user's own HTTP proxy.
    OwnProxy,
    /// The built-in permitted-region exits.
    BuiltinExits,
    /// Whether the DNS pool rotates, or only its first enabled member answers.
    DnsRotation,
    /// Whether a substituted address must present a valid Google certificate.
    VerifyTls,
}

impl Cap {
    pub fn needs_admin(self) -> bool {
        cfg!(target_os = "windows") && matches!(self, Cap::Dns | Cap::Watchdog)
    }

    pub fn title(self) -> &'static str {
        match self {
            Cap::ClientPatch => "Разблокировать вход в аккаунт",
            Cap::Watchdog => "Автопатч Antigravity",
            Cap::Dns => "Обход через DNS",
            Cap::LocalProxy => "Локальный прокси",
            Cap::OwnProxy => "Свой HTTP-прокси",
            Cap::BuiltinExits => "Встроенные выходы",
            Cap::DnsRotation => "Ротация DNS-серверов",
            Cap::VerifyTls => "Сверять TLS",
        }
    }
}

/// Where a capability actually is, as opposed to where the user put the switch.
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    On,
    Off,
    Partial(String),
    OffNote(String),
    Blocked(String),
}

impl State {
    pub fn is_on(&self) -> bool {
        matches!(self, State::On | State::Partial(_))
    }
    pub fn note(&self) -> Option<&str> {
        match self {
            State::Partial(s) | State::Blocked(s) | State::OffNote(s) => Some(s),
            _ => None,
        }
    }
}

pub(super) fn on_off(v: bool) -> State {
    if v {
        State::On
    } else {
        State::Off
    }
}

/// One Antigravity kind, as the paths list draws it.
#[derive(Debug, Clone)]
pub struct InstallRow {
    pub path: Option<PathBuf>,
    pub label: &'static str,
    pub patched: Option<bool>,
    pub manual: bool,
}

/// What the VPN measurement found, as the indicator draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VpnSeen {
    None,
    NotCarryingClient,
    CarryingClient,
    PartlyCarryingClient,
    ViaLocalProxy,
    Unmeasured,
}

#[derive(Debug, Clone)]
pub struct ProviderRow {
    pub name: String,
    pub enabled: bool,
}

/// A whole-system snapshot. Produced on the worker, rendered on the UI thread.
#[derive(Debug, Clone)]
pub struct Status {
    pub admin: bool,
    pub installs: Vec<InstallRow>,
    pub client_patch: State,
    pub watchdog: State,
    pub dns: State,
    pub local_proxy: State,
    pub own_proxy: State,
    pub builtin_exits: State,
    pub dns_rotation: State,
    pub verify_tls: State,
    pub vpn: Option<VpnSeen>,
    pub own_proxy_text: String,
    pub relay_outdated: bool,
    pub relay_running: bool,
    pub rules: bool,
    pub providers: Vec<ProviderRow>,
    pub relay_egress: Option<crate::egress::ClientEgress>,
    pub proxy_profiles: Vec<crate::settings::ProxyProfile>,
}

impl Status {
    pub fn get(&self, cap: Cap) -> &State {
        match cap {
            Cap::ClientPatch => &self.client_patch,
            Cap::Watchdog => &self.watchdog,
            Cap::Dns => &self.dns,
            Cap::LocalProxy => &self.local_proxy,
            Cap::OwnProxy => &self.own_proxy,
            Cap::BuiltinExits => &self.builtin_exits,
            Cap::DnsRotation => &self.dns_rotation,
            Cap::VerifyTls => &self.verify_tls,
        }
    }

    pub fn bypass_on(&self) -> bool {
        self.dns.is_on() || self.local_proxy.is_on() || self.builtin_exits.is_on()
    }

    pub(super) fn with_settings_switches(mut self, s: &Settings) -> Self {
        self.builtin_exits = on_off(s.builtin_exits);
        self.verify_tls = if s.verify_tls {
            State::On
        } else {
            State::OffNote("подменённые адреса не проверяются".into())
        };
        self.dns_rotation = if s.rotate_providers {
            State::On
        } else {
            State::OffNote(match first_enabled_provider(s) {
                Some(name) => format!("используется только {}", name),
                None => "ни один сервер не включён".to_string(),
            })
        };
        self.own_proxy = if s.own_proxy_enabled && upstream::configured().is_some() {
            State::On
        } else {
            State::Off
        };
        self.providers = crate::resolvers::ordered_provider_names()
            .iter()
            .map(|n| ProviderRow {
                name: (*n).to_string(),
                enabled: s.provider_enabled(n),
            })
            .collect();
        self.proxy_profiles = s.proxy_profiles.clone();
        self
    }
}

pub fn bypass_order(on: bool) -> [Cap; 3] {
    if on {
        [Cap::Dns, Cap::LocalProxy, Cap::BuiltinExits]
    } else {
        [Cap::LocalProxy, Cap::BuiltinExits, Cap::Dns]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Step,
    Info,
    Ok,
    Warn,
    Err,
}

pub enum Event {
    Log(Level, String),
    Status(Box<Status>),
    Busy(Option<String>),
}

pub enum Cmd {
    Unlocked,
    RemeasureVpn,
    Probed {
        vpn: VpnSeen,
        relay: bool,
        relay_egress: Option<crate::egress::ClientEgress>,
    },
    #[allow(dead_code)]
    DeepRefresh,
    Set(Cap, bool),
    AddPath(PathBuf),
    ForgetPath(PathBuf),
    SetOwnProxy(String),
    SaveProxyProfile { name: String, address: String },
    DeleteProxyProfile(usize),
    SelectProxyProfile(usize),
    SetProvider(String, bool),
    ReorderProviders(Vec<String>),
    EnableAll,
    DisableAll,
    Repair,
    KillHolder,
    AutoFix(crate::platform::diag::AutoFixAction),
    #[allow(dead_code)]
    Stop,
}

pub(super) fn all_providers_disabled(s: &Settings) -> bool {
    crate::resolvers::provider_names()
        .iter()
        .all(|n| !s.provider_enabled(n))
}

pub(super) fn no_usable_nameserver(s: &Settings) -> bool {
    crate::resolvers::udp_provider_names()
        .iter()
        .all(|n| !s.provider_enabled(n))
}

pub(super) fn first_enabled_provider(s: &Settings) -> Option<&'static str> {
    crate::resolvers::ordered_provider_names()
        .into_iter()
        .find(|n| s.provider_enabled(n))
}
