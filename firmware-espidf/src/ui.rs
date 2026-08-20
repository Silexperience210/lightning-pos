//! Écrans du POS — direction « Void » (synthèse Data Pulse × Singularity).
//!
//! Règles de composition appliquées partout :
//! - grille de 8 px (marges 16, gouttières 8, hauteurs multiples de 8) ;
//! - fond noir absolu, surfaces quasi-noires, **3 accents** (ambre, rose, menthe) ;
//! - hiérarchie : micro-libellé capitales tracked-out → nombre héros vectoriel
//!   antialiasé → texte secondaire gris ;
//! - aucune particule / constellation / dégradé clair : la profondeur vient du
//!   halo de l'éclair et des filets 1 px.

use std::time::Duration;

use crate::bolt;
use crate::display::Display;
use crate::store::Product;
use crate::theme::*;

// ─── Métrique de l'écran menu (partagée avec le test tactile de main.rs) ─────
/// Hauteur de l'en-tête (marque + batterie).
pub const HEAD_H: usize = 44;
/// Début de la grille produits.
pub const GRID_Y: usize = 136;
pub const CELL_W: usize = 96;
pub const CELL_H: usize = 104;
pub const GAP: usize = 8;
/// Pastille CALC : x < CALC_X_MAX dans la bande HEAD_H..GRID_Y.
pub const CALC_X_MAX: usize = 112;
/// Pastille VIDER : x >= CLEAR_X_MIN dans la même bande.
pub const CLEAR_X_MIN: usize = 208;

/// Formate des centimes en « 12.50 » (partie entière + décimales).
pub fn fmt_eur(cents: u64) -> String {
    format!("{}.{:02}", cents / 100, cents % 100)
}

/// Groupe les milliers avec une espace fine : 31847 → « 31 847 ».
pub fn fmt_group(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// Montant héros : chiffres blancs + code devise ambre, l'ensemble centré.
/// La devise en accent (et non en blanc) hiérarchise sans ajouter de couleur.
pub fn amount_center(
    d: &mut Display,
    cents: u64,
    y: usize,
    size: f32,
    color: u16,
    alpha: u8,
    cur: &str,
) {
    let num = fmt_eur(cents);
    let tr = size * 0.04;
    let wn = crate::glyph::width(&num, size, tr);
    let we = crate::glyph::width(cur, size * 0.55, tr);
    let gap = (size * 0.12) as usize;
    let total = wn + gap + we;
    let x = SCREEN_W.saturating_sub(total) / 2;
    d.draw_num(&num, x, y, size, tr, color, alpha);
    d.draw_num(
        cur,
        x + wn + gap,
        y + (size * 0.30) as usize,
        size * 0.55,
        tr,
        AMBER,
        alpha,
    );
}

/// Micro-libellé capitales, tracked-out, centré.
pub fn label_center(d: &mut Display, s: &str, y: usize, color: u16) {
    d.text_center(s, y, 1, 2, color);
}

// ─── En-tête ────────────────────────────────────────────────────────────────

/// Pourcentage → icône batterie (contour 1 px + remplissage accentué).
pub fn battery(d: &mut Display, pct: u8) {
    let (x, y, w, h) = (272usize, 16usize, 26usize, 12usize);
    d.stroke_round_rect_aa(x, y, w, h, 3.0, 1.0, TXT_FAINT);
    d.fill_round_rect_aa(x + w + 1, y + 4, 3, 4, 1.0, TXT_FAINT);
    let fill = (w - 6) * pct as usize / 100;
    if fill > 0 {
        let c = if pct <= 20 { ROSE } else { TXT_DIM };
        d.fill_round_rect_aa(x + 3, y + 3, fill, h - 6, 1.0, c);
    }
    let s = format!("{pct}%");
    let sw = Display::text_tracked_width(&s, 1, 1);
    d.draw_text_tracked(&s, x.saturating_sub(sw + 8), y + 3, 1, 1, TXT_MUTED);
}

/// Icône WiFi : 4 barres selon le RSSI (>= -50 : 4, >= -60 : 3, >= -70 : 2, sinon 1).
/// En x=210 : laisser la place au % de la batterie (étiquette droite du bloc
/// batterie, qui commence vers x=234 — un x=238 chevauchait l'étiquette).
pub fn wifi(d: &mut Display, rssi: i8) {
    let (x, y) = (210usize, 16usize);
    let bars = if rssi >= -50 {
        4
    } else if rssi >= -60 {
        3
    } else if rssi >= -70 {
        2
    } else {
        1
    };
    let c_active = TXT_DIM;
    let c_off = TXT_FAINT;
    for i in 0..4usize {
        let h = 3 + 2 * i;
        let on = i < bars;
        d.fill_round_rect_aa(
            x + i * 4,
            y + (11 - h),
            3,
            h,
            1.0,
            if on { c_active } else { c_off },
        );
    }
}

/// Marque : éclair miniature + « LIGHTNING », filet 1 px sous l'en-tête.
pub fn header(d: &mut Display, batt_pct: u8, rssi: i8) {
    bolt::draw(d.fb_mut(), SCREEN_W, SCREEN_H, 21.0, 22.0, 22.0, 0.5, 255);
    d.draw_text_tracked("LIGHTNING", 34, 18, 1, 2, TXT_DIM);
    wifi(d, rssi);
    battery(d, batt_pct);
    d.hline(PAD, HEAD_H - 1, SCREEN_W - 2 * PAD, HAIRLINE);
}

// ─── Écran de démarrage ─────────────────────────────────────────────────────

/// Séquence de boot : l'éclair se charge, la marque monte en fondu, un filet
/// de progression se remplit. ~1,1 s.
pub fn splash(d: &mut Display) {
    let steps = 14;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        d.clear(BG);
        // Le halo « respire » vers son plein régime, le cœur monte en opacité.
        bolt::draw(
            d.fb_mut(),
            SCREEN_W,
            SCREEN_H,
            160.0,
            176.0,
            150.0,
            0.15 + 0.85 * t,
            (60.0 + 195.0 * t) as u8,
        );
        let a = (t * 255.0) as u8;
        if a > 0 {
            d.text_center("LIGHTNING POS", 300, 3, 6, blend565_txt(a));
            d.text_center("TERMINAL LIGHTNING", 336, 1, 3, blend565_muted(a));
        }
        // Filet de progression : 96 px, 1 px d'épaisseur.
        d.fill_rect(112, 376, 96, 1, HAIRLINE);
        d.fill_rect(112, 376, (96.0 * t) as usize, 1, AMBER);
        d.flush();
        std::thread::sleep(Duration::from_millis(18));
    }
    std::thread::sleep(Duration::from_millis(260));
}

/// Blanc atténué (fondu du texte sur fond noir) — évite d'allouer un blend.
fn blend565_txt(a: u8) -> u16 {
    crate::display::blend565(BG, TXT, a)
}
fn blend565_muted(a: u8) -> u16 {
    crate::display::blend565(BG, TXT_MUTED, a)
}

// ─── Écran menu (panier + grille produits) ──────────────────────────────────

/// Pastille d'action (CALC / VIDER) : surface + contour 1 px + capitales.
fn pill(d: &mut Display, x: usize, y: usize, w: usize, h: usize, s: &str, fg: u16, border: u16) {
    let r = h as f32 / 2.0;
    d.fill_round_rect_aa(x, y, w, h, r, SURFACE);
    d.stroke_round_rect_aa(x, y, w, h, r, 1.0, border);
    let tw = Display::text_tracked_width(s, 1, 2);
    d.draw_text_tracked(s, x + w.saturating_sub(tw) / 2, y + (h - 7) / 2, 1, 2, fg);
}

/// Écran principal : total du panier, actions, grille produits paginée.
pub fn menu(
    d: &mut Display,
    products: &[Product],
    page: usize,
    total_cents: u64,
    items: u32,
    batt_pct: u8,
    rssi: i8,
    cur: &str,
) {
    d.clear(BG);
    header(d, batt_pct, rssi);

    // Bloc panier : montant héros + compteur d'articles entre les 2 pastilles.
    amount_center(d, total_cents, 56, 44.0, TXT, 255, cur);
    pill(d, PAD - 8, 100, 96, 32, "CALC", TXT_DIM, HAIRLINE);
    pill(d, CLEAR_X_MIN + 8, 100, 96, 32, "VIDER", ROSE, ROSE_DEEP);
    let cnt = if items > 1 {
        format!("{items} ARTICLES")
    } else {
        format!("{items} ARTICLE")
    };
    label_center(d, &cnt, 112, TXT_MUTED);

    // Grille 3×3 : 8 produits + navigation, ou 7 produits + navigation + PAYER.
    let base = page * 8;
    for slot in 0..9usize {
        let x = GAP + (slot % 3) * (CELL_W + GAP);
        let y = GRID_Y + (slot / 3) * (CELL_H + GAP);
        let last = if page == 0 { 8 } else { 7 };
        if slot < last {
            let p = &products[base + slot];
            d.fill_round_rect_aa(x, y, CELL_W, CELL_H, 14.0, SURFACE);
            d.stroke_round_rect_aa(x, y, CELL_W, CELL_H, 14.0, 1.0, HAIRLINE);
            let scale = if p.name.chars().count() <= 7 { 2 } else { 1 };
            let nw = Display::text_tracked_width(&p.name, scale, 1);
            d.draw_text_tracked(
                &p.name,
                x + CELL_W.saturating_sub(nw) / 2,
                y + if scale == 2 { 27 } else { 31 },
                scale,
                1,
                TXT,
            );
            // Prix en chiffres vectoriels : c'est le nombre qu'on lit, pas le nom.
            let price = fmt_eur(p.cents as u64);
            let pw = crate::glyph::width(&price, 20.0, 1.0);
            d.draw_num(
                &price,
                x + CELL_W.saturating_sub(pw) / 2,
                y + 58,
                20.0,
                1.0,
                AMBER,
                255,
            );
        } else if slot == last {
            // Navigation de page.
            d.fill_round_rect_aa(x, y, CELL_W, CELL_H, 14.0, BG);
            d.stroke_round_rect_aa(x, y, CELL_W, CELL_H, 14.0, 1.0, HAIRLINE);
            let (arrow, lbl) = if page == 0 {
                (">", "SUITE")
            } else {
                ("<", "RETOUR")
            };
            let aw = Display::text_tracked_width(arrow, 3, 0);
            d.draw_text_tracked(
                arrow,
                x + CELL_W.saturating_sub(aw) / 2,
                y + 34,
                3,
                0,
                TXT_DIM,
            );
            let lw = Display::text_tracked_width(lbl, 1, 2);
            d.draw_text_tracked(
                lbl,
                x + CELL_W.saturating_sub(lw) / 2,
                y + 68,
                1,
                2,
                TXT_MUTED,
            );
        } else {
            // PAYER : seule surface pleine en accent de tout l'écran.
            d.fill_round_rect_aa(x, y, CELL_W, CELL_H, 14.0, AMBER);
            bolt::draw(
                d.fb_mut(),
                SCREEN_W,
                SCREEN_H,
                (x + CELL_W / 2) as f32,
                (y + 38) as f32,
                34.0,
                0.0,
                230,
            );
            let lw = Display::text_tracked_width("PAYER", 2, 2);
            d.draw_text_tracked("PAYER", x + CELL_W.saturating_sub(lw) / 2, y + 66, 2, 2, BG);
        }
    }
}

// ─── Écran calculatrice ─────────────────────────────────────────────────────

/// Géométrie du clavier (identique au test tactile de main.rs).
pub const CALC_Y: usize = 100;

pub fn calc(d: &mut Display, keys: &[[&str; 4]; 5], value: &str, pending: Option<(f64, char)>) {
    d.clear(BG);
    // Retour (zone tactile : y < 24, x < 90).
    d.fill_round_rect_aa(8, 4, 84, 22, 11.0, SURFACE);
    d.stroke_round_rect_aa(8, 4, 84, 22, 11.0, 1.0, HAIRLINE);
    d.draw_text_tracked("< RETOUR", 20, 12, 1, 1, TXT_DIM);

    if let Some((acc, op)) = pending {
        let s = format!("{acc:.2} {op}");
        d.text_center(&s, 36, 1, 1, TXT_MUTED);
    }
    // Valeur courante : chiffres vectoriels, centrés.
    d.num_center(value, 52, 40.0, 1.6, TXT, 255);
    d.hline(PAD, CALC_Y - 8, SCREEN_W - 2 * PAD, HAIRLINE);

    let kw = SCREEN_W / 4;
    let kh = (SCREEN_H - CALC_Y) / 5;
    for (r, row) in keys.iter().enumerate() {
        for (c, label) in row.iter().enumerate() {
            let x = c * kw + 4;
            let y = CALC_Y + r * kh + 4;
            let (bw, bh) = (kw - 8, kh - 8);
            let (fill, fg) = match *label {
                "OK" => (AMBER, BG),
                "C" => (ROSE_DEEP, ROSE),
                "+" | "-" | "x" | "/" | "%" | "=" => (SURFACE_HI, TXT),
                _ => (SURFACE, TXT),
            };
            d.fill_round_rect_aa(x, y, bw, bh, 14.0, fill);
            if fill == SURFACE {
                d.stroke_round_rect_aa(x, y, bw, bh, 14.0, 1.0, HAIRLINE);
            }
            // Les touches numériques utilisent la fonte vectorielle.
            if label.chars().all(|c| c.is_ascii_digit() || c == '.') {
                let w = crate::glyph::width(label, 26.0, 1.0);
                d.draw_num(
                    label,
                    x + bw.saturating_sub(w) / 2,
                    y + (bh - 26) / 2,
                    26.0,
                    1.0,
                    fg,
                    255,
                );
            } else {
                let w = Display::text_tracked_width(label, 2, 2);
                d.draw_text_tracked(
                    label,
                    x + bw.saturating_sub(w) / 2,
                    y + (bh - 14) / 2,
                    2,
                    2,
                    fg,
                );
            }
        }
    }
}

// ─── Écrans du flux de paiement ─────────────────────────────────────────────

/// Écran d'attente pendant la création de la facture.
pub fn preparing(d: &mut Display, total_cents: u64, sats: u64, cur: &str) {
    d.clear(BG);
    label_center(d, "CREATION DE LA FACTURE", 120, TXT_MUTED);
    amount_center(d, total_cents, 152, 48.0, TXT, 255, cur);
    label_center(d, &format!("{} SATS", fmt_group(sats)), 224, AMBER);
    d.fill_rect(112, 264, 96, 1, HAIRLINE);
    d.fill_rect(112, 264, 32, 1, AMBER);
}

/// Invite NFC au repos : pastille avec mini-carte + « TAPEZ LA CARTE ».
/// Zone y 378..410, au-dessus du bouton ANNULER (420..464).
pub fn nfc_hint(d: &mut Display) {
    d.fill_rect(0, 378, SCREEN_W, 32, BG);
    const PILL_X: usize = 58;
    const PILL_W: usize = 204;
    d.fill_round_rect_aa(PILL_X, 380, PILL_W, 28, 14.0, SURFACE);
    d.stroke_round_rect_aa(PILL_X, 380, PILL_W, 28, 14.0, 1.0, HAIRLINE);
    // Mini-carte NFC : corps + puce + 3 ondes croissantes.
    d.fill_round_rect_aa(74, 387, 22, 14, 3.0, TXT_DIM);
    d.fill_round_rect_aa(79, 392, 5, 4, 1.0, BG);
    d.fill_round_rect_aa(101, 396, 2, 4, 1.0, TXT_FAINT);
    d.fill_round_rect_aa(105, 394, 2, 8, 1.0, TXT_DIM);
    d.fill_round_rect_aa(109, 392, 2, 12, 1.0, TXT_DIM);
    d.draw_text_tracked("TAPEZ LA CARTE", 118, 389, 1, 2, TXT_DIM);
}

/// Éclair 3D en rotation + pastille animée « PAIEMENT EN COURS » pendant le
/// règlement Lightning. L'éclair 3D (temps réel) remplace la zone QR : il
/// tourne d'un pas par poll (step), le montant reste affiché en haut.
pub fn pay_loading(d: &mut Display, step: u32) {
    // éclair 3D (zone QR 88..356 effacée, montant conservé)
    d.fill_rect(0, 88, SCREEN_W, 290, BG);
    let ang = step as f32 * 0.22;
    let tilt = (step as f32 * 0.11).sin() * 0.12;
    crate::render3d::draw_bolt(d.fb_mut(), SCREEN_W, SCREEN_H, 160, 225, ang, tilt);
    // pastille PAIEMENT EN COURS (bande y 378..410)
    d.fill_rect(0, 378, SCREEN_W, 32, BG);
    d.fill_round_rect_aa(58, 380, 204, 28, 14.0, SURFACE);
    d.stroke_round_rect_aa(58, 380, 204, 28, 14.0, 1.0, HAIRLINE);
    d.draw_text_tracked("PAIEMENT EN COURS", 74, 389, 1, 1, TXT_DIM);
    let lit = (step % 3) as usize;
    for i in 0..3usize {
        let c = if i == lit { AMBER } else { TXT_FAINT };
        d.fill_round_rect_aa(204 + i * 10, 391, 6, 6, 3.0, c);
    }
}

/// Bande de statut NFC (bas de l'écran QR), rendue en pastille.
/// Zone réservée : y 378..410 — elle est effacée à chaque message pour éviter
/// la superposition des textes successifs. Renvoie l'instant d'affichage pour
/// que l'appelant puisse restaurer l'invite (`nfc_hint`) après un délai.
pub fn nfc_status(d: &mut Display, msg: &str, color: u16) -> std::time::Instant {
    d.fill_rect(0, 378, SCREEN_W, 32, BG);
    let tw = Display::text_tracked_width(msg, 1, 2);
    let w = tw + 32;
    let x = SCREEN_W.saturating_sub(w) / 2;
    d.fill_round_rect_aa(x, 382, w, 24, 12.0, SURFACE);
    d.stroke_round_rect_aa(x, 382, w, 24, 12.0, 1.0, HAIRLINE);
    d.draw_text_tracked(msg, x + 16, 390, 1, 2, color);
    std::time::Instant::now()
}

/// Écran QR + montant + bouton ANNULER.
///
/// La zone tactile d'annulation (70..250 × 420..464, cf. `cancel_pressed`) est
/// exactement celle du bouton dessiné ici : les deux doivent rester alignées.
pub fn pay_screen(
    d: &mut Display,
    n: usize,
    bits: &[bool],
    total_cents: u64,
    sats: u64,
    cur: &str,
    receipt: &str,
) {
    const QR_TOP: usize = 88;
    const QR_MAX: usize = 268;
    d.clear(BG);

    label_center(d, "PAIEMENT LIGHTNING", 16, TXT_MUTED);
    amount_center(d, total_cents, 32, 38.0, TXT, 255, cur);
    label_center(d, &format!("{} SATS", fmt_group(sats)), 76, AMBER);

    // Module aussi grand que possible dans la fenêtre disponible.
    let cell = ((SCREEN_W - 2 * PAD) / (n + 4))
        .min(QR_MAX / (n + 4))
        .clamp(1, 6);
    let total = (n + 4) * cell;
    let qx = (SCREEN_W - total) / 2;
    let qy = QR_TOP + (QR_MAX - total) / 2;
    // Plaque blanche arrondie : le QR « flotte » au lieu d'être un carré collé.
    d.fill_round_rect_aa(qx - 6, qy - 6, total + 12, total + 12, 10.0, TXT);
    d.draw_qr(n, bits, qx + 2 * cell, qy + 2 * cell, cell, BG, TXT);

    // Reçu compact du panier sous le QR (« BIERE x2, CAFE »).
    let rw = Display::text_tracked_width(receipt, 1, 1);
    if rw > 0 && rw <= SCREEN_W {
        d.draw_text_tracked(receipt, (SCREEN_W - rw) / 2, 360, 1, 1, TXT_MUTED);
    }

    // Invite NFC permanente (remplacée transitoirement par nfc_status).
    nfc_hint(d);

    d.fill_round_rect_aa(70, 420, 180, 44, 22.0, ROSE_DEEP);
    d.stroke_round_rect_aa(70, 420, 180, 44, 22.0, 1.0, ROSE);
    let w = Display::text_tracked_width("ANNULER", 2, 3);
    d.draw_text_tracked("ANNULER", 160 - w / 2, 439, 2, 3, ROSE);
}

/// Compte à rebours de la facture : pastille « N s » (haut droite) + barre
/// de progression ambre en haut de l'écran qui se vide avec la facture.
/// Redessine par-dessus la valeur précédente — appelé à chaque poll.
pub fn pay_countdown(d: &mut Display, secs: u64, total: u64) {
    let s = format!("{} s", secs);
    d.fill_round_rect_aa(232, 10, 80, 24, 12.0, BG);
    let w = Display::text_tracked_width(&s, 1, 2);
    d.draw_text_tracked(&s, 312 - w, 15, 1, 2, TXT_MUTED);
    // Barre ambre pleine au démarrage, qui se vide au fil de la facture.
    let frac = secs as f32 / total.max(1) as f32;
    d.fill_rect(0, 0, SCREEN_W, 6, BG);
    if frac > 0.02 {
        d.fill_rect(0, 0, (SCREEN_W as f32 * frac) as usize, 6, AMBER);
    }
}

/// Écran de fin non nominale (annulé / expiré / erreur).
/// `tone` = couleur d'accent, `mark` = glyphe affiché dans l'anneau.
pub fn status(d: &mut Display, mark: &str, title: &str, sub: &str, tone: u16) {
    d.clear(BG);
    d.ring_aa(160.0, 168.0, 44.0, 1.5, tone, 180);
    d.ring_aa(160.0, 168.0, 60.0, 1.0, tone, 45);
    let mw = Display::text_tracked_width(mark, 4, 0);
    d.draw_text_tracked(mark, 160 - mw / 2, 154, 4, 0, tone);
    d.text_center(title, 256, 3, 4, TXT);
    if !sub.is_empty() {
        label_center(d, sub, 300, TXT_MUTED);
    }
}

// ─── Transitions subtils entre écrans ───────────────────────────────────────

/// Transition d'écran : fondu d'arrivée rapide et UNIQUE (pas de fade-out).
/// L'ancien fade-out + fade-in doublait le rafraîchissement à chaque
/// changement de fenêtre et paraissait lent ; ici le nouvel écran apparaît en
/// ~36 ms, le fond noir ne s'affiche jamais seul.
pub fn crossfade(
    d: &mut Display,
    _from: impl Fn(&mut Display),
    to: impl Fn(&mut Display),
    ms: u64,
) {
    to(d);
    let steps = 3u64;
    let step = (ms / steps).max(8);
    for i in 1..=steps {
        let a = (255.0 - i as f32 / steps as f32 * 255.0) as u8;
        d.fill_rect_blend(0, 0, SCREEN_W, SCREEN_H, BG, a);
        d.flush();
        std::thread::sleep(Duration::from_millis(step));
    }
}

// ─── Séquence PAYÉ ──────────────────────────────────────────────────────────

/// Centre et hauteur de l'éclair de la séquence de succès.
/// Hauteur ≥ 380 px, centrée verticalement dans la partie haute.
const PAID_CX: f32 = 160.0;
const PAID_CY: f32 = 180.0;
const PAID_H: f32 = 384.0;

/// Jaunes dégradés du glow par silhouettes décalées (Direction Singularity).
const GLOW_INNER: u16 = 0xFFFF; // blanc, cœur
const GLOW_MID: u16 = 0xFFE0; // jaune vif
const GLOW_OUTER: u16 = 0xFCC0; // ambre

/// Dessine le bloc texte du succès (montant héros + confirmation).
fn paid_text(d: &mut Display, total_cents: u64, alpha: u8, cur: &str) {
    if alpha == 0 {
        return;
    }
    amount_center(d, total_cents, 378, 34.0, TXT, alpha, cur);
    let w = Display::text_tracked_width("PAYE · MERCI", 2, 4);
    d.draw_text_tracked(
        "PAYE · MERCI",
        160 - w / 2,
        422,
        2,
        4,
        crate::display::blend565(BG, MINT, alpha),
    );
}

/// Dessine l'éclair avec son glow par 3 silhouettes décalées.
fn paid_bolt(d: &mut Display, cx: f32, cy: f32, h: f32, breath: f32) {
    bolt::draw_solid(
        d.fb_mut(),
        SCREEN_W,
        SCREEN_H,
        cx - 3.0,
        cy + 3.0,
        h,
        GLOW_OUTER,
        (170.0 * breath) as u8,
    );
    bolt::draw_solid(
        d.fb_mut(),
        SCREEN_W,
        SCREEN_H,
        cx + 2.0,
        cy + 1.0,
        h,
        GLOW_MID,
        (210.0 * breath) as u8,
    );
    bolt::draw_solid(d.fb_mut(), SCREEN_W, SCREEN_H, cx, cy, h, GLOW_INNER, 255);
}

/// Particule de l'explosion PAYÉ (tableau fixe, pas d'allocation à chaud).
struct Particle {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    life: f32,
    white: bool,
}

/// Séquence de confirmation cosmique : rotations 3D seedées → flash blanc
/// 50 ms → onde de choc (3 anneaux) → bolt maître unique (seed = payment
/// hash) → explosion de particules → texte → respiration douce. ~1,9 s.
pub fn paid(d: &mut Display, total_cents: u64, cur: &str) {
    // Éclair 3D : 2 rotations rapides (visible aussi en paiement QR).
    let t0 = std::time::Instant::now();
    let mut ang = 0f32;
    while t0.elapsed() < std::time::Duration::from_millis(500) {
        d.fill_rect(0, 120, SCREEN_W, 240, BG);
        ang += 0.5;
        crate::render3d::draw_bolt(d.fb_mut(), SCREEN_W, SCREEN_H, 160, 240, ang, 0.0);
        d.flush();
        std::thread::sleep(std::time::Duration::from_millis(24));
    }

    // 1) Flash blanc 50 ms.
    d.clear(TXT);
    d.flush();
    std::thread::sleep(Duration::from_millis(50));

    // 2) Éclair + glow par silhouettes décalées.
    d.clear(BG);
    paid_bolt(d, PAID_CX, PAID_CY, PAID_H, 1.0);
    d.flush();
    std::thread::sleep(Duration::from_millis(60));

    // 3) Onde de validation : anneau fin qui s'ouvre (3 images).
    for i in 0..3 {
        let t = i as f32 / 2.0;
        d.clear(BG);
        paid_bolt(d, PAID_CX, PAID_CY, PAID_H, 1.0);
        d.ring_aa(
            PAID_CX,
            PAID_CY,
            60.0 + 140.0 * t,
            2.0,
            AMBER,
            (170.0 * (1.0 - t)) as u8,
        );
        d.flush();
        std::thread::sleep(Duration::from_millis(25));
    }

    // 4) Montée du texte en fondu (3 images).
    for i in 1..=3 {
        let a = (85 * i) as u8;
        d.clear(BG);
        paid_bolt(d, PAID_CX, PAID_CY, PAID_H, 1.0);
        paid_text(d, total_cents, a, cur);
        d.flush();
        std::thread::sleep(Duration::from_millis(25));
    }

    // 5) Respiration douce : les halos pulse (~0,5 s).
    for i in 0..6 {
        let phase = i as f32 / 6.0 * core::f32::consts::TAU;
        let breath = 0.80 + 0.20 * phase.cos();
        d.clear(BG);
        paid_bolt(d, PAID_CX, PAID_CY, PAID_H, breath);
        paid_text(d, total_cents, 255, cur);
        d.flush();
        std::thread::sleep(Duration::from_millis(70));
    }
    std::thread::sleep(Duration::from_millis(150));
}
