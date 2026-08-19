//! Bolt Card — logique pure LNURL-withdraw (sans I/O), portée de lightningpos-core.
//! Flux : NDEF URI → GET → { tag:"withdrawRequest", k1, callback } →
//!        facture LNbits → callback?k1=..&pr=.. → poll → PAYÉ.

/// Réponse LNURL-withdraw (GET du LNURL)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LnurlWithdraw {
    pub callback: String,
    pub k1: String,
    pub max_withdrawable_msat: u64,
    pub min_withdrawable_msat: u64,
}

impl LnurlWithdraw {
    pub fn parse(json: &str) -> Result<Self, String> {
        let v: serde_json::Value =
            serde_json::from_str(json).map_err(|e| format!("json invalide: {e}"))?;

        // LUD-06 : un refus arrive en HTTP 200 avec {"status":"ERROR","reason":…}
        // et sans champ `tag`. Sans ce cas, tout motif serveur (« This link is
        // already used. », « Card UID mis-match. », « Max daily limit spent. »)
        // se réduisait à « champ tag manquant » — le vrai diagnostic était invisible.
        if let Some(reason) = v["reason"].as_str() {
            return Err(format!("serveur: {reason}"));
        }

        let tag = v["tag"].as_str().ok_or("champ tag manquant")?;
        if tag != "withdrawRequest" {
            return Err(format!("tag inattendu: {tag}"));
        }
        let callback = v["callback"]
            .as_str()
            .ok_or("callback manquant")?
            .to_string();
        let k1 = v["k1"].as_str().ok_or("k1 manquant")?.to_string();
        let max_withdrawable_msat = v["maxWithdrawable"].as_u64().unwrap_or(u64::MAX);
        let min_withdrawable_msat = v["minWithdrawable"].as_u64().unwrap_or(0);

        Ok(Self {
            callback,
            k1,
            max_withdrawable_msat,
            min_withdrawable_msat,
        })
    }

    /// Construit l'URL de callback avec la facture BOLT11
    pub fn build_callback_url(&self, invoice: &str) -> String {
        if self.callback.contains('?') {
            format!("{}&k1={}&pr={}", self.callback, self.k1, invoice)
        } else {
            format!("{}?k1={}&pr={}", self.callback, self.k1, invoice)
        }
    }

    pub fn amount_in_range_msat(&self, amount_msat: u64) -> bool {
        amount_msat >= self.min_withdrawable_msat && amount_msat <= self.max_withdrawable_msat
    }
}

/// EUR cents → sats (prix BTC en EUR cents). Arrondi vers le bas.
pub fn eur_cents_to_sats(eur_cents: u64, btc_price_cents: u64) -> u64 {
    if btc_price_cents == 0 {
        return 0;
    }
    ((eur_cents as u128) * 100_000_000 / (btc_price_cents as u128)) as u64
}

/// sats → msat (les montants LNURL sont en msat)
pub const fn sats_to_msat(sats: u64) -> u64 {
    sats.saturating_mul(1000)
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
    fn parse_valid() {
        let w = LnurlWithdraw::parse(SAMPLE).unwrap();
        assert_eq!(w.k1.len(), 64);
        assert_eq!(w.max_withdrawable_msat, 1_000_000);
    }

    #[test]
    fn callback_url() {
        let w = LnurlWithdraw::parse(SAMPLE).unwrap();
        let url = w.build_callback_url("lnbc1fake");
        assert!(url.starts_with("https://legend.lnbits.com/withdraw/api/v1/lnurl/cb/abc123?"));
        assert!(url.contains("k1=0123456789abcdef"));
        assert!(url.contains("pr=lnbc1fake"));
    }

    #[test]
    fn eur_to_sats() {
        assert_eq!(eur_cents_to_sats(100, 6_723_450), 1487);
        assert_eq!(eur_cents_to_sats(1_000, 6_723_450), 14_873);
        assert_eq!(eur_cents_to_sats(100, 0), 0);
    }
}
