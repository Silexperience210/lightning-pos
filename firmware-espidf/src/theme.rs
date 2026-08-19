//! Thème visuel « Void » — noir absolu, surfaces quasi-noires, 3 accents.
//!
//! Toutes les valeurs sont en RGB565 (5 bits R, 6 bits G, 5 bits B). Le choix
//! des teintes évite les dégradés clairs (qui « bandent » en 565) : les
//! surfaces sont très sombres et proches, les accents très saturés. Les
//! transitions douces passent par `blend`/dithering, jamais par une rampe de
//! couleurs voisines.

// ─── Fond & surfaces ────────────────────────────────────────────────────────
/// Noir absolu (le panneau AMOLED-like du JC3248W535C rend un vrai noir).
pub const BG: u16 = 0x0000;
/// Surface d'un panneau / touche au repos — #101410.
pub const SURFACE: u16 = 0x10A2;
/// Surface d'une touche « secondaire » (opérateurs) — #212421.
pub const SURFACE_HI: u16 = 0x2124;
/// Filet 1 px de séparation / bordure de carte — #292C29.
pub const HAIRLINE: u16 = 0x2965;

// ─── Texte (neutres, ne comptent pas comme accents) ─────────────────────────
pub const TXT: u16 = 0xFFFF;
/// Texte secondaire — #9C9A9C.
pub const TXT_DIM: u16 = 0x9CD3;
/// Libellés, unités — #6B696B.
pub const TXT_MUTED: u16 = 0x6B4D;
/// Micro-libellés, filets de texte — #42454A.
pub const TXT_FAINT: u16 = 0x4229;

// ─── Accent 1 : ambre Bitcoin (marque, montants, éclair) ────────────────────
/// Orange Bitcoin exact — #F7931A → RGB565 0xFD20.
pub const AMBER: u16 = 0xFD20;
/// Or chaud, cœur de l'éclair — #FFBA00.
pub const GOLD: u16 = 0xFCE0;
/// Braise, halo lointain — #842800.
pub const EMBER: u16 = 0x8140;

// ─── Accent 2 : rose (danger / annulation) ──────────────────────────────────
/// Rouge doux — #F73C5A.
pub const ROSE: u16 = 0xF1EB;
/// Fond de bouton danger — #421018.
pub const ROSE_DEEP: u16 = 0x4083;

// ─── Accent 3 : menthe (validation NFC, usage ponctuel) ─────────────────────
/// Vert menthe — #4AD6AD.
pub const MINT: u16 = 0x4EB5;

// ─── Grille 8 px ────────────────────────────────────────────────────────────
/// Unité de grille.
pub const U: usize = 8;
/// Marge latérale de l'écran (2 U).
pub const PAD: usize = 16;
/// Hauteur de l'en-tête (6 U).
pub const HEADER_H: usize = 48;
pub const SCREEN_W: usize = 320;
pub const SCREEN_H: usize = 480;
