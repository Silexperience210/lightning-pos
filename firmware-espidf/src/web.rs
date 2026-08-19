//! Portail web de configuration des prix.
//!
//! Sert un formulaire HTML sur `/` (15 lignes nom + prix en euros) et traite
//! l'enregistrement sur `/save` (POST application/x-www-form-urlencoded).

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use embedded_svc::http::server::Connection as HttpConnection;
use embedded_svc::http::{headers::content_type, Headers, Method};
use embedded_svc::io::{ErrorType, Write};

use esp_idf_svc::http::server::{Configuration, EspHttpServer};
use esp_idf_svc::io::EspIOError;
use esp_idf_svc::nvs::{EspDefaultNvs, EspDefaultNvsPartition, EspNvs};
use esp_idf_svc::sys::EspError;

use crate::admin;
use crate::config;
use crate::journal;
use crate::store::{self, Product, NUM_PRODUCTS};
use crate::wifi_cfg;

/// Taille maximale acceptée pour le corps d'un POST (15 champs texte + 15 prix).
const MAX_BODY_LEN: usize = 4096;

const CONFIRMATION_HTML: &str = "<!DOCTYPE html>\
<html><head><meta charset=\"utf-8\"><title>LightningPoS</title></head>\
<body><h1>Produits enregistr&eacute;s</h1>\
<p>Les prix ont &eacute;t&eacute; mis &agrave; jour.</p>\
<p><a href=\"/\">Retour</a></p></body></html>";

const WIFI_CONFIRM_HTML: &str = "<!DOCTYPE html>\
<html><head><meta charset=\"utf-8\"><title>LightningPoS</title></head>\
<body><h1>Configuration enregistr&eacute;e</h1>\
<p>Red&eacute;marrage du POS pour appliquer le nouveau r&eacute;seau WiFi&hellip;</p></body></html>";

const CONFIG_CONFIRM_HTML: &str = "<!DOCTYPE html>\
<html><head><meta charset=\"utf-8\"><title>LightningPoS</title></head>\
<body><h1>Configuration enregistr&eacute;e</h1>\
<p>L'instance LNbits du POS a &eacute;t&eacute; mise &agrave; jour (appliqu&eacute;e au prochain paiement).</p>\
<p><a href=\"/config\">Retour &agrave; la configuration</a> &middot; <a href=\"/\">Produits</a></p></body></html>";

/// Handle NVS du namespace `products`, ouvert une fois par [`start`].
static NVS: OnceLock<EspDefaultNvs> = OnceLock::new();

/// Handle NVS du namespace `journal` (transactions), ouvert une fois par [`start`].
static JOURNAL: OnceLock<EspDefaultNvs> = OnceLock::new();

/// Handle NVS du namespace `poscfg` (URL + clé LNbits), ouvert une fois par [`start`].
static CFG: OnceLock<EspDefaultNvs> = OnceLock::new();

/// Compteur global de requêtes admin (début de fenêtre, count), reset toutes
/// les 60 s. L'horloge est monotone (`Instant`) : `SystemTime` recule quand
/// SNTP règle l'horloge au boot, ce qui faisait paniquer la soustraction.
static RATE_LIMIT: OnceLock<Mutex<(Instant, u8)>> = OnceLock::new();

/// Accès au namespace `products` (doit être appelé après [`start`]).
pub fn products_nvs() -> &'static EspDefaultNvs {
    NVS.get().expect("web::start doit être appelé avant")
}

/// Accès au namespace `journal` (doit être appelé après [`start`]).
pub fn journal_nvs() -> &'static EspDefaultNvs {
    JOURNAL.get().expect("web::start doit être appelé avant")
}

/// Accès au namespace `poscfg` (doit être appelé après [`start`]).
pub fn cfg_nvs() -> &'static EspDefaultNvs {
    CFG.get().expect("web::start doit être appelé avant")
}

/// Lit le corps d'une requête jusqu'à `MAX_BODY_LEN` octets.
fn read_body<C>(req: &mut esp_idf_svc::http::server::Request<C>) -> Result<String, C::Error>
where
    C: HttpConnection,
{
    let len = req.content_len().unwrap_or(0) as usize;
    if len == 0 {
        return Ok(String::new());
    }
    if len > MAX_BODY_LEN {
        return Ok(String::new());
    }
    let mut buf = vec![0u8; len];
    let mut filled = 0;
    while filled < len {
        let n = req.read(&mut buf[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(String::from_utf8_lossy(&buf[..filled]).into_owned())
}

/// Extrait le PIN du **corps** urlencoded d'un POST, et retourne aussi le corps
/// lu (les handlers en ont besoin pour les autres champs).
///
/// Le PIN n'est plus jamais lu dans la query string : un `?pin=123456` finit
/// dans l'historique du navigateur, dans le champ Referer des requêtes
/// sortantes et dans les journaux de tout proxy sur le trajet. Les pages
/// d'administration ne sont donc accessibles qu'en POST ; leur GET sert le
/// formulaire de saisie du PIN.
fn req_pin<C>(req: &mut esp_idf_svc::http::server::Request<C>) -> (String, String)
where
    C: HttpConnection,
{
    let body = read_body(req).unwrap_or_default();
    let mut pin = String::new();
    for pair in body.split('&') {
        if let Some(v) = pair.strip_prefix("pin=") {
            pin = url_decode(v);
        }
    }
    (pin, body)
}

/// Échecs consécutifs de PIN + instant de déblocage (verrou temporaire).
///
/// En mémoire volontairement : l'état n'a pas à survivre à un reboot (seul
/// quelqu'un ayant l'accès physique peut redémarrer le POS — il a alors déjà
/// le PIN affiché à l'écran) et cela évite d'user la NVS à chaque tentative.
static PIN_FAILS: OnceLock<Mutex<(u32, Option<Instant>)>> = OnceLock::new();

/// Nombre d'échecs tolérés avant que le backoff ne s'enclenche.
const PIN_FREE_TRIES: u32 = 5;

/// Plafond du backoff (5 min) : au-delà, l'attente deviendrait un déni de
/// service pour le commerçant lui-même.
const PIN_MAX_BACKOFF_SECS: u64 = 300;

fn pin_fails() -> &'static Mutex<(u32, Option<Instant>)> {
    PIN_FAILS.get_or_init(|| Mutex::new((0, None)))
}

/// Secondes restantes de blocage, ou `None` si le PIN peut être tenté.
fn pin_locked_for() -> Option<u64> {
    let mut st = pin_fails().lock().unwrap();
    match st.1 {
        Some(until) => {
            let now = Instant::now();
            if now < until {
                Some((until - now).as_secs() + 1)
            } else {
                st.1 = None;
                None
            }
        }
        None => None,
    }
}

/// Enregistre un échec : au-delà de `PIN_FREE_TRIES`, bloque `2^n` secondes
/// (plafonné). Le rate-limit global de 20 req/min laissait sinon ~28 000
/// essais par jour, de quoi épuiser un PIN à 6 chiffres en quelques semaines
/// — et bien plus vite en visant les PIN probables.
fn pin_note_failure() {
    let mut st = pin_fails().lock().unwrap();
    st.0 = st.0.saturating_add(1);
    if st.0 > PIN_FREE_TRIES {
        let exp = (st.0 - PIN_FREE_TRIES).min(16);
        let secs = (1u64 << exp).min(PIN_MAX_BACKOFF_SECS);
        st.1 = Some(Instant::now() + Duration::from_secs(secs));
    }
}

/// PIN correct : remet le compteur d'échecs à zéro.
fn pin_note_success() {
    let mut st = pin_fails().lock().unwrap();
    st.0 = 0;
    st.1 = None;
}

/// Contrôle d'accès d'une page admin.
///
/// Retourne `None` si le PIN est bon, sinon `(status, corps)` à renvoyer tel
/// quel : 429 pendant le backoff, 403 + formulaire PIN sinon.
fn pin_challenge(pin: &str, path: &str, preserve: &[(String, String)]) -> Option<(u16, String)> {
    if let Some(secs) = pin_locked_for() {
        return Some((
            429,
            format!(
                "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Bloqu&eacute;</title>\
                 </head><body><h1>Trop de PIN erron&eacute;s</h1>\
                 <p>R&eacute;essayez dans {secs} s.</p></body></html>"
            ),
        ));
    }
    if admin::check_pin(cfg_nvs(), pin) {
        pin_note_success();
        return None;
    }
    // Un PIN vide = première visite (le formulaire n'a pas encore été rempli) :
    // ce n'est pas une tentative, elle ne doit pas déclencher le backoff.
    if !pin.is_empty() {
        pin_note_failure();
    }
    Some((403, render_pin_required(path, !pin.is_empty(), preserve)))
}

/// Vrai si le corps POST porte le champ `k` (distingue un vrai envoi de
/// formulaire d'un simple POST issu de la page « PIN requis »).
fn has_field(pairs: &[(String, String)], k: &str) -> bool {
    pairs.iter().any(|(key, _)| key == k)
}

/// Vérifie le rate-limit global. Retourne `false` si la limite est dépassée.
fn rate_limit_check(max: u8) -> bool {
    let mut rl = RATE_LIMIT
        .get_or_init(|| Mutex::new((Instant::now(), 0)))
        .lock()
        .unwrap();
    if rl.0.elapsed() > Duration::from_secs(60) {
        rl.0 = Instant::now();
        rl.1 = 0;
    }
    // Saturation : au-delà de 255 requêtes dans la fenêtre, l'incrément
    // déborderait et repasserait sous la limite.
    rl.1 = rl.1.saturating_add(1);
    rl.1 <= max
}

/// Page « PIN requis ». `path` = action du formulaire, `invalid` = PIN fourni mais faux,
/// `preserve` = champs cachés à reposter (ex. les champs d'origine d'un POST).
fn render_pin_required(path: &str, invalid: bool, preserve: &[(String, String)]) -> String {
    let mut html = String::new();
    html.push_str("<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>PIN requis</title>");
    html.push_str(
        "<style>body{font-family:sans-serif;max-width:640px;margin:2em auto}\
         input[type=password]{padding:6px;width:120px}\
         .err{color:#c00}</style>",
    );
    html.push_str("</head><body><h1>PIN requis</h1>");
    if invalid {
        html.push_str("<p class=\"err\">PIN incorrect.</p>");
    }
    html.push_str(&format!(
        "<form method=\"post\" action=\"{}\">",
        html_escape(path)
    ));
    for (k, v) in preserve {
        if k == "pin" {
            continue;
        }
        html.push_str(&format!(
            "<input type=\"hidden\" name=\"{}\" value=\"{}\">",
            html_escape(k),
            html_escape(v)
        ));
    }
    html.push_str("<label>PIN</label> <input type=\"password\" name=\"pin\" maxlength=\"6\">");
    html.push_str(" <input type=\"submit\" value=\"Valider\">");
    html.push_str("</form></body></html>");
    html
}

/// Parse tous les champs d'un corps urlencoded en conservant l'ordre.
fn parse_all_pairs(body: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for pair in body.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (url_decode(k), url_decode(v)),
            None => (url_decode(pair), String::new()),
        };
        out.push((k, v));
    }
    out
}

/// Crée le serveur HTTP et enregistre les routes `/`, `/save`, `/config`,
/// `/transactions` et `/transactions.csv`. Reçoit les clones de la partition
/// NVS par défaut (déjà `take()`-ée par le WiFi) : produits, journal, config.
pub fn start(
    partition: EspDefaultNvsPartition,
    journal_partition: EspDefaultNvsPartition,
    cfg_partition: EspDefaultNvsPartition,
) -> Result<EspHttpServer<'static>, EspError> {
    let _ = NVS.set(EspNvs::new(partition, "products", true).unwrap());
    let _ = JOURNAL.set(EspNvs::new(journal_partition, "journal", true).unwrap());
    let _ = CFG.set(EspNvs::new(cfg_partition, "poscfg", true).unwrap());
    let mut server = EspHttpServer::new(&Configuration::default()).map_err(|e: EspIOError| e.0)?;

    server.fn_handler("/", Method::Get, |req| -> Result<(), EspIOError> {
        let products = store::load(products_nvs());
        let html = render_form(&products);
        req.into_response(200, Some("OK"), &[content_type("text/html; charset=utf-8")])?
            .write_all(html.as_bytes())
            .map(|_| ())
    })?;

    server.fn_handler("/save", Method::Post, |mut req| -> Result<(), EspIOError> {
        if !rate_limit_check(20) {
            req.into_status_response(429)?
                .write_all(b"Trop de tentatives, reessayez dans 60 s.")
                .map(|_| ())?;
            return Ok(());
        }
        let (pin, body) = req_pin(&mut req);
        let pairs = parse_all_pairs(&body);
        if let Some((code, html)) = pin_challenge(&pin, "/save", &pairs) {
            req.into_response(code, None, &[content_type("text/html; charset=utf-8")])?
                .write_all(html.as_bytes())
                .map(|_| ())?;
            return Ok(());
        }

        let products = parse_products(&body);
        store::save(products_nvs(), &products).map_err(EspIOError)?;

        req.into_response(200, Some("OK"), &[content_type("text/html; charset=utf-8")])?
            .write_all(CONFIRMATION_HTML.as_bytes())
            .map(|_| ())
    })?;

    // Les pages d'administration n'existent qu'en POST (le PIN voyage dans le
    // corps) : leur GET sert le formulaire de saisie du PIN.
    server.fn_handler("/config", Method::Get, |req| -> Result<(), EspIOError> {
        let html = render_pin_required("/config", false, &[]);
        req.into_response(
            403,
            Some("Forbidden"),
            &[content_type("text/html; charset=utf-8")],
        )?
        .write_all(html.as_bytes())
        .map(|_| ())
    })?;

    server.fn_handler(
        "/config",
        Method::Post,
        |mut req| -> Result<(), EspIOError> {
            if !rate_limit_check(20) {
                req.into_status_response(429)?
                    .write_all(b"Trop de tentatives, reessayez dans 60 s.")
                    .map(|_| ())?;
                return Ok(());
            }
            let (pin, body) = req_pin(&mut req);
            let pairs = parse_all_pairs(&body);
            if let Some((code, html)) = pin_challenge(&pin, "/config", &pairs) {
                req.into_response(code, None, &[content_type("text/html; charset=utf-8")])?
                    .write_all(html.as_bytes())
                    .map(|_| ())?;
                return Ok(());
            }

            // POST venant de la page « PIN requis » (aucun champ de config) : le PIN
            // est bon, on sert le formulaire en y reportant le PIN.
            if !has_field(&pairs, "url") {
                let html = render_config(&pin);
                req.into_response(200, Some("OK"), &[content_type("text/html; charset=utf-8")])?
                    .write_all(html.as_bytes())
                    .map(|_| ())?;
                return Ok(());
            }

            let (url, key, cur) = parse_config(&body);
            if let Err(e) = config::save(cfg_nvs(), &url, &key, &cur) {
                req.into_status_response(400)?
                    .write_all(format!("Erreur: {}", e).as_bytes())
                    .map(|_| ())?;
                return Ok(());
            }

            req.into_response(200, Some("OK"), &[content_type("text/html; charset=utf-8")])?
                .write_all(CONFIG_CONFIRM_HTML.as_bytes())
                .map(|_| ())
        },
    )?;

    server.fn_handler("/wifi", Method::Get, |req| -> Result<(), EspIOError> {
        let html = render_pin_required("/wifi", false, &[]);
        req.into_response(
            403,
            Some("Forbidden"),
            &[content_type("text/html; charset=utf-8")],
        )?
        .write_all(html.as_bytes())
        .map(|_| ())
    })?;

    server.fn_handler("/wifi", Method::Post, |mut req| -> Result<(), EspIOError> {
        if !rate_limit_check(20) {
            req.into_status_response(429)?
                .write_all(b"Trop de tentatives, reessayez dans 60 s.")
                .map(|_| ())?;
            return Ok(());
        }
        let (pin, body) = req_pin(&mut req);
        let pairs = parse_all_pairs(&body);
        if let Some((code, html)) = pin_challenge(&pin, "/wifi", &pairs) {
            req.into_response(code, None, &[content_type("text/html; charset=utf-8")])?
                .write_all(html.as_bytes())
                .map(|_| ())?;
            return Ok(());
        }

        // POST venant de la page « PIN requis » (aucun champ réseau) : le PIN
        // est bon, on sert le formulaire WiFi en y reportant le PIN.
        if !has_field(&pairs, "ssid") && !has_field(&pairs, "wifi") {
            let html = render_wifi(&pin);
            req.into_response(200, Some("OK"), &[content_type("text/html; charset=utf-8")])?
                .write_all(html.as_bytes())
                .map(|_| ())?;
            return Ok(());
        }

        // Le champ texte prime ; s'il est vide on prend le réseau choisi dans
        // la liste du scan.
        let (mut ssid, pass, scanned) = parse_wifi(&body);
        if ssid.is_empty() {
            ssid = scanned;
        }
        let ssid = ssid.trim().to_string();
        if ssid.is_empty() {
            req.into_status_response(400)?
                .write_all(b"SSID requis")
                .map(|_| ())?;
            return Ok(());
        }
        if let Err(e) = wifi_cfg::save(&ssid, &pass) {
            req.into_status_response(400)?
                .write_all(format!("Erreur: {}", e).as_bytes())
                .map(|_| ())?;
            return Ok(());
        }

        req.into_response(200, Some("OK"), &[content_type("text/html; charset=utf-8")])?
            .write_all(WIFI_CONFIRM_HTML.as_bytes())
            .map(|_| ())?;

        // Laisse le temps à la réponse de partir avant de couper le WiFi.
        std::thread::sleep(Duration::from_millis(500));
        unsafe { esp_idf_sys::esp_restart() }
    })?;

    server.fn_handler(
        "/transactions",
        Method::Get,
        |req| -> Result<(), EspIOError> {
            let html = render_pin_required("/transactions", false, &[]);
            req.into_response(
                403,
                Some("Forbidden"),
                &[content_type("text/html; charset=utf-8")],
            )?
            .write_all(html.as_bytes())
            .map(|_| ())
        },
    )?;

    server.fn_handler(
        "/transactions",
        Method::Post,
        |mut req| -> Result<(), EspIOError> {
            if !rate_limit_check(20) {
                req.into_status_response(429)?
                    .write_all(b"Trop de tentatives, reessayez dans 60 s.")
                    .map(|_| ())?;
                return Ok(());
            }
            let (pin, body) = req_pin(&mut req);
            let pairs = parse_all_pairs(&body);
            if let Some((code, html)) = pin_challenge(&pin, "/transactions", &pairs) {
                req.into_response(code, None, &[content_type("text/html; charset=utf-8")])?
                    .write_all(html.as_bytes())
                    .map(|_| ())?;
                return Ok(());
            }
            let txs = journal::list(journal_nvs());
            let html = render_transactions(&txs, &pin);
            req.into_response(200, Some("OK"), &[content_type("text/html; charset=utf-8")])?
                .write_all(html.as_bytes())
                .map(|_| ())
        },
    )?;

    server.fn_handler(
        "/transactions.csv",
        Method::Get,
        |req| -> Result<(), EspIOError> {
            let html = render_pin_required("/transactions.csv", false, &[]);
            req.into_response(
                403,
                Some("Forbidden"),
                &[content_type("text/html; charset=utf-8")],
            )?
            .write_all(html.as_bytes())
            .map(|_| ())
        },
    )?;

    server.fn_handler(
        "/transactions.csv",
        Method::Post,
        |mut req| -> Result<(), EspIOError> {
            if !rate_limit_check(20) {
                req.into_status_response(429)?
                    .write_all(b"Trop de tentatives, reessayez dans 60 s.")
                    .map(|_| ())?;
                return Ok(());
            }
            let (pin, body) = req_pin(&mut req);
            let pairs = parse_all_pairs(&body);
            if let Some((code, html)) = pin_challenge(&pin, "/transactions.csv", &pairs) {
                req.into_response(code, None, &[content_type("text/html; charset=utf-8")])?
                    .write_all(html.as_bytes())
                    .map(|_| ())?;
                return Ok(());
            }
            let txs = journal::list(journal_nvs());
            let csv = render_transactions_csv(&txs);
            req.into_response(
                200,
                Some("OK"),
                &[
                    ("Content-Type", "text/csv; charset=utf-8"),
                    (
                        "Content-Disposition",
                        "attachment; filename=\"transactions.csv\"",
                    ),
                ],
            )?
            .write_all(csv.as_bytes())
            .map(|_| ())
        },
    )?;

    Ok(server)
}

/// Construit la page HTML du formulaire (15 lignes nom + prix en euros).
fn render_form(products: &[Product]) -> String {
    let mut html = String::new();
    html.push_str("<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>LightningPoS</title>");
    html.push_str(
        "<style>body{font-family:sans-serif;max-width:640px;margin:2em auto}\
         table{width:100%}td{padding:4px}\
         input[type=text]{width:100%}input[type=number]{width:120px}</style>",
    );
    html.push_str("</head><body><h1>Produits</h1>");
    html.push_str("<form method=\"post\" action=\"/save\"><table>");

    for (i, p) in products.iter().enumerate() {
        let euros = p.cents as f64 / 100.0;
        html.push_str(&format!(
            "<tr><td><input type=\"text\" name=\"n{i}\" maxlength=\"12\" value=\"{}\"></td>",
            html_escape(&p.name)
        ));
        html.push_str(&format!(
            "<td><input type=\"number\" name=\"p{i}\" step=\"0.01\" min=\"0\" value=\"{euros:.2}\"></td></tr>"
        ));
    }

    html.push_str("</table><br><input type=\"submit\" value=\"Enregistrer\"></form>");
    html.push_str("<p><a href=\"/config\">Configuration LNbits</a> &middot; <a href=\"/wifi\">Configuration WiFi</a> &middot; <a href=\"/transactions\">Voir les transactions</a></p>");
    html.push_str("</body></html>");
    html
}

/// Champ caché reportant le PIN d'une page admin à la suivante (le PIN ne
/// pouvant plus transiter par l'URL).
fn hidden_pin(pin: &str) -> String {
    if pin.is_empty() {
        return String::new();
    }
    format!(
        "<input type=\"hidden\" name=\"pin\" value=\"{}\">",
        html_escape(pin)
    )
}

/// Construit la page HTML du journal des transactions.
fn render_transactions(txs: &[journal::Tx], pin: &str) -> String {
    let mut html = String::new();
    html.push_str("<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>LightningPoS — Transactions</title>");
    html.push_str(
        "<style>body{font-family:sans-serif;max-width:720px;margin:2em auto}\
         table{width:100%;border-collapse:collapse}td,th{border:1px solid #ccc;padding:4px;text-align:left}\
         .hint{color:#666;font-size:0.9em}</style>",
    );
    html.push_str("</head><body><h1>Transactions</h1>");
    // L'export CSV est un POST : il porte le PIN dans son corps.
    html.push_str(&format!(
        "<p><a href=\"/\">Produits</a> &middot; \
         <form method=\"post\" action=\"/transactions.csv\" style=\"display:inline\">{}\
         <input type=\"submit\" value=\"Exporter CSV\"></form></p>",
        hidden_pin(pin)
    ));
    if txs.is_empty() {
        html.push_str("<p>Aucune transaction pour l'instant.</p>");
    } else {
        html.push_str(
            "<table><tr><th>#</th><th>Date (UTC)</th><th>Libell&eacute;</th><th>Montant</th>\
             <th>sats</th><th>hash</th></tr>",
        );
        for tx in txs {
            let montant = tx.cents as f64 / 100.0;
            let cur = if tx.cur.is_empty() { "EUR" } else { &tx.cur };
            html.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{montant:.2} {}</td>\
                 <td>{}</td><td>{}</td></tr>",
                tx.seq,
                html_escape(&fmt_ts(tx)),
                html_escape(&tx.label),
                html_escape(cur),
                tx.sats,
                html_escape(&tx.hash)
            ));
        }
        html.push_str("</table>");
        html.push_str(
            "<p class=\"hint\">Les dates « +Ns » sont relatives au d&eacute;marrage du POS \
             (horloge non encore synchronis&eacute;e par SNTP).</p>",
        );
    }
    html.push_str("</body></html>");
    html
}

/// Export CSV du journal (compatible tableur).
///
/// `datetime_utc` est vide quand l'horloge n'était pas synchronisée : la
/// colonne `uptime_s` porte alors la valeur relative au boot, et `seq` reste
/// la clé d'ordre fiable dans tous les cas.
fn render_transactions_csv(txs: &[journal::Tx]) -> String {
    let mut csv =
        String::from("seq,datetime_utc,uptime_s,amount_cents,currency,label,sats,payment_hash\n");
    for tx in txs {
        let (dt, up) = if tx.epoch {
            (fmt_ts(tx), String::new())
        } else {
            (String::new(), tx.ts.to_string())
        };
        let cur = if tx.cur.is_empty() { "EUR" } else { &tx.cur };
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{}\n",
            tx.seq,
            dt,
            up,
            tx.cents,
            cur,
            csv_escape(&tx.label),
            tx.sats,
            tx.hash
        ));
    }
    csv
}

/// Échappe une valeur CSV (virgule / guillemet / saut de ligne).
fn csv_escape(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Horodatage lisible : date UTC si l'horloge était synchronisée, sinon
/// l'uptime relatif au boot (`+123s`).
fn fmt_ts(tx: &journal::Tx) -> String {
    if !tx.epoch {
        return format!("+{}s", tx.ts);
    }
    let (y, m, d, hh, mm, ss) = civil_from_epoch(tx.ts);
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02}")
}

/// Epoch UNIX (UTC) → (année, mois, jour, h, min, s).
/// Algorithme `civil_from_days` de Howard Hinnant (ère de 400 ans) — évite
/// d'embarquer une crate de dates pour la seule page du journal.
fn civil_from_epoch(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (
        y,
        m,
        d,
        (rem / 3600) as u32,
        ((rem % 3600) / 60) as u32,
        (rem % 60) as u32,
    )
}

/// Page de configuration : URL + clé API LNbits (clé affichée masquée).
/// Champ clé vide = garder l'existante (elle n'est jamais renvoyée en clair).
fn render_config(pin: &str) -> String {
    let url = config::load_url(cfg_nvs());
    let tail = config::key_tail(cfg_nvs());
    let mut html = String::new();
    html.push_str("<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>LightningPoS — Configuration</title>");
    html.push_str(
        "<style>body{font-family:sans-serif;max-width:640px;margin:2em auto}\
         label{display:block;margin:12px 0 4px}input[type=text],input[type=password]{width:100%;padding:6px}\
         .hint{color:#666;font-size:0.9em}</style>",
    );
    html.push_str("</head><body><h1>Configuration LNbits</h1>");
    html.push_str(&format!(
        "<p class=\"hint\">Instance vers laquelle pointe le POS (factures QR + poll). \
         Les cartes Bolt Card, elles, suivent l'instance inscrite dans leur NDEF.</p>"
    ));
    html.push_str("<form method=\"post\" action=\"/config\">");
    html.push_str(&hidden_pin(pin));
    html.push_str(&format!(
        "<label>URL LNbits</label><input type=\"text\" name=\"url\" value=\"{}\">",
        html_escape(&url)
    ));
    html.push_str(&format!(
        "<label>Clé API (actuelle : …{})</label><input type=\"password\" name=\"key\" \
         placeholder=\"laisser vide pour garder\">",
        html_escape(&tail)
    ));
    let cur = config::load_currency(cfg_nvs());
    html.push_str(&format!(
        "<label>Devise</label><select name=\"cur\">\
         <option value=\"EUR\"{selected_eur}>EUR — Euro</option>\
         <option value=\"USD\"{selected_usd}>USD — Dollar US</option>\
         <option value=\"GBP\"{selected_gbp}>GBP — Livre sterling</option></select>",
        selected_eur = if cur == "EUR" { " selected" } else { "" },
        selected_usd = if cur == "USD" { " selected" } else { "" },
        selected_gbp = if cur == "GBP" { " selected" } else { "" },
    ));
    html.push_str("<br><br><input type=\"submit\" value=\"Enregistrer\"></form>");
    html.push_str(
        "<p><a href=\"/\">Produits</a> &middot; <a href=\"/transactions\">Transactions</a></p>",
    );
    html.push_str("</body></html>");
    html
}

/// Page de provisioning WiFi : SSID + mot de passe du réseau à rejoindre.
/// La liste déroulante reprend les réseaux détectés au boot (scan STA).
fn render_wifi(pin: &str) -> String {
    let ssid = wifi_cfg::ssid();
    let scan = wifi_cfg::scan_list();
    let ap = format!("LightningPoS-{}", wifi_cfg::ap_suffix());
    let mut html = String::new();
    html.push_str(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>LightningPoS — WiFi</title>",
    );
    html.push_str(
        "<style>body{font-family:sans-serif;max-width:640px;margin:2em auto}\
         label{display:block;margin:12px 0 4px}input[type=text],input[type=password],select{width:100%;padding:6px}\
         .hint{color:#666;font-size:0.9em}</style>",
    );
    html.push_str("</head><body><h1>Configuration WiFi</h1>");
    html.push_str(
        "<p class=\"hint\">R&eacute;seau que le POS rejoint au d&eacute;marrage. \
         Le POS red&eacute;marre apr&egrave;s l'enregistrement.</p>",
    );
    html.push_str("<form method=\"post\" action=\"/wifi\">");
    html.push_str(&hidden_pin(pin));
    if !scan.is_empty() {
        html.push_str("<label>R&eacute;seaux d&eacute;tect&eacute;s</label><select name=\"wifi\">");
        html.push_str("<option value=\"\">— réseau détecté —</option>");
        for s in &scan {
            html.push_str(&format!(
                "<option value=\"{0}\">{0}</option>",
                html_escape(s)
            ));
        }
        html.push_str("</select>");
    }
    html.push_str(&format!(
        "<label>Nom du r&eacute;seau (SSID)</label><input type=\"text\" name=\"ssid\" value=\"{}\">",
        html_escape(&ssid)
    ));
    html.push_str(
        "<label>Mot de passe</label><input type=\"password\" name=\"pass\" \
         placeholder=\"laisser vide si réseau ouvert\">",
    );
    html.push_str("<br><br><input type=\"submit\" value=\"Enregistrer et redémarrer\"></form>");
    // Le mot de passe de l'AP est affiché ici : généré au premier boot, il
    // n'apparaît sinon qu'à l'écran du POS pendant quelques secondes.
    html.push_str(&format!(
        "<h2>Point d'acc&egrave;s du POS</h2>\
         <p>R&eacute;seau : <b>{}</b><br>Mot de passe (WPA2) : <b><code>{}</code></b></p>\
         <p class=\"hint\">Le point d'acc&egrave;s reste actif : le portail est aussi \
         joignable sur <a href=\"http://192.168.4.1\">http://192.168.4.1</a></p>",
        html_escape(&ap),
        html_escape(&wifi_cfg::ap_pass())
    ));
    html.push_str(
        "<p><a href=\"/\">Produits</a> &middot; <a href=\"/config\">Configuration LNbits</a></p>",
    );
    html.push_str("</body></html>");
    html
}

/// Parse le corps d'un POST `/wifi` : champs `ssid`, `pass` et `wifi` (le
/// réseau choisi dans la liste du scan), urlencoded.
fn parse_wifi(body: &str) -> (String, String, String) {
    let mut ssid = String::new();
    let mut pass = String::new();
    let mut scanned = String::new();
    for pair in body.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (url_decode(k), url_decode(v)),
            None => (url_decode(pair), String::new()),
        };
        match k.as_str() {
            "ssid" => ssid = v,
            "pass" => pass = v,
            "wifi" => scanned = v,
            _ => {}
        }
    }
    (ssid, pass, scanned)
}

/// Parse le corps d'un POST `/config` : champs `url`, `key` et `cur` (urlencoded).
fn parse_config(body: &str) -> (String, String, String) {
    let mut url = String::new();
    let mut key = String::new();
    let mut cur = String::new();
    for pair in body.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (url_decode(k), url_decode(v)),
            None => (url_decode(pair), String::new()),
        };
        match k.as_str() {
            "url" => url = v,
            "key" => key = v,
            "cur" => cur = v,
            _ => {}
        }
    }
    (url, key, cur)
}

/// Échappe les caractères spéciaux HTML dans une valeur d'attribut.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Décode une valeur `application/x-www-form-urlencoded` (`+` → espace, `%XX`).
fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => match (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                (Some(hi), Some(lo)) => {
                    out.push(hi * 16 + lo);
                    i += 3;
                }
                _ => {
                    out.push(b'%');
                    i += 1;
                }
            },
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Parse le corps d'un POST : champs `n{i}` (nom) et `p{i}` (prix en euros).
/// Convertit les euros en centimes entiers (arrondi au plus proche) et
/// met les noms en majuscules (trim).
fn parse_products(body: &str) -> Vec<Product> {
    let defaults = store::defaults();
    let mut names: Vec<String> = defaults.iter().map(|p| p.name.clone()).collect();
    let mut prices: Vec<Option<f64>> = vec![None; NUM_PRODUCTS];

    for pair in body.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        let key = url_decode(key);
        let value = url_decode(value);

        if let Some(idx) = key.strip_prefix('n').and_then(|s| s.parse::<usize>().ok()) {
            if idx < NUM_PRODUCTS {
                names[idx] = value;
            }
        } else if let Some(idx) = key.strip_prefix('p').and_then(|s| s.parse::<usize>().ok()) {
            if idx < NUM_PRODUCTS {
                prices[idx] = value.parse::<f64>().ok();
            }
        }
    }

    (0..NUM_PRODUCTS)
        .map(|i| {
            // Borné à 12 caractères : au-delà, le nom déborde de la cellule du
            // menu (96 px) et de la fonte 5×7 embarquée.
            let name: String = names[i].trim().to_uppercase().chars().take(12).collect();
            let cents = prices[i]
                .map(|euros| (euros * 100.0).round().max(0.0) as u32)
                .unwrap_or(defaults[i].cents);
            Product { name, cents }
        })
        .collect()
}
