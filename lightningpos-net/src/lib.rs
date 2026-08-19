#![cfg_attr(not(test), no_std)]

/// Network module — WiFi, WebSocket, and HTTP API client
///
/// # Architecture
///
/// ```text
/// ┌─────────────┐    ┌───────────────┐    ┌────────────────┐
/// │ wifi.rs     │    │ websocket.rs  │    │ api.rs         │
/// │ Connection  │───▶│ WS Events     │───▶│ HTTP GET/POST  │
/// │ Management  │    │ LNbits Bridge │    │ Labels, BTC    │
/// └─────────────┘    └───────────────┘    └────────────────┘
/// ```
///
/// All network operations are async (embassy), allowing the firmware
/// to handle WiFi, WebSocket, and display updates concurrently.

extern crate alloc;

pub mod api;
pub mod websocket;
pub mod wifi;

pub use websocket::WebSocketClient;
pub use wifi::WifiManager;
