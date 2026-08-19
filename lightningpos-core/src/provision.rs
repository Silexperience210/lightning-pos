/// POS provisioning — configuration via the captive web portal
///
/// The POS boots unprovisioned into AP mode and serves a form where the
/// merchant enters WiFi credentials + LNbits connection info. This module
/// contains the pure logic: form parsing (percent-decoding), validation,
/// and the provisioning state machine. I/O (HTTP server, NVS) lives in the
/// firmware layer.
///
/// Security: the LNbits API key is treated as a secret. It is never logged
/// and is stored encrypted (eFuse AES key + encrypted NVS blob, see firmware).

use crate::config::Currency;
use crate::error::{ConfigError, Result};
use alloc::string::String;
use alloc::vec::Vec;
use heapless::String as HString;

pub const MAX_SSID_LEN: usize = 64;
pub const MAX_PASS_LEN: usize = 64;
pub const MAX_URL_LEN: usize = 256;
pub const MAX_KEY_LEN: usize = 64;

/// Fields submitted by the provisioning form (raw strings)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionForm {
    pub ssid: String,
    pub wifi_password: String,
    pub lnbits_url: String,
    pub lnbits_api_key: String,
    pub currency: String,
}

impl ProvisionForm {
    /// Parse an `application/x-www-form-urlencoded` body with percent-decoding.
    pub fn parse(body: &str) -> Result<Self> {
        let mut ssid = None;
        let mut wifi_password = None;
        let mut lnbits_url = None;
        let mut lnbits_api_key = None;
        let mut currency = None;

        for pair in body.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (key, value) = match pair.split_once('=') {
                Some((k, v)) => (k, v),
                None => (pair, ""),
            };
            let value = percent_decode(value)?;

            match key {
                "ssid" => ssid = Some(value),
                "wifipass" => wifi_password = Some(value),
                "lnbits_url" => lnbits_url = Some(value),
                "apikey" => lnbits_api_key = Some(value),
                "currency" => currency = Some(value),
                _ => {} // ignore unknown fields (forward compat)
            }
        }

        Ok(Self {
            ssid: ssid.ok_or_else(|| ConfigError::MissingField("ssid"))?,
            wifi_password: wifi_password
                .ok_or_else(|| ConfigError::MissingField("wifipass"))?,
            lnbits_url: lnbits_url.ok_or_else(|| ConfigError::MissingField("lnbits_url"))?,
            lnbits_api_key: lnbits_api_key
                .ok_or_else(|| ConfigError::MissingField("apikey"))?,
            currency: currency.unwrap_or_else(|| String::from("EUR")),
        })
    }

    /// Validate the form before attempting any network call.
    pub fn validate(&self) -> Result<()> {
        if self.ssid.is_empty() || self.ssid.len() > MAX_SSID_LEN {
            return Err(ConfigError::InvalidValue {
                field: "ssid",
                value: self.ssid.clone(),
            }
            .into());
        }
        if self.wifi_password.len() > MAX_PASS_LEN {
            return Err(ConfigError::InvalidValue {
                field: "wifipass",
                value: String::from("<redacted>"),
            }
            .into());
        }
        if self.lnbits_url.len() > MAX_URL_LEN
            || !(self.lnbits_url.starts_with("http://")
                || self.lnbits_url.starts_with("https://"))
        {
            return Err(ConfigError::InvalidValue {
                field: "lnbits_url",
                value: self.lnbits_url.clone(),
            }
            .into());
        }
        // LNbits invoice keys are 32 hex chars; accept 16..64 to be lenient
        if self.lnbits_api_key.len() < 16 || self.lnbits_api_key.len() > MAX_KEY_LEN {
            return Err(ConfigError::InvalidValue {
                field: "apikey",
                value: String::from("<redacted>"),
            }
            .into());
        }
        if Currency::from_str(&self.currency) == Currency::USD && self.currency != "USD" && self.currency != "usd" {
            // from_str defaults to USD for unknown values; reject unknown codes
            return Err(ConfigError::InvalidValue {
                field: "currency",
                value: self.currency.clone(),
            }
            .into());
        }
        Ok(())
    }
}

/// Percent-decode a form value (`+` = space, `%XX` = byte)
fn percent_decode(s: &str) -> Result<String> {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' => {
                if i + 2 >= bytes.len() + 1 || i + 2 > bytes.len() {
                    // malformed escape
                    return Err(ConfigError::Parse(String::from(
                        "truncated percent escape in form body",
                    ))
                    .into());
                }
                if i + 2 >= bytes.len() {
                    return Err(ConfigError::Parse(String::from(
                        "truncated percent escape in form body",
                    ))
                    .into());
                }
                let hi = hex_val(bytes[i + 1]);
                let lo = hex_val(bytes[i + 2]);
                if hi.is_none() || lo.is_none() {
                    return Err(ConfigError::Parse(String::from(
                        "invalid percent escape in form body",
                    ))
                    .into());
                }
                out.push((hi.unwrap() << 4) | lo.unwrap());
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out)
        .map_err(|_| ConfigError::Parse(String::from("invalid UTF-8 in form body")).into())
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Provisioning state machine (drives the firmware)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvisionState {
    /// No valid config — boot into AP mode
    Unprovisioned,
    /// Access point up, portal served
    ApReady,
    /// Form submitted — validating WiFi credentials
    ValidatingWifi,
    /// WiFi OK — validating the LNbits key against the server
    ValidatingLnbits,
    /// Everything valid — saving encrypted config
    Saving,
    /// Provisioned — reboot into POS mode
    Done,
    /// Invalid WiFi (wrong SSID/password)
    WifiError,
    /// LNbits unreachable or key rejected
    LnbitsError,
    /// Portal timeout — user walked away, return to AP ready
    Timeout,
}

/// Stored provisioning config (what actually gets persisted, encrypted)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionConfig {
    pub ssid: String,
    pub wifi_password: String,
    pub lnbits_url: String,
    pub lnbits_api_key: String,
    pub currency: Currency,
}

impl ProvisionConfig {
    /// Build from a validated form
    pub fn from_form(form: &ProvisionForm) -> Self {
        Self {
            ssid: form.ssid.clone(),
            wifi_password: form.wifi_password.clone(),
            lnbits_url: form.lnbits_url.clone(),
            lnbits_api_key: form.lnbits_api_key.clone(),
            currency: Currency::from_str(&form.currency),
        }
    }

    /// Strip the trailing slash of the LNbits URL for consistent use
    pub fn normalized_lnbits_url(&self) -> String {
        String::from(self.lnbits_url.trim_end_matches('/'))
    }
}

// heapless re-export used by the firmware for fixed-size buffers
pub type UrlStr = HString<MAX_URL_LEN>;
pub type KeyStr = HString<MAX_KEY_LEN>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_form() {
        let body = "ssid=MaBoutique&wifipass=secret123&lnbits_url=http%3A%2F%2F192.168.1.176%3A3007&apikey=0123456789abcdef0123456789abcdef&currency=EUR";
        let f = ProvisionForm::parse(body).unwrap();
        assert_eq!(f.ssid, "MaBoutique");
        assert_eq!(f.wifi_password, "secret123");
        assert_eq!(f.lnbits_url, "http://192.168.1.176:3007");
        assert_eq!(f.lnbits_api_key, "0123456789abcdef0123456789abcdef");
        assert_eq!(f.currency, "EUR");
    }

    #[test]
    fn parse_plus_and_percent() {
        let body = "ssid=Caf%C3%A9+du+Coin&wifipass=&lnbits_url=http%3A%2F%2Fx&apikey=0123456789abcdef0123456789abcdef&currency=EUR";
        let f = ProvisionForm::parse(body).unwrap();
        assert_eq!(f.ssid, "Café du Coin");
        assert_eq!(f.wifi_password, ""); // open network allowed
    }

    #[test]
    fn parse_missing_field() {
        let body = "ssid=x&wifipass=y&lnbits_url=http://x";
        assert!(ProvisionForm::parse(body).is_err());
    }

    #[test]
    fn parse_unknown_field_ignored() {
        let body = "ssid=x&wifipass=y&lnbits_url=http://x&apikey=0123456789abcdef0123456789abcdef&currency=EUR&evil=1";
        assert!(ProvisionForm::parse(body).is_ok());
    }

    #[test]
    fn parse_bad_percent_escape() {
        let body = "ssid=%ZZ&wifipass=&lnbits_url=http://x&apikey=0123456789abcdef0123456789abcdef";
        assert!(ProvisionForm::parse(body).is_err());
    }

    #[test]
    fn validate_ok() {
        let body = "ssid=Shop&wifipass=pw&lnbits_url=http://192.168.1.176:3007&apikey=0123456789abcdef0123456789abcdef&currency=EUR";
        let f = ProvisionForm::parse(body).unwrap();
        assert!(f.validate().is_ok());
    }

    #[test]
    fn validate_bad_url() {
        let body = "ssid=Shop&wifipass=pw&lnbits_url=ftp://nope&apikey=0123456789abcdef0123456789abcdef";
        let f = ProvisionForm::parse(body).unwrap();
        assert!(f.validate().is_err());
    }

    #[test]
    fn validate_short_key() {
        let body = "ssid=Shop&wifipass=pw&lnbits_url=http://x&apikey=tooshort";
        let f = ProvisionForm::parse(body).unwrap();
        assert!(f.validate().is_err());
    }

    #[test]
    fn validate_bad_currency() {
        let body = "ssid=Shop&wifipass=pw&lnbits_url=http://x&apikey=0123456789abcdef0123456789abcdef&currency=XXX";
        let f = ProvisionForm::parse(body).unwrap();
        assert!(f.validate().is_err());
    }

    #[test]
    fn from_form_builds_config() {
        let body = "ssid=Shop&wifipass=pw&lnbits_url=http://192.168.1.176:3007/&apikey=0123456789abcdef0123456789abcdef&currency=EUR";
        let f = ProvisionForm::parse(body).unwrap();
        let cfg = ProvisionConfig::from_form(&f);
        assert_eq!(cfg.currency, Currency::EUR);
        assert_eq!(cfg.normalized_lnbits_url(), "http://192.168.1.176:3007");
    }
}
