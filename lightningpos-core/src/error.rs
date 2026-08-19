/// Error types for LightningPoS Core
use thiserror::Error;

use alloc::string::String;

#[derive(Error, Debug, Clone, PartialEq)]
pub enum Error {
    #[error("Invalid state transition: from {from:?} to {to:?}")]
    InvalidStateTransition {
        from: crate::state::DeviceState,
        to: crate::state::DeviceState,
    },

    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("Payment error: {0}")]
    Payment(#[from] PaymentError),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Encoding error: {0}")]
    Encoding(String),

    #[error("API error: HTTP {status} — {body}")]
    Api { status: u16, body: String },

    #[error("Not configured: {field}")]
    NotConfigured { field: &'static str },

    #[error("Buffer overflow: max {max} bytes")]
    BufferOverflow { max: usize },
}

#[derive(Error, Debug, Clone, PartialEq)]
pub enum ConfigError {
    #[error("Missing required field: {0}")]
    MissingField(&'static str),

    #[error("Invalid value for {field}: {value}")]
    InvalidValue { field: &'static str, value: String },

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Parse error: {0}")]
    Parse(String),
}

#[derive(Error, Debug, Clone, PartialEq)]
pub enum PaymentError {
    #[error("LNURL generation failed: {0}")]
    LnUrlGeneration(String),

    #[error("Bech32 encoding failed")]
    Bech32Encoding,

    #[error("Invalid invoice")]
    InvalidInvoice,

    #[error("Payment timeout after {0}ms")]
    Timeout(u64),

    #[error("Invoice expired")]
    InvoiceExpired,
}

/// Result type alias for the crate
pub type Result<T> = core::result::Result<T, Error>;
