/// Configuration types for LightningPoS
///
/// All configuration lives in typed structs that can be serialized/deserialized
/// from JSON (for the web installer / serial config) and persisted to NVS.
///
/// The original C++ LightningPoS stores config as a flat JSON array in FFat.
/// We improve this with a typed, validated approach.

use alloc::vec::Vec;
use heapless::String as HString;
use serde::{Deserialize, Serialize};

/// Maximum config field length
pub const MAX_STR_LEN: usize = 128;
/// Maximum URL length
pub const MAX_URL_LEN: usize = 256;
/// Maximum SSID length
pub const MAX_SSID_LEN: usize = 64;
/// Maximum password length
pub const MAX_PASS_LEN: usize = 64;

/// Typed string alias for embedded use
pub type ShortStr = HString<MAX_STR_LEN>;
pub type LongStr = HString<MAX_URL_LEN>;
pub type SsidStr = HString<MAX_SSID_LEN>;

/// WiFi configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WifiConfig {
    pub ssid: ShortStr,
    pub password: ShortStr,
    /// WebSocket URL: ws://server/extension/api/v1/ws/deviceId
    pub switch_url: LongStr,
    /// Extracted server hostname:port
    #[serde(skip)]
    pub lnbits_server: ShortStr,
    /// Extracted device ID (last 22 chars)
    #[serde(skip)]
    pub device_id: ShortStr,
}

impl Default for WifiConfig {
    fn default() -> Self {
        Self {
            ssid: ShortStr::new(),
            password: ShortStr::new(),
            switch_url: LongStr::new(),
            lnbits_server: ShortStr::new(),
            device_id: ShortStr::new(),
        }
    }
}

impl WifiConfig {
    /// Parse switch_url into lnbits_server and device_id
    pub fn parse_switch_url(&mut self) {
        let url = self.switch_url.as_str();
        // Find "://" separator
        if let Some(proto_end) = url.find("://") {
            let after_proto = &url[proto_end + 3..];
            // Find first "/" after host
            if let Some(path_start) = after_proto.find('/') {
                let server = &after_proto[..path_start];
                let _ = self.lnbits_server.push_str(server);

                // Device ID = last 22 characters
                let uid_len = 22.min(url.len());
                let did = &url[url.len() - uid_len..];
                let _ = self.device_id.push_str(did);
            }
        }
    }
}

/// Display orientation and theme
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DisplayConfig {
    /// "h" = horizontal, "v" = vertical, "hi" = h-inverse, "vi" = v-inverse
    pub orientation: ShortStr,
    /// Theme name: "black-white", "purple-yellow", etc.
    pub theme: ShortStr,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            orientation: ShortStr::try_from("h").unwrap(),
            theme: ShortStr::try_from("black-white").unwrap(),
        }
    }
}

/// Lightning payment configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LightningConfig {
    /// QR format: "bech32" or "lud17"
    pub qr_format: ShortStr,
    /// LightNing prefix: "lightning:" or "LIGHTNING:"
    #[serde(skip)]
    pub lightning_prefix: ShortStr,
    /// BOLT11 invoice buffer (up to 640 chars for route hints)
    pub lightning_buffer: HString<640>,
}

impl Default for LightningConfig {
    fn default() -> Self {
        Self {
            qr_format: ShortStr::try_from("bech32").unwrap(),
            lightning_prefix: ShortStr::try_from("lightning:").unwrap(),
            lightning_buffer: HString::new(),
        }
    }
}

/// Power management configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PowerConfig {
    pub screensaver: ShortStr,
    pub deep_sleep: ShortStr,
    /// Activation timeout in minutes (string for web installer compat)
    pub activation_time: ShortStr,
    pub deep_sleep_time: ShortStr,
}

impl Default for PowerConfig {
    fn default() -> Self {
        Self {
            screensaver: ShortStr::try_from("off").unwrap(),
            deep_sleep: ShortStr::try_from("off").unwrap(),
            activation_time: ShortStr::try_from("5").unwrap(),
            deep_sleep_time: ShortStr::try_from("30").unwrap(),
        }
    }
}

/// Special mode configuration (blink, pulse, strobe)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpecialModeConfig {
    /// Mode: "standard", "frequency", "brightness"
    pub mode: ShortStr,
    /// Frequency in Hz (0.1–10.0)
    pub frequency: f32,
    /// Duty cycle ratio (0.1–10.0)
    pub duty_cycle: f32,
}

impl Default for SpecialModeConfig {
    fn default() -> Self {
        Self {
            mode: ShortStr::try_from("standard").unwrap(),
            frequency: 1.0,
            duty_cycle: 1.0,
        }
    }
}

/// Multi-channel control mode
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MultiChannelConfig {
    /// Mode: "off", "duo", "quattro", "servo"
    pub mode: ShortStr,
    /// BTC ticker: "off", "always", "selecting"
    pub btc_ticker_mode: ShortStr,
    /// Currently selected product (0 = selection screen, 1-N = product)
    pub current_product: i8,
}

impl Default for MultiChannelConfig {
    fn default() -> Self {
        Self {
            mode: ShortStr::try_from("off").unwrap(),
            btc_ticker_mode: ShortStr::try_from("off").unwrap(),
            current_product: -1,
        }
    }
}

/// Currency code for BTC price display
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Currency {
    USD,
    EUR,
    GBP,
    CHF,
    JPY,
    CAD,
    AUD,
}

impl Default for Currency {
    fn default() -> Self {
        Self::USD
    }
}

impl Currency {
    pub fn from_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "EUR" => Self::EUR,
            "GBP" => Self::GBP,
            "CHF" => Self::CHF,
            "JPY" => Self::JPY,
            "CAD" => Self::CAD,
            "AUD" => Self::AUD,
            _ => Self::USD,
        }
    }
}

/// Complete device configuration (matches config.json structure)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceConfig {
    pub wifi: WifiConfig,
    pub display: DisplayConfig,
    pub lightning: LightningConfig,
    pub power: PowerConfig,
    pub special_mode: SpecialModeConfig,
    pub multi_channel: MultiChannelConfig,
    pub currency: Currency,
    /// Extension API path: "bitcoinswitch" or "lightningpos"
    pub extension_path: ShortStr,
    /// Minimum activation time for screensaver (minutes)
    pub activation_time: u8,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            wifi: WifiConfig::default(),
            display: DisplayConfig::default(),
            lightning: LightningConfig::default(),
            power: PowerConfig::default(),
            special_mode: SpecialModeConfig::default(),
            multi_channel: MultiChannelConfig::default(),
            currency: Currency::default(),
            extension_path: ShortStr::try_from("bitcoinswitch").unwrap(),
            activation_time: 5,
        }
    }
}

impl DeviceConfig {
    /// Validate config integrity, returning a list of issues
    pub fn validate(&self) -> Vec<&'static str> {
        let mut issues = Vec::new();

        if self.wifi.ssid.is_empty() {
            issues.push("WiFi SSID is empty");
        }
        if self.wifi.switch_url.is_empty() {
            issues.push("Switch URL is empty");
        }
        if self.extension_path.as_str() != "bitcoinswitch"
            && self.extension_path.as_str() != "lightningpos"
        {
            issues.push("Extension path must be 'bitcoinswitch' or 'lightningpos'");
        }

        issues
    }

    /// Parse the switch URL to extract server and device ID
    pub fn parse_urls(&mut self) {
        self.wifi.parse_switch_url();
    }
}
