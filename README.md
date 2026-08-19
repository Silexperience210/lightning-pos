# ⚡ LightningPoS

[![CI](https://github.com/Silexperience210/lightning-pos/actions/workflows/ci.yml/badge.svg)](https://github.com/Silexperience210/lightning-pos/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Silexperience210/lightning-pos)](https://github.com/Silexperience210/lightning-pos/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

**Terminal de caisse (POS) Bitcoin Lightning NFC** — Rust + ESP-IDF.
Carte **JC3248W535C** (ESP32-S3, écran tactile 320×480, lecteur NFC RC522).

Le client compose son panier sur l'écran tactile, le montant est converti EUR → sats,
une facture Lightning est créée sur **LNbits**, payée par **QR code** ou **carte NFC**
(Bolt Card / NTAG / MIFARE Classic) — et l'écran s'illumine d'un ⚡ quand c'est payé.

**Le POS ne touche jamais les fonds** : il crée des factures sur *ton* serveur LNbits
et écoute la confirmation. Bitcoin-only.

---

## ✨ Fonctionnalités (toutes les options)

### Caisse
- **Menu produits** : jusqu'à 15 produits, nom (≤ 12 caractères) + prix modifiables
  depuis le portail web
- **Panier** : touches produits, totaux € + nombre d'articles en direct
- **Mode calculatrice** : montant libre (bouton en tête d'écran)
- **Devise** : EUR / GBP / USD (configurable)
- **Conversion BTC** : prix Bitcoin (CoinGecko) → montant en sats affiché en
  permanence sur la facture et le QR
- **Délai de paiement** : la facture expire (90 s par défaut), le QR est régénéré
  au polling (300 ms)

### Paiement
- **QR code** : facture Lightning (LNbits `api/v1/payments`)
- **NFC** : 3 familles de cartes supportées
  - **NTAG213** (Type 2, SAK 0x00)
  - **Bolt Card / NTAG424 DNA** (Type 4, RATS + APDU, compteurs SDM)
  - **MIFARE Classic 1K** (auth Crypto-1 + lecture NDEF)
- **LNURL-withdraw** : la carte porte un LNURL-w withdraw, le POS résout, vérifie la
  plage de montant (`amount_in_range_msat`), construit le callback avec la facture
- **Confirmation** : flash blanc → éclair 3D ⚡ « PAYÉ » (montant € + sats affichés)
- **Journal** : chaque vente est enregistrée (horodatage, montant, sats, hash)

### Écran / UI
- Thème trou noir, éclair ambre, chiffres vectoriels anti-aliasés
- **Éclair 3D** : sprites pré-calculés (36 frames, flash) + mini-renderer temps réel
  (rotation pendant le règlement et l'attract)
- **Attract** : ~8 s d'inactivité → éclair 3D animé + « LightningPoS »
- **Veille** : double-tap en tête d'écran → veille animée (60 s max) → extinction
  écran ; double-tap n'importe où réveille ; filtre anti-fantôme tactile
- **Retour** : double-tap dans la bande d'en-tête (y < 24, hors zone x < 90)
- Crossfade 36 ms entre les vues, backlight piloté (GPIO1)

### Réseau
- **Provisionning WiFi** : au 1er boot, le POS crée son propre point d'accès
  **`LightningPoS-XXXX`** (sans mot de passe — le portail est protégé par PIN)
- **Scan WiFi** : liste des réseaux détectés proposée dans le portail
- **AP permanent** : le hotspot reste actif même une fois le WiFi configuré
  (reconfiguration à tout moment)
- Icône RSSI + niveau batterie en tête d'écran

### Sécurité
- **Zéro secret dans le binaire** : ni mot de passe WiFi ni clé LNbits compilés —
  tout se provisionne via le portail web (stockage NVS)
- **PIN admin** : 6 chiffres générés au 1er boot (NVS), affichés 4 s à l'écran
  + sur la console série — jamais dans le binaire ni le réseau
- **Rate-limit** : 20 tentatives/min sur les routes sensibles (anti brute-force)
- **Anti-double-débit** : verrou `callback_sent` + anti-rebond RC522 — une carte
  ne peut pas déclencher deux paiements
- **Journal exposé** seulement avec PIN

---

## 🖥️ Portail web

Le POS embarque un serveur web (port 80). Accessible sur le réseau local
(IP du POS) ou via le point d'accès `LightningPoS-XXXX` → **http://192.168.4.1**.

| Route | Méthode | Description | PIN |
|---|---|---|---|
| `/` | GET | Liste des produits (nom + prix) | ❌ |
| `/save` | POST | Enregistrer les produits (nom ≤ 12 car., prix ≥ 0) | ✅ |
| `/config` | GET/POST | URL LNbits + clé API + devise (EUR/GBP/USD) | ✅ |
| `/wifi` | GET/POST | Scan + choix du réseau + mot de passe → redémarre | ✅ |
| `/transactions` | GET | Journal des ventes (tableau) | ✅ |
| `/transactions.csv` | GET | Export CSV des ventes | ✅ |

> La clé LNbits n'est **jamais** renvoyée en clair par le portail (masquée).

---

## 🚀 Premier démarrage

1. Flasher le firmware (voir ci-dessous)
2. Le POS démarre en **AP mode** : hotspot `LightningPoS-XXXX` (sans mot de passe)
3. Connecter le téléphone au hotspot → ouvrir **http://192.168.4.1**
4. Entrer le **PIN affiché à l'écran au boot**
5. **`/wifi`** : choisir son réseau (liste du scan) + mot de passe → le POS redémarre connecté
6. **`/config`** : renseigner l'URL LNbits + la clé API (wallet) + la devise
7. **`/`** : renommer/reprixer les produits → enregistrer

La première facture est refusée tant que la clé LNbits n'est pas configurée
(message « clé LNbits non configurée (portail /config) »).

---

## 🛠️ Build

### Prérequis
- Toolchain Rust esp (espup) : `cargo install espup && espup install`
- ESP-IDF v5.2.3 (installé automatiquement par esp-idf-sys au premier build)

### Compiler le firmware
```bash
cd firmware-espidf
source ~/export-esp.sh
cargo +esp build --release
```

### Flasher
```bash
espflash flash --port /dev/ttyACM0 \
  target/xtensa-esp32s3-espidf/release/firmware-espidf
```

> ⚠️ Le port natif USB-CDC du JC3248W535C est **`/dev/ttyACM0`** (ESP32-S3).
> Un autre ESP32 (dev board CH340) apparaît en `/dev/ttyUSB0` — vérifier le chip
> avant de flasher.

### CI (GitHub Actions)
Chaque push compile le firmware (toolchain Xtensa épinglée, ESP-IDF v5.2.3)
+ audit de sécurité. Le binaire est disponible en artifact du run.

---

## 📁 Structure

```
lightning-pos/
├── firmware-espidf/          # ⚡ LE firmware du POS (JC3248W535C, ESP32-S3)
│   ├── src/
│   │   ├── main.rs           # Boucle principale, WiFi, paiement, veille
│   │   ├── ui.rs             # Menu, facture, QR, 3D paiement
│   │   ├── display.rs        # Pilote AXS15231B (QSPI 320×480)
│   │   ├── rc522.rs          # Pilote RC522 (FM17522) — NTAG/Bolt/MIFARE
│   │   ├── boltcard.rs       # LNURL-withdraw (logique pure)
│   │   ├── render3d.rs       # Mini-renderer 3D (éclair temps réel)
│   │   ├── sprites.rs        # Sprites éclair pré-calculés (flash)
│   │   ├── web.rs            # Portail web (PIN, produits, config, WiFi, journal)
│   │   ├── wifi_cfg.rs       # Provisionning WiFi (AP + STA, NVS)
│   │   ├── admin.rs          # PIN admin + rate-limit
│   │   └── store.rs          # Produits en NVS
│   ├── assets/               # bolt_sprites.bin (36 frames RGB565)
│   └── tools/                # Générateur de sprites (numpy)
│
├── lightningpos-core/        # (legacy) logique métier
├── lightningpos-hal/         # (legacy) couche matérielle
├── lightningpos-net/         # (legacy) réseau async
├── lightningpos-display/     # (legacy) rendu
├── lightningpos-nfc/         # (legacy) NFC
├── lightningpos-firmware/    # (legacy) variante T-Display-S3
├── firmware-touch35/         # (legacy) variante Touch 3.5"
├── firmware-headless/        # (legacy) variante headless
├── firmware-c3/              # (legacy) variante ESP32-C3
└── enclosure/                # Boîtier imprimable 3 pièces (STL + générateur)
```

> Les crates/variantes « legacy » sont les restes de l'ancien projet ZapBox.
> Le produit actif est **`firmware-espidf`** — les autres ne sont pas maintenus.

---

## 🧪 Cartes NFC testées

| Carte | Type | SAK | Statut |
|---|---|---|---|
| NTAG213 | Type 2 | 0x00 | ✅ |
| Bolt Card (NTAG424 DNA) | Type 4 | 0x20 | ✅ (compteurs SDM lus) |
| MIFARE Classic 1K | Classic | 0x08 | ✅ (auth Crypto-1) |

> **Piège Bolt Card** : le plafond `daily_limit` se règle à la **création** de la
> carte (LNbits) — une carte créée avec 0 refuse les paiements. Ce n'est pas le
> compteur.

---

## 📄 Licence

MIT — voir [LICENSE](LICENSE).
