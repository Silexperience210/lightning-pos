//! Configuration de déploiement — CE FICHIER PEUT ÊTRE COMMITÉ (aucun secret).
//!
//! Plus aucun secret dans le binaire : le WiFi (portail /wifi) et la clé LNbits
//! (portail /config) se provisionnent via le portail web, stockés en NVS.
//! Si un secret doit un jour revenir ici, le sortir vers NVS ou le git-ignorer.

pub const LNBITS_URL: &str = "http://192.168.1.176:3007";
pub const LNBITS_KEY: &str = ""; // La clé LNbits se provisionne via le portail /config (NVS) — jamais dans le binaire.
