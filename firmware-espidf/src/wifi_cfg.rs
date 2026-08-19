//! Provisioning WiFi : config (SSID + mot de passe) stockée en NVS (namespace
//! `wifi`), résultat du scan des réseaux disponible pour le portail /wifi.
//! Plus aucun identifiant WiFi dans le binaire.

use std::sync::{Mutex, OnceLock};

use esp_idf_svc::nvs::{EspDefaultNvs, EspDefaultNvsPartition, EspNvs};
use esp_idf_svc::sys::EspError;

const KEY_SSID: &str = "ssid";
const KEY_PASS: &str = "pass";

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
    SCAN.get().map(|s| s.lock().unwrap().clone()).unwrap_or_default()
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
