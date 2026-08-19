//! Provisioning WiFi : config (SSID + mot de passe) stockée en NVS (namespace
//! `wifi`), résultat du scan des réseaux disponible pour le portail /wifi.
//! Plus aucun identifiant WiFi dans le binaire.

use std::sync::{Mutex, OnceLock};

use esp_idf_svc::nvs::{EspDefaultNvs, EspDefaultNvsPartition, EspNvs};
use esp_idf_svc::sys::EspError;

const KEY_SSID: &str = "ssid";
const KEY_PASS: &str = "pass";
const KEY_AP_PASS: &str = "ap_pass";

/// Longueur du mot de passe WPA2 de l'AP (WPA2 exige ≥ 8 caractères).
const AP_PASS_LEN: usize = 10;

/// Alphabet du mot de passe AP : sans I/l/O/0/1, illisibles à l'écran 5×7.
const AP_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// Handle NVS du namespace `wifi`, ouvert une fois par [`start`].
static NVS: OnceLock<EspDefaultNvs> = OnceLock::new();

/// Réseaux détectés au boot (SSID), servis par le portail /wifi.
static SCAN: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

/// Ouvre le namespace `wifi` (à appeler au boot, avant toute autre fonction).
pub fn start(partition: EspDefaultNvsPartition) {
    let _ = NVS.set(EspNvs::new(partition, "wifi", true).unwrap());
    let _ = SCAN.set(Mutex::new(Vec::new()));
}

fn nvs() -> &'static EspDefaultNvs {
    NVS.get().expect("wifi_cfg::start doit être appelé avant")
}

/// SSID configuré (chaîne vide = non provisionné).
pub fn ssid() -> String {
    let mut buf = [0u8; 64];
    nvs()
        .get_str(KEY_SSID, &mut buf)
        .ok()
        .flatten()
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// Mot de passe configuré (chaîne vide = réseau ouvert ou non provisionné).
pub fn pass() -> String {
    let mut buf = [0u8; 128];
    nvs()
        .get_str(KEY_PASS, &mut buf)
        .ok()
        .flatten()
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// Mot de passe WPA2 du point d'accès du POS, généré au premier boot et
/// persisté en NVS.
///
/// L'AP servait le portail en clair : le PIN admin saisi dessus était
/// sniffable par n'importe qui à portée. En WPA2, le trafic du portail est
/// chiffré par la liaison. Le mot de passe est affiché à l'écran au boot et
/// dans la page /wifi (le propriétaire le retrouve sans redémarrer).
pub fn ap_pass() -> String {
    let mut buf = [0u8; 32];
    if let Ok(Some(p)) = nvs().get_str(KEY_AP_PASS, &mut buf) {
        if p.len() >= 8 {
            return p.to_string();
        }
    }
    let pass: String = (0..AP_PASS_LEN)
        .map(|_| {
            let r = unsafe { esp_idf_sys::esp_random() } as usize % AP_ALPHABET.len();
            AP_ALPHABET[r] as char
        })
        .collect();
    let _ = nvs().set_str(KEY_AP_PASS, &pass);
    pass
}

/// Enregistre la config (SSID vide = effacer la config).
pub fn save(ssid: &str, pass: &str) -> Result<(), EspError> {
    if ssid.is_empty() {
        nvs().remove(KEY_SSID)?;
        nvs().remove(KEY_PASS)?;
    } else {
        nvs().set_str(KEY_SSID, ssid)?;
        if pass.is_empty() {
            nvs().remove(KEY_PASS)?;
        } else {
            nvs().set_str(KEY_PASS, pass)?;
        }
    }
    Ok(())
}

/// Stocke le résultat du scan (appelé au boot).
pub fn store_scan(list: Vec<String>) {
    if let Some(s) = SCAN.get() {
        *s.lock().unwrap() = list;
    }
}

/// Liste des SSID détectés au boot (pour le portail /wifi).
pub fn scan_list() -> Vec<String> {
    SCAN.get()
        .map(|s| s.lock().unwrap().clone())
        .unwrap_or_default()
}

/// Suffixe AP : 4 derniers hex du MAC STA (ex. « A1B2 »).
pub fn ap_suffix() -> String {
    let mut mac = [0u8; 6];
    unsafe {
        esp_idf_sys::esp_read_mac(
            mac.as_mut_ptr(),
            esp_idf_sys::esp_mac_type_t_ESP_MAC_WIFI_STA,
        );
    }
    format!("{:02X}{:02X}", mac[4], mac[5])
}
