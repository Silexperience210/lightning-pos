/// Board variant enum — selects pin mappings at compile time
///
/// Each variant has its own firmware binary with `#[cfg(feature = "...")]`
/// gates. This keeps the binary small (no dead code) while sharing
/// the same high-level logic.

/// Supported board variants
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum BoardVariant {
    /// LilyGo T-Display-S3 (ESP32-S3, ST7789 170×320, 4 channels)
    TDisplayS3,
    /// JC3248W535C Touch 3.5" (ESP32-S3, QSPI 480×320, 6 channels)
    Touch35,
    /// ESP32 Dev Module headless (no display, 12 channels)
    Headless,
    /// ESP32-C3-DevKitM-1 (RISC-V, 1 relay + 2 flex)
    Esp32C3,
}

impl BoardVariant {
    /// Number of relay channels on this board
    pub fn relay_channels(&self) -> u8 {
        match self {
            Self::TDisplayS3 => 4,
            Self::Touch35 => 6,
            Self::Headless => 12,
            Self::Esp32C3 => 1,
        }
    }

    /// Whether this variant has a display
    pub fn has_display(&self) -> bool {
        matches!(self, Self::TDisplayS3 | Self::Touch35)
    }

    /// Whether this variant supports NFC
    pub fn has_nfc(&self) -> bool {
        !matches!(self, Self::Esp32C3) // C3 has no NFC in current design
    }

    /// Whether this variant has touch input
    pub fn has_touch(&self) -> bool {
        matches!(self, Self::TDisplayS3 | Self::Touch35)
    }

    /// Whether this variant has a battery gauge
    pub fn has_battery_gauge(&self) -> bool {
        matches!(self, Self::Touch35)
    }

    /// Display width (0 if headless)
    pub fn display_width(&self) -> u16 {
        match self {
            Self::TDisplayS3 => 170,
            Self::Touch35 => 320,
            _ => 0,
        }
    }

    /// Display height (0 if headless)
    pub fn display_height(&self) -> u16 {
        match self {
            Self::TDisplayS3 => 320,
            Self::Touch35 => 480,
            _ => 0,
        }
    }

    /// Memory size in MB
    pub fn flash_size_mb(&self) -> u8 {
        match self {
            Self::TDisplayS3 | Self::Touch35 => 16,
            _ => 4,
        }
    }

    /// Human-readable name
    pub fn name(&self) -> &'static str {
        match self {
            Self::TDisplayS3 => "LightningPoS T-Display-S3",
            Self::Touch35 => "LightningPoS Touch 3.5",
            Self::Headless => "LightningPoS Headless",
            Self::Esp32C3 => "LightningPoS ESP32-C3",
        }
    }

    /// Firmware suffix for release naming (matches original C++ convention)
    pub fn firmware_suffix(&self) -> &'static str {
        match self {
            Self::TDisplayS3 => "",
            Self::Touch35 => "t",
            Self::Headless => "h",
            Self::Esp32C3 => "c",
        }
    }
}
