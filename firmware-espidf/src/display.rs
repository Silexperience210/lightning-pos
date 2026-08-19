//! Driver AXS15231B (JC3248W535C) sur ESP-IDF natif — reproduit EXACTEMENT
//! Arduino_ESP32QSPI.cpp (spi_device_polling_start/end, flags MULTILINE_CMD/ADDR,
//! MODE_QIO). Framebuffer RGB565 320×480 + helpers de dessin.

use esp_idf_hal::gpio::{Output, PinDriver};
use esp_idf_sys as sys;
use std::ptr;

const SCK: i32 = 47;
const D0: i32 = 21; // mosi / SIO0
const D1: i32 = 48; // miso / SIO1
const D2: i32 = 40; // quadwp / SIO2
const D3: i32 = 39; // quadhd / SIO3

/// Taille de paquet d'écriture QSPI : 16384 pixels (32 Ko) — 1024 pixels
/// multipliait les transactions SPI (~300/flush) et leur surcoût (~40-55 ms
/// de latence perçue sur chaque appui). 19 transactions suffisent désormais.
const MAX_PIXELS_AT_ONCE: usize = 16384;

/// Buffer de swap RGB565→QIO réutilisé (jamais alloué à chaud). Le display
/// n'est écrit que par la boucle principale (mono-thread) : accès `static
/// mut` sûr dans ce contexte, comme le reste du driver SPI.
static mut SWAP_BUF: [u32; MAX_PIXELS_AT_ONCE / 2 + 1] = [0; MAX_PIXELS_AT_ONCE / 2 + 1];
const FREQ_HZ: i32 = 32_000_000; // AXS15231B max = 32 MHz (80 MHz → pixels doublés/flous)

pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 480;

/// Matrice de Bayer 4×4 (0..15) — dithering ordonné.
///
/// En RGB565 une rampe douce vers le noir « bande » (32 niveaux de rouge/bleu).
/// Bruiter l'alpha d'un demi-cran avant la quantification casse les bandes sans
/// coûter un cycle de plus qu'un accès tableau.
pub const BAYER4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Mélange deux couleurs RGB565 (`a` = 0 → dst, 255 → src).
#[inline]
pub fn blend565(dst: u16, src: u16, a: u8) -> u16 {
    if a >= 255 {
        return src;
    }
    let a = a as u32;
    let ia = 255 - a;
    let (dr, dg, db) = (
        (dst >> 11) as u32 & 0x1F,
        (dst >> 5) as u32 & 0x3F,
        dst as u32 & 0x1F,
    );
    let (sr, sg, sb) = (
        (src >> 11) as u32 & 0x1F,
        (src >> 5) as u32 & 0x3F,
        src as u32 & 0x1F,
    );
    let r = (dr * ia + sr * a + 127) / 255;
    let g = (dg * ia + sg * a + 127) / 255;
    let b = (db * ia + sb * a + 127) / 255;
    ((r << 11) | (g << 5) | b) as u16
}

pub struct Display<'d> {
    qspi: Qspi<'d>,
    fb: Vec<u16>,
}

struct Qspi<'d> {
    handle: sys::spi_device_handle_t,
    cs: PinDriver<'d, Output>,
}

impl<'d> Qspi<'d> {
    fn new(cs: PinDriver<'d, Output>) -> Self {
        let mut buscfg: sys::spi_bus_config_t = Default::default();
        unsafe {
            buscfg.__bindgen_anon_1.mosi_io_num = D0;
            buscfg.__bindgen_anon_2.miso_io_num = D1;
            buscfg.__bindgen_anon_3.quadwp_io_num = D2;
            buscfg.__bindgen_anon_4.quadhd_io_num = D3;
        }
        buscfg.sclk_io_num = SCK;
        buscfg.data4_io_num = -1;
        buscfg.data5_io_num = -1;
        buscfg.data6_io_num = -1;
        buscfg.data7_io_num = -1;
        buscfg.max_transfer_sz = (MAX_PIXELS_AT_ONCE * 16 + 8) as i32;
        buscfg.flags = sys::SPICOMMON_BUSFLAG_MASTER | sys::SPICOMMON_BUSFLAG_GPIO_PINS;
        let ret = unsafe {
            sys::spi_bus_initialize(
                sys::spi_host_device_t_SPI2_HOST,
                &buscfg,
                sys::spi_common_dma_t_SPI_DMA_CH_AUTO,
            )
        };
        if ret != 0 {
            panic!("spi_bus_initialize failed: {}", ret);
        }

        let devcfg = sys::spi_device_interface_config_t {
            command_bits: 8,
            address_bits: 24,
            dummy_bits: 0,
            mode: 0,
            clock_source: sys::soc_periph_spi_clk_src_t_SPI_CLK_SRC_DEFAULT,
            clock_speed_hz: FREQ_HZ,
            spics_io_num: -1,
            flags: sys::SPI_DEVICE_HALFDUPLEX,
            queue_size: 1,
            ..Default::default()
        };
        let mut handle: sys::spi_device_handle_t = ptr::null_mut();
        let ret = unsafe {
            sys::spi_bus_add_device(sys::spi_host_device_t_SPI2_HOST, &devcfg, &mut handle)
        };
        if ret != 0 {
            panic!("spi_bus_add_device failed: {}", ret);
        }

        Self { handle, cs }
    }

    fn polling(&mut self, trans: &mut sys::spi_transaction_ext_t) {
        unsafe {
            let r = sys::spi_device_polling_start(self.handle, &mut trans.base, 0xffff_ffff);
            if r != 0 {
                panic!("polling_start: {}", r);
            }
            let r = sys::spi_device_polling_end(self.handle, 0xffff_ffff);
            if r != 0 {
                panic!("polling_end: {}", r);
            }
        }
    }

    fn write_command(&mut self, c: u8) {
        self.cs.set_low().unwrap();
        let mut trans: sys::spi_transaction_ext_t = unsafe { std::mem::zeroed() };
        trans.base.flags = sys::SPI_TRANS_MULTILINE_CMD | sys::SPI_TRANS_MULTILINE_ADDR;
        trans.base.cmd = 0x02;
        trans.base.addr = (c as u64) << 8;
        unsafe {
            trans.base.__bindgen_anon_1.tx_buffer = ptr::null();
        }
        trans.base.length = 0;
        self.polling(&mut trans);
        self.cs.set_high().unwrap();
    }

    fn write_c8d8(&mut self, c: u8, d: u8) {
        self.cs.set_low().unwrap();
        let mut trans: sys::spi_transaction_ext_t = unsafe { std::mem::zeroed() };
        trans.base.flags = sys::SPI_TRANS_USE_TXDATA
            | sys::SPI_TRANS_MULTILINE_CMD
            | sys::SPI_TRANS_MULTILINE_ADDR;
        trans.base.cmd = 0x02;
        trans.base.addr = (c as u64) << 8;
        unsafe {
            trans.base.__bindgen_anon_1.tx_data[0] = d;
        }
        trans.base.length = 8;
        self.polling(&mut trans);
        self.cs.set_high().unwrap();
    }

    fn write_c8_bytes(&mut self, c: u8, data: &[u8]) {
        self.cs.set_low().unwrap();
        let mut trans: sys::spi_transaction_ext_t = unsafe { std::mem::zeroed() };
        trans.base.flags = sys::SPI_TRANS_MULTILINE_CMD | sys::SPI_TRANS_MULTILINE_ADDR;
        trans.base.cmd = 0x02;
        trans.base.addr = (c as u64) << 8;
        unsafe {
            trans.base.__bindgen_anon_1.tx_buffer = data.as_ptr() as *const core::ffi::c_void;
        }
        trans.base.length = (data.len() * 8) as usize;
        self.polling(&mut trans);
        self.cs.set_high().unwrap();
    }

    fn write_pixels(&mut self, pixels: &[u16]) {
        self.cs.set_low().unwrap();
        let mut first_send = true;
        let mut off = 0usize;
        // Buffer statique : pas d'allocation heap à chaud (le display n'est
        // écrit que par la boucle principale, mono-thread).
        let buf: &mut [u32] = unsafe { &mut SWAP_BUF };
        while off < pixels.len() {
            let remaining = pixels.len() - off;
            let l = remaining.min(MAX_PIXELS_AT_ONCE);
            let words = (l + 1) / 2;
            for i in 0..words {
                let p1 = pixels[off + i * 2];
                let p2 = if i * 2 + 1 < l {
                    pixels[off + i * 2 + 1]
                } else {
                    p1
                };
                buf[i] = ((p2 as u32 & 0xff) << 24)
                    | ((p2 as u32 & 0xff00) << 8)
                    | ((p1 as u32 & 0xff00) >> 8)
                    | ((p1 as u32 & 0xff) << 8);
            }
            let mut trans: sys::spi_transaction_ext_t = unsafe { std::mem::zeroed() };
            if first_send {
                trans.base.flags = sys::SPI_TRANS_MODE_QIO;
                trans.base.cmd = 0x32;
                trans.base.addr = 0x002C00; // RAMWR : reset la position d'écriture
                first_send = false;
            } else {
                trans.base.flags = sys::SPI_TRANS_MODE_QIO
                    | sys::SPI_TRANS_VARIABLE_CMD
                    | sys::SPI_TRANS_VARIABLE_ADDR
                    | sys::SPI_TRANS_VARIABLE_DUMMY;
            }
            unsafe {
                trans.base.__bindgen_anon_1.tx_buffer = buf.as_ptr() as *const core::ffi::c_void;
            }
            trans.base.length = (l * 16) as usize;
            self.polling(&mut trans);
            off += l;
        }
        self.cs.set_high().unwrap();
    }
}

impl<'d> Display<'d> {
    /// `cs` = GPIO45, `backlight` = GPIO1 (déjà configurés en Output).
    pub fn new(cs: PinDriver<'d, Output>) -> Self {
        let qspi = Qspi::new(cs);
        let fb = vec![0u16; WIDTH * HEIGHT];
        Self { qspi, fb }
    }

    pub fn init(&mut self) {
        self.qspi.write_command(0x01); // SWRESET
        std::thread::sleep(std::time::Duration::from_millis(200));

        // Table v1.4.9 (BlockClock) — 0xA0 = 0xC0
        let init: &[(u8, &[u8], u32)] = &[
            (0xBB, &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x5A, 0xA5], 0),
            (
                0xA0,
                &[
                    0xC0, 0x10, 0x00, 0x02, 0x00, 0x00, 0x04, 0x3F, 0x20, 0x05, 0x3F, 0x3F, 0x00,
                    0x00, 0x00, 0x00, 0x00,
                ],
                0,
            ),
            (
                0xA2,
                &[
                    0x30, 0x3C, 0x24, 0x14, 0xD0, 0x20, 0xFF, 0xE0, 0x40, 0x19, 0x80, 0x80, 0x80,
                    0x20, 0xf9, 0x10, 0x02, 0xff, 0xff, 0xF0, 0x90, 0x01, 0x32, 0xA0, 0x91, 0xE0,
                    0x20, 0x7F, 0xFF, 0x00, 0x5A,
                ],
                0,
            ),
            (
                0xD0,
                &[
                    0xE0, 0x40, 0x51, 0x24, 0x08, 0x05, 0x10, 0x01, 0x20, 0x15, 0xC2, 0x42, 0x22,
                    0x22, 0xAA, 0x03, 0x10, 0x12, 0x60, 0x14, 0x1E, 0x51, 0x15, 0x00, 0x8A, 0x20,
                    0x00, 0x03, 0x3A, 0x12,
                ],
                0,
            ),
            (
                0xA3,
                &[
                    0xA0, 0x06, 0xAA, 0x00, 0x08, 0x02, 0x0A, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04,
                    0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x00, 0x55, 0x55,
                ],
                0,
            ),
            (
                0xC1,
                &[
                    0x31, 0x04, 0x02, 0x02, 0x71, 0x05, 0x24, 0x55, 0x02, 0x00, 0x41, 0x00, 0x53,
                    0xFF, 0xFF, 0xFF, 0x4F, 0x52, 0x00, 0x4F, 0x52, 0x00, 0x45, 0x3B, 0x0B, 0x02,
                    0x0d, 0x00, 0xFF, 0x40,
                ],
                0,
            ),
            (
                0xC3,
                &[
                    0x00, 0x00, 0x00, 0x50, 0x03, 0x00, 0x00, 0x00, 0x01, 0x80, 0x01,
                ],
                0,
            ),
            (
                0xC4,
                &[
                    0x00, 0x24, 0x33, 0x80, 0x00, 0xea, 0x64, 0x32, 0xC8, 0x64, 0xC8, 0x32, 0x90,
                    0x90, 0x11, 0x06, 0xDC, 0xFA, 0x00, 0x00, 0x80, 0xFE, 0x10, 0x10, 0x00, 0x0A,
                    0x0A, 0x44, 0x50,
                ],
                0,
            ),
            (
                0xC5,
                &[
                    0x18, 0x00, 0x00, 0x03, 0xFE, 0x3A, 0x4A, 0x20, 0x30, 0x10, 0x88, 0xDE, 0x0D,
                    0x08, 0x0F, 0x0F, 0x01, 0x3A, 0x4A, 0x20, 0x10, 0x10, 0x00,
                ],
                0,
            ),
            (
                0xC6,
                &[
                    0x05, 0x0A, 0x05, 0x0A, 0x00, 0xE0, 0x2E, 0x0B, 0x12, 0x22, 0x12, 0x22, 0x01,
                    0x03, 0x00, 0x3F, 0x6A, 0x18, 0xC8, 0x22,
                ],
                0,
            ),
            (
                0xC7,
                &[
                    0x50, 0x32, 0x28, 0x00, 0xa2, 0x80, 0x8f, 0x00, 0x80, 0xff, 0x07, 0x11, 0x9c,
                    0x67, 0xff, 0x24, 0x0c, 0x0d, 0x0e, 0x0f,
                ],
                0,
            ),
            (0xC9, &[0x33, 0x44, 0x44, 0x01], 0),
            (
                0xCF,
                &[
                    0x2C, 0x1E, 0x88, 0x58, 0x13, 0x18, 0x56, 0x18, 0x1E, 0x68, 0x88, 0x00, 0x65,
                    0x09, 0x22, 0xC4, 0x0C, 0x77, 0x22, 0x44, 0xAA, 0x55, 0x08, 0x08, 0x12, 0xA0,
                    0x08,
                ],
                0,
            ),
            (
                0xD5,
                &[
                    0x40, 0x8E, 0x8D, 0x01, 0x35, 0x04, 0x92, 0x74, 0x04, 0x92, 0x74, 0x04, 0x08,
                    0x6A, 0x04, 0x46, 0x03, 0x03, 0x03, 0x03, 0x82, 0x01, 0x03, 0x00, 0xE0, 0x51,
                    0xA1, 0x00, 0x00, 0x00,
                ],
                0,
            ),
            (
                0xD6,
                &[
                    0x10, 0x32, 0x54, 0x76, 0x98, 0xBA, 0xDC, 0xFE, 0x93, 0x00, 0x01, 0x83, 0x07,
                    0x07, 0x00, 0x07, 0x07, 0x00, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x00, 0x84,
                    0x00, 0x20, 0x01, 0x00,
                ],
                0,
            ),
            (
                0xD7,
                &[
                    0x03, 0x01, 0x0b, 0x09, 0x0f, 0x0d, 0x1E, 0x1F, 0x18, 0x1d, 0x1f, 0x19, 0x40,
                    0x8E, 0x04, 0x00, 0x20, 0xA0, 0x1F,
                ],
                0,
            ),
            (
                0xD8,
                &[
                    0x02, 0x00, 0x0a, 0x08, 0x0e, 0x0c, 0x1E, 0x1F, 0x18, 0x1d, 0x1f, 0x19,
                ],
                0,
            ),
            (
                0xD9,
                &[
                    0x1F, 0x1F, 0x1F, 0x1F, 0x1F, 0x1F, 0x1F, 0x1F, 0x1F, 0x1F, 0x1F, 0x1F,
                ],
                0,
            ),
            (
                0xDD,
                &[
                    0x1F, 0x1F, 0x1F, 0x1F, 0x1F, 0x1F, 0x1F, 0x1F, 0x1F, 0x1F, 0x1F, 0x1F,
                ],
                0,
            ),
            (0xDF, &[0x44, 0x73, 0x4B, 0x69, 0x00, 0x0A, 0x02, 0x90], 0),
            (
                0xE0,
                &[
                    0x3B, 0x28, 0x10, 0x16, 0x0c, 0x06, 0x11, 0x28, 0x5c, 0x21, 0x0D, 0x35, 0x13,
                    0x2C, 0x33, 0x28, 0x0D,
                ],
                0,
            ),
            (
                0xE1,
                &[
                    0x37, 0x28, 0x10, 0x16, 0x0b, 0x06, 0x11, 0x28, 0x5C, 0x21, 0x0D, 0x35, 0x14,
                    0x2C, 0x33, 0x28, 0x0F,
                ],
                0,
            ),
            (
                0xE2,
                &[
                    0x3B, 0x07, 0x12, 0x18, 0x0E, 0x0D, 0x17, 0x35, 0x44, 0x32, 0x0C, 0x14, 0x14,
                    0x36, 0x3A, 0x2F, 0x0D,
                ],
                0,
            ),
            (
                0xE3,
                &[
                    0x37, 0x07, 0x12, 0x18, 0x0E, 0x0D, 0x17, 0x35, 0x44, 0x32, 0x0C, 0x14, 0x14,
                    0x36, 0x32, 0x2F, 0x0F,
                ],
                0,
            ),
            (
                0xE4,
                &[
                    0x3B, 0x07, 0x12, 0x18, 0x0E, 0x0D, 0x17, 0x39, 0x44, 0x2E, 0x0C, 0x14, 0x14,
                    0x36, 0x3A, 0x2F, 0x0D,
                ],
                0,
            ),
            (
                0xE5,
                &[
                    0x37, 0x07, 0x12, 0x18, 0x0E, 0x0D, 0x17, 0x39, 0x44, 0x2E, 0x0C, 0x14, 0x14,
                    0x36, 0x3A, 0x2F, 0x0F,
                ],
                0,
            ),
            (
                0xA4,
                &[
                    0x85, 0x85, 0x95, 0x82, 0xAF, 0xAA, 0xAA, 0x80, 0x10, 0x30, 0x40, 0x40, 0x20,
                    0xFF, 0x60, 0x30,
                ],
                0,
            ),
            (0xA4, &[0x85, 0x85, 0x95, 0x85], 0),
            (0xBB, &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], 0),
            (0x11, &[0x00], 120),
            (0x29, &[0x00], 100),
            (0x2C, &[0x00, 0x00, 0x00, 0x00], 0),
        ];

        for &(cmd, data, delay_ms) in init {
            if data.len() == 1 {
                self.qspi.write_c8d8(cmd, data[0]);
            } else {
                self.qspi.write_c8_bytes(cmd, data);
            }
            if delay_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(delay_ms as u64));
            }
        }
        self.qspi.write_command(0x20); // INVOFF
        self.qspi.write_c8d8(0x36, 0x00); // MADCTL (RGB, pas de rotation)
        self.qspi.write_c8d8(0x3A, 0x55); // COLMOD = RGB565 16-bit (sinon pixels doublés)

        // Fenêtre complète 320×480
        self.qspi.write_c8_bytes(0x2a, &[0x00, 0x00, 0x01, 0x3F]); // CASET 0..319
        self.qspi.write_c8_bytes(0x2b, &[0x00, 0x00, 0x01, 0xDF]); // RASET 0..479
    }

    /// Remplit tout l'écran d'une couleur RGB565 (sans flusher).
    pub fn clear(&mut self, color: u16) {
        self.fb.fill(color);
    }

    /// Dessine un rectangle plein.
    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u16) {
        for yy in y..(y + h).min(HEIGHT) {
            for xx in x..(x + w).min(WIDTH) {
                self.fb[yy * WIDTH + xx] = color;
            }
        }
    }

    /// Dessine un pixel.
    pub fn set_pixel(&mut self, x: usize, y: usize, color: u16) {
        if x < WIDTH && y < HEIGHT {
            self.fb[y * WIDTH + x] = color;
        }
    }

    /// Envoie le framebuffer complet au panneau.
    pub fn flush(&mut self) {
        self.qspi.write_pixels(&self.fb);
    }

    /// Dessine un texte (police 5×7) à partir de (x, y).
    pub fn draw_text(&mut self, s: &str, x: usize, y: usize, color: u16) {
        let mut cx = x;
        for c in s.chars() {
            cx += crate::font::draw_char(&mut self.fb, WIDTH, c, cx, y, color);
        }
    }

    /// Dessine un texte à l'échelle `scale`.
    pub fn draw_text_scaled(&mut self, s: &str, x: usize, y: usize, scale: usize, color: u16) {
        let mut cx = x;
        for c in s.chars() {
            cx += crate::font::draw_char_scaled(&mut self.fb, WIDTH, c, cx, y, scale, color);
        }
    }

    /// Accès brut au framebuffer (rendus vectoriels : glyphes, éclair).
    pub fn fb_mut(&mut self) -> &mut [u16] {
        &mut self.fb
    }

    /// Pixel mélangé (`a` = 0..255).
    #[inline]
    pub fn blend_pixel(&mut self, x: usize, y: usize, color: u16, a: u8) {
        if x < WIDTH && y < HEIGHT {
            let i = y * WIDTH + x;
            self.fb[i] = blend565(self.fb[i], color, a);
        }
    }

    /// Rectangle plein translucide (voile, séparateurs doux).
    pub fn fill_rect_blend(&mut self, x: usize, y: usize, w: usize, h: usize, color: u16, a: u8) {
        for yy in y..(y + h).min(HEIGHT) {
            for xx in x..(x + w).min(WIDTH) {
                let i = yy * WIDTH + xx;
                self.fb[i] = blend565(self.fb[i], color, a);
            }
        }
    }

    /// Filet horizontal 1 px.
    pub fn hline(&mut self, x: usize, y: usize, w: usize, color: u16) {
        self.fill_rect(x, y, w, 1, color);
    }

    /// Distance signée à un rectangle à coins arrondis (négative à l'intérieur).
    #[inline]
    fn sd_round_rect(px: f32, py: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
        let qx = (px - cx).abs() - (hw - r);
        let qy = (py - cy).abs() - (hh - r);
        let mx = qx.max(0.0);
        let my = qy.max(0.0);
        (mx * mx + my * my).sqrt() + qx.max(qy).min(0.0) - r
    }

    /// Rectangle arrondi plein **antialiasé**.
    pub fn fill_round_rect_aa(
        &mut self,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        r: f32,
        color: u16,
    ) {
        self.round_rect_aa(x, y, w, h, r, color, 255, None)
    }

    /// Contour 1 px (ou plus) d'un rectangle arrondi, antialiasé.
    pub fn stroke_round_rect_aa(
        &mut self,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        r: f32,
        t: f32,
        color: u16,
    ) {
        self.round_rect_aa(x, y, w, h, r, color, 255, Some(t))
    }

    #[allow(clippy::too_many_arguments)]
    fn round_rect_aa(
        &mut self,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        r: f32,
        color: u16,
        alpha: u8,
        stroke: Option<f32>,
    ) {
        if w == 0 || h == 0 {
            return;
        }
        let (hw, hh) = (w as f32 / 2.0, h as f32 / 2.0);
        let r = r.min(hw).min(hh);
        let cx = x as f32 + hw;
        let cy = y as f32 + hh;
        let pad = stroke.unwrap_or(0.0).ceil() as usize + 1;
        let x0 = x.saturating_sub(pad);
        let y0 = y.saturating_sub(pad);
        for py in y0..(y + h + pad).min(HEIGHT) {
            for px in x0..(x + w + pad).min(WIDTH) {
                let d = Self::sd_round_rect(px as f32 + 0.5, py as f32 + 0.5, cx, cy, hw, hh, r);
                let cov = match stroke {
                    None => 0.5 - d,
                    Some(t) => 0.5 + t * 0.5 - (d + t * 0.5).abs(),
                };
                if cov <= 0.0 {
                    continue;
                }
                let a = (cov.min(1.0) * alpha as f32) as u8;
                if a == 0 {
                    continue;
                }
                let i = py * WIDTH + px;
                self.fb[i] = blend565(self.fb[i], color, a);
            }
        }
    }

    /// Anneau antialiasé d'épaisseur `t` (ondes de validation, jauges).
    pub fn ring_aa(&mut self, cx: f32, cy: f32, r: f32, t: f32, color: u16, alpha: u8) {
        if alpha == 0 || r <= 0.0 {
            return;
        }
        let outer = r + t * 0.5 + 1.0;
        let x0 = (cx - outer).max(0.0) as usize;
        let x1 = ((cx + outer) as usize + 1).min(WIDTH);
        let y0 = (cy - outer).max(0.0) as usize;
        let y1 = ((cy + outer) as usize + 1).min(HEIGHT);
        for py in y0..y1 {
            let dy = py as f32 + 0.5 - cy;
            for px in x0..x1 {
                let dx = px as f32 + 0.5 - cx;
                let d = (dx * dx + dy * dy).sqrt() - r;
                let cov = 0.5 + t * 0.5 - d.abs();
                if cov <= 0.0 {
                    continue;
                }
                let a = (cov.min(1.0) * alpha as f32) as u8;
                if a == 0 {
                    continue;
                }
                let i = py * WIDTH + px;
                self.fb[i] = blend565(self.fb[i], color, a);
            }
        }
    }

    /// Disque antialiasé.
    pub fn disc_aa(&mut self, cx: f32, cy: f32, r: f32, color: u16, alpha: u8) {
        if alpha == 0 || r <= 0.0 {
            return;
        }
        let x0 = (cx - r - 1.0).max(0.0) as usize;
        let x1 = ((cx + r + 1.0) as usize + 1).min(WIDTH);
        let y0 = (cy - r - 1.0).max(0.0) as usize;
        let y1 = ((cy + r + 1.0) as usize + 1).min(HEIGHT);
        for py in y0..y1 {
            let dy = py as f32 + 0.5 - cy;
            for px in x0..x1 {
                let dx = px as f32 + 0.5 - cx;
                let cov = 0.5 + r - (dx * dx + dy * dy).sqrt();
                if cov <= 0.0 {
                    continue;
                }
                let a = (cov.min(1.0) * alpha as f32) as u8;
                if a == 0 {
                    continue;
                }
                let i = py * WIDTH + px;
                self.fb[i] = blend565(self.fb[i], color, a);
            }
        }
    }

    /// Texte 5×7 avec interlettrage (`tracking` px entre glyphes) — les
    /// libellés capitales tracked-out sont la signature « Linear/Stripe ».
    pub fn draw_text_tracked(
        &mut self,
        s: &str,
        x: usize,
        y: usize,
        scale: usize,
        tracking: usize,
        color: u16,
    ) {
        let mut cx = x;
        for c in s.chars() {
            crate::font::draw_char_scaled(&mut self.fb, WIDTH, c, cx, y, scale, color);
            cx += 6 * scale + tracking;
        }
    }

    /// Largeur d'un texte tracked.
    pub fn text_tracked_width(s: &str, scale: usize, tracking: usize) -> usize {
        let n = s.chars().count();
        if n == 0 {
            0
        } else {
            n * (6 * scale + tracking) - tracking - scale
        }
    }

    /// Texte 5×7 tracked, centré horizontalement.
    pub fn text_center(&mut self, s: &str, y: usize, scale: usize, tracking: usize, color: u16) {
        let w = Self::text_tracked_width(s, scale, tracking);
        self.draw_text_tracked(s, WIDTH.saturating_sub(w) / 2, y, scale, tracking, color);
    }

    /// Chiffres vectoriels antialiasés (`size` = hauteur de capitale en px).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_num(
        &mut self,
        s: &str,
        x: usize,
        y: usize,
        size: f32,
        tracking: f32,
        color: u16,
        alpha: u8,
    ) {
        crate::glyph::draw(
            &mut self.fb,
            WIDTH,
            HEIGHT,
            s,
            x as i32,
            y as i32,
            size,
            tracking,
            color,
            alpha,
        );
    }

    /// Chiffres vectoriels centrés.
    pub fn num_center(
        &mut self,
        s: &str,
        y: usize,
        size: f32,
        tracking: f32,
        color: u16,
        alpha: u8,
    ) {
        let w = crate::glyph::width(s, size, tracking);
        self.draw_num(
            s,
            WIDTH.saturating_sub(w) / 2,
            y,
            size,
            tracking,
            color,
            alpha,
        );
    }

    /// Dessine un QR code (matrice booléenne) dans le framebuffer.
    pub fn draw_qr(
        &mut self,
        n: usize,
        bits: &[bool],
        x0: usize,
        y0: usize,
        cell: usize,
        fg: u16,
        bg: u16,
    ) {
        crate::qr::draw(&mut self.fb, WIDTH, HEIGHT, n, bits, x0, y0, cell, fg, bg);
    }

    pub fn fill_circle(&mut self, cx: usize, cy: usize, r: usize, color: u16) {
        let r2 = (r as isize) * (r as isize);
        for dy in 0..=r {
            for dx in 0..=r {
                let dx2 = (dx as isize) * (dx as isize);
                let dy2 = (dy as isize) * (dy as isize);
                if dx2 + dy2 <= r2 {
                    self.set_pixel(cx + dx, cy + dy, color);
                    if dx > 0 {
                        self.set_pixel(cx - dx, cy + dy, color);
                    }
                    if dy > 0 {
                        self.set_pixel(cx + dx, cy - dy, color);
                    }
                    if dx > 0 && dy > 0 {
                        self.set_pixel(cx - dx, cy - dy, color);
                    }
                }
            }
        }
    }

    /// Rectangle plein à coins arrondis (rayon `r`).
    pub fn fill_round_rect(
        &mut self,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        r: usize,
        color: u16,
    ) {
        if w == 0 || h == 0 {
            return;
        }
        let r = r.min(w / 2).min(h / 2);
        self.fill_rect(x + r, y, w - 2 * r, h, color);
        self.fill_rect(x, y + r, w, h - 2 * r, color);
        // 4 coins (quarts de cercle)
        for (cx, cy) in [
            (x + r - 1, y + r - 1),
            (x + w - r, y + r - 1),
            (x + r - 1, y + h - r),
            (x + w - r, y + h - r),
        ] {
            for dy in 0..r {
                for dx in 0..r {
                    let ddx = dx as isize - r as isize + 1;
                    let ddy = dy as isize - r as isize + 1;
                    if ddx * ddx + ddy * ddy <= (r as isize) * (r as isize) {
                        self.set_pixel(
                            (cx as isize + ddx) as usize,
                            (cy as isize + ddy) as usize,
                            color,
                        );
                    }
                }
            }
        }
    }

    /// Remplit un polygone (règle pair/impair, scanline). Coordonnées en i32.
    pub fn fill_polygon(&mut self, pts: &[(i32, i32)], color: u16) {
        if pts.len() < 3 {
            return;
        }
        let mut min_y = i32::MAX;
        let mut max_y = i32::MIN;
        for &(_, y) in pts {
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
        for y in min_y..=max_y {
            let mut xs: Vec<i32> = Vec::new();
            for i in 0..pts.len() {
                let (x0, y0) = pts[i];
                let (x1, y1) = pts[(i + 1) % pts.len()];
                if (y0 <= y && y1 > y) || (y1 <= y && y0 > y) {
                    let x = x0 + (y - y0) * (x1 - x0) / (y1 - y0);
                    xs.push(x);
                }
            }
            xs.sort_unstable();
            let mut i = 0;
            while i + 1 < xs.len() {
                let xa = xs[i].clamp(0, 319) as usize;
                let xb = xs[i + 1].clamp(0, 319) as usize;
                if xb > xa {
                    self.fill_rect(xa, y as usize, xb - xa, 1, color);
                }
                i += 2;
            }
        }
    }
}
