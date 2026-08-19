#![cfg_attr(not(test), no_std)]

/// LightningPoS Core — platform-agnostic business logic
///
/// This crate contains the state machine, payment processing,
/// LNURL/bech32 encoding, configuration types, and all logic that
/// doesn't touch hardware.
///
/// # Architecture
///
/// ```text
/// ┌─────────┐  ┌──────────┐  ┌───────────┐
/// │ config  │  │ payment  │  │ bitcoin   │
/// │ Config  │  │ LnUrl    │  │ Price     │
/// │ NVS I/O │  │ Bech32   │  │ Height    │
/// └────┬────┘  └────┬─────┘  └─────┬─────┘
///      │            │              │
///      └────────────┼──────────────┘
///                   │
///            ┌──────┴──────┐
///            │   state     │
///            │ StateMachine│
///            │ Transitions │
///            └──────┬──────┘
///                   │
///      ┌────────────┼──────────────┐
///      │            │              │
/// ┌────┴────┐ ┌────┴─────┐ ┌──────┴──────┐
/// │ qrcode  │ │ protocol │ │  events     │
/// │ Render  │ │ LNbits   │ │  WebSocket  │
/// │ Module  │ │ API      │ │  Messages   │
/// └─────────┘ └──────────┘ └─────────────┘
/// ```

// Needed for no_std + alloc environments (embedded)
extern crate alloc;

pub mod boltcard;
pub mod config;
pub mod error;
pub mod events;
pub mod payment;
pub mod protocol;
pub mod provision;
pub mod state;

pub use error::Error;
pub use state::{DeviceState, StateMachine, StateTransition};
