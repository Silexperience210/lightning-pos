/// WiFi connection manager
///
/// Handles WiFi station mode connection with:
/// - Automatic reconnection on disconnect
/// - Connection timeout detection
/// - RSSI monitoring for link quality
/// - Event callbacks for state changes
///
/// Uses embassy-net for async WiFi operations on ESP32.

use log::{info, warn};
use lightningpos_core::state::WifiState;

/// WiFi connection configuration
#[derive(Debug, Clone)]
pub struct WifiConfig {
    /// SSID (max 32 bytes)
    pub ssid: heapless::String<32>,
    /// WPA2 password (max 63 bytes)
    pub password: heapless::String<64>,
}

/// WiFi manager state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiStatus {
    /// Not initialized
    Uninitialized,
    /// Connecting to AP
    Connecting,
    /// Connected (with RSSI)
    Connected(i8),
    /// Connection failed
    Failed,
    /// Disconnected
    Disconnected,
}

impl WifiStatus {
    pub fn to_wifi_state(&self) -> WifiState {
        match self {
            Self::Uninitialized | Self::Disconnected => WifiState::Disconnected,
            Self::Connecting => WifiState::Connecting,
            Self::Connected(_) => WifiState::Connected,
            Self::Failed => WifiState::Error,
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected(_))
    }
}

/// WiFi connection manager
///
/// On real hardware this wraps `esp-wifi` and `embassy-net`.
/// The current implementation provides the control interface;
/// the hardware-specific init will be in the firmware binary's main.rs.
pub struct WifiManager {
    config: Option<WifiConfig>,
    status: WifiStatus,
    /// Connection attempt counter (for exponential backoff)
    attempt: u32,
    /// Max retry attempts before giving up
    max_attempts: u32,
    /// Retry backoff base (ms)
    backoff_ms: u32,
}

impl WifiManager {
    pub fn new() -> Self {
        Self {
            config: None,
            status: WifiStatus::Uninitialized,
            attempt: 0,
            max_attempts: 10,
            backoff_ms: 5000,
        }
    }

    /// Configure WiFi credentials
    pub fn configure(&mut self, ssid: &str, password: &str) {
        self.config = Some(WifiConfig {
            ssid: heapless::String::try_from(ssid).unwrap_or_default(),
            password: heapless::String::try_from(password).unwrap_or_default(),
        });
    }

    /// Get current status
    pub fn status(&self) -> WifiStatus {
        self.status
    }

    /// Whether WiFi is connected
    pub fn is_connected(&self) -> bool {
        self.status.is_connected()
    }

    /// Get RSSI if connected
    pub fn rssi(&self) -> Option<i8> {
        match self.status {
            WifiStatus::Connected(rssi) => Some(rssi),
            _ => None,
        }
    }

    /// Mark as connected (called by hardware event handler)
    pub fn on_connected(&mut self, rssi: i8) {
        self.status = WifiStatus::Connected(rssi);
        self.attempt = 0;
        info!("WiFi connected (RSSI: {} dBm)", rssi);
    }

    /// Mark as disconnected (called by hardware event handler)
    pub fn on_disconnected(&mut self) {
        self.status = WifiStatus::Disconnected;
        self.attempt += 1;
        warn!("WiFi disconnected (attempt {}/{})", self.attempt, self.max_attempts);
    }

    /// Mark connection as failed
    pub fn on_connection_failed(&mut self) {
        self.status = WifiStatus::Failed;
        self.attempt += 1;
        warn!("WiFi connection failed (attempt {}/{})", self.attempt, self.max_attempts);
    }

    /// Calculate retry delay with exponential backoff
    pub fn retry_delay_ms(&self) -> u32 {
        let exp = self.attempt.min(6); // Cap at 2^6 = 64x
        self.backoff_ms * (1 << exp)
    }

    /// Whether we should keep retrying
    pub fn should_retry(&self) -> bool {
        self.config.is_some() && self.attempt < self.max_attempts
    }

    /// Get the configured SSID (if any)
    pub fn ssid(&self) -> Option<&str> {
        self.config.as_ref().map(|c| c.ssid.as_str())
    }

    /// Reset retry counter (e.g. after user reconfiguration)
    pub fn reset_retries(&mut self) {
        self.attempt = 0;
    }
}

impl Default for WifiManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wifi_lifecycle() {
        let mut wifi = WifiManager::new();
        assert_eq!(wifi.status(), WifiStatus::Uninitialized);
        assert!(!wifi.is_connected());

        wifi.configure("MyNetwork", "password123");
        wifi.on_connected(-45);
        assert!(wifi.is_connected());
        assert_eq!(wifi.rssi(), Some(-45));

        wifi.on_disconnected();
        assert!(!wifi.is_connected());
        assert!(wifi.should_retry());
    }

    #[test]
    fn test_exponential_backoff() {
        let mut wifi = WifiManager::new();
        wifi.configure("test", "pass");

        // After 0 attempts: 5s base
        assert_eq!(wifi.retry_delay_ms(), 5000);

        // After 3 failures
        wifi.on_connection_failed();
        wifi.on_connection_failed();
        wifi.on_connection_failed();
        assert_eq!(wifi.retry_delay_ms(), 40000); // 5s * 2^3 = 40s
    }

    #[test]
    fn test_max_retries() {
        let mut wifi = WifiManager::new();
        wifi.configure("test", "pass");

        for _ in 0..10 {
            wifi.on_connection_failed();
        }
        assert!(!wifi.should_retry());
    }
}
