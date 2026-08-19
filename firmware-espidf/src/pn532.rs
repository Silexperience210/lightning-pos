//! Driver PN532 (HW-147C) en I2C — adresse 0x24.
//! Commandes de base : GetFirmwareVersion, SAMConfiguration,
//! InListPassiveTarget (détection carte ISO14443A), InDataExchange.

use esp_idf_hal::i2c::I2cDriver;

const PN532_ADDR: u8 = 0x28;
const TIMEOUT: u32 = 1000; // ticks

pub struct Pn532<'d> {
    i2c: I2cDriver<'d>,
}

/// Trame de réponse PN532 décodée
pub struct Response {
    pub command: u8,
    pub data: Vec<u8>,
}

impl<'d> Pn532<'d> {
    pub fn new(i2c: I2cDriver<'d>) -> Self {
        Self { i2c }
    }

    /// Construit la trame host→PN532 : preamble + len + lcs + 0xD4 + data + dcs + 0x00.
    /// Checksums = complément à deux (0x100 − x), DCS inclut le start code 0xFF
    /// (identique à la lib Adafruit/Seeed, référence).
    fn frame(command: &[u8]) -> Vec<u8> {
        let len = (1 + command.len()) as u8; // TFI + data
        let mut f = vec![0x00, 0x00, 0xFF, len, (!len).wrapping_add(1), 0xD4];
        f.extend_from_slice(command);
        let mut sum: u8 = 0xFFu8.wrapping_add(0xD4);
        for &b in command {
            sum = sum.wrapping_add(b);
        }
        f.push((!sum).wrapping_add(1));
        f.push(0x00);
        f
    }

    /// Envoie une commande et lit la réponse.
    pub fn send(&mut self, command: &[u8]) -> Result<Response, String> {
        let frame = Self::frame(command);
        self.i2c
            .write(PN532_ADDR, &frame, TIMEOUT)
            .map_err(|e| format!("i2c write: {e}"))?;

        // Poll status byte → 0x01 = prêt
        let mut ready = false;
        for i in 0..200 {
            let mut status = [0u8; 1];
            self.i2c
                .read(PN532_ADDR, &mut status, TIMEOUT)
                .map_err(|e| format!("i2c read status: {e}"))?;
            if i < 5 {
                println!("[PN532] debug status={:02X}", status[0]);
            }
            if status[0] == 0x01 {
                ready = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        if !ready {
            // Debug : lit la réponse brute pour identifier le périphérique
            let mut raw = [0u8; 16];
            if self.i2c.read(PN532_ADDR, &mut raw, TIMEOUT).is_ok() {
                println!("[PN532] debug raw={:02X?}", &raw[..8]);
            }
            return Err("PN532 pas prêt (timeout status)".into());
        }

        // Lit la trame de réponse : 0x00 0x00 0xFF 0x00 0xFF 0x00 len lcs 0xD5 data dcs 0x00
        let mut buf = [0u8; 64];
        self.i2c
            .read(PN532_ADDR, &mut buf, TIMEOUT)
            .map_err(|e| format!("i2c read frame: {e}"))?;

        // Cherche 0xD5 (le TFI de réponse) après le preamble
        let tfi_pos = buf
            .iter()
            .position(|&b| b == 0xD5)
            .ok_or("pas de 0xD5 dans la réponse")?;
        let cmd = buf.get(tfi_pos + 1).copied().unwrap_or(0);
        // La longueur utile est à tfi_pos-2 (le champ len), ou on lit jusqu'au DCS
        let len = buf.get(tfi_pos - 2).copied().unwrap_or(0) as usize;
        let data_start = tfi_pos + 2;
        let data_end = (data_start + (len.saturating_sub(2))).min(buf.len());
        let data = buf[data_start..data_end].to_vec();

        Ok(Response { command: cmd, data })
    }

    /// GetFirmwareVersion (0x02) → (ic, ver, rev, support)
    pub fn get_firmware_version(&mut self) -> Result<(u8, u8, u8, u8), String> {
        let r = self.send(&[0x02])?;
        if r.data.len() >= 4 {
            Ok((r.data[0], r.data[1], r.data[2], r.data[3]))
        } else {
            Err(format!("firmware: réponse courte {:?}", r.data))
        }
    }

    /// SAMConfiguration (0x14) mode normal, timeout 1 s
    pub fn sam_configuration(&mut self) -> Result<(), String> {
        self.send(&[0x14, 0x01, 0x14, 0x01])?;
        Ok(())
    }

    /// InListPassiveTarget (0x4A) — 1 cible, 106 kbps type A.
    /// Retourne l'UID de la carte (ou None si aucune carte).
    pub fn in_list_passive_target(&mut self) -> Result<Option<Vec<u8>>, String> {
        let r = self.send(&[0x4A, 0x01, 0x00])?;
        // r.data[0] = nb cibles (1), r.data[1] = longueur UID, r.data[2..] = UID
        if r.data.is_empty() || r.data[0] != 1 {
            return Ok(None);
        }
        let uid_len = r.data[1] as usize;
        if r.data.len() >= 2 + uid_len {
            Ok(Some(r.data[2..2 + uid_len].to_vec()))
        } else {
            Ok(None)
        }
    }

    /// InDataExchange (0x40) — échange de données avec la cible.
    pub fn in_data_exchange(&mut self, data: &[u8]) -> Result<Vec<u8>, String> {
        let mut cmd = vec![0x40, 0x01]; // Tg=1
        cmd.extend_from_slice(data);
        let r = self.send(&cmd)?;
        if r.data.is_empty() {
            return Err("InDataExchange: réponse vide".into());
        }
        let status = r.data[0];
        if status != 0x00 {
            return Err(format!("InDataExchange status 0x{:02X}", status));
        }
        Ok(r.data[1..].to_vec())
    }

    /// Lit un bloc mémoire MIFARE Ultralight / NTAG (commande READ 0x30).
    pub fn read_block(&mut self, block: u8) -> Result<[u8; 4], String> {
        let r = self.in_data_exchange(&[0x30, block])?;
        let mut out = [0u8; 4];
        if r.len() >= 4 {
            out.copy_from_slice(&r[..4]);
        }
        Ok(out)
    }

    /// Lit 4 pages (16 octets) — READ NTAG/MIFARE Ultralight.
    pub fn read_pages(&mut self, block: u8) -> Result<[u8; 16], String> {
        let r = self.in_data_exchange(&[0x30, block])?;
        let mut out = [0u8; 16];
        let n = r.len().min(16);
        out[..n].copy_from_slice(&r[..n]);
        Ok(out)
    }

    /// Lit l'URI NDEF d'une carte NTAG (Bolt Card) → URL LNURL-withdraw.
    pub fn read_ndef_uri(&mut self) -> Result<String, String> {
        // NDEF commence à la page 4 (NTAG) ; on lit jusqu'à la page 28.
        let mut data = Vec::new();
        for block in (4u8..28).step_by(4) {
            match self.read_pages(block) {
                Ok(p) => data.extend_from_slice(&p),
                Err(_) => break,
            }
        }
        // Parse TLV : 0x03 = NDEF, 0xFE = terminator
        let mut i = 0usize;
        while i + 1 < data.len() {
            let t = data[i];
            if t == 0xFE {
                break;
            }
            let len = data[i + 1] as usize;
            if t == 0x03 {
                let start = i + 2;
                let end = (start + len).min(data.len());
                if let Some(uri) = parse_ndef_uri(&data[start..end]) {
                    return Ok(uri);
                }
            }
            i += 2 + len;
        }
        Err("TLV NDEF introuvable".into())
    }
}

/// Parse un message NDEF (record URI) → URI décodée
fn parse_ndef_uri(ndef: &[u8]) -> Option<String> {
    if ndef.len() < 5 {
        return None;
    }
    // ndef[0]=0xD1 (MB+ME+SR+TNF=1), ndef[1]=type len (=1), ndef[2]=payload len
    let type_len = ndef[1] as usize;
    let payload_len = ndef[2] as usize;
    let type_start = 3;
    let payload_start = type_start + type_len;
    if ndef.len() < payload_start + payload_len {
        return None;
    }
    if &ndef[type_start..payload_start] != b"U" {
        return None;
    }
    let payload = &ndef[payload_start..payload_start + payload_len];
    if payload.is_empty() {
        return None;
    }
    let prefix = URI_PREFIXES
        .get(payload[0] as usize)
        .copied()
        .unwrap_or("");
    let rest = String::from_utf8_lossy(&payload[1..]).to_string();
    Some(format!("{prefix}{rest}"))
}

/// Table des préfixes URI NFC Forum (0x00..0x23)
const URI_PREFIXES: [&str; 36] = [
    "",                     // 0x00
    "http://www.",          // 0x01
    "https://www.",         // 0x02
    "http://",              // 0x03
    "https://",             // 0x04
    "tel:",                 // 0x05
    "mailto:",              // 0x06
    "ftp://anonymous:anonymous@", // 0x07
    "ftp://ftp.",           // 0x08
    "ftps://",              // 0x09
    "sftp://",              // 0x0A
    "smb://",               // 0x0B
    "nfs://",               // 0x0C
    "ftp://",               // 0x0D
    "dav://",               // 0x0E
    "news:",                // 0x0F
    "telnet://",            // 0x10
    "imap:",                // 0x11
    "rtsp://",              // 0x12
    "urn:",                 // 0x13
    "pop:",                 // 0x14
    "sip:",                 // 0x15
    "sips:",                // 0x16
    "tftp:",                // 0x17
    "btspp://",             // 0x18
    "btl2cap://",           // 0x19
    "btgoep://",            // 0x1A
    "tcpobex://",           // 0x1B
    "irdaobex://",          // 0x1C
    "file://",              // 0x1D
    "urn:epc:id:",          // 0x1E
    "urn:epc:tag:",         // 0x1F
    "urn:epc:pat:",         // 0x20
    "urn:epc:raw:",         // 0x21
    "urn:epc:",             // 0x22
    "urn:nfc:",             // 0x23
];
