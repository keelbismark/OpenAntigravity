//! Choosing an unblock resolver at query time instead of trusting one forever.
//!
//! Submodules:
//! - `providers`: DNS unblock provider definitions, transports, and address lists.
//! - `verdict`: Classification of DNS responses, liveness probing, and proxy sets.
//! - `engine`: Multi-provider racing, upstream query dispatch, caching, and routing.

pub mod engine;
pub mod providers;
pub mod verdict;

#[cfg(test)]
mod tests;

pub use engine::{
    force_substitution, forget_names, genuine_a, refresh, resolve_a_best, resolve_best,
    served_verdict, set_vpn_active, substitution_forced, substitution_forced_for,
    substituting_addrs, tunnel_carries_client, vpn_is_active, warm,
};
pub use providers::{
    all_v6, fallback_v4, ordered_provider_names, provider_names, udp_provider_names,
    ANSWER_TTL, DNS_AI, PROVIDERS, REFERENCE_V4, Provider, Transport,
};
pub use verdict::{classify, Verdict};
