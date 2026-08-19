/// Bolt Card — LNURL-withdraw handshake for the POS flow
///
/// Flow (Bolt Card standard, https://boltcard.org):
///
/// ```text
/// NFC tap → NDEF URI (LNURL-withdraw URL)
///    → GET URL → { tag:"withdrawRequest", k1, callback, max/minWithdrawable }
///    → POST callback?k1=<k1>&pr=<bolt11 invoice>
///    → card wallet pays the invoice
///    → poll LNbits /api/v1/payments/<hash> until paid
/// ```
///
/// This module is pure logic (no I/O) so it can be unit-tested on the host.

use crate::error::{Error, PaymentError, Result};
use alloc::string::String;

/// LNURL-withdraw response (parsed from the LNURL GET)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LnurlWithdraw {
    /// Callback URL where the invoice is submitted
    pub callback: String,
    /// One-time secret (hex) that authenticates the withdraw request
    pub k1: String,
    /// Max amount the card accepts (millisatoshis)
    pub max_withdrawable_msat: u64,
    /// Min amount the card accepts (millisatoshis)
    pub min_withdrawable_msat: u64,
    /// Default description provided by the card service
    pub default_description: String,
}

impl LnurlWithdraw {
    /// Parse a LNURL-withdraw JSON response
    pub fn parse(json: &str) -> Result<Self> {
        let v: serde_json::Value =
            serde_json::from_str(json).map_err(|e| Error::Payment(PaymentError::LnUrlGeneration(
                alloc::format!("invalid JSON: {e}"),
            )))?;

        let tag = v["tag"].as_str().ok_or_else(|| {
            Error::Payment(PaymentError::LnUrlGeneration(
                "missing tag field".into(),
            ))
        })?;

        if tag != "withdrawRequest" {
            return Err(Error::Payment(PaymentError::LnUrlGeneration(
                alloc::format!("unexpected tag: {tag} (expected withdrawRequest)"),
            )));
        }

        let callback = String::from(
            v["callback"]
                .as_str()
                .ok_or_else(|| {
                    Error::Payment(PaymentError::LnUrlGeneration("missing callback".into()))
                })?,
        );

        let k1 = String::from(
            v["k1"]
                .as_str()
                .ok_or_else(|| Error::Payment(PaymentError::LnUrlGeneration("missing k1".into())))?,
        );

        // maxWithdrawable/minWithdrawable are in millisatoshis
        let max_withdrawable_msat = v["maxWithdrawable"].as_u64().unwrap_or(u64::MAX);
        let min_withdrawable_msat = v["minWithdrawable"].as_u64().unwrap_or(0);
        let default_description = String::from(v["defaultDescription"].as_str().unwrap_or(""));

        Ok(Self {
            callback,
            k1,
            max_withdrawable_msat,
            min_withdrawable_msat,
            default_description,
        })
    }

    /// Build the callback URL with the merchant's BOLT11 invoice
    ///
    /// The callback may already contain query parameters; in that case we
    /// append with `&`, otherwise with `?`.
    pub fn build_callback_url(&self, invoice: &str) -> String {
        if self.callback.contains('?') {
            alloc::format!("{}&k1={}&pr={}", self.callback, self.k1, invoice)
        } else {
            alloc::format!("{}?k1={}&pr={}", self.callback, self.k1, invoice)
        }
    }

    /// True if `amount_msat` is within the card's accepted range
    pub fn amount_in_range_msat(&self, amount_msat: u64) -> bool {
        amount_msat >= self.min_withdrawable_msat && amount_msat <= self.max_withdrawable_msat
    }
}

/// Convert an amount in EUR cents to satoshis, given the BTC price in EUR cents.
///
/// `btc_price_cents` = price of 1 BTC in EUR cents (e.g. 6_723_450 = €67,234.50).
/// The result is rounded down (merchant-friendly: never overcharges).
pub fn eur_cents_to_sats(eur_cents: u64, btc_price_cents: u64) -> u64 {
    if btc_price_cents == 0 {
        return 0;
    }
    ((eur_cents as u128) * 100_000_000 / (btc_price_cents as u128)) as u64
}

/// Convert satoshis to millisatoshis (LNURL amounts are in msat)
pub const fn sats_to_msat(sats: u64) -> u64 {
    sats.saturating_mul(1000)
}

/// POS payment phases (drives the firmware UI)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PosPhase {
    /// Waiting for the merchant to enter an amount
    Idle,
    /// Amount being entered on the keypad
    AmountEntry,
    /// Invoice being created on LNbits
    CreatingInvoice,
    /// Invoice created, waiting for a card tap
    AwaitingCard,
    /// Card read, LNURL-withdraw in progress
    ProcessingCard,
    /// Callback sent, waiting for the payment to settle
    AwaitingPayment,
    /// Payment confirmed
    Paid,
    /// Something failed (see error message on the UI)
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "tag": "withdrawRequest",
        "callback": "https://legend.lnbits.com/withdraw/api/v1/lnurl/cb/abc123",
        "k1": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "maxWithdrawable": 1000000,
        "minWithdrawable": 1000,
        "defaultDescription": "Bolt Card"
    }"#;

    #[test]
    fn parse_valid_lnurl() {
        let w = LnurlWithdraw::parse(SAMPLE).unwrap();
        assert_eq!(w.callback, "https://legend.lnbits.com/withdraw/api/v1/lnurl/cb/abc123");
        assert!(w.k1.len() == 64);
        assert_eq!(w.max_withdrawable_msat, 1_000_000);
        assert_eq!(w.min_withdrawable_msat, 1_000);
        assert_eq!(w.default_description, "Bolt Card");
    }

    #[test]
    fn parse_wrong_tag() {
        let json = r#"{"tag": "payRequest"}"#;
        assert!(LnurlWithdraw::parse(json).is_err());
    }

    #[test]
    fn parse_missing_k1() {
        let json = r#"{"tag": "withdrawRequest", "callback": "https://x"}"#;
        assert!(LnurlWithdraw::parse(json).is_err());
    }

    #[test]
    fn callback_without_query() {
        let w = LnurlWithdraw::parse(SAMPLE).unwrap();
        let invoice = "lnbc10n1p4gpqt9pp5fake";
        let url = w.build_callback_url(invoice);
        assert!(url.starts_with("https://legend.lnbits.com/withdraw/api/v1/lnurl/cb/abc123?"));
        assert!(url.contains("k1=0123456789abcdef"));
        assert!(url.contains(invoice));
    }

    #[test]
    fn callback_with_existing_query() {
        let json = r#"{
            "tag": "withdrawRequest",
            "callback": "https://x.com/api?nonce=42",
            "k1": "aa",
            "maxWithdrawable": 100,
            "minWithdrawable": 1,
            "defaultDescription": ""
        }"#;
        let w = LnurlWithdraw::parse(json).unwrap();
        let url = w.build_callback_url("lnbc1");
        assert!(url.starts_with("https://x.com/api?nonce=42&k1=aa&pr=lnbc1"));
    }

    #[test]
    fn amount_in_range() {
        let w = LnurlWithdraw::parse(SAMPLE).unwrap();
        assert!(w.amount_in_range_msat(1_000));
        assert!(w.amount_in_range_msat(1_000_000));
        assert!(!w.amount_in_range_msat(999));
        assert!(!w.amount_in_range_msat(1_000_001));
    }

    #[test]
    fn eur_to_sats_conversion() {
        // 1 € at €67,234.50/BTC → 100 * 1e8 / 6_723_450 = 1487 sats
        assert_eq!(eur_cents_to_sats(100, 6_723_450), 1487);
        // 10 € → 14_873 sats
        assert_eq!(eur_cents_to_sats(1_000, 6_723_450), 14_873);
        // 0 € → 0 sats
        assert_eq!(eur_cents_to_sats(0, 6_723_450), 0);
        // price 0 → 0 (no div by zero)
        assert_eq!(eur_cents_to_sats(100, 0), 0);
    }

    #[test]
    fn sats_to_msat_conversion() {
        assert_eq!(sats_to_msat(1), 1000);
        assert_eq!(sats_to_msat(1487), 1_487_000);
    }
}
