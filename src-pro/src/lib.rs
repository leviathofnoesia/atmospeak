//! Atmospeak Pro modules.
//!
//! Linked only when the desktop app is built with `--features pro`.
//! For production hardening, move this crate to a private repository and
//! depend on it from Pro CI via a private git/source dependency so Pro
//! feature code is absent from the public free remote.

mod airplane;
mod network_ledger;

pub use airplane::{AirplaneMode, AirplaneModeError};
pub use network_ledger::{LedgerEntry, NetworkLedger, NetworkLedgerError};

/// Compile-time marker so the host app can assert it is a Pro binary.
pub const IS_PRO_BUILD: bool = true;

/// First shippable Pro capability set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProCapability {
    /// Attested airplane mode — dictation path refuses new outbound sockets.
    AirplaneMode,
    /// Append-only outbound connection ledger for compliance export.
    NetworkLedger,
}

impl ProCapability {
    pub const ALL: &[ProCapability] = &[Self::AirplaneMode, Self::NetworkLedger];

    pub fn id(self) -> &'static str {
        match self {
            Self::AirplaneMode => "airplane_mode",
            Self::NetworkLedger => "network_ledger",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::AirplaneMode => "Airplane mode",
            Self::NetworkLedger => "Network ledger",
        }
    }
}
