//! The hardcoded UI and buffer fonts. Families never change; sizes only change
//! through in-memory zoom, which resets on restart.

use std::sync::Arc;
use std::sync::LazyLock;

use gpui::App;
use gpui::Context;
use gpui::Font;
use gpui::FontFeatures;
use gpui::FontStyle;
use gpui::FontWeight;
use gpui::Global;
use gpui::Pixels;
use gpui::Subscription;
use gpui::Window;
use gpui::px;

pub const UI_FONT_SIZE: Pixels = px(16.0);
pub const BUFFER_FONT_SIZE: Pixels = px(15.0);

const MIN_FONT_SIZE: Pixels = px(6.0);
const MAX_FONT_SIZE: Pixels = px(100.0);

static UI_FONT: LazyLock<Font> = LazyLock::new(|| Font {
    family: ".SystemUIFont".into(),
    features: FontFeatures::disable_ligatures(),
    fallbacks: None,
    weight: FontWeight::NORMAL,
    style: FontStyle::Normal,
});

static BUFFER_FONT: LazyLock<Font> = LazyLock::new(|| Font {
    family: "JetBrainsMono Nerd Font".into(),
    features: FontFeatures(Arc::default()),
    fallbacks: None,
    weight: FontWeight::NORMAL,
    style: FontStyle::Normal,
});

/// The font used for UI elements.
pub fn ui_font() -> &'static Font {
    &UI_FONT
}

/// The font used for buffers and the terminal.
pub fn buffer_font() -> &'static Font {
    &BUFFER_FONT
}

struct UiFontSize(Pixels);

impl Global for UiFontSize {}

struct BufferFontSize(Pixels);

impl Global for BufferFontSize {}

/// The UI font size, including zoom.
pub fn ui_font_size(cx: &App) -> Pixels {
    cx.try_global::<UiFontSize>().map_or(UI_FONT_SIZE, |size| size.0)
}

/// The buffer font size, including zoom.
pub fn buffer_font_size(cx: &App) -> Pixels {
    cx.try_global::<BufferFontSize>().map_or(BUFFER_FONT_SIZE, |size| size.0)
}

pub fn adjust_ui_font_size(cx: &mut App, f: impl FnOnce(Pixels) -> Pixels) {
    let size = clamp_font_size(f(ui_font_size(cx)));
    cx.set_global(UiFontSize(size));
    cx.refresh_windows();
}

pub fn reset_ui_font_size(cx: &mut App) {
    if cx.has_global::<UiFontSize>() {
        cx.remove_global::<UiFontSize>();
        cx.refresh_windows();
    }
}

pub fn adjust_buffer_font_size(cx: &mut App, f: impl FnOnce(Pixels) -> Pixels) {
    let size = clamp_font_size(f(buffer_font_size(cx)));
    cx.set_global(BufferFontSize(size));
    cx.refresh_windows();
}

pub fn reset_buffer_font_size(cx: &mut App) {
    if cx.has_global::<BufferFontSize>() {
        cx.remove_global::<BufferFontSize>();
        cx.refresh_windows();
    }
}

pub fn observe_buffer_font_size_adjustment<V: 'static>(
    cx: &mut Context<V>,
    f: impl 'static + Fn(&mut V, &mut Context<V>),
) -> Subscription {
    cx.observe_global::<BufferFontSize>(f)
}

/// Shifts `size` by the current buffer zoom, so other fixed sizes zoom along with the buffer.
pub fn adjusted_font_size(size: Pixels, cx: &App) -> Pixels {
    shift_by_buffer_zoom(size, buffer_font_size(cx))
}

fn shift_by_buffer_zoom(size: Pixels, buffer_font_size: Pixels) -> Pixels {
    clamp_font_size(size + (buffer_font_size - BUFFER_FONT_SIZE))
}

/// Sets the window's rem size from the UI font size and returns the UI font.
pub fn setup_ui_font(window: &mut Window, cx: &App) -> Font {
    window.set_rem_size(ui_font_size(cx));
    ui_font().clone()
}

pub fn clamp_font_size(size: Pixels) -> Pixels {
    size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clamp_font_size() {
        assert_eq!(clamp_font_size(px(1.0)), MIN_FONT_SIZE);
        assert_eq!(clamp_font_size(px(15.0)), px(15.0));
        assert_eq!(clamp_font_size(px(500.0)), MAX_FONT_SIZE);
    }

    #[test]
    fn test_shift_by_buffer_zoom() {
        assert_eq!(shift_by_buffer_zoom(px(12.0), BUFFER_FONT_SIZE), px(12.0));
        assert_eq!(shift_by_buffer_zoom(px(12.0), BUFFER_FONT_SIZE + px(3.0)), px(15.0));
        assert_eq!(shift_by_buffer_zoom(px(12.0), BUFFER_FONT_SIZE - px(10.0)), MIN_FONT_SIZE);
    }

    #[test]
    fn test_fonts() {
        assert_eq!(ui_font().family.as_ref(), ".SystemUIFont");
        assert_eq!(ui_font().features.tag_value_list(), &[("calt".to_string(), 0)]);
        assert_eq!(buffer_font().family.as_ref(), "JetBrainsMono Nerd Font");
        assert!(buffer_font().features.tag_value_list().is_empty());
    }
}
