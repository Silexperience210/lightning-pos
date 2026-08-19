/// HTTP API client for LNbits and external services
///
/// Handles:
/// - Switch label fetching from LNbits (GET /api/v1/public/{deviceId})
/// - Bitcoin price from CoinGecko and block height from Mempool.space
/// - Mini-PoS invoice creation
/// - Authy LNURL-auth requests
/// - NFC auth verification
///
/// On ESP32, HTTP requests use `esp-wifi`'s TCP stack + embedded TLS.
/// This module provides the request building and response parsing logic.

use alloc::string::String;
use heapless::String as HString;
use log::{debug, info, warn};
use serde_json;

/// API endpoints for the LNbits server
#[derive(Debug)]
pub struct ApiEndpoints {
    pub server: HString<128>,
    pub extension: HString<32>,
    pub device_id: HString<24>,
}

impl ApiEndpoints {
    pub fn new(server: &str, extension: &str, device_id: &str) -> Self {
        Self {
            server: HString::try_from(server).unwrap_or_default(),
            extension: HString::try_from(extension).unwrap_or_default(),
            device_id: HString::try_from(device_id).unwrap_or_default(),
        }
    }

    /// GET switch labels
    pub fn labels_url(&self) -> String {
        alloc::format!(
            "https://{}/{}/api/v1/public/{}",
            self.server, self.extension, self.device_id
        )
    }

    /// POST Mini-PoS invoice
    pub fn minipos_invoice_url(&self) -> String {
        alloc::format!(
            "https://{}/{}/api/v1/minipos/{}",
            self.server, self.extension, self.device_id
        )
    }

    /// GET Mini-PoS last payment
    pub fn minipos_lastpay_url(&self) -> String {
        alloc::format!(
            "https://{}/{}/api/v1/minipos/{}/lastpay",
            self.server, self.extension, self.device_id
        )
    }

    /// GET Authy LNURL-auth challenge
    pub fn auth_lnurl_url(&self) -> String {
        alloc::format!(
            "https://{}/{}/api/v1/auth/{}",
            self.server, self.extension, self.device_id
        )
    }

    /// GET NFC auth (Ring-Login)
    pub fn nfc_auth_url(
        &self,
        external_id: &str,
        p: &str,
        c: &str,
        pin: Option<&str>,
    ) -> String {
        let mut url = alloc::format!(
            "https://{}/{}/api/v1/nfc/auth/{}?external_id={}&p={}&c={}",
            self.server, self.extension, self.device_id, external_id, p, c
        );
        if let Some(pin) = pin {
            url.push_str("&pin=");
            url.push_str(pin);
        }
        url
    }

    /// GET NFC teach (Ring-Login enrollment)
    pub fn nfc_teach_url(&self, external_id: &str, p: &str, c: &str) -> String {
        alloc::format!(
            "https://{}/{}/api/v1/nfc/teach/{}?external_id={}&p={}&c={}",
            self.server, self.extension, self.device_id, external_id, p, c
        )
    }

    /// POST POS invoice (Bolt Card flow) — LNbits wallet API on the local network
    ///
    /// Body: `{"out": false, "amount": <sats>, "memo": "..."}`
    /// Header: `X-Api-Key: <invoice key>`
    pub fn pos_invoice_url(&self) -> String {
        alloc::format!("http://{}/api/v1/payments", self.server)
    }

    /// GET payment status — `{"paid": true|false, "amount": ...}`
    pub fn pos_payment_status_url(&self, payment_hash: &str) -> String {
        alloc::format!("http://{}/api/v1/payments/{}", self.server, payment_hash)
    }
}

/// Parse the BOLT11 invoice from the LNbits invoice creation response
pub fn parse_invoice_bolt11(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let bolt11 = v["bolt11"].as_str()?;
    Some(String::from(bolt11))
}

/// Parse the payment hash from the LNbits invoice creation response
pub fn parse_invoice_payment_hash(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let hash = v["payment_hash"].as_str()?;
    Some(String::from(hash))
}

/// Parse the `paid` flag from the LNbits payment status response
pub fn parse_payment_paid(json: &str) -> Option<bool> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    v["paid"].as_bool()
}

/// Bitcoin price data
#[derive(Debug, Clone, Default)]
pub struct BitcoinData {
    /// Price string (e.g., "$67,234.50")
    pub price: HString<32>,
    /// Block height string
    pub block_height: HString<16>,
    /// Timestamp of last successful fetch (ms)
    pub last_update: u64,
    /// Whether the last fetch failed
    pub has_error: bool,
}

impl BitcoinData {
    /// Parse price from CoinGecko API response
    pub fn parse_coingecko(json: &str, currency: &str) -> Option<HString<32>> {
        let v: serde_json::Value = serde_json::from_str(json).ok()?;
        let price = v["bitcoin"][currency.to_lowercase()].as_f64()?;

        let formatted = alloc::format!("{:.2}", price);
        HString::try_from(formatted.as_str()).ok()
    }

    /// Parse block height from Mempool API response
    pub fn parse_mempool_height(body: &str) -> Option<u64> {
        body.trim().parse().ok()
    }

    /// Format a price with currency symbol
    pub fn format_price(price: &str, currency: &str) -> HString<32> {
        let symbol = match currency {
            "EUR" => "€",
            "GBP" => "£",
            "CHF" => "CHF ",
            "JPY" => "¥",
            _ => "$",
        };

        let formatted = alloc::format!("{}{}", symbol, price);
        HString::try_from(formatted.as_str()).unwrap_or_default()
    }
}

/// External API URLs (CoinGecko, Mempool)
pub struct ExternalApis;

impl ExternalApis {
    /// CoinGecko simple price endpoint
    pub fn btc_price_url(currency: &str) -> String {
        alloc::format!(
            "https://api.coingecko.com/api/v3/simple/price?ids=bitcoin&vs_currencies={}",
            currency.to_lowercase()
        )
    }

    /// Mempool.space block tip height
    pub const fn block_height_url() -> &'static str {
        "https://mempool.space/api/blocks/tip/height"
    }
}

/// Result of an HTTP request
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: HString<2048>,
}

impl HttpResponse {
    pub fn is_ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn is_not_found(&self) -> bool {
        self.status == 404
    }
}

/// HTTP client abstraction
///
/// On real hardware this will use `esp-wifi`'s HTTP client or `reqwest`
/// compiled for `no_std`. This module defines the interface.
#[derive(Debug)]
pub struct HttpClient {
    endpoints: Option<ApiEndpoints>,
    bitcoin_data: BitcoinData,
    /// Last update timestamps
    last_labels_fetch: u64,
    last_btc_fetch: u64,
    /// Retry backoff state
    labels_backoff_ms: u64,
}

impl HttpClient {
    pub fn new() -> Self {
        Self {
            endpoints: None,
            bitcoin_data: BitcoinData::default(),
            last_labels_fetch: 0,
            last_btc_fetch: 0,
            labels_backoff_ms: 0,
        }
    }

    /// Configure the API client with server details
    pub fn configure(&mut self, server: &str, extension: &str, device_id: &str) {
        self.endpoints = Some(ApiEndpoints::new(server, extension, device_id));
    }

    pub fn bitcoin_data(&self) -> &BitcoinData {
        &self.bitcoin_data
    }

    pub fn endpoints(&self) -> Option<&ApiEndpoints> {
        self.endpoints.as_ref()
    }

    /// Check if labels should be fetched (every 5 minutes)
    pub fn should_fetch_labels(&self, now: u64) -> bool {
        self.endpoints.is_some()
            && now.saturating_sub(self.last_labels_fetch) >= 300_000 // 5 min
    }

    /// Check if BTC price should be fetched (every 5 minutes)
    pub fn should_fetch_btc(&self, now: u64) -> bool {
        now.saturating_sub(self.last_btc_fetch) >= 300_000 // 5 min
    }

    /// Mark labels as fetched
    pub fn labels_fetched(&mut self, now: u64) {
        self.last_labels_fetch = now;
        self.labels_backoff_ms = 0;
    }

    /// Mark labels fetch as failed (apply backoff)
    pub fn labels_fetch_failed(&mut self, now: u64) {
        self.last_labels_fetch = now;
        // Exponential backoff: 30s → 60s → 120s → 240s (max)
        self.labels_backoff_ms = if self.labels_backoff_ms == 0 {
            30_000
        } else {
            (self.labels_backoff_ms * 2).min(300_000)
        };
    }

    /// Mark BTC data as fetched
    pub fn btc_fetched(&mut self, now: u64) {
        self.last_btc_fetch = now;
        self.bitcoin_data.last_update = now;
        self.bitcoin_data.has_error = false;
    }

    /// Mark BTC data fetch as failed
    pub fn btc_fetch_failed(&mut self, now: u64) {
        self.last_btc_fetch = now;
        self.bitcoin_data.has_error = true;
    }

    /// Update bitcoin price data
    pub fn update_price(&mut self, price: &str, block_height: u64) {
        let _ = self.bitcoin_data.price.push_str(price);
        let h = alloc::format!("{}", block_height);
        let _ = self.bitcoin_data.block_height.push_str(&h);
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoints_urls() {
        let api = ApiEndpoints::new("lnbits.example.com", "lightningpos", "abc123");
        assert_eq!(
            api.labels_url(),
            "https://lnbits.example.com/lightningpos/api/v1/public/abc123"
        );
        assert_eq!(
            api.minipos_invoice_url(),
            "https://lnbits.example.com/lightningpos/api/v1/minipos/abc123"
        );
    }

    #[test]
    fn test_nfc_auth_url() {
        let api = ApiEndpoints::new("lnbits.example.com", "lightningpos", "abc123");
        let url = api.nfc_auth_url("ext_id_1", "p_value", "c_value", Some("1234"));
        assert!(url.contains("external_id=ext_id_1"));
        assert!(url.contains("&pin=1234"));
    }

    #[test]
    fn test_pos_endpoints() {
        let api = ApiEndpoints::new("192.168.1.176:3007", "lightningpos", "abc123");
        assert_eq!(api.pos_invoice_url(), "http://192.168.1.176:3007/api/v1/payments");
        assert_eq!(
            api.pos_payment_status_url("deadbeef"),
            "http://192.168.1.176:3007/api/v1/payments/deadbeef"
        );
    }

    #[test]
    fn test_parse_invoice() {
        let json = r#"{"payment_hash":"abc","bolt11":"lnbc10n1p4gpqt9pp5fake","payment_request":"x"}"#;
        assert_eq!(parse_invoice_bolt11(json).unwrap(), "lnbc10n1p4gpqt9pp5fake");
        assert_eq!(parse_invoice_payment_hash(json).unwrap(), "abc");
        assert!(parse_payment_paid(json).is_none());
    }

    #[test]
    fn test_parse_paid() {
        assert_eq!(parse_payment_paid(r#"{"paid": true}"#), Some(true));
        assert_eq!(parse_payment_paid(r#"{"paid": false}"#), Some(false));
        assert_eq!(parse_payment_paid(r#"{"nope": 1}"#), None);
    }

    #[test]
    fn test_btc_price_formatting() {
        let formatted = BitcoinData::format_price("67234.50", "USD");
        assert_eq!(formatted.as_str(), "$67234.50");

        let formatted = BitcoinData::format_price("52300.00", "EUR");
        assert_eq!(formatted.as_str(), "€52300.00");
    }

    #[test]
    fn test_fetch_timing() {
        let mut client = HttpClient::new();
        client.configure("server", "lightningpos", "abc");

        // Should fetch immediately (now - 0 >= 300000)
        assert!(client.should_fetch_labels(300_000));
        client.labels_fetched(300_000);

        // Should NOT fetch right away
        assert!(!client.should_fetch_labels(300_001));
        // Should fetch after 5 minutes
        assert!(client.should_fetch_labels(600_001));
    }

    #[test]
    fn test_backoff_on_failure() {
        let mut client = HttpClient::new();
        client.configure("server", "lightningpos", "abc");

        client.labels_fetch_failed(0);
        assert_eq!(client.labels_backoff_ms, 30_000);

        client.labels_fetch_failed(30_000);
        assert_eq!(client.labels_backoff_ms, 60_000);

        // Max backoff
        for _ in 0..10 {
            client.labels_fetch_failed(999_999);
        }
        assert_eq!(client.labels_backoff_ms, 300_000);
    }
}
