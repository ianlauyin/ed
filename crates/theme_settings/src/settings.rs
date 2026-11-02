use gpui::App;
use gpui::Context;
use gpui::Font;
use gpui::FontFallbacks;
use gpui::FontStyle;
use gpui::Global;
use gpui::Pixels;
use gpui::SharedString;
use gpui::Subscription;
use gpui::Window;
use gpui::px;
pub use settings::FontFamilyName;
use settings::IntoGpui;
use settings::Settings;
use theme::Theme;

const MIN_FONT_SIZE: Pixels = px(6.0);
const MAX_FONT_SIZE: Pixels = px(100.0);
const MIN_LINE_HEIGHT: f32 = 1.0;

/// Customizable settings for the UI and theme system.
#[derive(Clone, PartialEq)]
pub struct ThemeSettings {
    /// The UI font size. Determines the size of text in the UI,
    /// as well as the size of a [gpui::Rems] unit.
    ///
    /// Changing this will impact the size of all UI elements.
    ui_font_size: Pixels,
    /// The font used for UI elements.
    pub ui_font: Font,
    /// The font size used for buffers, and the terminal.
    ///
    /// The terminal font size can be overridden using it's own setting.
    buffer_font_size: Pixels,
    /// The font used for buffers, and the terminal.
    ///
    /// The terminal font family can be overridden using it's own setting.
    pub buffer_font: Font,
    git_commit_buffer_font_size: Option<Pixels>,
    /// The font family used for Mermaid diagrams.
    /// Falls back to the UI font family if unset.
    mermaid_font_family: Option<SharedString>,
    /// The line height for buffers, and the terminal.
    ///
    /// Changing this may affect the spacing of some UI elements.
    ///
    /// The terminal font family can be overridden using it's own setting.
    pub buffer_line_height: BufferLineHeight,
    /// The amount of fading applied to unnecessary code.
    pub unnecessary_code_fade: f32,
}

settings::register_setting!(ThemeSettings);

#[derive(Default)]
struct BufferFontSize(Pixels);

impl Global for BufferFontSize {}

#[derive(Default)]
pub(crate) struct UiFontSize(Pixels);

impl Global for UiFontSize {}

#[derive(Default)]
pub struct GitCommitBufferFontSize(Pixels);

impl Global for GitCommitBufferFontSize {}

/// In-memory override for the markdown preview font size.
#[derive(Default)]
pub struct MarkdownPreviewFontSize(Pixels);

impl Global for MarkdownPreviewFontSize {}

pub use theme::BufferLineHeight;

pub const fn buffer_line_height_from_settings(value: settings::BufferLineHeight) -> BufferLineHeight {
    match value {
        settings::BufferLineHeight::Comfortable => BufferLineHeight::Comfortable,
        settings::BufferLineHeight::Standard => BufferLineHeight::Standard,
        settings::BufferLineHeight::Custom(line_height) => BufferLineHeight::Custom(line_height),
    }
}

impl ThemeSettings {
    /// Returns the buffer font size.
    pub fn buffer_font_size(&self, cx: &App) -> Pixels {
        let font_size = cx.try_global::<BufferFontSize>().map(|size| size.0).unwrap_or(self.buffer_font_size);
        clamp_font_size(font_size)
    }

    /// Returns the UI font size.
    pub fn ui_font_size(&self, cx: &App) -> Pixels {
        let font_size = cx.try_global::<UiFontSize>().map(|size| size.0).unwrap_or(self.ui_font_size);
        clamp_font_size(font_size)
    }

    pub fn git_commit_buffer_font_size(&self, cx: &App) -> Pixels {
        cx.try_global::<GitCommitBufferFontSize>()
            .map(|size| size.0)
            .or(self.git_commit_buffer_font_size)
            .map(clamp_font_size)
            .unwrap_or_else(|| self.buffer_font_size(cx))
    }

    /// Returns the font family to use for Mermaid diagrams, falling back to
    /// the buffer font family when unset: diagrams are laid out for a
    /// monospace font.
    pub fn mermaid_font_family(&self) -> &SharedString {
        self.mermaid_font_family.as_ref().unwrap_or(&self.buffer_font.family)
    }

    /// Returns the markdown preview font size: the configured UI font size, unless zoomed.
    ///
    /// Note: this deliberately uses `self.ui_font_size` instead of `ui_font_size(cx)`,
    /// so that temporary UI zoom does not also resize the markdown preview.
    pub fn markdown_preview_font_size(&self, cx: &App) -> Pixels {
        clamp_font_size(cx.try_global::<MarkdownPreviewFontSize>().map_or(self.ui_font_size, |size| size.0))
    }

    /// Returns the buffer font size, read from the settings.
    ///
    /// The real buffer font size is stored in-memory, to support temporary font size changes.
    /// Use [`Self::buffer_font_size`] to get the real font size.
    pub const fn buffer_font_size_settings(&self) -> Pixels {
        self.buffer_font_size
    }

    /// Returns the UI font size, read from the settings.
    ///
    /// The real UI font size is stored in-memory, to support temporary font size changes.
    /// Use [`Self::ui_font_size`] to get the real font size.
    pub const fn ui_font_size_settings(&self) -> Pixels {
        self.ui_font_size
    }

    pub const fn git_commit_buffer_font_size_settings(&self) -> Option<Pixels> {
        self.git_commit_buffer_font_size
    }

    /// Returns the buffer's line height.
    pub const fn line_height(&self) -> f32 {
        f32::max(self.buffer_line_height.value(), MIN_LINE_HEIGHT)
    }
}

/// Floating-card look: everything around the editor and terminal cards (gaps, title bar,
/// status bar, side docks) is one opaque chrome colour, the theme's panel background.
pub(crate) fn apply_floating_chrome(theme: &mut Theme) {
    /// How far the editor / terminal content sits from the chrome, in HSL lightness.
    const CONTENT_CONTRAST: f32 = 0.05;

    let is_light = theme.appearance.is_light();
    let colors = &mut theme.styles.colors;
    let chrome = colors.panel_background;
    colors.background = chrome;
    colors.title_bar_background = chrome;
    colors.title_bar_inactive_background = chrome;
    colors.status_bar_background = chrome;

    // Content is darker than the chrome in dark themes and lighter in light themes. Only ever
    // add contrast: themes that already have more are left alone.
    let shift = if is_light {
        ((chrome.l + CONTENT_CONTRAST).min(1.) - colors.editor_background.l).max(0.)
    } else {
        ((chrome.l - CONTENT_CONTRAST).max(0.) - colors.editor_background.l).min(0.)
    };
    let mut surfaces = vec![
        &mut colors.editor_background,
        &mut colors.editor_gutter_background,
        &mut colors.editor_subheader_background,
        &mut colors.toolbar_background,
        &mut colors.terminal_background,
        &mut colors.terminal_ansi_background,
    ];
    // Translucent active-line overlays already follow the background.
    if colors.editor_active_line_background.a >= 1. {
        surfaces.push(&mut colors.editor_active_line_background);
    }
    for color in surfaces {
        color.l = (color.l + shift).clamp(0., 1.);
    }

    // Tab bars sit on the chrome: inactive tabs blend into it, the active tab takes the card
    // colour and joins the card below it.
    colors.tab_bar_background = chrome;
    colors.tab_inactive_background = chrome;
    colors.tab_active_background = colors.editor_background;
}

/// Observe changes to the adjusted buffer font size.
pub fn observe_buffer_font_size_adjustment<V: 'static>(
    cx: &mut Context<V>,
    f: impl 'static + Fn(&mut V, &mut Context<V>),
) -> Subscription {
    cx.observe_global::<BufferFontSize>(f)
}

/// Gets the font size, adjusted by the difference between the current buffer font size and the one set in the settings.
pub fn adjusted_font_size(size: Pixels, cx: &App) -> Pixels {
    let adjusted_font_size = if let Some(BufferFontSize(adjusted_size)) = cx.try_global::<BufferFontSize>() {
        let buffer_font_size = ThemeSettings::get_global(cx).buffer_font_size;
        let delta = *adjusted_size - buffer_font_size;
        size + delta
    } else {
        size
    };
    clamp_font_size(adjusted_font_size)
}

/// Adjusts the buffer font size, without persisting the result in the settings.
/// This will be effective until the app is restarted.
pub fn adjust_buffer_font_size(cx: &mut App, f: impl FnOnce(Pixels) -> Pixels) {
    let buffer_font_size = ThemeSettings::get_global(cx).buffer_font_size;
    let adjusted_size = cx.try_global::<BufferFontSize>().map_or(buffer_font_size, |adjusted_size| adjusted_size.0);
    cx.set_global(BufferFontSize(clamp_font_size(f(adjusted_size))));
    cx.refresh_windows();
}

/// Resets the buffer font size to the default value.
pub fn reset_buffer_font_size(cx: &mut App) {
    if cx.has_global::<BufferFontSize>() {
        cx.remove_global::<BufferFontSize>();
        cx.refresh_windows();
    }
}

#[allow(missing_docs)]
pub fn setup_ui_font(window: &mut Window, cx: &mut App) -> gpui::Font {
    let (ui_font, ui_font_size) = {
        let theme_settings = ThemeSettings::get_global(cx);
        let font = theme_settings.ui_font.clone();
        (font, theme_settings.ui_font_size(cx))
    };

    window.set_rem_size(ui_font_size);
    ui_font
}

/// Sets the adjusted UI font size.
pub fn adjust_ui_font_size(cx: &mut App, f: impl FnOnce(Pixels) -> Pixels) {
    let ui_font_size = ThemeSettings::get_global(cx).ui_font_size(cx);
    let adjusted_size = cx.try_global::<UiFontSize>().map_or(ui_font_size, |adjusted_size| adjusted_size.0);
    cx.set_global(UiFontSize(clamp_font_size(f(adjusted_size))));
    cx.refresh_windows();
}

/// Resets the UI font size to the default value.
pub fn reset_ui_font_size(cx: &mut App) {
    if cx.has_global::<UiFontSize>() {
        cx.remove_global::<UiFontSize>();
        cx.refresh_windows();
    }
}

pub fn adjust_git_commit_buffer_font_size(cx: &mut App, f: impl FnOnce(Pixels) -> Pixels) {
    let git_commit_buffer_font_size = ThemeSettings::get_global(cx).git_commit_buffer_font_size(cx);
    let adjusted_size =
        cx.try_global::<GitCommitBufferFontSize>().map_or(git_commit_buffer_font_size, |adjusted_size| adjusted_size.0);
    cx.set_global(GitCommitBufferFontSize(clamp_font_size(f(adjusted_size))));
    cx.refresh_windows();
}

pub fn reset_git_commit_buffer_font_size(cx: &mut App) {
    if cx.has_global::<GitCommitBufferFontSize>() {
        cx.remove_global::<GitCommitBufferFontSize>();
        cx.refresh_windows();
    }
}

/// Sets the adjusted font size of the markdown preview.
pub fn adjust_markdown_preview_font_size(cx: &mut App, f: impl FnOnce(Pixels) -> Pixels) {
    let markdown_preview_font_size = ThemeSettings::get_global(cx).markdown_preview_font_size(cx);
    let adjusted_size =
        cx.try_global::<MarkdownPreviewFontSize>().map_or(markdown_preview_font_size, |adjusted_size| adjusted_size.0);
    cx.set_global(MarkdownPreviewFontSize(clamp_font_size(f(adjusted_size))));
    cx.refresh_windows();
}

/// Resets the markdown preview font size to the default value.
pub fn reset_markdown_preview_font_size(cx: &mut App) {
    if cx.has_global::<MarkdownPreviewFontSize>() {
        cx.remove_global::<MarkdownPreviewFontSize>();
        cx.refresh_windows();
    }
}

/// Ensures font size is within the valid range.
pub fn clamp_font_size(size: Pixels) -> Pixels {
    size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)
}

fn font_fallbacks_from_settings(fallbacks: Option<Vec<settings::FontFamilyName>>) -> Option<FontFallbacks> {
    fallbacks.map(|fallbacks| {
        FontFallbacks::from_fonts(fallbacks.into_iter().map(|font_family| font_family.0.to_string()).collect())
    })
}

impl settings::Settings for ThemeSettings {
    fn from_settings(settings_content: &settings::SettingsContent) -> Self {
        let content = &settings_content.theme;
        Self {
            ui_font_size: clamp_font_size(content.ui_font_size.unwrap().into_gpui()),
            ui_font: Font {
                family: content.ui_font_family.as_ref().unwrap().0.clone().into(),
                features: content.ui_font_features.clone().unwrap().into_gpui(),
                fallbacks: font_fallbacks_from_settings(content.ui_font_fallbacks.clone()),
                weight: content.ui_font_weight.unwrap().into_gpui(),
                style: FontStyle::default(),
            },
            buffer_font: Font {
                family: content.buffer_font_family.as_ref().unwrap().0.clone().into(),
                features: content.buffer_font_features.clone().unwrap().into_gpui(),
                fallbacks: font_fallbacks_from_settings(content.buffer_font_fallbacks.clone()),
                weight: content.buffer_font_weight.unwrap().into_gpui(),
                style: FontStyle::default(),
            },
            buffer_font_size: clamp_font_size(content.buffer_font_size.unwrap().into_gpui()),
            buffer_line_height: buffer_line_height_from_settings(content.buffer_line_height.unwrap()),
            git_commit_buffer_font_size: content.git_commit_buffer_font_size.map(|s| s.into_gpui()),
            mermaid_font_family: content.mermaid_font_family.as_ref().map(|font| font.0.clone().into()),
            unnecessary_code_fade: content.unnecessary_code_fade.unwrap().0.clamp(0.0, 0.9),
        }
    }
}
