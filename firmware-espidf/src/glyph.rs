//! Police vectorielle antialiasée pour les CHIFFRES (montants € / sats).
//! Chaque chiffre = segments épais rasterisés par champ de distance (couverture)
//! → bords lissés. Les autres caractères (lettres, symboles) retombent sur le
//! bitmap 5×7 agrandi de `font`. Priorité : lisibilité des chiffres du montant.

use crate::font;

/// Largeur totale d'une chaîne : ~0.714×size par glyphe vectoriel (10/14 de la
/// grille) + tracking entre caractères. Doit MATCHER le rendu réel de draw()
/// (10×scale par glyphe), sinon le centrage et l'espacement dérivent.
pub fn width(s: &str, size: f32, tracking: f32) -> usize {
    if s.is_empty() {
        return 0;
    }
    let n = s.chars().count();
    ((n as f32 * size * (10.0 / 14.0) + (n as f32 - 1.0) * tracking).max(0.0) + 0.5) as usize
}

/// Rasterise la chaîne dans le framebuffer (coin haut-gauche `x`,`y`).
/// `size` = hauteur de capitale en px, `alpha` = opacité globale (0..255).
pub fn draw(
    fb: &mut [u16],
    w: usize,
    h: usize,
    s: &str,
    x: i32,
    y: i32,
    size: f32,
    tracking: f32,
    color: u16,
    alpha: u8,
) {
    if size <= 0.0 || fb.is_empty() || s.is_empty() {
        return;
    }
    let mut cx = x as f32;
    for c in s.chars() {
        cx += draw_char(fb, w, h, c, cx, y as f32, size, color, alpha) + tracking;
    }
}

/// Dessine un caractère, retourne sa largeur (px).
fn draw_char(
    fb: &mut [u16],
    w: usize,
    h: usize,
    c: char,
    x: f32,
    y: f32,
    size: f32,
    color: u16,
    alpha: u8,
) -> f32 {
    let scale = size / 14.0; // grille vectorielle 10×14 → size = hauteur capitale
    if let Some(segs) = digit_segments(c) {
        let t = (scale * 1.8).max(1.2); // épaisseur du trait
        let minx = x - t;
        let maxx = x + 10.0 * scale + t;
        let miny = y - t;
        let maxy = y + 14.0 * scale + t;
        let x0 = minx.floor().max(0.0) as usize;
        let x1 = (maxx.ceil() as usize).min(w);
        let y0 = miny.floor().max(0.0) as usize;
        let y1 = (maxy.ceil() as usize).min(h);
        for py in y0..y1 {
            for px in x0..x1 {
                let fx = px as f32 + 0.5;
                let fy = py as f32 + 0.5;
                let mut d = f32::MAX;
                for s in segs {
                    let ds = dist_seg(
                        fx,
                        fy,
                        x + s[0] * scale,
                        y + s[1] * scale,
                        x + s[2] * scale,
                        y + s[3] * scale,
                    );
                    if ds < d {
                        d = ds;
                    }
                }
                // Couverture : 1px de bord doux autour du trait.
                let cov = ((t * 0.5 + 0.5) - d).clamp(0.0, 1.0);
                if cov > 0.0 {
                    blend(fb, w, px, py, color, alpha, cov);
                }
            }
        }
        10.0 * scale
    } else {
        // Repli bitmap 5×7 agrandi (lettres, €, symboles) — sans anti-aliasing.
        let g = font::glyph(c);
        let s = (size / 7.0).max(0.5);
        for col in 0..5usize {
            let bits = g[col];
            for row in 0..7usize {
                if bits & (1 << row) == 0 {
                    continue;
                }
                let px0 = (x + col as f32 * s).round() as i32;
                let py0 = (y + row as f32 * s).round() as i32;
                let px1 = ((x + (col as f32 + 1.0) * s).round() as i32).max(px0 + 1);
                let py1 = ((y + (row as f32 + 1.0) * s).round() as i32).max(py0 + 1);
                for py in py0..py1 {
                    for px in px0..px1 {
                        if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) * w + (px as usize) < fb.len()
                        {
                            blend(fb, w, px as usize, py as usize, color, alpha, 1.0);
                        }
                    }
                }
            }
        }
        6.0 * s
    }
}

/// Distance point ↔ segment.
fn dist_seg(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = bx - ax;
    let dy = by - ay;
    let l2 = dx * dx + dy * dy;
    if l2 <= 0.0 {
        return ((px - ax).powi(2) + (py - ay).powi(2)).sqrt();
    }
    let t = (((px - ax) * dx + (py - ay) * dy) / l2).clamp(0.0, 1.0);
    let cx = ax + t * dx;
    let cy = ay + t * dy;
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

/// Mélange alpha RGB565.
fn blend(fb: &mut [u16], w: usize, px: usize, py: usize, color: u16, alpha: u8, cov: f32) {
    let idx = py * w + px;
    if idx >= fb.len() {
        return;
    }
    let a = (alpha as f32 / 255.0) * cov;
    if a >= 1.0 {
        fb[idx] = color;
    } else if a > 0.0 {
        let old = fb[idx];
        let or = (old >> 11) & 0x1F;
        let og = (old >> 5) & 0x3F;
        let ob = old & 0x1F;
        let nr = (color >> 11) & 0x1F;
        let ng = (color >> 5) & 0x3F;
        let nb = color & 0x1F;
        let r = (or as f32 + (nr as f32 - or as f32) * a).round() as u16;
        let g = (og as f32 + (ng as f32 - og as f32) * a).round() as u16;
        let b = (ob as f32 + (nb as f32 - ob as f32) * a).round() as u16;
        fb[idx] = (r << 11) | (g << 5) | b;
    }
}

/// Segments de chaque chiffre dans une grille 10×14
/// (A=top, B/C=right haut/bas, D=bottom, E/F=left bas/haut, G=mid).
fn digit_segments(c: char) -> Option<&'static [[f32; 4]]> {
    const A: [f32; 4] = [2.0, 1.0, 8.0, 1.0];
    const B: [f32; 4] = [8.0, 2.0, 8.0, 6.0];
    const C: [f32; 4] = [8.0, 8.0, 8.0, 12.0];
    const D: [f32; 4] = [2.0, 13.0, 8.0, 13.0];
    const E: [f32; 4] = [2.0, 8.0, 2.0, 12.0];
    const F: [f32; 4] = [2.0, 2.0, 2.0, 6.0];
    const G: [f32; 4] = [2.0, 7.0, 8.0, 7.0];
    // 1 = trait vertical + flag en haut (plus lisible qu'une barre droite).
    const V1: [f32; 4] = [5.0, 3.0, 5.0, 12.0];
    const FL1: [f32; 4] = [3.0, 2.0, 5.0, 4.0];
    const DOT: [f32; 4] = [3.0, 11.0, 7.0, 12.0];
    Some(match c {
        '0' => &[A, B, C, D, E, F],
        '1' => &[V1, FL1],
        '2' => &[A, B, G, E, D],
        '3' => &[A, B, G, C, D],
        '4' => &[F, G, B, C],
        '5' => &[A, F, G, C, D],
        '6' => &[A, F, G, E, C, D],
        '7' => &[A, B, C],
        '8' => &[A, B, C, D, E, F, G],
        '9' => &[A, B, C, D, F, G],
        '.' => &[DOT],
        ',' => &[DOT],
        _ => return None,
    })
}
