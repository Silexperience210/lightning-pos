/// Event types for the LightningPoS event bus
///
/// The firmware uses an event-driven architecture where hardware
/// events (button presses, NFC taps, WebSocket messages) are
/// dispatched as typed events and processed in the main loop.

use crate::state::DeviceState;

/// Events that can occur in the system
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Payment received from LNbits (via WebSocket)
    PaymentReceived {
        pin: u8,
        amount_sats: Option<u64>,
        payment_hash: Option<heapless::String<64>>,
    },

    /// NFC Bolt Card tapped — LNURLW extracted
    NfcBoltCardTapped {
        lnurlw: heapless::String<512>,
    },

    /// NFC phone tap detected (NT3H2111)
    NfcPhoneTap,

    /// Button pressed (0 = left/BOOT, 1 = right/HELP)
    ButtonPress(u8),

    /// Button long-press (> 3s) — triggers config mode
    ButtonLongPress(u8),

    /// Touch screen gesture detected
    TouchGesture {
        x: u16,
        y: u16,
        clicks: u8,
    },

    /// WiFi connected
    WifiConnected,

    /// WiFi disconnected
    WifiDisconnected,

    /// WebSocket connected to LNbits
    WebSocketConnected,

    /// WebSocket disconnected
    WebSocketDisconnected,

    /// Bitcoin price updated
    BtcPriceUpdated {
        price: heapless::String<32>,
        currency: heapless::String<8>,
    },

    /// Block height updated
    BlockHeightUpdated(u64),

    /// Screensaver timeout reached
    ScreensaverTimeout,

    /// Deep sleep timeout reached
    DeepSleepTimeout,

    /// State transition occurred
    StateChanged {
        from: DeviceState,
        to: DeviceState,
    },

    /// Error occurred
    Error {
        critical: bool,
        message: heapless::String<128>,
    },
}

/// Event severity for logging
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Debug,
    Info,
    Warn,
    Error,
}

impl Event {
    /// Severity level for logging
    pub fn severity(&self) -> Severity {
        match self {
            Self::PaymentReceived { .. } => Severity::Info,
            Self::NfcBoltCardTapped { .. } => Severity::Info,
            Self::NfcPhoneTap => Severity::Info,
            Self::ButtonPress(_) => Severity::Debug,
            Self::ButtonLongPress(_) => Severity::Info,
            Self::TouchGesture { .. } => Severity::Debug,
            Self::WifiConnected => Severity::Info,
            Self::WifiDisconnected => Severity::Warn,
            Self::WebSocketConnected => Severity::Info,
            Self::WebSocketDisconnected => Severity::Warn,
            Self::BtcPriceUpdated { .. } => Severity::Debug,
            Self::BlockHeightUpdated(_) => Severity::Debug,
            Self::ScreensaverTimeout => Severity::Debug,
            Self::DeepSleepTimeout => Severity::Debug,
            Self::StateChanged { .. } => Severity::Debug,
            Self::Error { critical, .. } if *critical => Severity::Error,
            Self::Error { .. } => Severity::Warn,
        }
    }

    /// Human-readable event name for logging
    pub fn name(&self) -> &'static str {
        match self {
            Self::PaymentReceived { .. } => "PaymentReceived",
            Self::NfcBoltCardTapped { .. } => "NfcBoltCardTapped",
            Self::NfcPhoneTap => "NfcPhoneTap",
            Self::ButtonPress(_) => "ButtonPress",
            Self::ButtonLongPress(_) => "ButtonLongPress",
            Self::TouchGesture { .. } => "TouchGesture",
            Self::WifiConnected => "WifiConnected",
            Self::WifiDisconnected => "WifiDisconnected",
            Self::WebSocketConnected => "WebSocketConnected",
            Self::WebSocketDisconnected => "WebSocketDisconnected",
            Self::BtcPriceUpdated { .. } => "BtcPriceUpdated",
            Self::BlockHeightUpdated(_) => "BlockHeightUpdated",
            Self::ScreensaverTimeout => "ScreensaverTimeout",
            Self::DeepSleepTimeout => "DeepSleepTimeout",
            Self::StateChanged { .. } => "StateChanged",
            Self::Error { .. } => "Error",
        }
    }
}

/// Event bus for dispatching events to handlers
///
/// Uses a lock-free ring buffer (heavily simplified for ESP32 no_std).
/// In the real firmware this would use `embassy_sync::pubsub` or
/// a `heapless::spsc::Queue`.
#[derive(Debug)]
pub struct EventQueue<const N: usize> {
    buffer: heapless::Deque<Event, N>,
}

impl<const N: usize> EventQueue<N> {
    pub fn new() -> Self {
        Self {
            buffer: heapless::Deque::new(),
        }
    }

    pub fn push(&mut self, event: Event) -> Result<(), Event> {
        self.buffer.push_back(event)
    }

    pub fn pop(&mut self) -> Option<Event> {
        self.buffer.pop_front()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Drain all events and pass them to a handler
    pub fn drain<F>(&mut self, mut handler: F)
    where
        F: FnMut(Event),
    {
        while let Some(event) = self.pop() {
            handler(event);
        }
    }
}

impl<const N: usize> Default for EventQueue<N> {
    fn default() -> Self {
        Self::new()
    }
}
