/// WebSocket client for LNbits communication
///
/// Manages the persistent WebSocket connection to the LNbits server
/// (bitcoinswitch_extension or lightningpos_extension). Handles:
///
/// - Connection lifecycle (connect, disconnect, reconnect)
/// - Heartbeat/ping-pong keepalive
/// - Payment event parsing
/// - NFC LNURLW event sending
/// - Half-open connection detection
///
/// # Protocol
///
/// ```text
/// ESP32 WebSocket Client              LNbits Server
///        │                                   │
///        │──── TCP connect ──────────────────│
///        │◄─── 101 Upgrade ──────────────────│
///        │──── "Connected" ──────────────────│
///        │                                   │
///        │◄─── {"paid":true,"pin":12}────────│ (payment received)
///        │──── {"event":"lnurlw",...} ───────│ (NFC bolt card)
///        │◄─── PING (every 30s) ────────────│ (keepalive)
///        │──── PONG ─────────────────────────│
/// ```

use log::{debug, error, info, warn};
use lightningpos_core::protocol::{LnUrlWEvent, PaymentNotification, WsIncoming};

/// WebSocket connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsState {
    Disconnected,
    Connecting,
    Connected,
    /// TCP is connected but we haven't validated the device config yet
    AwaitingValidation,
    Error,
}

/// WebSocket client configuration
#[derive(Debug, Clone)]
pub struct WebSocketConfig {
    /// WebSocket URL: ws://server/extension/api/v1/ws/deviceId
    pub url: heapless::String<256>,
    /// LNbits server hostname
    pub server: heapless::String<128>,
    /// Device ID (22-char UUID from LNbits)
    pub device_id: heapless::String<24>,
    /// Extension API path: "bitcoinswitch" or "lightningpos"
    pub extension: heapless::String<32>,
}

/// WebSocket client
///
/// In the real firmware this wraps `esp-wifi`'s WebSocket implementation
/// or an embedded WebSocket library. This module provides the control
/// interface and event parsing logic.
pub struct WebSocketClient {
    config: Option<WebSocketConfig>,
    state: WsState,
    /// Time of last pong received (ms)
    last_pong: u64,
    /// Time of last ping sent (ms)
    last_ping: u64,
    /// Whether we're waiting for a pong response
    waiting_for_pong: bool,
    /// Consecutive ping timeouts (for half-open detection)
    ping_timeouts: u8,
    /// Max ping timeouts before reconnect
    max_ping_timeouts: u8,
    /// Ping interval (ms)
    ping_interval_ms: u64,
    /// Pong timeout (ms) — if no pong within this time, connection is dead
    pong_timeout_ms: u64,
    /// When the TCP connection was established
    connected_since: u64,
    /// Payment queue for incoming WebSocket messages
    payment_queue: heapless::Deque<PaymentNotification, 8>,
}

impl WebSocketClient {
    pub fn new() -> Self {
        Self {
            config: None,
            state: WsState::Disconnected,
            last_pong: 0,
            last_ping: 0,
            waiting_for_pong: false,
            ping_timeouts: 0,
            max_ping_timeouts: 3,
            ping_interval_ms: 30000,   // 30 seconds
            pong_timeout_ms: 10000,    // 10 seconds
            connected_since: 0,
            payment_queue: heapless::Deque::new(),
        }
    }

    /// Configure the WebSocket client
    pub fn configure(&mut self, url: &str, server: &str, device_id: &str, extension: &str) {
        self.config = Some(WebSocketConfig {
            url: heapless::String::try_from(url).unwrap_or_default(),
            server: heapless::String::try_from(server).unwrap_or_default(),
            device_id: heapless::String::try_from(device_id).unwrap_or_default(),
            extension: heapless::String::try_from(extension).unwrap_or_default(),
        });
    }

    pub fn state(&self) -> WsState {
        self.state
    }

    pub fn is_connected(&self) -> bool {
        self.state == WsState::Connected
    }

    /// Called when TCP connection is established
    pub fn on_connected(&mut self, now: u64) {
        self.state = WsState::Connected;
        self.last_pong = now;
        self.connected_since = now;
        self.waiting_for_pong = false;
        self.ping_timeouts = 0;
        info!("WebSocket connected to {}", self.config.as_ref().map(|c| c.url.as_str()).unwrap_or("unknown"));
    }

    /// Called when WebSocket disconnects
    pub fn on_disconnected(&mut self) {
        self.state = WsState::Disconnected;
        self.waiting_for_pong = false;
        warn!("WebSocket disconnected");
    }

    /// Called when a text message is received
    pub fn on_message(&mut self, text: &str) -> Option<WsIncoming> {
        let msg = WsIncoming::from_json(text);

        if let Some(ref msg) = msg {
            match msg {
                WsIncoming::Payment(payment) => {
                    if payment.paid {
                        info!("Payment received: pin={}, amount={:?}", payment.pin, payment.amount);
                        // Queue the payment for processing in main loop
                        let _ = self.payment_queue.push_back(payment.clone());
                    }
                }
                WsIncoming::NfcEnrolled(_) => {
                    info!("NFC card enrolled");
                }
                WsIncoming::Other(_) => {
                    debug!("Unknown WS message: {}", &text[..text.len().min(100)]);
                }
            }
        }

        msg
    }

    /// Called when a ping is received from the server
    pub fn on_ping(&mut self, now: u64) {
        self.last_ping = now;
        debug!("WebSocket ping received");
    }

    /// Called when a pong is received
    pub fn on_pong(&mut self, now: u64) {
        self.last_pong = now;
        self.waiting_for_pong = false;
        self.ping_timeouts = 0;
        debug!("WebSocket pong received");
    }

    /// Check if we should send a keepalive ping
    pub fn should_ping(&self, now: u64) -> bool {
        self.state == WsState::Connected
            && now.saturating_sub(self.last_ping) >= self.ping_interval_ms
    }

    /// Check if the connection is half-open (no pong within timeout)
    pub fn is_half_open(&self, now: u64) -> bool {
        self.waiting_for_pong
            && now.saturating_sub(self.last_ping) >= self.pong_timeout_ms
    }

    /// Mark ping as sent
    pub fn ping_sent(&mut self, now: u64) {
        self.last_ping = now;
        self.waiting_for_pong = true;
    }

    /// Record a ping timeout and return whether we should reconnect
    pub fn on_ping_timeout(&mut self) -> bool {
        self.ping_timeouts += 1;
        warn!(
            "WebSocket ping timeout ({}/{})",
            self.ping_timeouts, self.max_ping_timeouts
        );
        self.ping_timeouts >= self.max_ping_timeouts
    }

    /// Dequeue a payment for processing
    pub fn dequeue_payment(&mut self) -> Option<PaymentNotification> {
        self.payment_queue.pop_front()
    }

    /// Number of queued payments
    pub fn pending_payments(&self) -> usize {
        self.payment_queue.len()
    }

    /// Build an LNURLW event JSON for NFC bolt card
    pub fn build_lnurlw_event(lnurlw: &str, pin: u8) -> String {
        let event = LnUrlWEvent::new(lnurlw, pin);
        event.to_json()
    }

    /// Build the WebSocket URL from components
    pub fn build_url(server: &str, extension: &str, device_id: &str) -> String {
        alloc::format!(
            "wss://{}/{}/api/v1/ws/{}",
            server,
            extension,
            device_id
        )
    }

    /// Get time since connection was established
    pub fn uptime_ms(&self, now: u64) -> u64 {
        if self.connected_since == 0 {
            0
        } else {
            now.saturating_sub(self.connected_since)
        }
    }
}

impl Default for WebSocketClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_url() {
        let url = WebSocketClient::build_url(
            "lnbits.example.com",
            "lightningpos",
            "abc123def456ghi789jkl0",
        );
        assert_eq!(
            url,
            "wss://lnbits.example.com/lightningpos/api/v1/ws/abc123def456ghi789jkl0"
        );
    }

    #[test]
    fn test_payment_parsing() {
        let mut client = WebSocketClient::new();
        client.configure(
            "wss://example.com/ws/abc",
            "example.com",
            "abc",
            "lightningpos",
        );
        client.on_connected(1000);

        let json = r#"{"paid":true,"pin":12,"amount":1000}"#;
        let msg = client.on_message(json);
        assert!(msg.is_some());
        assert!(msg.unwrap().is_paid());

        assert_eq!(client.pending_payments(), 1);
        let payment = client.dequeue_payment().unwrap();
        assert_eq!(payment.pin, 12);
        assert_eq!(payment.amount, Some(1000));
    }

    #[test]
    fn test_half_open_detection() {
        let mut client = WebSocketClient::new();
        client.on_connected(1000);

        // Send ping
        client.ping_sent(5000);
        assert!(client.waiting_for_pong);

        // After pong_timeout, connection is half-open
        assert!(client.is_half_open(16000)); // 5000 + 10000 + 1000 buffer
    }

    #[test]
    fn test_ping_timeout_reconnect() {
        let mut client = WebSocketClient::new();
        client.on_connected(1000);

        // Three ping timeouts should trigger reconnect
        assert!(!client.on_ping_timeout()); // 1
        assert!(!client.on_ping_timeout()); // 2
        assert!(client.on_ping_timeout());  // 3 → reconnect
    }

    #[test]
    fn test_nfc_event_building() {
        let json = WebSocketClient::build_lnurlw_event(
            "lnurlw://lnbits.example.com/withdraw/abc",
            12,
        );
        assert!(json.contains("\"event\":\"lnurlw\""));
        assert!(json.contains("\"pin\":12"));
    }
}
