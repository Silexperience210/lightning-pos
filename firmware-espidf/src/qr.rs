//! QR code — génération via qrcodegen (Nayuki).
//!
//! Two helpers used by the POS firmware:
//! - [`encode`]: turn a text string into a QR matrix (ECC Medium).
//! - [`draw`]: render the matrix into a row-major RGB565 framebuffer,
//!   adding the 2-module quiet zone that `qrcodegen` does not include.

use qrcodegen::{QrCode, QrCodeEcc};

/// Encode `text` into a QR code at ECC level Medium.
///
/// Returns `Some((n, bits))` where `n` is the module size (width == height)
/// and `bits` holds `n * n` booleans in row-major order (`true` = dark module).
/// Returns `None` if the text is too long to fit (or on any encoding error).
pub fn encode(text: &str) -> Option<(usize, Vec<bool>)> {
    let qr = QrCode::encode_text(text, QrCodeEcc::Medium).ok()?;
    let n = qr.size() as usize;
    let mut bits = Vec::with_capacity(n * n);
    for y in 0..n {
        for x in 0..n {
            bits.push(qr.get_module(x as i32, y as i32));
        }
    }
    Some((n, bits))
}

/// Render a QR matrix into a row-major RGB565 framebuffer.
///
/// * `fb`   — framebuffer, row-major `[u16]`, `fb_w * fb_h` pixels.
/// * `fb_w` / `fb_h` — framebuffer width / height in pixels.
/// * `n`    — QR module size (width == height), `bits.len() == n * n`.
/// * `bits` — row-major module colors (`true` = dark).
/// * `x0` / `y0` — top-left pixel of the QR *data* (modules), not the quiet zone.
/// * `cell` — pixels per module.
/// * `fg` / `bg` — dark-module color / light (background) color.
///
/// A 2-module white quiet zone is added around the QR: the whole
/// `(n + 4) * cell` square is cleared to `bg` first, then the modules are
/// drawn offset by `2 * cell` from that square's top-left corner.
/// Every write is bounds-checked (`px < fb_w`, `py < fb_h`, `idx < fb.len()`).
pub fn draw(
    fb: &mut [u16],
    fb_w: usize,
    fb_h: usize,
    n: usize,
    bits: &[bool],
    x0: usize,
    y0: usize,
    cell: usize,
    fg: u16,
    bg: u16,
) {
    const QUIET: usize = 2; // modules of quiet zone on each side

    // Top-left pixel of the full quiet-zone square (signed so that a quiet zone
    // extending above/left of (0,0) is handled without usize underflow).
    let square = (n + 4) * cell;
    let qx = x0 as isize - (QUIET * cell) as isize;
    let qy = y0 as isize - (QUIET * cell) as isize;

    // 1) Clear the whole (n + 4) * cell square to the background color.
    for py in 0..square {
        let ay = qy + py as isize;
        if ay < 0 || ay >= fb_h as isize {
            continue;
        }
        for px in 0..square {
            let ax = qx + px as isize;
            if ax < 0 || ax >= fb_w as isize {
                continue;
            }
            let idx = ay as usize * fb_w + ax as usize;
            if idx < fb.len() {
                fb[idx] = bg;
            }
        }
    }

    // 2) Draw each module, offset by QUIET * cell from the square's top-left.
    for my in 0..n {
        for mx in 0..n {
            let color = if bits.get(my * n + mx).copied().unwrap_or(false) {
                fg
            } else {
                bg
            };
            let bx = qx + ((QUIET + mx) * cell) as isize;
            let by = qy + ((QUIET + my) * cell) as isize;
            for dy in 0..cell {
                let ay = by + dy as isize;
                if ay < 0 || ay >= fb_h as isize {
                    continue;
                }
                for dx in 0..cell {
                    let ax = bx + dx as isize;
                    if ax < 0 || ax >= fb_w as isize {
                        continue;
                    }
                    let idx = ay as usize * fb_w + ax as usize;
                    if idx < fb.len() {
                        fb[idx] = color;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_hello() {
        let (n, bits) = encode("Hello, world!").expect("should encode");
        assert_eq!(bits.len(), n * n);
        // Version 1 = 21 modules; a short string at Medium fits in 21..=25.
        assert!(n >= 21 && n <= 29);
        // The top-left finder pattern corner is always dark.
        assert!(bits[0]);
    }

    #[test]
    fn encode_too_long_returns_none() {
        let long = "A".repeat(3000);
        assert!(encode(&long).is_none());
    }

    #[test]
    fn draw_within_bounds() {
        let (n, bits) = encode("lnbc").expect("should encode");
        let fb_w = 320;
        let fb_h = 480;
        let mut fb = vec![0u16; fb_w * fb_h];
        // Place QR so its data starts at (0,0) — the 2-module quiet zone
        // extends above/left, which must be silently clipped.
        draw(&mut fb, fb_w, fb_h, n, &bits, 0, 0, 4, 0xFFFF, 0x0000);
        // Some dark pixels must have been drawn (the finder patterns).
        assert!(fb.iter().any(|&p| p == 0xFFFF));
    }

    #[test]
    fn draw_never_panics_on_small_buffer() {
        let (n, bits) = encode("test").expect("should encode");
        // A tiny buffer forces aggressive bounds-checking / clipping.
        let mut fb = vec![0u16; 10];
        draw(&mut fb, 5, 2, n, &bits, 2, 1, 3, 0xFFFF, 0x0000);
        // Should not have written outside the buffer.
        assert!(fb.len() == 10);
    }
}
