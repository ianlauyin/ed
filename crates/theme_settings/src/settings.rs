use std::sync::Arc;

use collections::HashMap;
use gpui::App;
use gpui::Context;
use gpui::Font;
use gpui::FontFallbacks;
use gpui::FontStyle;
use gpui::FontWeight;
use gpui::Global;
use gpui::Pixels;
use gpui::SharedString;
use gpui::Subscription;
use gpui::Window;
use gpui::px;
use refineable::Refineable;
use serde::Deserialize;
use serde::Serialize;
pub use settings::FontFamilyName;
use settings::IntoGpui;
use settings::Settings;
use settings::SettingsContent;
pub use settings::ThemeAppearanceMode;
pub use settings::ThemeName;
use theme::Appearance;
use theme::SyntaxTheme;
use theme::Theme;
use theme::UiDensity;

use crate::merge_accent_colors;
use crate::merge_player_color;
use crate::schema::status_colors_refinement;
use crate::schema::syntax_overrides;
use crate::schema::theme_colors_refinement;

const MIN_FONT_SIZE: Pixels = px(6.0);
const MAX_FONT_SIZE: Pixels = px(100.0);
const MIN_LINE_HEIGHT: f32 = 1.0;

pub(crate) const fn ui_density_from_settings(val: settings::UiDensity) -> UiDensity {
    match val {
        settings::UiDensity::Compact => UiDensity::Compact,
        settings::UiDensity::Default => UiDensity::Default,
        settings::UiDensity::Comfortable => UiDensity::Comfortable,
    }
}

pub const fn appearance_to_mode(appearance: Appearance) -> ThemeAppearanceMode {
    match appearance {
        Appearance::Light => ThemeAppearanceMode::Light,
        Appearance::Dark => ThemeAppearanceMode::Dark,
    }
}

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
    /// The font family to use for rendering in the markdown preview.
    /// Falls back to the UI font family if unset.
    markdown_preview_font_family: Option<SharedString>,
    /// The font family to use for code in the markdown preview.
    /// Falls back to the buffer font family if unset.
    markdown_preview_code_font_family: Option<SharedString>,
    /// The font size to use for rendering in the markdown preview.
    /// Falls back to the UI font size if unset.
    markdown_preview_font_size: Option<Pixels>,
    /// The theme to use for the markdown preview.
    /// Falls back to the main editor theme if unset.
    pub markdown_preview_theme: Option<ThemeSelection>,
    /// The font family used for Mermaid diagrams.
    /// Falls back to the UI font family if unset.
    mermaid_font_family: Option<SharedString>,
    /// The font weight for headings in the markdown preview.
    markdown_preview_heading_font_weight: FontWeight,
    /// The line height for buffers, and the terminal.
    ///
    /// Changing this may affect the spacing of some UI elements.
    ///
    /// The terminal font family can be overridden using it's own setting.
    pub buffer_line_height: BufferLineHeight,
    /// The current theme selection.
    pub theme: ThemeSelection,
    /// Manual overrides for the active theme.
    ///
    /// Note: This setting is still experimental. See [this tracking issue](https://github.com/zed-industries/zed/issues/18078)
    pub experimental_theme_overrides: Option<settings::ThemeStyleContent>,
    /// Manual overrides per theme
    pub theme_overrides: HashMap<String, settings::ThemeStyleContent>,
    /// The density of the UI.
    /// Note: This setting is still experimental. See [this tracking issue](
    pub ui_density: UiDensity,
    /// The amount of fading applied to unnecessary code.
    pub unnecessary_code_fade: f32,
}

settings::register_setting!(ThemeSettings);

/// Returns the name of the default theme for the given [`Appearance`].
pub const fn default_theme(appearance: Appearance) -> &'static str {
    match appearance {
        Appearance::Light => settings::DEFAULT_LIGHT_THEME,
        Appearance::Dark => settings::DEFAULT_DARK_THEME,
    }
}

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

/// Represents the selection of a theme, which can be either static or dynamic.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ThemeSelection {
    /// A static theme selection, represented by a single theme name.
    Static(ThemeName),
    /// A dynamic theme selection, which can change based the [ThemeMode].
    Dynamic {
        /// The mode used to determine which theme to use.
        #[serde(default)]
        mode: ThemeAppearanceMode,
        /// The theme to use for light mode.
        light: ThemeName,
        /// The theme to use for dark mode.
        dark: ThemeName,
    },
}

impl From<settings::ThemeSelection> for ThemeSelection {
    fn from(selection: settings::ThemeSelection) -> Self {
        match selection {
            settings::ThemeSelection::Static(theme) => ThemeSelection::Static(theme),
            settings::ThemeSelection::Dynamic { mode, light, dark } => ThemeSelection::Dynamic { mode, light, dark },
        }
    }
}

impl ThemeSelection {
    /// Returns the theme name for the selected [ThemeMode].
    pub fn name(&self, system_appearance: Appearance) -> ThemeName {
        match self {
            Self::Static(theme) => theme.clone(),
            Self::Dynamic { mode, light, dark } => match mode {
                ThemeAppearanceMode::Light => light.clone(),
                ThemeAppearanceMode::Dark => dark.clone(),
                ThemeAppearanceMode::System => match system_appearance {
                    Appearance::Light => light.clone(),
                    Appearance::Dark => dark.clone(),
                },
            },
        }
    }

    /// Returns the [ThemeMode] for the [ThemeSelection].
    pub const fn mode(&self) -> Option<ThemeAppearanceMode> {
        match self {
            ThemeSelection::Static(_) => None,
            ThemeSelection::Dynamic { mode, .. } => Some(*mode),
        }
    }
}

/// Sets the theme for the given appearance to the theme with the specified name.
///
/// The caller should make sure that the [`Appearance`] matches the theme associated with the name.
///
/// If the current [`ThemeAppearanceMode`] is set to [`System`] and the user's system [`Appearance`]
/// is different than the new theme's [`Appearance`], this function will update the
/// [`ThemeAppearanceMode`] to the new theme's appearance in order to display the new theme.
///
/// [`System`]: ThemeAppearanceMode::System
pub fn set_theme(
    current: &mut SettingsContent,
    theme_name: impl Into<Arc<str>>,
    theme_appearance: Appearance,
    system_appearance: Appearance,
) {
    let theme_name = ThemeName(theme_name.into());

    let Some(selection) = current.theme.theme.as_mut() else {
        current.theme.theme = Some(settings::ThemeSelection::Static(theme_name));
        return;
    };

    match selection {
        settings::ThemeSelection::Static(theme) => {
            *theme = theme_name;
        }
        settings::ThemeSelection::Dynamic { mode, light, dark } => {
            match theme_appearance {
                Appearance::Light => *light = theme_name,
                Appearance::Dark => *dark = theme_name,
            }

            let should_update_mode = !(mode == &ThemeAppearanceMode::System && theme_appearance == system_appearance);

            if should_update_mode {
                *mode = appearance_to_mode(theme_appearance);
            }
        }
    }
}

/// Sets the mode for the theme.
pub fn set_mode(content: &mut SettingsContent, mode: ThemeAppearanceMode) {
    let theme = content.theme.as_mut();

    if let Some(selection) = theme.theme.as_mut() {
        match selection {
            settings::ThemeSelection::Static(_) => {
                *selection = settings::ThemeSelection::Dynamic {
                    mode: ThemeAppearanceMode::System,
                    light: ThemeName(settings::DEFAULT_LIGHT_THEME.into()),
                    dark: ThemeName(settings::DEFAULT_DARK_THEME.into()),
                };
            }
            settings::ThemeSelection::Dynamic { mode: mode_to_update, .. } => *mode_to_update = mode,
        }
    } else {
        theme.theme = Some(settings::ThemeSelection::Dynamic {
            mode,
            light: ThemeName(settings::DEFAULT_LIGHT_THEME.into()),
            dark: ThemeName(settings::DEFAULT_DARK_THEME.into()),
        });
    }
}

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

    /// Returns the font family to use in the markdown preview,
    /// falling back to the UI font family when unset.
    pub fn markdown_preview_font_family(&self) -> &SharedString {
        self.markdown_preview_font_family.as_ref().unwrap_or(&self.ui_font.family)
    }

    /// Returns the font family to use for code in the markdown preview,
    /// falling back to the buffer font family when unset.
    pub fn markdown_preview_code_font_family(&self) -> &SharedString {
        self.markdown_preview_code_font_family.as_ref().unwrap_or(&self.buffer_font.family)
    }

    /// Returns the font family to use for Mermaid diagrams, falling back to
    /// the buffer font family when unset: diagrams are laid out for a
    /// monospace font.
    pub fn mermaid_font_family(&self) -> &SharedString {
        self.mermaid_font_family.as_ref().unwrap_or(&self.buffer_font.family)
    }

    /// Returns the font weight to use for headings in the markdown preview.
    pub const fn markdown_preview_heading_font_weight(&self) -> FontWeight {
        self.markdown_preview_heading_font_weight
    }

    /// Returns the markdown preview font size.
    ///
    /// Note: the fallback deliberately uses `self.ui_font_size` instead of `ui_font_size(cx)`,
    /// so that temporary UI zoom does not also resize the markdown preview.
    pub fn markdown_preview_font_size(&self, cx: &App) -> Pixels {
        cx.try_global::<MarkdownPreviewFontSize>()
            .map(|size| size.0)
            .or(self.markdown_preview_font_size)
            .map(clamp_font_size)
            .unwrap_or_else(|| clamp_font_size(self.ui_font_size))
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

    /// Returns the markdown preview font size, read from the settings.
    ///
    /// The real markdown preview font size is stored in-memory, to support temporary
    /// font size changes. Use [`Self::markdown_preview_font_size`] to get the real font size.
    pub const fn markdown_preview_font_size_settings(&self) -> Option<Pixels> {
        self.markdown_preview_font_size
    }

    /// Returns the buffer's line height.
    pub const fn line_height(&self) -> f32 {
        f32::max(self.buffer_line_height.value(), MIN_LINE_HEIGHT)
    }

    /// Applies the theme overrides, if there are any, to the current theme.
    pub fn apply_theme_overrides(&self, mut arc_theme: Arc<Theme>) -> Arc<Theme> {
        let mut theme = (*arc_theme).clone();
        apply_floating_chrome(&mut theme);
        arc_theme = Arc::new(theme);

        if let Some(experimental_theme_overrides) = &self.experimental_theme_overrides {
            let mut theme = (*arc_theme).clone();
            ThemeSettings::modify_theme(&mut theme, experimental_theme_overrides);
            arc_theme = Arc::new(theme);
        }

        if let Some(theme_overrides) = self.theme_overrides.get(arc_theme.name.as_ref()) {
            let mut theme = (*arc_theme).clone();
            ThemeSettings::modify_theme(&mut theme, theme_overrides);
            arc_theme = Arc::new(theme);
        }

        arc_theme
    }

    fn modify_theme(base_theme: &mut Theme, theme_overrides: &settings::ThemeStyleContent) {
        if let Some(window_background_appearance) = theme_overrides.window_background_appearance {
            base_theme.styles.window_background_appearance = window_background_appearance.into_gpui();
        }
        let status_color_refinement = status_colors_refinement(&theme_overrides.status);

        let theme_color_refinement = theme_colors_refinement(
            &theme_overrides.colors,
            &status_color_refinement,
            base_theme.appearance.is_light(),
        );
        base_theme.styles.colors.refine(&theme_color_refinement);
        base_theme.styles.status.refine(&status_color_refinement);
        merge_player_color(&mut base_theme.styles.player, &theme_overrides.players);
        merge_accent_colors(&mut base_theme.styles.accents, &theme_overrides.accents);
        base_theme.styles.syntax =
            SyntaxTheme::merge(base_theme.styles.syntax.clone(), syntax_overrides(theme_overrides));
    }
}

/// Floating-card look: everything around the editor and terminal cards (gaps, title bar,
/// status bar, side docks) is one opaque chrome colour, the theme's panel background.
/// Applied before user theme overrides so they can still adjust it.
fn apply_floating_chrome(theme: &mut Theme) {
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
        let markdown_preview = settings_content.markdown_preview.as_ref();
        let theme_selection: ThemeSelection = content.theme.clone().unwrap().into();
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
            markdown_preview_font_family: markdown_preview
                .and_then(|preview| preview.font_family.as_ref())
                .map(|f| f.0.clone().into()),
            markdown_preview_code_font_family: markdown_preview
                .and_then(|preview| preview.code_font_family.as_ref())
                .map(|f| f.0.clone().into()),
            markdown_preview_font_size: markdown_preview
                .and_then(|preview| preview.font_size)
                .map(|size| size.into_gpui()),
            markdown_preview_theme: markdown_preview
                .and_then(|preview| preview.theme.clone())
                .map(ThemeSelection::from),
            mermaid_font_family: content.mermaid_font_family.as_ref().map(|font| font.0.clone().into()),
            markdown_preview_heading_font_weight: markdown_preview
                .and_then(|preview| preview.heading_font_weight)
                .map(|weight| weight.into_gpui())
                .unwrap_or(FontWeight::SEMIBOLD),
            theme: theme_selection,
            experimental_theme_overrides: content.experimental_theme_overrides.clone(),
            theme_overrides: content.theme_overrides.clone(),
            ui_density: ui_density_from_settings(content.ui_density.unwrap_or_default()),
            unnecessary_code_fade: content.unnecessary_code_fade.unwrap().0.clamp(0.0, 0.9),
        }
    }
}
