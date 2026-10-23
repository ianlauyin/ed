//! Text widths for a monospace font. Every glyph advances by a fixed fraction of the font size,
//! so widths need no font tables, canvas, or DOM, and match in every host.

/// Bold text, as a style bit.
pub(crate) const BOLD: u8 = 1;
/// Italic text, as a style bit.
pub(crate) const ITALIC: u8 = 2;
/// Monospace text, as a style bit. Every style measures as monospace.
pub(crate) const MONO: u8 = 4;

/// Advance of an ordinary glyph, in thousandths of an em.
const ADVANCE: u32 = 600;
/// Advance of a wide (East Asian or emoji) glyph, in thousandths of an em.
const WIDE: u32 = 1200;

/// The width of `text` at `size` pixels. `style` is accepted for parity with Pele's measurer and
/// does not change monospace widths.
pub(crate) fn width(text: &str, size: f64, _style: u8) -> f64 {
    let mut units: u32 = 0;
    for c in text.chars() {
        let c = u32::from(c);
        if c < 32 || is_zero_width(c) {
            continue;
        }
        units = units.saturating_add(if is_wide(c) { WIDE } else { ADVANCE });
    }
    f64::from(units) * size / 1000.0
}

// Pele tests UTF-16 units and counts a surrogate pair as one wide unit and one zero-width unit,
// so every supplementary character is wide here.
fn is_wide(c: u32) -> bool {
    (0x1100..=0x115f).contains(&c)
        || (0x2e80..=0xa4cf).contains(&c)
        || (0xac00..=0xd7a3).contains(&c)
        || (0xf900..=0xfaff).contains(&c)
        || (0xfe30..=0xfe4f).contains(&c)
        || (0xff00..=0xff60).contains(&c)
        || (0xffe0..=0xffe6).contains(&c)
        || c > 0xffff
}

fn is_zero_width(c: u32) -> bool {
    (0x300..=0x36f).contains(&c)
        || (0x200b..=0x200f).contains(&c)
        || (0xfe00..=0xfe0f).contains(&c)
        || c == 0xfeff
}
