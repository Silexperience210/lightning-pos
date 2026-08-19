//! Sprites pré-calculés de l'éclair — attract mode.
//!
//! `assets/bolt_sprites.bin` contient 36 frames de 128×128 px en RGB565
//! little-endian, ligne par ligne, sur fond noir.

pub const SPR_W: usize = 128;
pub const SPR_H: usize = 128;
pub const SPR_N: usize = 36;
pub const SPR_BYTES: usize = SPR_N * SPR_W * SPR_H * 2; // 1_179_648

pub static FRAMES: &[u8] = include_bytes!("../assets/bolt_sprites.bin");

/// Blitte la frame `idx` centrée en `(cx, cy)`. Les pixels noirs sont
/// transparents pour ne pas écraser un fond éventuel.
pub fn draw_frame(
    fb: &mut [u16],
    fbw: usize,
    _fbh: usize,
    cx: usize,
    cy: usize,
    idx: usize,
) {
    let idx = idx % SPR_N;
    let off = idx * SPR_W * SPR_H * 2;
    let half_w = SPR_W / 2;
    let half_h = SPR_H / 2;
    let x0 = cx.saturating_sub(half_w);
    let y0 = cy.saturating_sub(half_h);

    for sy in 0..SPR_H {
        let dy = y0 + sy;
        if dy >= 480 {
            continue;
        }
        let row = dy * fbw;
        for sx in 0..SPR_W {
            let dx = x0 + sx;
            if dx >= fbw {
                continue;
            }
            let i = off + (sy * SPR_W + sx) * 2;
            let lo = FRAMES[i] as u16;
            let hi = FRAMES[i + 1] as u16;
            let c = (hi << 8) | lo;
            if c != 0 {
                fb[row + dx] = c;
            }
        }
    }
}
