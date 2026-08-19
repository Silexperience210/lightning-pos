//! Journal des transactions : anneau de 50 entrées persisté en NVS.
//!
//! Chaque transaction est rangée sous une clé `t{slot}` (format
//! `seq|ts|cents|sats|hash`), avec un compteur monotone `cnt` qui donne le
//! numéro absolu. La lecture restitue les entrées de la plus récente à la plus
//! ancienne. Le timestamp est l'uptime en secondes (esp_timer) — pas d'horloge
//! murale tant que SNTP n'est pas configuré.

use esp_idf_svc::nvs::EspDefaultNvs;
use esp_idf_svc::sys::EspError;

/// Nombre maximal de transactions conservées (anneau).
pub const MAX_TX: usize = 50;

/// Une transaction enregistrée.
#[derive(Debug, Clone)]
pub struct Tx {
    pub seq: u32,
    pub ts: u64, // secondes depuis le boot (esp_timer)
    pub cents: u64,
    pub sats: u64,
    pub hash: String,
}

/// Ajoute une transaction à l'anneau.
pub fn record(nvs: &EspDefaultNvs, tx: &Tx) -> Result<(), EspError> {
    let seq = nvs.get_u32("cnt")?.unwrap_or(0);
    let slot = seq % (MAX_TX as u32);
    let line = format!("{}|{}|{}|{}|{}", seq, tx.ts, tx.cents, tx.sats, tx.hash);
    nvs.set_str(&format!("t{slot}"), &line)?;
    nvs.set_u32("cnt", seq + 1)?;
    Ok(())
}

/// Restitue les transactions, de la plus récente à la plus ancienne.
pub fn list(nvs: &EspDefaultNvs) -> Vec<Tx> {
    let cnt = nvs.get_u32("cnt").ok().flatten().unwrap_or(0);
    let n = MAX_TX.min(cnt as usize);
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let slot = (cnt - 1 - k as u32) % (MAX_TX as u32);
        let mut buf = [0u8; 160];
        if let Some(s) = nvs.get_str(&format!("t{slot}"), &mut buf).ok().flatten() {
            if let Some(tx) = parse(s) {
                out.push(tx);
            }
        }
    }
    out
}

/// Parse une ligne `seq|ts|cents|sats|hash`.
fn parse(s: &str) -> Option<Tx> {
    let mut it = s.splitn(5, '|');
    let seq = it.next()?.parse().ok()?;
    let ts = it.next()?.parse().ok()?;
    let cents = it.next()?.parse().ok()?;
    let sats = it.next()?.parse().ok()?;
    let hash = it.next()?.to_string();
    Some(Tx {
        seq,
        ts,
        cents,
        sats,
        hash,
    })
}
