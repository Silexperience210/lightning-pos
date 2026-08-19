//! Mini-renderer 3D logiciel : l'éclair LightningPoS en rotation temps réel.
//!
//! Modèle : prisme de la silhouette de `bolt.rs` (SHAPE, 6 points, hauteur
//! 100 em) extrudé sur z ∈ [-THICK/2, +THICK/2]. Projection perspective
//! douce, painter's algorithm (tri par profondeur), ombrage plat ambre.
//! Aucune allocation à chaud, tables fixes.

use crate::theme;

/// Silhouette (unités em, hauteur 100) — identique à `bolt::SHAPE`.
const SHAPE: [(f32, f32); 6] = [
    (7.0, -50.0),
    (-33.0, 2.0),
    (-1.0, 2.0),
    (-9.0, 50.0),
    (33.0, -6.0),
    (1.0, -6.0),
];

/// Épaisseur de l'extrusion (unités em).
const THICK: f32 = 16.0;
/// Distance focale de la projection.
const FOCAL: f32 = 260.0;
/// Mise à l'échelle em → pixels.
const SCALE: f32 = 1.15;
/// Direction de la lumière (normalisée à l'usage).
const LIGHT: [f32; 3] = [0.35, 0.5, 0.79];

const N: usize = 6; // sommets par cap
const NV: usize = 12; // 2 caps × 6

/// Dégradé blanc → or (17 paliers RGB565), indexé par fraction de hauteur.
const GRAD: [u16; 17] = grad_ramp();

const fn grad_ramp() -> [u16; 17] {
    let mut out = [0u16; 17];
    let mut i = 0;
    while i < 17 {
        let t = i as f32 / 16.0;
        let r = (255.0 - 1.0 * t) as u16;
        let g = (255.0 - 23.0 * t) as u16;
        let b = (255.0 - 127.0 * t) as u16;
        out[i] = ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3);
        i += 1;
    }
    out
}

/// Sommets du prisme après rotation Y (`ang`) + bascule Z (`tilt`).
/// Ordre : cap z=-T (0..5) puis cap z=+T (6..11).
fn bolt_verts(ang: f32, tilt: f32) -> [[f32; 3]; NV] {
    let (ca, sa) = (ang.cos(), ang.sin());
    let (ct, st) = (tilt.cos(), tilt.sin());
    let mut out = [[0.0f32; 3]; NV];
    let mut i = 0;
    while i < N {
        let (x, y) = (SHAPE[i].0 * SCALE, SHAPE[i].1 * SCALE);
        let mut k = 0;
        while k < 2 {
            let z = if k == 0 { -THICK / 2.0 } else { THICK / 2.0 };
            // rotation Y
            let (rx, rz) = (x * ca + z * sa, -x * sa + z * ca);
            // bascule Z (tilt léger)
            let (fx, fy) = (rx * ct - y * st, rx * st + y * ct);
            out[i + k * N] = [fx, fy, rz];
            k += 1;
        }
        i += 1;
    }
    out
}

/// Projection perspective : caméra à z=-FOCAL, regard vers +Z.
#[inline]
fn project(v: [f32; 3], cx: f32, cy: f32) -> (f32, f32) {
    let s = FOCAL / (FOCAL - v[2]);
    (cx + v[0] * s, cy + v[1] * s)
}

/// Test point-dans-polygone (even-odd).
#[inline]
fn inside(px: f32, py: f32, poly: &[(f32, f32)]) -> bool {
    let n = poly.len();
    let mut inside = false;
    let mut i = 0;
    while i < n {
        let (x1, y1) = poly[i];
        let (x2, y2) = poly[(i + 1) % n];
        if y1 != y2 && py >= y1.min(y2) && py < y1.max(y2) {
            let xint = x1 + (py - y1) * (x2 - x1) / (y2 - y1);
            if px < xint {
                inside = !inside;
            }
        }
        i += 1;
    }
    inside
}

/// Bbox projetée d'un polygone.
fn poly_bbox(poly: &[(f32, f32)]) -> (f32, f32, f32, f32) {
    let mut x0 = f32::MAX;
    let mut x1 = f32::MIN;
    let mut y0 = f32::MAX;
    let mut y1 = f32::MIN;
    for (x, y) in poly {
        x0 = x0.min(*x);
        x1 = x1.max(*x);
        y0 = y0.min(*y);
        y1 = y1.max(*y);
    }
    (x0, x1, y0, y1)
}

/// Remplit un polygone d'une couleur plate.
fn fill(fb: &mut [u16], fbw: usize, fbh: usize, poly: &[(f32, f32)], color: u16) {
    let (x0, x1, y0, y1) = poly_bbox(poly);
    let ix0 = (x0.max(0.0)) as usize;
    let ix1 = (x1.min(fbw as f32 - 1.0)) as usize;
    let iy0 = (y0.max(0.0)) as usize;
    let iy1 = (y1.min(fbh as f32 - 1.0)) as usize;
    let mut py = iy0;
    while py <= iy1 {
        let row = py * fbw;
        let mut px = ix0;
        while px <= ix1 {
            if inside(px as f32 + 0.5, py as f32 + 0.5, poly) {
                fb[row + px] = color;
            }
            px += 1;
        }
        py += 1;
    }
}

/// Remplit la face avant avec le dégradé blanc→or (le long de la hauteur).
fn fill_gradient(fb: &mut [u16], fbw: usize, fbh: usize, poly: &[(f32, f32)]) {
    let (x0, x1, y0, y1) = poly_bbox(poly);
    let span = (y1 - y0).max(1.0);
    let ix0 = (x0.max(0.0)) as usize;
    let ix1 = (x1.min(fbw as f32 - 1.0)) as usize;
    let iy0 = (y0.max(0.0)) as usize;
    let iy1 = (y1.min(fbh as f32 - 1.0)) as usize;
    let mut py = iy0;
    while py <= iy1 {
        // fraction le long de l'axe vertical projeté (top→bot)
        let t = ((py as f32) - y0) / span;
        let t = t.clamp(0.0, 1.0);
        let ci = ((t * 16.0) as usize).min(16);
        let c = GRAD[ci];
        let row = py * fbw;
        let mut px = ix0;
        while px <= ix1 {
            if inside(px as f32 + 0.5, py as f32 + 0.5, poly) {
                fb[row + px] = c;
            }
            px += 1;
        }
        py += 1;
    }
}

/// Rend l'éclair 3D centré en (cx, cy), hauteur ~115 px, rotation `ang`
/// autour de Y (radians). `tilt` : bascule Z supplémentaire (radians).
pub fn draw_bolt(
    fb: &mut [u16],
    fbw: usize,
    fbh: usize,
    cx: usize,
    cy: usize,
    ang: f32,
    tilt: f32,
) {
    let v = bolt_verts(ang, tilt);
    let (cx, cy) = (cx as f32, cy as f32);

    // --- faces : profondeur moyenne (ordre painter, du plus loin au plus près)
    // cap arrière (z=-T), cap avant (z=+T), 6 quads latéraux [i, j, 6+j, 6+i]
    let mut zs = [0.0f32; 8];
    // cap arrière
    zs[0] = (v[0][2] + v[1][2] + v[2][2] + v[3][2] + v[4][2] + v[5][2]) / 6.0;
    // cap avant
    zs[1] = (v[6][2] + v[7][2] + v[8][2] + v[9][2] + v[10][2] + v[11][2]) / 6.0;
    let mut qi = 0;
    while qi < N {
        let j = (qi + 1) % N;
        zs[2 + qi] = (v[qi][2] + v[j][2] + v[N + j][2] + v[N + qi][2]) / 4.0;
        qi += 1;
    }
    // tri par insertion (8 éléments)
    let mut ord = [0usize; 8];
    let mut oi = 0;
    while oi < 8 {
        ord[oi] = oi;
        oi += 1;
    }
    let mut a = 0;
    while a < 8 {
        let mut b = a + 1;
        while b < 8 {
            if zs[ord[b]] < zs[ord[a]] {
                ord.swap(a, b);
            }
            b += 1;
        }
        a += 1;
    }

    // lumière
    let ln = (LIGHT[0] * LIGHT[0] + LIGHT[1] * LIGHT[1] + LIGHT[2] * LIGHT[2]).sqrt();

    // --- rendu des faces dans l'ordre
    let mut fi = 0;
    while fi < 8 {
        let face = ord[fi];
        let mut poly = [(0.0f32, 0.0f32); 6];
        let mut pc = 0;
        if face == 0 {
            // cap arrière : ambre sombre
            for i in 0..N {
                poly[pc] = project(v[i], cx, cy);
                pc += 1;
            }
            fill(fb, fbw, fbh, &poly[..pc], theme::EMBER);
        } else if face == 1 {
            // cap avant : dégradé blanc→or
            for i in 0..N {
                poly[pc] = project(v[N + i], cx, cy);
                pc += 1;
            }
            fill_gradient(fb, fbw, fbh, &poly[..pc]);
        } else {
            // quad latéral : ambre ombré par la normale
            let q = face - 2;
            let j = (q + 1) % N;
            let (a, b, c2) = (v[q], v[j], v[N + j]);
            let ux = b[0] - a[0];
            let uy = b[1] - a[1];
            let uz = b[2] - a[2];
            let vx = c2[0] - a[0];
            let vy = c2[1] - a[1];
            let vz = c2[2] - a[2];
            let (nx, ny, nz) = (uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx);
            let nl = (nx * nx + ny * ny + nz * nz).sqrt().max(1e-6);
            let d = (nx / nl * LIGHT[0] + ny / nl * LIGHT[1] + nz / nl * LIGHT[2]) / ln;
            let sh = 0.22 + 0.78 * d.max(0.0);
            let r = (255.0 * sh) as u16;
            let g = (168.0 * sh) as u16;
            let b2 = (40.0 * sh) as u16;
            let col = ((r >> 3) << 11) | ((g >> 2) << 5) | (b2 >> 3);
            let (p0, p1, p2, p3) = (
                project(v[q], cx, cy),
                project(v[j], cx, cy),
                project(v[N + j], cx, cy),
                project(v[N + q], cx, cy),
            );
            let quad = [(p0.0, p0.1), (p1.0, p1.1), (p2.0, p2.1), (p3.0, p3.1)];
            fill(fb, fbw, fbh, &quad, col);
        }
        fi += 1;
    }
}
