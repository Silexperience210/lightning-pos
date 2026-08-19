//! Éclair Lightning : silhouette vectorielle + halo, rendus par champ de
//! distance signée.
//!
//! L'ancien éclair était un `fill_polygon` de 52 px de large aux bords en
//! escalier. Ici la silhouette est décrite une seule fois en unités « em »
//! (hauteur 100, pointe haute et pointe basse effilées) puis rastérisée à la
//! taille voulue :
//!
//! - **bords antialiasés** : la couverture vient de la distance signée ;
//! - **halo réel** : la même distance sert de rampe (or → ambre → braise) avec
//!   une décroissance cubique, donc pas de « couches » visibles ;
//! - **anti-banding** : l'alpha du halo est dithered (Bayer 4×4) avant la
//!   quantification RGB565.

use crate::display::{blend565, BAYER4};
use crate::theme;

/// Silhouette centrée sur (0,0), hauteur 100 unités, largeur 66.
/// Ordre : pointe haute → épaule gauche → cran → pointe basse → épaule droite
/// → cran. Les deux pointes sont des angles aigus (« effilées »).
pub const SHAPE: [(f32, f32); 6] = [
    (7.0, -50.0),
    (-33.0, 2.0),
    (-1.0, 2.0),
    (-9.0, 50.0),
    (33.0, -6.0),
    (1.0, -6.0),
];

/// Demi-largeur / demi-hauteur de la boîte englobante, en unités em.
const HALF_W: f32 = 33.0;
const HALF_H: f32 = 50.0;

/// Rayon du halo, en fraction de la hauteur de l'éclair.
const GLOW_R: f32 = 0.22;

/// Blanc du cœur en haut de l'éclair.
const CORE_TOP: u16 = 0xFFFF;
/// Or du cœur en bas (dégradé vertical, dithered).
const CORE_BOT: u16 = 0xFE80;

/// Rend l'éclair centré en (cx, cy) pour une hauteur `h` px.
///
/// - `glow` : gain du halo (0 = éteint, 1 = plein).
/// - `core` : opacité de la silhouette (0..255) — permet de la faire monter.
pub fn draw(fb: &mut [u16], fbw: usize, fbh: usize, cx: f32, cy: f32, h: f32, glow: f32, core: u8) {
    let k = h / 100.0;
    let r_glow = (GLOW_R * h).max(1.0);
    let mut p = [(0.0f32, 0.0f32); 6];
    for i in 0..6 {
        p[i] = (cx + SHAPE[i].0 * k, cy + SHAPE[i].1 * k);
    }
    let (bw, bh) = (HALF_W * k, HALF_H * k);
    let x0 = (cx - bw - r_glow).max(0.0) as usize;
    let x1 = (((cx + bw + r_glow) as usize) + 1).min(fbw);
    let y0 = (cy - bh - r_glow).max(0.0) as usize;
    let y1 = (((cy + bh + r_glow) as usize) + 1).min(fbh);
    let r2 = r_glow * r_glow;
    let top = cy - bh;

    for py in y0..y1 {
        let fy = py as f32 + 0.5;
        let by = (fy - cy).abs() - bh; // distance verticale à la boîte
        let row = py * fbw;
        let bayer = BAYER4[py & 3];
        for px in x0..x1 {
            let fx = px as f32 + 0.5;
            // Rejet rapide : la distance à la boîte englobante minore la
            // distance au polygone.
            let bx = (fx - cx).abs() - bw;
            if bx > 0.0 || by > 0.0 {
                let ex = bx.max(0.0);
                let ey = by.max(0.0);
                if ex * ex + ey * ey >= r2 {
                    continue;
                }
            }

            // Distance non signée au contour + test d'appartenance (pair/impair).
            let mut d2 = f32::MAX;
            let mut inside = false;
            for i in 0..6 {
                let (ax, ay) = p[i];
                let (bx2, by2) = p[(i + 1) % 6];
                let (ex, ey) = (bx2 - ax, by2 - ay);
                let l2 = ex * ex + ey * ey;
                let t = (((fx - ax) * ex + (fy - ay) * ey) / l2).clamp(0.0, 1.0);
                let (qx, qy) = (fx - (ax + t * ex), fy - (ay + t * ey));
                let dd = qx * qx + qy * qy;
                if dd < d2 {
                    d2 = dd;
                }
                if (ay > fy) != (by2 > fy) && fx < ax + (fy - ay) * ex / ey {
                    inside = !inside;
                }
            }
            let sd = if inside { -d2.sqrt() } else { d2.sqrt() };

            // 1) Halo : décroissance cubique, teinte or → ambre → braise.
            if glow > 0.0 && sd < r_glow {
                let t = (1.0 - (sd.max(0.0) / r_glow)).clamp(0.0, 1.0);
                let g = t * t * t * glow;
                let hue = if t > 0.5 {
                    blend565(theme::AMBER, theme::GOLD, ((t - 0.5) * 510.0) as u8)
                } else {
                    blend565(theme::EMBER, theme::AMBER, (t * 510.0) as u8)
                };
                // Dithering ordonné : casse les anneaux de quantification 565.
                let a = (g * 255.0) as i32 + (bayer[px & 3] as i32 - 8) / 2;
                if a > 0 {
                    let i = row + px;
                    fb[i] = blend565(fb[i], hue, a.min(255) as u8);
                }
            }

            // 2) Cœur : couverture antialiasée + dégradé vertical blanc → or.
            if core > 0 && sd < 0.5 {
                let cov = (0.5 - sd).clamp(0.0, 1.0);
                let a = (cov * core as f32) as u8;
                if a > 0 {
                    let g = (((fy - top) / h).clamp(0.0, 1.0) * 255.0) as u8;
                    let c = blend565(CORE_TOP, CORE_BOT, g);
                    let i = row + px;
                    fb[i] = blend565(fb[i], c, a);
                }
            }
        }
    }
}

/// Silhouette pleine de l'éclair, sans halo, dans une couleur unie.
/// Utilisé pour l'effet « glow par silhouettes décalées » de la séquence PAYÉ :
/// on superpose 2-3 copies légèrement décalées en blanc / jaune / ambre.
pub fn draw_solid(
    fb: &mut [u16],
    fbw: usize,
    fbh: usize,
    cx: f32,
    cy: f32,
    h: f32,
    color: u16,
    alpha: u8,
) {
    if alpha == 0 || h <= 0.0 {
        return;
    }
    let k = h / 100.0;
    let mut p = [(0.0f32, 0.0f32); 6];
    for i in 0..6 {
        p[i] = (cx + SHAPE[i].0 * k, cy + SHAPE[i].1 * k);
    }
    let (bw, bh) = (HALF_W * k, HALF_H * k);
    let x0 = (cx - bw).max(0.0) as usize;
    let x1 = (((cx + bw) as usize) + 1).min(fbw);
    let y0 = (cy - bh).max(0.0) as usize;
    let y1 = (((cy + bh) as usize) + 1).min(fbh);

    for py in y0..y1 {
        let fy = py as f32 + 0.5;
        let row = py * fbw;
        for px in x0..x1 {
            let fx = px as f32 + 0.5;
            let mut d2 = f32::MAX;
            let mut inside = false;
            for i in 0..6 {
                let (ax, ay) = p[i];
                let (bx2, by2) = p[(i + 1) % 6];
                let (ex, ey) = (bx2 - ax, by2 - ay);
                let l2 = ex * ex + ey * ey;
                if l2 <= 0.0 {
                    continue;
                }
                let t = (((fx - ax) * ex + (fy - ay) * ey) / l2).clamp(0.0, 1.0);
                let (qx, qy) = (fx - (ax + t * ex), fy - (ay + t * ey));
                let dd = qx * qx + qy * qy;
                if dd < d2 {
                    d2 = dd;
                }
                if (ay > fy) != (by2 > fy) && fx < ax + (fy - ay) * ex / ey {
                    inside = !inside;
                }
            }
            let sd = if inside { -d2.sqrt() } else { d2.sqrt() };
            let cov = (0.5 - sd).clamp(0.0, 1.0);
            if cov > 0.0 {
                let a = (cov * alpha as f32) as u8;
                if a > 0 {
                    let i = row + px;
                    fb[i] = blend565(fb[i], color, a);
                }
            }
        }
    }
}
