/// LNURL and Bech32 encoding for Lightning payments
///
/// Generates LNURLs in bech32 or LUD17 format, identical to the
/// original C++ LightningPoS implementation but with Rust's safety guarantees.
///
/// # Formats
///
/// - **bech32**: `lightning:LNURL1...` — uppercase bech32-encoded LNURL
/// - **lud17**: `lnurlp:https://...` — raw URL with `lnurlp:` prefix
///
/// # Usage
///
/// ```ignore
/// use lightningpos_core::payment::LnUrl;
///
/// let lnurl = LnUrl::new("lnbits.example.com", "lightningpos", "abc123...xyz");
/// let bech32 = lnurl.to_bech32(12).unwrap();  // pin 12
/// let lud17 = lnurl.to_lud17(12).unwrap();
/// ```

use crate::error::{PaymentError, Result};
use alloc::string::String;
use alloc::vec::Vec;
use bech32::{Bech32, Hrp};

/// LNURL generator
#[derive(Debug, Clone)]
pub struct LnUrl {
    server: String,
    extension: String,
    device_id: String,
}

impl LnUrl {
    pub fn new(server: &str, extension: &str, device_id: &str) -> Self {
        Self {
            server: server.to_string(),
            extension: extension.to_string(),
            device_id: device_id.to_string(),
        }
    }

    /// Build the raw HTTPS URL for a given relay pin
    pub fn build_url(&self, pin: u8) -> String {
        alloc::format!(
            "https://{}/{}/api/v1/lnurl/{}?pin={}",
            self.server,
            self.extension,
            self.device_id,
            pin
        )
    }

    /// Generate bech32-encoded LNURL (lightning:LNURL1...)
    pub fn to_bech32(&self, pin: u8) -> Result<String> {
        let url = self.build_url(pin);
        self.encode_bech32(&url)
    }

    /// Generate LUD17 format (lnurlp:https://...)
    pub fn to_lud17(&self, pin: u8) -> Result<String> {
        let url = self.build_url(pin);
        Ok(alloc::format!("lnurlp:{}", url))
    }

    /// Bech32-encode a URL as LNURL
    fn encode_bech32(&self, url: &str) -> Result<String> {
        // Convert URL bytes to 5-bit words
        let data_bytes: Vec<u8> = url.bytes().collect();
        let data_5bit = convert_bits(&data_bytes, 8, 5, true);

        if data_5bit.is_empty() {
            return Err(PaymentError::Bech32Encoding);
        }

        // HRP is "lnurl"
        let hrp = Hrp::parse("lnurl").map_err(|_| PaymentError::Bech32Encoding)?;

        // Encode with bech32 (not bech32m)
        let bech32 = bech32::encode::<Bech32>(hrp, &data_5bit)
            .map_err(|_| PaymentError::Bech32Encoding)?;

        // Return as lightning: prefix + uppercase (LNURL standard)
        Ok(alloc::format!("lightning:{}", bech32.to_uppercase()))
    }
}

/// Convert between bit widths with optional padding
fn convert_bits(data: &[u8], from_bits: u32, to_bits: u32, pad: bool) -> Vec<u8> {
    let mut result = Vec::new();
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    let maxv: u32 = (1 << to_bits) - 1;

    for &value in data {
        acc = (acc << from_bits) | value as u32;
        bits += from_bits;

        while bits >= to_bits {
            bits -= to_bits;
            result.push(((acc >> bits) & maxv) as u8);
        }
    }

    if pad {
        if bits > 0 {
            result.push(((acc << (to_bits - bits)) & maxv) as u8);
        }
    } else if bits >= from_bits || ((acc << (to_bits - bits)) & maxv) != 0 {
        return Vec::new(); // Invalid padding
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_url() {
        let lnurl = LnUrl::new("lnbits.example.com", "lightningpos", "abc123def456ghi789jkl0");
        let url = lnurl.build_url(12);
        assert_eq!(
            url,
            "https://lnbits.example.com/lightningpos/api/v1/lnurl/abc123def456ghi789jkl0?pin=12"
        );
    }

    #[test]
    fn test_lud17_format() {
        let lnurl = LnUrl::new("lnbits.example.com", "bitcoinswitch", "abc123def456ghi789jkl0");
        let lud17 = lnurl.to_lud17(1).unwrap();
        assert!(lud17.starts_with("lnurlp:https://"));
        assert!(lud17.contains("?pin=1"));
    }

    #[test]
    fn test_bech32_format() {
        let lnurl = LnUrl::new("lnbits.example.com", "lightningpos", "abc123def456ghi789jkl0");
        let bech32 = lnurl.to_bech32(4).unwrap();
        assert!(bech32.starts_with("lightning:LNURL1"));
    }

    #[test]
    fn test_bech32_roundtrip() {
        // Test that bech32 encoding is decodable
        let lnurl = LnUrl::new("test.com", "lightningpos", "abc123def456ghi789jkl0");
        let encoded = lnurl.to_bech32(1).unwrap();

        // Strip lightning: prefix
        let bech32_str = &encoded[10..]; // after "lightning:"
        let (hrp, data) = bech32::decode(bech32_str).unwrap();
        assert_eq!(hrp.to_string(), "lnurl");

        // Convert 5-bit back to 8-bit
        let bytes = convert_bits(&data, 5, 8, false);
        let decoded = String::from_utf8(bytes).unwrap();
        assert_eq!(decoded, "https://test.com/lightningpos/api/v1/lnurl/abc123def456ghi789jkl0?pin=1");
    }
}
