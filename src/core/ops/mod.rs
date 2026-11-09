//! The switch layer: every capability the window can turn on or off, and the
//! one worker thread that does it.
//!
//! Submodules:
//! - `types`: Core data models, capability enums, states, and event/command types.
//! - `status`: System probe and diagnostic state collection.
//! - `apply`: Operation execution and capability toggling logic.
//! - `worker`: Background worker thread and scan dispatch loop.

pub mod apply;
pub mod status;
pub mod types;
pub mod worker;

#[cfg(test)]
mod tests;

pub use apply::resolve_manual_path;
pub use types::{
    bypass_order, Cap, Cmd, Event, InstallRow, Level, ProviderRow, State, Status, VpnSeen,
};
pub use worker::{spawn, Worker};
