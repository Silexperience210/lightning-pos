/// LNbits protocol types — WebSocket messages and API responses
///
/// Defines strongly-typed structs for all communication with the
/// LNbits server (bitcoinswitch_extension or lightningpos_extension).
///
/// # WebSocket Message Flow
///
/// ```text
/// ESP32                              LNbits Server
///   │                                      │
///   │──── WS CONNECT ──────────────────────│
///   │──── "Connected" ─────────────────────│
///   │                                      │
///   │◄─── {"paid": true, "pin": 12, ...} ──│  (payment received)
///   │◄─── {"ping": ...} ───────────────────│  (keepalive)
///   │──── {"event":"lnurlw", ...} ─────────│  (NFC bolt card)
///   │──── "pong" ──────────────────────────│
/// ```
///
/// # API Endpoints
///
/// - GET  /{ext}/api/v1/public/{deviceId}       → Switch labels/config
/// - GET  /{ext}/api/v1/lnurl/{deviceId}?pin=N  → LNURL generation (server-side)
/// - POST /{ext}/api/v1/minipos/{deviceId}      → Mini-PoS invoice
/// - GET  /{ext}/api/v1/nfc/auth/{deviceId}     → Ring-Login NFC auth

use alloc::string::String;
use alloc::vec::Vec;
use heapless::String as HString;
use serde::{Deserialize, Serialize};

/// Maximum payload size for WebSocket messages
pub const WS_MAX_PAYLOAD: usize = 2048;
/// Maximum number of product labels
pub const MAX_PRODUCTS: usize = 14;

// ═══════════════════════════════════════════════════════════════
// WebSocket Incoming Messages (Server → Device)
// ═══════════════════════════════════════════════════════════════

/// Payment notification from the server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentNotification {
    /// Whether the invoice was paid
    pub paid: bool,
    /// Relay pin that was triggered
    pub pin: u8,
    /// Payment amount in sats (optional)
    #[serde(default)]
    pub amount: Option<u64>,
    /// Payment hash (optional)
    #[serde(default)]
    pub payment_hash: Option<String>,
}

/// NFID enrollment confirmation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NfcEnrolled {
    pub nfc_enrolled: bool,
}

/// Generic WebSocket message from the server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WsIncoming {
    Payment(PaymentNotification),
    NfcEnrolled(NfcEnrolled),
    /// Catch-all for unknown messages (logged, not processed)
    Other(serde_json::Value),
}

impl WsIncoming {
    /// Parse a raw WebSocket text frame
    pub fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str::<WsIncoming>(json).ok()
    }

    pub fn is_payment(&self) -> bool {
        matches!(self, Self::Payment(_))
    }

    pub fn is_paid(&self) -> bool {
        matches!(self, Self::Payment(PaymentNotification { paid: true, .. }))
    }
}

// ═══════════════════════════════════════════════════════════════
// WebSocket Outgoing Messages (Device → Server)
// ═══════════════════════════════════════════════════════════════

/// NFC Bolt Card LNURLW event
#[derive(Debug, Clone, Serialize)]
pub struct LnUrlWEvent {
    pub event: String,
    pub lnurlw: String,
    pub pin: u8,
}

impl LnUrlWEvent {
    pub fn new(lnurlw: &str, pin: u8) -> Self {
        Self {
            event: "lnurlw".into(),
            lnurlw: lnurlw.into(),
            pin,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

// ═══════════════════════════════════════════════════════════════
// API Responses
// ═══════════════════════════════════════════════════════════════

/// Switch labels response from GET /api/v1/public/{deviceId}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchConfig {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub pin: Option<u8>,
    #[serde(default)]
    pub duration: Option<u64>,
    #[serde(default)]
    pub amount: Option<u64>,
}

/// Full switch labels response (array of switches)
pub type SwitchesResponse = Vec<SwitchConfig>;

/// Mini-PoS invoice response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiniPosInvoice {
    pub payment_hash: String,
    pub payment_request: String,
    #[serde(default)]
    pub amount: Option<String>,
}

/// Mini-PoS last payment response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiniPosLastPay {
    pub amount: String,
}

/// Authy LNURL-auth challenge response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthLnUrl {
    pub lnurl: String,
    #[serde(default)]
    pub action: Option<String>,
}

/// NFC auth response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NfcAuthResponse {
    pub status: String,
    #[serde(default)]
    pub message: Option<String>,
}

/// Bitcoin price from CoinGecko
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoinGeckoPrice {
    #[serde(rename = "bitcoin")]
    pub bitcoin: CoinGeckoPriceInner,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoinGeckoPriceInner {
    #[serde(flatten)]
    pub currencies: serde_json::Map<String, serde_json::Value>,
}

/// Mempool block height
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MempoolBlockHeight(pub u64);
