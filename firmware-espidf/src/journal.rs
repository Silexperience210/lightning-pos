//! Journal des transactions : anneau persisté en NVS.
//!
//! Chaque transaction est rangée sous une clé `t{slot}` (format
//! `seq|ts|epoch|cents|sats|cur|label|hash`), avec un compteur monotone `cnt`
//! qui donne le numéro absolu. La lecture restitue les entrées de la plus
//! récente à la plus ancienne.
//!
//! Horodatage : `epoch = true` ⇒ `ts` est une date UNIX réelle (SNTP a
//! synchronisé l'horloge) ; `epoch = false` ⇒ `ts` est l'uptime en secondes
//! (esp_timer), qui repart de zéro à chaque reboot. Le `seq` reste la clé
//! d'ordre entre entrées, quel que soit le mode.

use std::time::{SystemTime, UNIX_EPOCH};

use esp_idf_svc::nvs::EspDefaultNvs;
use esp_idf_svc::sys::EspError;

/// Nombre maximal de transactions conservées (anneau).
///
/// Plafonné par la partition NVS : la table `partitions_singleapp` réserve
/// 24 Ko (6 pages de 4 Ko, dont une gardée libre pour le compactage), soit
/// ~630 entrées de 32 octets. Une ligne de journal (hash de 64 hex) occupe
/// 5 entrées ; les produits et la config en consomment déjà ~70. 200 entrées
/// (1000 entrées NVS) déborderaient — les écritures échoueraient en silence.
pub const MAX_TX: usize = 100;

/// Longueur maximale du libellé stocké (déborde sinon la ligne NVS).
const MAX_LABEL: usize = 24;

/// Une transaction enregistrée.
#[derive(Debug, Clone)]
pub struct Tx {
    pub seq: u32,
    /// Date UNIX si `epoch`, sinon secondes depuis le boot (esp_timer).
    pub ts: u64,
    /// `true` si `ts` est une date réelle (horloge synchronisée par SNTP).
    pub epoch: bool,
    pub cents: u64,
    pub sats: u64,
    /// Devise de facturation au moment de la vente ("EUR", "USD", "GBP").
    pub cur: String,
    /// Libellé de la vente (produit, « 3x BIERE », « MONTANT LIBRE »…).
    pub label: String,
    pub hash: String,
}

/// Horodatage courant : date UNIX si l'horloge a été réglée (SNTP), sinon
/// l'uptime. Le booléen dit lequel des deux.
pub fn now_ts() -> (u64, bool) {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Seuil = 1er janvier 2023 : en deçà, l'horloge n'a jamais été synchronisée
    // (l'ESP démarre à l'epoch 0) et la « date » n'aurait aucun sens.
    if epoch > 1_672_531_200 {
        (epoch, true)
    } else {
        let up = (unsafe { esp_idf_sys::esp_timer_get_time() } / 1_000_000) as u64;
        (up, false)
    }
}

/// Neutralise le séparateur de champ dans une valeur libre.
fn sanitize(s: &str, max: usize) -> String {
    s.chars()
        .map(|c| if c == '|' || c == '\n' { ' ' } else { c })
        .take(max)
        .collect()
}

/// Ajoute une transaction à l'anneau.
pub fn record(nvs: &EspDefaultNvs, tx: &Tx) -> Result<(), EspError> {
    let seq = nvs.get_u32("cnt")?.unwrap_or(0);
    let slot = seq % (MAX_TX as u32);
    let line = format!(
        "{}|{}|{}|{}|{}|{}|{}|{}",
        seq,
        tx.ts,
        if tx.epoch { 1 } else { 0 },
        tx.cents,
        tx.sats,
        sanitize(&tx.cur, 4),
        sanitize(&tx.label, MAX_LABEL),
        tx.hash
    );
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
        let mut buf = [0u8; 256];
        if let Some(s) = nvs.get_str(&format!("t{slot}"), &mut buf).ok().flatten() {
            if let Some(tx) = parse(s) {
                out.push(tx);
            }
        }
    }
    out
}

/// Parse une ligne `seq|ts|epoch|cents|sats|cur|label|hash`, ou l'ancien format
/// `seq|ts|cents|sats|hash` (journaux écrits avant l'ajout de la date réelle :
/// ils restent lisibles au lieu de disparaître de la table après mise à jour).
fn parse(s: &str) -> Option<Tx> {
    if s.matches('|').count() >= 7 {
        let mut it = s.splitn(8, '|');
        let seq = it.next()?.parse().ok()?;
        let ts = it.next()?.parse().ok()?;
        let epoch = it.next()? == "1";
        let cents = it.next()?.parse().ok()?;
        let sats = it.next()?.parse().ok()?;
        let cur = it.next()?.to_string();
        let label = it.next()?.to_string();
        let hash = it.next()?.to_string();
        return Some(Tx {
            seq,
            ts,
            epoch,
            cents,
            sats,
            cur,
            label,
            hash,
        });
    }
    let mut it = s.splitn(5, '|');
    let seq = it.next()?.parse().ok()?;
    let ts = it.next()?.parse().ok()?;
    let cents = it.next()?.parse().ok()?;
    let sats = it.next()?.parse().ok()?;
    let hash = it.next()?.to_string();
    Some(Tx {
        seq,
        ts,
        epoch: false,
        cents,
        sats,
        cur: String::new(),
        label: String::new(),
        hash,
    })
}
