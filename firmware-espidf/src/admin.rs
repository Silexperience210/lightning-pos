//! PIN admin du portail web : généré aléatoirement au premier boot, stocké en NVS.
//! Affiché à l'écran au boot (4 s) et en serial — jamais transmis par le réseau.
//! Le portail /config le demande pour toute action d'administration.

use esp_idf_svc::nvs::EspDefaultNvs;
use esp_idf_svc::sys::EspError;

const KEY_PIN: &str = "admin_pin";

/// PIN 6 chiffres aléatoire (esp_random).
fn gen_pin() -> String {
    let mut out = String::with_capacity(6);
    for _ in 0..6 {
        let r = unsafe { esp_idf_sys::esp_random() % 10 };
        out.push((b'0' + r as u8) as char);
    }
    out
}

/// Charge le PIN, ou le génère et le persiste s'il n'existe pas encore.
pub fn ensure_pin(nvs: &EspDefaultNvs) -> String {
    let mut buf = [0u8; 8];
    if let Ok(Some(p)) = nvs.get_str(KEY_PIN, &mut buf) {
        return p.to_string();
    }
    let pin = gen_pin();
    let _ = nvs.set_str(KEY_PIN, &pin);
    pin
}

/// Vérifie un PIN saisi (comparaison à temps constant, en dur).
pub fn check_pin(nvs: &EspDefaultNvs, input: &str) -> bool {
    let pin = ensure_pin(nvs);
    if pin.len() != input.len() {
        return false;
    }
    let a = pin.as_bytes();
    let b = input.as_bytes();
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}
