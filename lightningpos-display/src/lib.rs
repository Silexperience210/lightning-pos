#![cfg_attr(not(test), no_std)]

/// AXS15231B QSPI 320×480 display driver (JC3248W535C "Touch 3.5\"" panel).
///
/// Ported from <https://github.com/H01001000/esp-axs15231b-display>
/// (esp-hal 1.x + custom `spi-half-duplex-async-v1` branch) to esp-hal 0.23.1.
/// See the module docs for the full API-differences report.
pub mod axs15231b;

/// Display rendering module
///
/// Handles all screen rendering: QR codes, payment confirmation,
/// product selection, ticker, error screens, help pages, and
/// screensaver dimming.

/// Display dimensions for current variant
#[derive(Debug, Clone, Copy)]
pub struct DisplayDimensions {
    pub width: u16,
    pub height: u16,
}

impl DisplayDimensions {
    pub const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }
}

/// Screen that the display is currently showing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// Boot-up splash
    Boot,
    /// WiFi connecting animation
    ConnectingWifi,
    /// Main QR code (awaiting payment)
    QrCode,
    /// Payment received confirmation
    PaymentReceived,
    /// Product selection (multi-channel mode)
    ProductSelection,
    /// BTC price ticker
    BtcTicker,
    /// Help/documentation pages (0-based page index)
    Help(u8),
    /// Diagnostic report
    Report,
    /// Error screen with message
    Error { critical: bool },
    /// Screensaver (display off)
    Screensaver,
    /// Config mode (serial interface)
    Config,
    /// Mini-PoS amount entry (Touch 3.5" only)
    MiniPosAmount,
    /// Mini-PoS invoice QR
    MiniPosInvoice,
    /// Authy identity QR (LNURL-auth)
    AuthyQr,
    /// NFC pending screen
    NfcPending,
}

/// Theme colors for display rendering
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub background: u16,
    pub foreground: u16,
    pub accent: u16,
    pub error: u16,
    pub success: u16,
}

impl Theme {
    /// Default black & white theme
    pub const fn default_bw() -> Self {
        Self {
            background: 0x0000, // Black
            foreground: 0xFFFF, // White
            accent: 0xFFE0,     // Yellow
            error: 0xF800,      // Red
            success: 0x07E0,    // Green
        }
    }

    /// Purple on yellow (high contrast, outdoor readable)
    pub const fn purple_yellow() -> Self {
        Self {
            background: 0x780F, // Purple
            foreground: 0xFFE0, // Yellow
            accent: 0x07E0,     // Green
            error: 0xF800,      // Red
            success: 0x001F,    // Blue
        }
    }
}

/// Display controller abstraction
pub struct DisplayController {
    dimensions: DisplayDimensions,
    current_screen: Screen,
    theme: Theme,
    /// Whether the backlight is on
    backlight: bool,
}

impl DisplayController {
    pub fn new(dimensions: DisplayDimensions) -> Self {
        Self {
            dimensions,
            current_screen: Screen::Boot,
            theme: Theme::default_bw(),
            backlight: true,
        }
    }

    pub fn dimensions(&self) -> DisplayDimensions {
        self.dimensions
    }

    pub fn current_screen(&self) -> Screen {
        self.current_screen
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    pub fn set_screen(&mut self, screen: Screen) {
        self.current_screen = screen;
    }

    pub fn set_backlight(&mut self, on: bool) {
        self.backlight = on;
    }

    pub fn is_backlight_on(&self) -> bool {
        self.backlight
    }
}
