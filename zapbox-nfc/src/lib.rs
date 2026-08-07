/// NFC module — Bolt Card reader and NT3H2111 phone tap
///
/// Supports two NFC modes:
/// - **Bolt Card** (PN532 + NTAG424 DNA): read LNURLW from card
/// - **NT3H2111**: phone tap NFC tag emulation

/// NFC reader status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NfcStatus {
    /// Not initialized
    Uninitialized,
    /// Waiting for a card/phone
    Idle,
    /// Card detected, reading
    Reading,
    /// Card read complete, data available
    DataReady,
    /// Phone tap detected (NT3H2111 field detection)
    PhoneTap,
    /// Error state
    Error,
}

/// Result of an NFC read operation
#[derive(Debug, Clone)]
pub enum NfcEvent {
    /// Bolt Card LNURLW extracted
    BoltCard {
        lnurlw: heapless::String<512>,
    },
    /// Phone tap detected (for NT3H2111)
    PhoneTap,
    /// NTAG424 DNA external ID read (for Ring-Login)
    IdentityRead {
        external_id: heapless::String<32>,
        p: heapless::String<64>,
        c: heapless::String<64>,
    },
    /// No valid data found
    NoData,
    /// Read error
    Error(heapless::String<64>),
}

/// NFC controller configuration
pub struct NfcConfig {
    /// I2C address of PN532
    pub pn532_addr: u8,
    /// IRQ pin (GPIO number)
    pub irq_pin: u8,
    /// Whether NT3H2111 phone tap is enabled
    pub nt3h_enabled: bool,
}

impl Default for NfcConfig {
    fn default() -> Self {
        Self {
            pn532_addr: 0x24, // Default PN532 I2C address
            irq_pin: 1,       // GPIO 1 on T-Display-S3
            nt3h_enabled: false,
        }
    }
}

/// NFC controller
pub struct NfcController {
    config: NfcConfig,
    status: NfcStatus,
}

impl NfcController {
    pub fn new(config: NfcConfig) -> Self {
        Self {
            config,
            status: NfcStatus::Uninitialized,
        }
    }

    pub fn status(&self) -> NfcStatus {
        self.status
    }

    pub fn set_status(&mut self, status: NfcStatus) {
        self.status = status;
    }

    /// Mark as idle (ready for next card)
    pub fn reset(&mut self) {
        self.status = NfcStatus::Idle;
    }
}
