//! LightningPoS — JC3248W535C (ESP32-S3) sur esp-idf-hal (ESP-IDF natif).
//! Test : WiFi + LNbits (création facture + poll) + affichage du statut.

mod admin;
mod bech32;
mod bolt;
mod boltcard;
mod config;
mod display;
mod font;
mod glyph;
mod journal;
mod qr;
mod rc522;
mod render3d;
mod sprites;
mod store;
mod theme;
mod touch;
mod ui;
mod web;
mod wifi_cfg;

use core::convert::TryInto;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use boltcard::LnurlWithdraw;
use display::Display;
use esp_idf_hal::adc::attenuation::DB_12;
use esp_idf_hal::adc::oneshot::config::AdcChannelConfig;
use esp_idf_hal::adc::oneshot::{AdcChannelDriver, AdcDriver};
use esp_idf_hal::gpio::PinDriver;
use esp_idf_hal::i2c::{I2cConfig, I2cDriver};
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::spi::{SpiConfig, SpiDeviceDriver, SpiDriver, SpiDriverConfig};
use esp_idf_hal::units::Hertz;
use rc522::Rc522;
use theme::*;
use touch::Touch;
use ui::{CALC_X_MAX, CALC_Y, CELL_H, CELL_W, CLEAR_X_MIN, GAP, GRID_Y, HEAD_H};

use embedded_svc::{
    http::{client::Client as HttpClient, Method},
    io::Write,
    utils::io,
    wifi::{AccessPointConfiguration, AuthMethod, ClientConfiguration, Configuration},
};
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    http::client::EspHttpConnection,
    log::EspLogger,
    nvs::{EspDefaultNvs, EspDefaultNvsPartition},
    sntp::EspSntp,
    sys,
    wifi::{BlockingWifi, EspWifi},
};

// Alias temporaires pour les écrans d'erreur qui n'ont pas encore été migrés.
const BLACK: u16 = BG;
const RED: u16 = ROSE;
const ORANGE: u16 = AMBER;

/// Requête HTTP minimale → (status, body)
fn http_req(
    method: Method,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> Result<(u16, String), String> {
    let mut conf = esp_idf_svc::http::client::Configuration::default();
    conf.crt_bundle_attach = Some(esp_idf_sys::esp_crt_bundle_attach);
    let mut client =
        HttpClient::wrap(EspHttpConnection::new(&conf).map_err(|e| format!("conn: {e}"))?);
    let mut request = client
        .request(method, url, headers)
        .map_err(|e| format!("req: {e}"))?;
    if let Some(b) = body {
        request
            .write_all(b.as_bytes())
            .map_err(|e| format!("write: {e}"))?;
    }
    request.flush().map_err(|e| format!("flush: {e}"))?;
    let mut response = request.submit().map_err(|e| format!("submit: {e}"))?;
    let status = response.status();
    let mut buf = [0u8; 2048];
    // Accumulation en octets : un caractère multi-octets à cheval sur deux
    // lectures ne doit pas faire perdre un chunk entier, et une erreur de flux
    // ne doit pas passer pour une fin de corps (JSON tronqué signalé « invalide »).
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        match io::try_read_full(&mut response, &mut buf) {
            Ok(0) => break,
            Ok(n) => {
                bytes.extend_from_slice(&buf[..n]);
                if n < buf.len() {
                    break;
                }
            }
            Err(e) => return Err(format!("read: {}", e.0)),
        }
    }
    let resp = String::from_utf8(bytes).map_err(|e| format!("réponse non utf8: {e}"))?;
    Ok((status, resp))
}

/// Durée de vie d'une facture, en secondes. Le POS abandonne le poll au bout du
/// même délai : passée cette limite, la facture est morte côté LNbits — une
/// vente annulée (bouton ANNULER) ne peut plus être payée « dans le dos » du POS,
/// qui ne l'enregistrerait pas.
const INVOICE_TTL_SECS: u64 = 90;

/// Crée une facture LNbits (montant en sats) → (payment_hash, bolt11).
/// L'instance LNbits est configurable via le portail web (/config).
fn create_invoice(amount_sats: u64, memo: &str) -> Result<(String, String), String> {
    if config::load_key(web::cfg_nvs()).is_empty() {
        return Err("clé LNbits non configurée (portail /config)".to_string());
    }
    let url = format!("{}/api/v1/payments", config::load_url(web::cfg_nvs()));
    let key = config::load_key(web::cfg_nvs());
    let headers = [
        ("X-Api-Key", key.as_str()),
        ("Content-Type", "application/json"),
    ];
    // JSON construit par serde_json : le memo est échappé quoi qu'il contienne
    // (une construction à la main casserait le corps sur un guillemet).
    let body = serde_json::json!({
        "out": false,
        "amount": amount_sats,
        "memo": memo,
        "expiry": INVOICE_TTL_SECS,
    })
    .to_string();
    let (status, resp) = http_req(Method::Post, &url, &headers, Some(&body))?;
    if status != 201 && status != 200 {
        return Err(format!("invoice HTTP {status}: {resp}"));
    }
    let v: serde_json::Value =
        serde_json::from_str(&resp).map_err(|e| format!("invoice json: {e}"))?;
    let hash = v["payment_hash"]
        .as_str()
        .ok_or("pas de payment_hash")?
        .to_string();
    let bolt11 = v["bolt11"].as_str().unwrap_or("").to_string();
    Ok((hash, bolt11))
}

/// Poll le statut d'un paiement → bool (paid)
fn poll_paid(hash: &str) -> Result<bool, String> {
    if config::load_key(web::cfg_nvs()).is_empty() {
        return Err("clé LNbits non configurée (portail /config)".to_string());
    }
    let url = format!(
        "{}/api/v1/payments/{}",
        config::load_url(web::cfg_nvs()),
        hash
    );
    let key = config::load_key(web::cfg_nvs());
    let headers = [("X-Api-Key", key.as_str())];
    let (status, resp) = http_req(Method::Get, &url, &headers, None)?;
    if status != 200 {
        return Err(format!("status HTTP {status}: {resp}"));
    }
    let v: serde_json::Value =
        serde_json::from_str(&resp).map_err(|e| format!("status json: {e}"))?;
    Ok(v["paid"].as_bool().unwrap_or(false))
}

/// Normalise une URI LNURL-withdraw lue sur une carte → URL absolue à requêter.
///
/// Seules les **formes LNURL** sont acceptées : `lnurlw://` (LUD-17), le bech32
/// `lnurl1…` (LUD-01), éventuellement préfixés de `lightning:` / `lightning://`.
/// Une URL http(s) brute est refusée : n'importe quel tag NFC posé sur le
/// lecteur déclenchait sinon un GET vers l'adresse de son choix, y compris un
/// hôte du réseau local du commerçant (SSRF). `lnurlc://` (channel) et
/// `lnurlp://` (pay) sont également refusés — le POS n'encaisse qu'en withdraw.
///
/// L'URL obtenue est revalidée (schéma, hôte, userinfo, longueur) avant tout
/// appel réseau.
fn normalize_lnurl(uri: &str) -> Result<String, String> {
    let mut u = uri.trim();

    // "lightning://" puis "lightning:" (insensible à la casse).
    let lower = u.to_ascii_lowercase();
    for p in ["lightning://", "lightning:"] {
        if lower.starts_with(p) {
            u = &u[p.len()..];
            break;
        }
    }

    let lower = u.to_ascii_lowercase();
    // `to_ascii_lowercase` conserve la longueur en octets : les index calculés
    // sur `lower` sont valides sur `u`.
    let url = if lower.starts_with("lnurlw://") {
        // LUD-17 : lnurlw:// → https://
        format!("https://{}", &u["lnurlw://".len()..])
    } else if lower.starts_with("lnurl1") {
        // bech32 `lnurl1…` (LUD-01) → URL absolue.
        bech32::decode_lnurl(u)?
    } else {
        return Err(format!(
            "lnurl: forme non reconnue ({})",
            u.chars().take(40).collect::<String>()
        ));
    };

    config::validate_url(&url, config::MAX_LNURL_LEN)
        .map_err(|e| format!("lnurl: URL refusée ({e})"))?;
    Ok(url)
}

/// Résout l'URI NDEF de la carte → LnurlWithdraw (normalise + GET + parse).
fn resolve_lnurl(uri: &str) -> Result<LnurlWithdraw, String> {
    let url = normalize_lnurl(uri)?;
    let (status, resp) = http_req(Method::Get, &url, &[], None)?;
    if status != 200 {
        return Err(format!("lnurl HTTP {status}: {resp}"));
    }
    let w = LnurlWithdraw::parse(&resp)?;
    // Le `callback` vient de la réponse d'un serveur désigné par la carte : il
    // est aussi peu digne de confiance que l'URI de la carte elle-même.
    config::validate_url(&w.callback, config::MAX_LNURL_LEN)
        .map_err(|e| format!("lnurl: callback refusé ({e})"))?;
    Ok(w)
}

/// Parse la réponse JSON d'un callback LNURL-withdraw → Ok(()) si `status == "OK"`.
/// LUD-03 impose `{"status":"OK"}` ou `{"status":"ERROR","reason":…}` : un corps
/// vide ou sans champ `status` (502 d'un reverse-proxy, par ex.) n'est pas un succès.
fn parse_callback(status: u16, resp: &str) -> Result<(), String> {
    let v: serde_json::Value = serde_json::from_str(resp.trim()).map_err(|e| {
        format!(
            "callback HTTP {status} json invalide: {e} ({})",
            resp.chars().take(80).collect::<String>()
        )
    })?;
    match v["status"].as_str() {
        Some(s) if s.eq_ignore_ascii_case("OK") => Ok(()),
        Some(s) => Err(format!(
            "callback status={s} reason={}",
            v["reason"].as_str().unwrap_or("")
        )),
        None => Err(format!("callback sans champ status (HTTP {status})")),
    }
}

/// RSSI WiFi courant (dBm) — pour l'icône de l'en-tête. -100 si indisponible.
fn wifi_rssi() -> i8 {
    unsafe {
        let mut ap: esp_idf_sys::wifi_ap_record_t = core::mem::zeroed();
        if esp_idf_sys::esp_wifi_sta_get_ap_info(&mut ap) == esp_idf_sys::ESP_OK {
            ap.rssi
        } else {
            -100
        }
    }
}

/// Récupère le prix BTC dans la devise configurée (CoinGecko, sans clé API).
fn fetch_btc_price(cur: &str) -> Result<f64, String> {
    let vs = match cur {
        "USD" => "usd",
        "GBP" => "gbp",
        _ => "eur",
    };
    let url =
        format!("https://api.coingecko.com/api/v3/simple/price?ids=bitcoin&vs_currencies={vs}");
    let (status, resp) = http_req(Method::Get, &url, &[], None)?;
    if status != 200 {
        return Err(format!("coingecko HTTP {status}"));
    }
    let v: serde_json::Value =
        serde_json::from_str(&resp).map_err(|e| format!("coingecko json: {e}"))?;
    v["bitcoin"][vs]
        .as_f64()
        .ok_or_else(|| "pas de prix".to_string())
}

/// Cache du prix BTC : (prix en centimes, instant du dernier fetch réussi).
static PRICE: Mutex<Option<(u64, Instant)>> = Mutex::new(None);

/// Durée de validité du prix en cache. Au-delà, il est rafraîchi au paiement
/// suivant : un POS allumé plusieurs jours ne facture plus au cours du boot.
const PRICE_TTL: Duration = Duration::from_secs(600);

/// Prix BTC en centimes, frais à ≤ `PRICE_TTL`. Sur échec réseau, le dernier
/// prix connu est conservé (facturer au cours d'il y a 10 min vaut mieux que
/// refuser la vente) ; 0 uniquement si aucun prix n'a jamais été obtenu.
fn btc_price_cents_fresh(cur: &str) -> u64 {
    // Copie puis relâche le verrou : le fetch HTTP dure des secondes, le garder
    // bloquerait tout autre appelant (et l'appelant courant se re-verrouille plus bas).
    let cached = *PRICE.lock().unwrap();
    if let Some((cents, at)) = cached {
        if at.elapsed() < PRICE_TTL {
            return cents;
        }
    }
    let last = cached.map(|(c, _)| c).unwrap_or(0);
    match fetch_btc_price(cur) {
        Ok(p) => {
            let cents = (p * 100.0).round().max(0.0) as u64;
            if cents == 0 {
                println!("[BTC] prix nul reçu → dernier prix connu ({last})");
                return last;
            }
            println!("[BTC] prix {cur} = {p}");
            *PRICE.lock().unwrap() = Some((cents, Instant::now()));
            cents
        }
        Err(e) => {
            println!("[BTC] rafraîchissement échoué: {e} → dernier prix connu ({last})");
            last
        }
    }
}

/// Pourcentage de charge estimé à partir de la tension batterie (mV).
/// 3.0 V = vide, 4.2 V = plein (LiPo 1S).
fn battery_pct(mv: u32) -> u8 {
    if mv <= 3000 {
        0
    } else if mv >= 4200 {
        100
    } else {
        ((mv - 3000) * 100 / 1200) as u8
    }
}

/// Appui sur le bouton ANNULER, dans la zone exacte où il est dessiné
/// (70..250 × 420..464). Le contrôle X est OBLIGATOIRE : le bouton PAYER
/// (x 216-312, y 340-463) déborde dans la bande Y du bouton ANNULER — sans
/// la borne X, un tap sur PAYER dans le bas du bouton annulait la vente.
fn cancel_pressed(touch: &mut Touch) -> bool {
    matches!(touch.read(), Some(p) if (70..250).contains(&p.x) && (420..464).contains(&p.y))
}

/// Attend jusqu'à `ms` ms une 2e frappe (détection de double tap). Retourne
/// true si un appui arrive ; son relâchement sera drainé au tour suivant.
fn poll_second_tap(touch: &mut Touch, ms: u64) -> bool {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        if touch.read().is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// Boucle d'animation éclair jusqu'à un toucher ou `max_ms` écoulés.
/// Retourne true si un toucher est survenu (le relâchement est drainé).
fn attract_until_touch(display: &mut Display, touch: &mut Touch, max_ms: u64) -> bool {
    let start = Instant::now();
    let mut fi = 0u32;
    loop {
        display.clear(BG);
        sprites::draw_frame(display.fb_mut(), SCREEN_W, SCREEN_H, 160, 205, fi as usize);
        display.draw_text_tracked("LightningPoS", 118, 330, 1, 2, TXT_DIM);
        display.flush();
        let t0 = Instant::now();
        // Anti-fantôme : un toucher n'est confirmé qu'après 2 lectures
        // consécutives (le contrôleur tactile renvoie parfois un contact
        // parasite d'un échantillon — bruit du QSPI, résidu de relâchement).
        let mut confirmed = 0u8;
        while t0.elapsed() < Duration::from_millis(42) {
            if touch.read().is_some() {
                confirmed += 1;
                if confirmed >= 2 {
                    while touch.read().is_some() {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    return true;
                }
                std::thread::sleep(Duration::from_millis(8));
            } else {
                confirmed = 0;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        fi += 1;
        if max_ms > 0 && start.elapsed() >= Duration::from_millis(max_ms) {
            return false;
        }
    }
}

/// Flux de paiement : €→sats → facture LNbits → QR + NFC (Bolt Card) → poll → PAYÉ / ANNULÉ.
fn pay(
    display: &mut Display,
    touch: &mut Touch,
    rc522: &mut Rc522,
    total_cents: u64,
    label: &str,
    journal_nvs: &EspDefaultNvs,
) -> bool {
    if total_cents == 0 {
        return false;
    }
    // Le tap sur PAYER est encore actif à l'entrée : attendre le relâchement,
    // sinon le MÊME appui (encore en contact) est lu comme ANNULER dès la
    // première itération — le bas du bouton PAYER déborde dans la bande Y du
    // bouton ANNULER (bug signalé : « quand je clique sur PAYER ça annule »).
    for _ in 0..100 {
        if touch.read().is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let cur = config::load_currency(web::cfg_nvs());
    // Cours rafraîchi à chaque vente (cache 10 min) : le prix du boot n'est
    // plus figé pour la durée de vie de l'appareil.
    let btc_price_cents = btc_price_cents_fresh(&cur);
    let sats = boltcard::eur_cents_to_sats(total_cents, btc_price_cents);
    if sats == 0 {
        display.clear(BLACK);
        display.draw_text_scaled("PRIX BTC ?", (320 - 9 * 6 * 3) / 2, 210, 3, ORANGE);
        display.flush();
        std::thread::sleep(Duration::from_millis(1500));
        return false;
    }
    println!("[POS] facture {} sats", sats);
    // Écran FACTURE : les deux unités (€ + sats) toujours visibles.
    ui::preparing(display, total_cents, sats, &cur);
    display.flush();

    match create_invoice(sats, "LIGHTNING POS") {
        Ok((hash, bolt11)) => {
            println!("[POS] hash={} bolt11 {} chars", hash, bolt11.len());
            let (n, bits) = match qr::encode(&bolt11) {
                Some(x) => x,
                None => {
                    // QR non générable : sortie propre au lieu d'un écran figé sur FACTURE.
                    println!("[POS] qr::encode échec (bolt11 {} chars)", bolt11.len());
                    display.clear(BLACK);
                    display.draw_text_scaled("QR ERREUR", (320 - 9 * 6 * 3) / 2, 200, 3, RED);
                    display.flush();
                    std::thread::sleep(Duration::from_millis(1500));
                    return false;
                }
            };
            // Écran QR : les deux unités (€ + sats) toujours visibles.
            ui::pay_screen(display, n, &bits, total_cents, sats, &cur);
            display.flush();
            let mut paid = false;
            let mut cancelled = false;
            // Verrou du retrait : posé *avant* l'envoi du callback, qui est
            // irréversible (rejouer = double débit). Un échec survenu *avant* le
            // callback n'arme que l'anti-rebond par URI : une autre carte reste
            // lisible, seule celle qui a échoué n'est pas réessayée en boucle.
            let mut callback_sent = false;
            // Statut NFC transitoire : timestamp du dernier message affiché,
            // pour restaurer l'invite « TAPEZ LA CARTE » après ~1,6 s.
            let mut status_at: Option<Instant> = None;
            // Animation « PAIEMENT EN COURS » pendant le règlement Lightning.
            let mut pay_step = 0u32;
            // Anti-rebond par CARTE (chemin de l'URI, sans la query) : l'URI
            // complète change à chaque lecture (padding aléatoire NXP §9.3.4 +
            // compteur), la comparer entière n'aurait jamais matché → la boucle
            // re-tapait le serveur à chaque itération, brûlant des compteurs SDM
            // (« le compteur prend presque 9 ») et créant des hits orphelins.
            let mut last_failed_card: Option<String> = None;
            // Borne de temps réelle : une itération dure de 1,3 à 3 s (NFC + HTTP),
            // un simple compteur de tours donnait un timeout de 80 à 180 s.
            let deadline = Instant::now() + Duration::from_secs(INVOICE_TTL_SECS);
            while Instant::now() < deadline {
                // Compte à rebours de la facture (secondes restantes).
                let remain = deadline.saturating_duration_since(Instant::now()).as_secs();
                ui::pay_countdown(display, remain);
                display.flush();
                if cancel_pressed(touch) {
                    cancelled = true;
                    break;
                }
                // Carte NFC (Bolt Card Type 4 ou badge NTAG Type 2) : LNURL-withdraw.
                if !callback_sent {
                    match rc522.read_ndef_uri() {
                        Ok(Some(uri))
                            if last_failed_card.as_deref()
                                == Some(uri.split('?').next().unwrap_or(&uri)) =>
                        {
                            // Carte déjà refusée : inutile de la rejouer à chaque tour.
                        }
                        Ok(Some(uri)) => {
                            println!("[POS] carte: {uri}");
                            status_at = Some(ui::nfc_status(display, "CARTE LUE", MINT));
                            match resolve_lnurl(&uri) {
                                // Facture hors de la plage LNURL de la carte.
                                Ok(w) if !w.amount_in_range_msat(boltcard::sats_to_msat(sats)) => {
                                    println!("[POS] montant hors plage carte");
                                    status_at = Some(ui::nfc_status(display, "REFUSE", ROSE));
                                    last_failed_card =
                                        Some(uri.split('?').next().unwrap_or(&uri).to_string());
                                }
                                Ok(w) => {
                                    let cb = w.build_callback_url(&bolt11);
                                    callback_sent = true;
                                    match http_req(Method::Get, &cb, &[], None) {
                                        Ok((s, body)) => match parse_callback(s, &body) {
                                            Ok(()) => {
                                                println!("[POS] callback OK (HTTP {s})");
                                                status_at =
                                                    Some(ui::nfc_status(display, "CARTE OK", MINT));
                                            }
                                            Err(e) => {
                                                println!("[POS] callback refusé: {e}");
                                                status_at =
                                                    Some(ui::nfc_status(display, "REFUSE", ROSE));
                                            }
                                        },
                                        // État inconnu côté service : on ne rejoue pas.
                                        Err(e) => {
                                            println!("[POS] callback err: {e}");
                                            status_at =
                                                Some(ui::nfc_status(display, "ATTENDEZ", GOLD));
                                        }
                                    }
                                }
                                Err(e) => {
                                    println!("[POS] lnurl err: {e}");
                                    status_at = Some(ui::nfc_status(display, "RESSAYEZ", GOLD));
                                    last_failed_card =
                                        Some(uri.split('?').next().unwrap_or(&uri).to_string());
                                }
                            }
                        }
                        Ok(None) => {}
                        Err(e) => println!("[POS] nfc err: {e}"),
                    }
                    // Restaure l'invite « TAPEZ LA CARTE » ~1,6 s après le
                    // dernier message transitoire (REFUSE / RESSAYEZ / …).
                    if let Some(t) = status_at {
                        if t.elapsed() > Duration::from_millis(1600) {
                            ui::nfc_hint(display);
                            status_at = None;
                        }
                    }
                }
                // Carte acceptée (callback OK) : l'animation « PAIEMENT EN
                // COURS » remplace l'invite pendant le règlement Lightning.
                if callback_sent {
                    ui::pay_loading(display, pay_step);
                    pay_step += 1;
                }
                // Attente ~300 ms, tactile sondé toutes les 50 ms : un appui sur
                // ANNULER pendant l'attente n'est plus perdu.
                let wait_end = Instant::now() + Duration::from_millis(300);
                while Instant::now() < wait_end {
                    if cancel_pressed(touch) {
                        cancelled = true;
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                if cancelled {
                    break;
                }
                match poll_paid(&hash) {
                    Ok(true) => {
                        paid = true;
                        break;
                    }
                    Ok(false) => {}
                    Err(e) => println!("[POS] poll err: {e}"),
                }
            }
            if cancelled {
                println!("[POS] paiement annulé");
            }
            display.clear(BLACK);
            if paid {
                // Journal : persiste la transaction (date, montant, sats, hash).
                let (ts, epoch) = journal::now_ts();
                if let Err(e) = journal::record(
                    journal_nvs,
                    &journal::Tx {
                        seq: 0,
                        ts,
                        epoch,
                        cents: total_cents,
                        sats,
                        cur: cur.clone(),
                        label: label.to_string(),
                        hash: hash.clone(),
                    },
                ) {
                    println!("[POS] journal err: {e}");
                } else {
                    println!("[POS] transaction enregistrée ({total_cents} cents / {sats} sats)");
                }
                // Séquence PAYÉ : flash blanc 50 ms → éclair ≥ 380 px → respiration.
                ui::paid(display, total_cents, &cur);
            } else if cancelled {
                ui::status(display, "X", "ANNULE", "PAIEMENT ANNULE", ROSE);
                display.flush();
                std::thread::sleep(Duration::from_millis(900));
            } else {
                ui::status(display, "!", "TIMEOUT", "RECOMMENCEZ S'IL VOUS PLAIT", ROSE);
                display.flush();
                std::thread::sleep(Duration::from_millis(900));
            }
            paid
        }
        Err(e) => {
            println!("[POS] facture err: {e}");
            display.clear(BLACK);
            display.draw_text_scaled("ERREUR", (320 - 6 * 6 * 3) / 2, 200, 3, RED);
            display.flush();
            std::thread::sleep(Duration::from_millis(1500));
            false
        }
    }
}

/// Formate la valeur affichée par la calculatrice (garde la virgule saisie).
fn fmt_calc(v: f64, has_dot: bool, frac: u32) -> String {
    if has_dot {
        if frac == 0 {
            format!("{}.", v as i64)
        } else {
            format!("{:.*}", frac as usize, v)
        }
    } else {
        format!("{}", v as i64)
    }
}

/// Applique un opérateur binaire.
fn apply(a: f64, o: char, b: f64) -> f64 {
    match o {
        '+' => a + b,
        '-' => a - b,
        'x' => a * b,
        '/' => {
            if b != 0.0 {
                a / b
            } else {
                a
            }
        }
        _ => b,
    }
}

fn main() {
    sys::link_patches();
    EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();

    // --- Écran ---
    let mut backlight = PinDriver::output(peripherals.pins.gpio1).unwrap();
    backlight.set_high().unwrap();
    let cs = PinDriver::output(peripherals.pins.gpio45).unwrap();
    let mut display = Display::new(cs);
    display.init();
    display.clear(BLACK);
    display.flush();
    println!("[DISPLAY] init OK");

    // --- Écran de démarrage cosmique ---
    ui::splash(&mut display);

    // --- WiFi ---
    let sys_loop = EspSystemEventLoop::take().unwrap();
    let nvs = EspDefaultNvsPartition::take().unwrap();
    let nvs_products = nvs.clone(); // clone pour le namespace « products » (web + menu)
    let nvs_journal = nvs.clone(); // clone pour le namespace « journal » (transactions)
    let nvs_cfg = nvs.clone(); // clone pour le namespace « poscfg » (URL + clé LNbits)
    let nvs_wifi = nvs.clone(); // clone pour le namespace « wifi » (SSID + mot de passe)

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs)).unwrap(),
        sys_loop,
    )
    .unwrap();
    wifi_cfg::start(nvs_wifi);

    // Provisioning WiFi : mode Mixte (AP « LightningPoS-XXXX » toujours actif
    // pour le portail + STA vers le réseau configuré via /wifi).
    let cfg_ssid = wifi_cfg::ssid();
    let cfg_pass = wifi_cfg::pass();
    let ap_ssid = format!("LightningPoS-{}", wifi_cfg::ap_suffix());
    let ap_pass = wifi_cfg::ap_pass();
    let mut sta_cfg = ClientConfiguration {
        ssid: cfg_ssid.as_str().try_into().unwrap_or_default(),
        password: cfg_pass.as_str().try_into().unwrap_or_default(),
        auth_method: if cfg_pass.is_empty() {
            AuthMethod::None
        } else {
            AuthMethod::WPA2Personal
        },
        ..Default::default()
    };
    if cfg_ssid.is_empty() {
        sta_cfg.ssid = Default::default();
        sta_cfg.password = Default::default();
    }
    let wifi_configuration: Configuration = Configuration::Mixed(
        sta_cfg,
        AccessPointConfiguration {
            ssid: ap_ssid.as_str().try_into().unwrap_or_default(),
            // WPA2 : le portail (et donc le PIN admin qui y transite) n'est
            // plus lisible en clair par un tiers à portée radio.
            password: ap_pass.as_str().try_into().unwrap_or_default(),
            auth_method: AuthMethod::WPA2Personal,
            ..Default::default()
        },
    );
    wifi.set_configuration(&wifi_configuration).unwrap();
    wifi.start().unwrap();

    // Scan des réseaux (l'interface STA est active même sans connexion).
    if let Ok(aps) = wifi.scan() {
        let list: Vec<String> = aps
            .iter()
            .map(|a| a.ssid.to_string())
            .filter(|s| !s.is_empty())
            .collect();
        println!("[WIFI] scan: {} réseaux", list.len());
        wifi_cfg::store_scan(list);
    }

    // Horloge murale : SNTP démarré une fois la STA connectée. Sans lui, le
    // journal n'aurait que l'uptime (remis à zéro à chaque reboot). Le handle
    // doit vivre aussi longtemps que le programme — d'où le `_sntp` dans main.
    let mut _sntp = None;
    if cfg_ssid.is_empty() {
        println!("[WIFI] non configuré → AP « {ap_ssid} » + portail http://192.168.4.1");
    } else {
        match wifi.connect() {
            Ok(_) => {
                wifi.wait_netif_up().unwrap();
                println!("[WIFI] connecté à {cfg_ssid}");
                match EspSntp::new_default() {
                    // La synchro est asynchrone : les toutes premières ventes
                    // après le boot peuvent encore être horodatées en uptime.
                    Ok(s) => {
                        println!("[SNTP] démarré (pool.ntp.org, UTC)");
                        _sntp = Some(s);
                    }
                    Err(e) => println!("[SNTP] démarrage échoué: {e}"),
                }
            }
            Err(e) => println!("[WIFI] échec connexion à {cfg_ssid}: {e} (AP toujours actif)"),
        }
    }

    // --- Portail web de config des prix + instance LNbits + prix BTC ---
    let _server = web::start(nvs_products, nvs_journal, nvs_cfg).unwrap();
    println!("[WEB] portail config prêt");

    // Amorce le cache de prix (il sera rafraîchi à chaque vente, cf. PRICE_TTL).
    // Retry : CoinGecko rate-limite après plusieurs boots → on retente 3×.
    let cur = config::load_currency(web::cfg_nvs());
    for attempt in 0..3u32 {
        if btc_price_cents_fresh(&cur) > 0 {
            break;
        }
        println!("[BTC] tentative {}/3 prix indisponible", attempt + 1);
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    // --- RC522 (SPI3 : SCK=6, MOSI=15, MISO=7, CS=17, RST=14) ---
    // RST déplacé sur GPIO14 : GPIO5 est le diviseur de tension batterie (ADC1_CH4).
    let spi = SpiDriver::new(
        peripherals.spi3,
        peripherals.pins.gpio6,       // SCK
        peripherals.pins.gpio15,      // MOSI (SDO)
        Some(peripherals.pins.gpio7), // MISO (SDI)
        &SpiDriverConfig::new(),
    )
    .unwrap();
    let rc522_rst = PinDriver::output(peripherals.pins.gpio14).unwrap();
    let spi_dev = SpiDeviceDriver::new(
        spi,
        Some(peripherals.pins.gpio17), // CS (broche brute, gérée par le driver)
        &SpiConfig::new().baudrate(Hertz(4_000_000)),
    )
    .unwrap();
    let mut rc522 = Rc522::new(spi_dev, rc522_rst);
    rc522
        .init()
        .unwrap_or_else(|e| println!("[RC522] init err: {e}"));
    match rc522.get_version() {
        Ok(v) => println!("[RC522] version 0x{v:02X}"),
        Err(e) => println!("[RC522] version err: {e}"),
    }

    // RC522 prêt — la lecture Bolt Card est faite dans le flux de paiement (pay()).

    // --- Batterie : ADC1_CH4 (GPIO5), diviseur 68k/100k → V_bat = V_adc × 168/100 ---
    let adc = AdcDriver::new(peripherals.adc1).unwrap();
    let mut batt_cfg = AdcChannelConfig::new();
    batt_cfg.attenuation = DB_12;
    let mut batt_ch = AdcChannelDriver::new(&adc, peripherals.pins.gpio5, &batt_cfg).unwrap();

    // --- Tactile AXS15231B (I2C1 SDA=4, SCL=8) ---
    let i2c1_config = I2cConfig::new().baudrate(Hertz(100_000));
    let i2c1 = I2cDriver::new(
        peripherals.i2c1,
        peripherals.pins.gpio4, // SDA
        peripherals.pins.gpio8, // SCL
        &i2c1_config,
    )
    .unwrap();
    let mut touch = Touch::new(i2c1);

    // --- Système de vente : menu produits (NVS) + clavier + paiement ---
    let products = store::load(web::products_nvs());
    let mut page: usize = 0;
    const CALC_KEYS: [[&str; 4]; 5] = [
        ["C", "%", "/", "x"],
        ["7", "8", "9", "-"],
        ["4", "5", "6", "+"],
        ["1", "2", "3", "="],
        ["0", "00", ".", "OK"],
    ];
    let mut total_cents: u64 = 0;
    let mut items: u32 = 0;
    // Libellé du panier pour le journal : le premier article ajouté (le nombre
    // total d'articles est préfixé au moment de l'encaissement).
    let mut cart_label = String::new();
    let mut calc_mode = false;
    // État calculatrice
    let mut acc: f64 = 0.0;
    let mut cur: f64 = 0.0;
    let mut op: Option<char> = None;
    let mut has_dot = false;
    let mut frac: u32 = 0;
    let mut just_eq = false;
    println!("[POS] menu prêt");

    // Accès admin : PIN du portail + mot de passe WPA2 de l'AP, affichés 6 s au
    // boot (aussi en serial). Tout est centré : à l'échelle 3, « PIN ADMIN:
    // 123456 » mesurait 306 px et débordait des 320 px de l'écran — les
    // derniers chiffres du PIN étaient coupés.
    let pin = admin::ensure_pin(web::cfg_nvs());
    println!("[ADMIN] pin={pin}  ap={ap_ssid} mdp={ap_pass}");
    display.clear(BG);
    display.text_center("ACCES ADMIN", 140, 2, 2, TXT_MUTED);
    display.text_center(&format!("PIN {pin}"), 178, 3, 2, AMBER);
    display.text_center("WIFI DU POS", 250, 1, 2, TXT_MUTED);
    display.text_center(&ap_ssid, 274, 1, 1, TXT_DIM);
    display.text_center(&ap_pass, 300, 2, 2, MINT);
    display.flush();
    std::thread::sleep(Duration::from_millis(6000));

    /// Dessine l'écran courant (menu ou calculatrice) avec le thème cosmique.
    fn draw_ui(
        d: &mut Display,
        calc_mode: bool,
        products: &[store::Product],
        page: usize,
        total_cents: u64,
        items: u32,
        batt_pct: u8,
        rssi: i8,
        cur_code: &str,
        cur: f64,
        has_dot: bool,
        frac: u32,
        op: Option<char>,
        acc: f64,
    ) {
        if calc_mode {
            let s = fmt_calc(cur, has_dot, frac);
            let pending = op.map(|o| (acc, o));
            ui::calc(d, &CALC_KEYS, &s, pending);
        } else {
            ui::menu(
                d,
                products,
                page,
                total_cents,
                items,
                batt_pct,
                rssi,
                cur_code,
            );
        }
    }

    // Écran ON/OFF par double tap : quand l'écran est éteint, le backlight
    // (GPIO1) est coupé et on n'affiche rien — seul un double tap le rallume.
    let mut screen_on = true;
    // Valeurs d'en-tête rafraîchies à chaque tour quand l'écran est allumé
    // (les fermetures de crossfade les lisent aussi).
    let mut batt_pct = 100u8;
    let mut rssi = -100i8;
    let mut cur_code = String::from("EUR");
    // Attract mode : dernière activité tactile (réinitialisée à chaque appui).
    let mut last_touch = Instant::now();

    loop {
        // --- Dessine l'UI selon le mode ---
        if screen_on {
            batt_pct = battery_pct(batt_ch.read().unwrap_or(0) as u32 * 168 / 100);
            rssi = wifi_rssi();
            cur_code = config::load_currency(web::cfg_nvs());
            draw_ui(
                &mut display,
                calc_mode,
                &products,
                page,
                total_cents,
                items,
                batt_pct,
                rssi,
                &cur_code,
                cur,
                has_dot,
                frac,
                op,
                acc,
            );
            display.flush();
        }

        // --- Attend le relâchement puis un appui ---
        while touch.read().is_some() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // --- Attract mode : éclair 3D en rotation après ~8 s d'inactivité ---
        // Uniquement sur le menu (pas en calculatrice) et écran allumé. Le
        // polling tactile reste actif (une frame toutes les ~42 ms ≈ 24 fps).
        if !calc_mode && screen_on && last_touch.elapsed() > Duration::from_secs(8) {
            attract_until_touch(&mut display, &mut touch, 0);
            last_touch = Instant::now();
            continue; // la boucle principale redessine le menu
        }

        let p = loop {
            if let Some(p) = touch.read() {
                break p;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        last_touch = Instant::now();
        let x = p.x as usize;
        let y = p.y as usize;
        let old_calc_mode = calc_mode;

        // --- Double tap : Switch On/Off de l'écran ---
        // Détecté UNIQUEMENT dans la zone neutre de l'en-tête (y<24, hors
        // bouton RETOUR de la calculatrice x<90). Les appuis produits restent
        // instantanés : plus aucun délai ni faux déclenchement en tapant deux
        // produits rapidement (le bug de l'écran qui s'éteignait tout seul).
        let in_header = y < 24 && (x >= 90 || !calc_mode);
        if screen_on && in_header {
            while touch.read().is_some() {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            if poll_second_tap(&mut touch, 280) {
                screen_on = false;
                // Drain : le doigt du 2e tap est encore posé — attendre le
                // relâchement avant l'animation, sinon le premier read() de
                // attract_until_touch réveille aussitôt (veille → menu).
                while touch.read().is_some() {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                // Stabilisation : le contrôleur tactile peut laisser traîner
                // des contacts résiduels ~150 ms après le relâchement.
                let settle = Instant::now() + Duration::from_millis(150);
                while Instant::now() < settle {
                    std::thread::sleep(Duration::from_millis(10));
                }
                // Veille avec logo : éclair 3D + LightningPoS pendant 60 s max.
                if attract_until_touch(&mut display, &mut touch, 60_000) {
                    screen_on = true;
                    backlight.set_high().unwrap();
                    // Sans ça, l'attract d'inactivité (8 s) repartirait aussitôt
                    // et le tap de réveil semblerait sans effet.
                    last_touch = Instant::now();
                } else {
                    backlight.set_low().unwrap();
                    display.clear(BG);
                    display.flush();
                }
            }
            continue;
        }
        if !screen_on {
            // Écran éteint : seul un double tap (n'importe où) réveille.
            while touch.read().is_some() {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            if poll_second_tap(&mut touch, 280) {
                screen_on = true;
                backlight.set_high().unwrap();
                // Évite que l'attract d'inactivité (8 s) reparte aussitôt après
                // le réveil : le menu doit s'afficher d'abord.
                last_touch = Instant::now();
            }
            continue;
        }

        // --- Gère l'appui ---
        if calc_mode {
            if y < 24 && x < 90 {
                // RETOUR
                calc_mode = false;
                acc = 0.0;
                cur = 0.0;
                op = None;
                has_dot = false;
                frac = 0;
                just_eq = false;
            } else if y >= CALC_Y {
                let kw = SCREEN_W / 4;
                let kh = (SCREEN_H - CALC_Y) / 5;
                let c = (x / kw).min(3);
                let r = ((y - CALC_Y) / kh).min(4);
                let label = CALC_KEYS[r][c];
                match label {
                    "C" => {
                        acc = 0.0;
                        cur = 0.0;
                        op = None;
                        has_dot = false;
                        frac = 0;
                        just_eq = false;
                    }
                    "." => has_dot = true,
                    "00" => cur *= 100.0,
                    "=" => {
                        if let Some(o) = op {
                            acc = apply(acc, o, cur);
                            op = None;
                        }
                        cur = acc;
                        has_dot = false;
                        frac = 0;
                        just_eq = true;
                    }
                    "%" => {
                        cur = acc * cur / 100.0;
                        has_dot = false;
                        frac = 0;
                    }
                    "OK" => {
                        let v = if let Some(o) = op {
                            apply(acc, o, cur)
                        } else {
                            cur
                        };
                        let cents = (v * 100.0).round() as i64;
                        if cents > 0 {
                            total_cents += cents as u64;
                            items += 1;
                            if cart_label.is_empty() {
                                cart_label = "MONTANT LIBRE".to_string();
                            }
                            println!("[POS] +calcul {} cents", cents);
                        }
                        calc_mode = false;
                        acc = 0.0;
                        cur = 0.0;
                        op = None;
                        has_dot = false;
                        frac = 0;
                        just_eq = false;
                    }
                    _ => {
                        if let Some(dig) = label.parse::<u32>().ok() {
                            if just_eq {
                                cur = 0.0;
                                has_dot = false;
                                frac = 0;
                                just_eq = false;
                            }
                            if has_dot {
                                cur += dig as f64 / 10f64.powi(frac as i32 + 1);
                                frac += 1;
                            } else {
                                cur = cur * 10.0 + dig as f64;
                            }
                        } else {
                            // Opérateur + - x /
                            let o = label.chars().next().unwrap();
                            if let Some(prev) = op {
                                acc = apply(acc, prev, cur);
                            } else {
                                acc = cur;
                            }
                            op = Some(o);
                            cur = 0.0;
                            has_dot = false;
                            frac = 0;
                            just_eq = false;
                        }
                    }
                }
            }
        } else {
            // Pastilles CALC / VIDER dans la bande sous l'en-tête.
            if y >= HEAD_H && y < GRID_Y {
                if x < CALC_X_MAX {
                    calc_mode = true;
                    acc = 0.0;
                    cur = 0.0;
                    op = None;
                    has_dot = false;
                    frac = 0;
                    just_eq = false;
                } else if x >= CLEAR_X_MIN {
                    total_cents = 0;
                    items = 0;
                    cart_label.clear();
                    println!("[POS] vente annulée");
                }
            } else if y >= GRID_Y {
                let c = (x.saturating_sub(GAP) / (CELL_W + GAP)).min(2);
                let r = ((y - GRID_Y) / (CELL_H + GAP)).min(2);
                let slot = r * 3 + c;
                let base = page * 8;
                let kind: u8 = if page == 0 {
                    if slot < 8 {
                        0
                    } else {
                        1
                    }
                } else if slot < 7 {
                    0
                } else if slot == 7 {
                    1
                } else {
                    2
                };
                match kind {
                    0 => {
                        let p = &products[base + slot];
                        total_cents += p.cents as u64;
                        items += 1;
                        if cart_label.is_empty() {
                            cart_label = p.name.clone();
                        }
                        println!("[POS] +{} ({} cents)", p.name, p.cents);
                    }
                    1 => {
                        page = if page == 0 { 1 } else { 0 };
                    }
                    _ => {
                        // Libellé du journal : « 3x BIERE » pour un panier
                        // multi-articles, le nom seul pour un article unique.
                        let label = if items > 1 {
                            format!("{items}x {cart_label}")
                        } else {
                            cart_label.clone()
                        };
                        if pay(
                            &mut display,
                            &mut touch,
                            &mut rc522,
                            total_cents,
                            &label,
                            web::journal_nvs(),
                        ) {
                            total_cents = 0;
                            items = 0;
                            cart_label.clear();
                            println!("[POS] vente encaissée, panier vidé");
                        }
                    }
                }
            }
        }

        // Transition subtile menu ↔ calculatrice.
        if calc_mode != old_calc_mode {
            ui::crossfade(
                &mut display,
                |d| {
                    draw_ui(
                        d,
                        old_calc_mode,
                        &products,
                        page,
                        total_cents,
                        items,
                        batt_pct,
                        rssi,
                        &cur_code,
                        cur,
                        has_dot,
                        frac,
                        op,
                        acc,
                    )
                },
                |d| {
                    draw_ui(
                        d,
                        calc_mode,
                        &products,
                        page,
                        total_cents,
                        items,
                        batt_pct,
                        rssi,
                        &cur_code,
                        cur,
                        has_dot,
                        frac,
                        op,
                        acc,
                    )
                },
                36,
            );
        }

        // Anti-rebond : relâchement + délai long
        while touch.read().is_some() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
}
