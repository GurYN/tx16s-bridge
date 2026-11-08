//! Colors of the app, kept in one place.

use gpui::{Hsla, rgb};

use crate::bridge::Tone;

pub const BACKGROUND: u32 = 0x14161a;
pub const PANEL: u32 = 0x1c1f25;
pub const FIELD: u32 = 0x101216;
pub const BORDER: u32 = 0x2c3039;
pub const TEXT: u32 = 0xe6e8eb;
pub const MUTED: u32 = 0x8b919b;
pub const ACCENT: u32 = 0x4f8cff;
pub const OK: u32 = 0x3ecf8e;
pub const WARNING: u32 = 0xf5a524;
pub const ERROR: u32 = 0xf06464;

pub fn text() -> Hsla {
    rgb(TEXT).into()
}

pub fn muted() -> Hsla {
    rgb(MUTED).into()
}

pub fn accent() -> Hsla {
    rgb(ACCENT).into()
}

pub fn tone(tone: Tone) -> u32 {
    match tone {
        Tone::Ok => OK,
        Tone::Warning => WARNING,
        Tone::Error => ERROR,
        Tone::Muted => MUTED,
        Tone::Accent => ACCENT,
    }
}
