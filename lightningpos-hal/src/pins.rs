/// GPIO Pin assignments for all board variants
///
/// Pins are selected at compile time via feature flags:
/// - `board-tdisplay-s3` (default)
/// - `board-touch35`
/// - `board-headless`
/// - `board-esp32c3`
///
/// This maps 1:1 to the original C++ PinConfig.h definitions.

/// Relay channel GPIO assignments
pub mod relay {
    /// Default relay pin (CH01) — used in single-channel mode
    pub const DEFAULT_RELAY: u8 = 12;

    /// Get the GPIO pin for a given relay channel (1-indexed)
    #[cfg(any(feature = "board-tdisplay-s3", not(any(
        feature = "board-touch35",
        feature = "board-headless",
        feature = "board-esp32c3"
    ))))]
    pub fn channel_pin(channel: u8) -> Option<u8> {
        match channel {
            1 => Some(12), // CH01
            2 => Some(13), // CH02
            3 => Some(10), // CH03 (ESP32-S3 only)
            4 => Some(11), // CH04 (also ambient-light sync)
            _ => None,
        }
    }

    #[cfg(feature = "board-touch35")]
    pub fn channel_pin(channel: u8) -> Option<u8> {
        match channel {
            1 => Some(14), // CH01 (default)
            2 => Some(15), // CH02
            3 => Some(16), // CH03
            4 => Some(5),  // CH04 (also battery ADC)
            5 => Some(6),  // CH05
            6 => Some(7),  // CH06
            _ => None,
        }
    }

    #[cfg(feature = "board-headless")]
    pub fn channel_pin(channel: u8) -> Option<u8> {
        match channel {
            1 => Some(12), 2 => Some(13), 3 => Some(14), 4 => Some(16),
            5 => Some(19), 6 => Some(22), 7 => Some(23), 8 => Some(25),
            9 => Some(26), 10 => Some(27), 11 => Some(32), 12 => Some(33),
            _ => None,
        }
    }

    #[cfg(feature = "board-esp32c3")]
    pub fn channel_pin(channel: u8) -> Option<u8> {
        match channel {
            1 => Some(4), // Only one relay channel on C3
            _ => None,
        }
    }

    /// Total number of relay channels for the current board
    #[cfg(any(feature = "board-tdisplay-s3", not(any(
        feature = "board-touch35",
        feature = "board-headless",
        feature = "board-esp32c3"
    ))))]
    pub const CHANNEL_COUNT: u8 = 4;

    #[cfg(feature = "board-touch35")]
    pub const CHANNEL_COUNT: u8 = 6;

    #[cfg(feature = "board-headless")]
    pub const CHANNEL_COUNT: u8 = 12;

    #[cfg(feature = "board-esp32c3")]
    pub const CHANNEL_COUNT: u8 = 1;
}

/// Display pins (T-Display-S3: ST7789 8-bit parallel)
#[cfg(any(feature = "board-tdisplay-s3", not(any(
    feature = "board-touch35",
    feature = "board-headless",
    feature = "board-esp32c3"
))))]
pub mod display {
    pub const BACKLIGHT: u8 = 38;
    pub const CS: u8 = 6;
    pub const DC: u8 = 7;
    pub const RST: u8 = 5;
    pub const WR: u8 = 8;
    pub const RD: u8 = 9;
    pub const D0: u8 = 39;
    pub const D1: u8 = 40;
    pub const D2: u8 = 41;
    pub const D3: u8 = 42;
    pub const D4: u8 = 45;
    pub const D5: u8 = 46;
    pub const D6: u8 = 47;
    pub const D7: u8 = 48;
    pub const WIDTH: u16 = 170;
    pub const HEIGHT: u16 = 320;
}

/// Display pins (Touch 3.5": QSPI via AXS15231B)
#[cfg(feature = "board-touch35")]
pub mod display {
    pub const BACKLIGHT: u8 = 1;
    pub const QSPI_CS: u8 = 45;
    pub const QSPI_CLK: u8 = 47;
    pub const QSPI_D0: u8 = 21;
    pub const QSPI_D1: u8 = 48;
    pub const QSPI_D2: u8 = 40;
    pub const QSPI_D3: u8 = 39;
    pub const WIDTH: u16 = 320;
    pub const HEIGHT: u16 = 480;
}

/// I2C bus pins (shared: Touch + NFC on T-Display-S3, external NFC on Touch 3.5")
#[cfg(not(feature = "board-esp32c3"))]
pub mod i2c {
    pub const SDA: u8 = 18;
    pub const SCL: u8 = 17;
}

#[cfg(feature = "board-esp32c3")]
pub mod i2c {
    pub const SDA: u8 = 20; // UART0 RXD repurposed
    pub const SCL: u8 = 21; // UART0 TXD repurposed
}

/// Touch controller pins (CST816S on T-Display-S3)
#[cfg(any(feature = "board-tdisplay-s3", not(any(
    feature = "board-touch35",
    feature = "board-headless",
    feature = "board-esp32c3"
))))]
pub mod touch {
    pub const INT: u8 = 16;
    pub const RST: u8 = 21;
}

/// NFC reader pins (PN532)
#[cfg(not(feature = "board-esp32c3"))]
pub mod nfc {
    /// IRQ pin — GPIO 1 on ESP32-S3 (free), GPIO 4 on ESP32 (UART0 TX conflict)
    #[cfg(any(
        feature = "board-tdisplay-s3",
        not(any(
            feature = "board-touch35",
            feature = "board-headless",
            feature = "board-esp32c3"
        ))
    ))]
    pub const IRQ: u8 = 1;

    #[cfg(feature = "board-headless")]
    pub const IRQ: u8 = 4;

    #[cfg(feature = "board-touch35")]
    pub const IRQ: u8 = 9; // GPIO 9 is free on JC3248W535C
}

#[cfg(feature = "board-esp32c3")]
pub mod nfc {
    pub const IRQ: u8 = 10; // GPIO 10 on ESP32-C3
}

/// Button pins
#[cfg(not(feature = "board-esp32c3"))]
pub mod button {
    pub const LEFT: u8 = 0;  // BOOT button
    pub const RIGHT: u8 = 14; // HELP button
}

#[cfg(feature = "board-esp32c3")]
pub mod button {
    pub const LEFT: u8 = 9;   // IO9 = BOOT button
    // No right button on C3
}

/// LED pins
#[cfg(any(feature = "board-tdisplay-s3", not(any(
    feature = "board-touch35",
    feature = "board-headless",
    feature = "board-esp32c3"
))))]
pub mod led {
    pub const LED_BUTTON: u8 = 43; // External LED button
    pub const LED_BUTTON_SW: u8 = 44; // Button switch input
}

#[cfg(feature = "board-headless")]
pub mod led {
    pub const LED_BUTTON: u8 = 21;  // LED output
    pub const ONBOARD: u8 = 2;      // Onboard LED
}

#[cfg(feature = "board-touch35")]
pub mod led {
    pub const LED_BUTTON: u8 = 43;  // TX pin
    pub const LED_BUTTON_SW: u8 = 44; // RX pin
}

#[cfg(feature = "board-esp32c3")]
pub mod led {
    pub const LED_BUTTON: u8 = 5;   // IO5 = status LED
}

/// Sensor / vending machine inputs
pub mod sensor {
    /// Default light barrier pin (T-Display-S3 only)
    #[cfg(any(feature = "board-tdisplay-s3", not(any(
        feature = "board-touch35",
        feature = "board-headless",
        feature = "board-esp32c3"
    ))))]
    pub const LIGHT_BARRIER: u8 = 2;

    /// Headless: two sensor inputs
    #[cfg(feature = "board-headless")]
    pub const SENSOR_1: u8 = 22;
    #[cfg(feature = "board-headless")]
    pub const SENSOR_2: u8 = 23;

    /// Touch 3.5": three sensor inputs (on CH04/CH05/CH06 = GPIO 5/6/7)
    #[cfg(feature = "board-touch35")]
    pub const SENSOR_1: u8 = 7;  // CH06
    #[cfg(feature = "board-touch35")]
    pub const SENSOR_2: u8 = 5;  // CH04
    #[cfg(feature = "board-touch35")]
    pub const SENSOR_3: u8 = 6;  // CH05
}

/// Power management
pub mod power {
    /// Power-on pin (T-Display-S3 only)
    #[cfg(any(feature = "board-tdisplay-s3", feature = "board-touch35"))]
    pub const POWER_ON: u8 = 15;

    /// Battery ADC pin
    #[cfg(any(feature = "board-tdisplay-s3", not(any(
        feature = "board-touch35",
        feature = "board-headless",
        feature = "board-esp32c3"
    ))))]
    pub const BAT_ADC: u8 = 4;

    /// Touch 3.5": battery ADC on GPIO 5 (shared with CH04)
    #[cfg(feature = "board-touch35")]
    pub const BAT_ADC: u8 = 5;
}

/// Flex channel pins (ESP32-C3 only)
#[cfg(feature = "board-esp32c3")]
pub mod flex {
    pub const CH01: u8 = 6; // GPIO 6
    pub const CH02: u8 = 7; // GPIO 7
}
