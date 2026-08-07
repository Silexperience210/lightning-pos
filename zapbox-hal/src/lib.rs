/// Hardware Abstraction Layer for LightningPoS
///
/// Maps board variants to GPIO pin assignments and provides
/// type-safe abstractions for relay control, servo, sensors, etc.
///
/// # Board Variants
///
/// | Variant        | Chip        | Display    | Relay Ch | Memory       |
/// |----------------|-------------|------------|----------|--------------|
/// | T-Display-S3   | ESP32-S3    | ST7789     | 4        | 16MB/8MB     |
/// | Touch 3.5"     | ESP32-S3    | QSPI 480×320| 6       | 16MB/8MB     |
/// | Headless ESP32 | ESP32       | None       | 12       | 4MB/512KB    |
/// | ESP32-C3       | ESP32-C3 R5 | None       | 1 (+2fx) | 4MB/400KB    |

pub mod pins;
pub mod relay;
pub mod variant;

pub use pins::*;
pub use variant::BoardVariant;
