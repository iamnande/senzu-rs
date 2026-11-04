//! everforest dark soft. filled panels need a shade between the background
//! and the text, and the 16 ansi colors don't have one, so these are fixed.

use ratatui::style::{Color, Style};

pub const BG: Color = Color::Rgb(0x33, 0x3c, 0x43);
pub const PANEL: Color = Color::Rgb(0x3a, 0x46, 0x4c);
pub const RAISED: Color = Color::Rgb(0x43, 0x4f, 0x55);
pub const FG: Color = Color::Rgb(0xd3, 0xc6, 0xaa);
pub const MUTED: Color = Color::Rgb(0x85, 0x92, 0x89);
pub const FAINT: Color = Color::Rgb(0x5c, 0x6a, 0x72);
/// the one accent: everforest orange, same as the zellij bar
pub const ACCENT: Color = Color::Rgb(0xe6, 0x98, 0x75);
pub const GREEN: Color = Color::Rgb(0xa7, 0xc0, 0x80);
/// only for urgent, blocking and stale
pub const RED: Color = Color::Rgb(0xe6, 0x7e, 0x80);

pub fn fg(c: Color) -> Style {
    Style::new().fg(c)
}
