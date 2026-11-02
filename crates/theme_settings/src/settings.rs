pub use settings::FontFamilyName;
use theme::Theme;

const MIN_LINE_HEIGHT: f32 = 1.0;

/// Customizable settings for the UI and theme system.
#[derive(Clone, PartialEq)]
pub struct ThemeSettings {
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

pub use theme::BufferLineHeight;

pub const fn buffer_line_height_from_settings(value: settings::BufferLineHeight) -> BufferLineHeight {
    match value {
        settings::BufferLineHeight::Comfortable => BufferLineHeight::Comfortable,
        settings::BufferLineHeight::Standard => BufferLineHeight::Standard,
        settings::BufferLineHeight::Custom(line_height) => BufferLineHeight::Custom(line_height),
    }
}

impl ThemeSettings {
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

impl settings::Settings for ThemeSettings {
    fn from_settings(settings_content: &settings::SettingsContent) -> Self {
        let content = &settings_content.theme;
        Self {
            buffer_line_height: buffer_line_height_from_settings(content.buffer_line_height.unwrap()),
            unnecessary_code_fade: content.unnecessary_code_fade.unwrap().0.clamp(0.0, 0.9),
        }
    }
}
