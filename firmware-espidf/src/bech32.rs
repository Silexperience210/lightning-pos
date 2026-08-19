//! Décodeur bech32 minimal (LUD-01) pour les chaînes `lnurl1…`.
//! Convertit un LNURL encodé en bech32 → URL https absolue.
//! Sans dépendance externe, ~70 lignes, vérifie le checksum bech32 (pas bech32m).

const CHARSET: &[u8; 32] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";
const GEN: [u32; 5] = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3];

fn polymod(values: &[u8]) -> u32 {
    let mut chk: u32 = 1;
    for &v in values {
        let b = chk >> 25;
        chk = ((chk & 0x01ff_ffff) << 5) ^ (v as u32);
        for i in 0..5 {
            if (b >> i) & 1 != 0 {
                chk ^= GEN[i];
            }
        }
    }
    chk
}

fn hrp_expand(hrp: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(hrp.len() * 2 + 1);
    for c in hrp.bytes() {
        out.push(c >> 5);
    }
    out.push(0);
    for c in hrp.bytes() {
        out.push(c & 31);
    }
    out
}

/// Décode une chaîne bech32 → (hrp, données 5 bits, checksum retiré).
pub fn decode(s: &str) -> Result<(String, Vec<u8>), String> {
    if s.len() < 8 || s.len() > 200 {
        return Err("bech32: longueur invalide".into());
    }
    let has_lower = s.bytes().any(|c| c.is_ascii_lowercase());
    let has_upper = s.bytes().any(|c| c.is_ascii_uppercase());
    if has_lower && has_upper {
        return Err("bech32: casse mixte interdite".into());
    }
    let sl = s.to_ascii_lowercase();
    let sep = sl.rfind('1').ok_or("bech32: séparateur '1' manquant")?;
    let hrp = &sl[..sep];
    let data_str = &sl[sep + 1..];
    if hrp.is_empty() || data_str.is_empty() {
        return Err("bech32: hrp ou data vide".into());
    }
    let mut data = Vec::with_capacity(data_str.len());
    for c in data_str.bytes() {
        let pos = CHARSET
            .iter()
            .position(|&x| x == c)
            .ok_or_else(|| format!("bech32: caractère invalide '{}'", c as char))?;
        data.push(pos as u8);
    }
    // BIP-173 : la partie data contient au minimum les 6 caractères de checksum.
    // Vérifié avant le checksum, sinon `data.len() - 6` peut déborder.
    if data.len() < 6 {
        return Err("bech32: partie data trop courte".into());
    }
    let mut check = hrp_expand(hrp);
    check.extend_from_slice(&data);
    if polymod(&check) != 1 {
        return Err("bech32: checksum invalide".into());
    }
    data.truncate(data.len() - 6);
    Ok((hrp.to_string(), data))
}

/// Regroupe des bits (5→8 avec pad=false pour LNURL ; 8→5 pad=true pour encoder).
pub fn convert_bits(data: &[u8], from: u32, to: u32, pad: bool) -> Result<Vec<u8>, String> {
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    let maxv = (1u32 << to) - 1;
    let max_acc = (1u32 << (from + to - 1)) - 1;
    let mut out = Vec::new();
    for &v in data {
        let value = v as u32;
        if (value >> from) != 0 {
            return Err("convert_bits: valeur hors bornes".into());
        }
        acc = ((acc << from) | value) & max_acc;
        bits += from;
        while bits >= to {
            bits -= to;
            out.push(((acc >> bits) & maxv) as u8);
        }
    }
    if pad {
        if bits > 0 {
            out.push(((acc << (to - bits)) & maxv) as u8);
        }
    } else if bits >= from || ((acc << (to - bits)) & maxv) != 0 {
        return Err("convert_bits: padding invalide".into());
    }
    Ok(out)
}

/// `lnurl1…` → URL https absolue. Vérifie le HRP « lnurl ».
pub fn decode_lnurl(s: &str) -> Result<String, String> {
    let (hrp, data) = decode(s)?;
    if hrp != "lnurl" {
        return Err(format!("bech32: hrp inattendu '{hrp}' (attendu 'lnurl')"));
    }
    let bytes = convert_bits(&data, 5, 8, false)?;
    String::from_utf8(bytes).map_err(|e| format!("bech32: utf8 invalide: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_lnurl(url: &str) -> String {
        let data5 = convert_bits(url.as_bytes(), 8, 5, true).unwrap();
        let hrp = "lnurl";
        let mut check = hrp_expand(hrp);
        check.extend_from_slice(&data5);
        check.extend_from_slice(&[0u8; 6]);
        let pm = polymod(&check) ^ 1;
        let mut out = String::from(hrp);
        out.push('1');
        for &d in &data5 {
            out.push(CHARSET[d as usize] as char);
        }
        for i in 0..6 {
            out.push(CHARSET[((pm >> (5 * (5 - i))) & 31) as usize] as char);
        }
        out
    }

    #[test]
    fn roundtrip() {
        let url = "https://lnbits.example.com/withdraw/api/v1/lnurl/abc123";
        let enc = encode_lnurl(url);
        assert!(enc.starts_with("lnurl1"));
        assert_eq!(decode_lnurl(&enc).unwrap(), url);
    }

    #[test]
    fn mixed_case_rejected() {
        let enc = encode_lnurl("https://example.com/x");
        // Casse réellement mixte : le reste est minuscule, le 1er caractère majuscule.
        let mut mixed = enc.to_ascii_lowercase();
        unsafe { mixed.as_bytes_mut()[0] = b'L' };
        assert!(decode_lnurl(&mixed).is_err());
    }

    #[test]
    fn uppercase_accepted() {
        // BIP-173 : une chaîne entièrement majuscule est valide.
        let url = "https://example.com/x";
        let enc = encode_lnurl(url);
        assert_eq!(decode_lnurl(&enc.to_ascii_uppercase()).unwrap(), url);
    }

    #[test]
    fn short_data_rejected() {
        // Partie data < 6 caractères : rejet explicite (pas d'underflow).
        let err = decode("aaaaa1qq").unwrap_err();
        assert!(err.contains("trop courte"), "erreur inattendue: {err}");
    }

    #[test]
    fn bad_checksum_rejected() {
        let enc = encode_lnurl("https://example.com/x");
        let mut bad = enc.into_bytes();
        let last = bad.len() - 1;
        bad[last] = if bad[last] == b'q' { b'p' } else { b'q' };
        assert!(decode_lnurl(&String::from_utf8(bad).unwrap()).is_err());
    }

    #[test]
    fn wrong_hrp_rejected() {
        let enc = encode_lnurl("https://example.com/x");
        let bad = enc.replacen("lnurl1", "bc1", 1);
        assert!(decode_lnurl(&bad).is_err());
    }

    #[test]
    fn known_vector() {
        // Vecteur LUD-01 de référence (bc1q… n'est pas lnurl, mais on vérifie le décodeur
        // générique sur un vecteur bech32 public : "a" encode -> hrp "a", data vide).
        let (hrp, data) = decode("a12uel5l").unwrap();
        assert_eq!(hrp, "a");
        assert!(data.is_empty());
    }
}
