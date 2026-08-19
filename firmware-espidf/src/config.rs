//! Configuration du POS : URL + clé API LNbits, persistées en NVS (namespace
//! `poscfg`). Le portail web (`/config`) permet de changer l'instance LNbits
//! vers laquelle pointe le POS (factures QR + poll).
//!
//! Note : les cartes Bolt Card, elles, pointent vers l'instance inscrite dans
//! leur NDEF (21pay, LNbits local, …) — le POS suit l'URI lue sur la carte
//! quelle que soit l'instance. Seul le back-end des factures QR est ici.

use esp_idf_svc::nvs::EspDefaultNvs;

const KEY_URL: &str = "lnbits_url";
const KEY_API: &str = "lnbits_key";
const KEY_CUR: &str = "currency";

/// URL LNbits proposée tant que rien n'a été configuré via `/config`.
/// Ce n'est pas un secret : juste une valeur de départ, remplaçable au portail.
const DEFAULT_LNBITS_URL: &str = "http://192.168.1.176:3007";

/// Longueur maximale autorisée pour l'URL LNbits (portail /config).
pub const MAX_URL_LEN: usize = 256;
/// Longueur maximale autorisée pour une URL LNURL lue sur une carte.
pub const MAX_LNURL_LEN: usize = 512;

/// Devise du POS : "EUR" (défaut), "USD" ou "GBP".
pub fn load_currency(nvs: &EspDefaultNvs) -> String {
    let mut buf = [0u8; 8];
    nvs.get_str(KEY_CUR, &mut buf)
        .ok()
        .flatten()
        .map(|s| s.to_string())
        .filter(|s| matches!(s.as_str(), "EUR" | "USD" | "GBP"))
        .unwrap_or_else(|| "EUR".to_string())
}

/// URL de l'instance LNbits (défaut : [`DEFAULT_LNBITS_URL`]).
pub fn load_url(nvs: &EspDefaultNvs) -> String {
    let mut buf = [0u8; 256];
    nvs.get_str(KEY_URL, &mut buf)
        .ok()
        .flatten()
        .map(|s| s.to_string())
        .unwrap_or_else(|| DEFAULT_LNBITS_URL.to_string())
}

/// Clé API LNbits. Aucune valeur par défaut : la clé se provisionne au portail
/// `/config` et n'existe qu'en NVS — le binaire ne contient aucun secret, et
/// aucune constante compilée ne peut plus en réintroduire un par inadvertance.
pub fn load_key(nvs: &EspDefaultNvs) -> String {
    let mut buf = [0u8; 128];
    nvs.get_str(KEY_API, &mut buf)
        .ok()
        .flatten()
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// Derniers 4 caractères de la clé (affichage masqué dans le portail web).
pub fn key_tail(nvs: &EspDefaultNvs) -> String {
    let k = load_key(nvs);
    if k.len() <= 4 {
        return k;
    }
    format!("…{}", &k[k.len() - 4..])
}

/// Vérifie qu'une URL est autorisée : schéma http(s), hôte non vide, pas
/// d'userinfo, longueur ≤ `max_len`, et https obligatoire pour les hôtes
/// publics.
pub fn validate_url(url: &str, max_len: usize) -> Result<(), &'static str> {
    if url.len() > max_len {
        return Err("URL trop longue");
    }
    let lower = url.to_ascii_lowercase();
    let (scheme, rest) = if lower.starts_with("https://") {
        ("https", &url[8..])
    } else if lower.starts_with("http://") {
        ("http", &url[7..])
    } else {
        return Err("URL invalide");
    };

    let host_port = rest.split('/').next().unwrap_or(rest);
    if host_port.is_empty() {
        return Err("URL invalide");
    }
    if host_port.contains('@') {
        return Err("URL invalide");
    }
    let host = host_port.split(':').next().unwrap_or(host_port);
    if host.is_empty() {
        return Err("URL invalide");
    }
    if scheme == "http" && !is_private_host(host) {
        return Err("https requis pour cet hôte");
    }
    Ok(())
}

fn is_private_host(host: &str) -> bool {
    let lower = host.to_ascii_lowercase();
    if lower.ends_with(".local") {
        return true;
    }
    // IPv4 privées : 10/8, 127/8, 172.16-31/12, 192.168/16.
    let mut octets = lower.split('.').map(|s| s.parse::<u8>().ok());
    if let (Some(Some(a)), Some(Some(b))) = (octets.next(), octets.next()) {
        if a == 10 || a == 127 {
            return true;
        }
        if a == 172 && (16..=31).contains(&b) {
            return true;
        }
        if a == 192 && b == 168 {
            return true;
        }
    }
    false
}

/// Enregistre URL + clé + devise. Un champ vide = « garder l'existante » (le
/// portail affiche la clé masquée, l'utilisateur ne la retape pas à chaque fois).
pub fn save(nvs: &EspDefaultNvs, url: &str, key: &str, currency: &str) -> Result<(), String> {
    let url = url.trim();
    if !url.is_empty() {
        validate_url(url, MAX_URL_LEN).map_err(|e| e.to_string())?;
        nvs.set_str(KEY_URL, url).map_err(|e| e.to_string())?;
    }
    let key = key.trim();
    if !key.is_empty() {
        nvs.set_str(KEY_API, key).map_err(|e| e.to_string())?;
    }
    let currency = currency.trim().to_uppercase();
    if matches!(currency.as_str(), "EUR" | "USD" | "GBP") {
        nvs.set_str(KEY_CUR, &currency).map_err(|e| e.to_string())?;
    }
    Ok(())
}
