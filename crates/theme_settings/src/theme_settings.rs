mod schema;
mod settings;

use std::sync::Arc;

use ::settings::IntoGpui;
use ::settings::Settings;
use ::settings::SettingsStore;
use anyhow::Context as _;
use gpui::App;
use gpui::Font;
use gpui::HighlightStyle;
use gpui::Pixels;
use gpui::Refineable;
use gpui::px;
use gpui_util::ResultExt;
use theme::AccentColors;
use theme::Appearance;
use theme::AppearanceContent;
use theme::DEFAULT_DARK_THEME;
use theme::GlobalTheme;
use theme::LoadThemes;
use theme::PlayerColor;
use theme::StatusColors;
use theme::SyntaxTheme;
use theme::SystemAppearance;
use theme::SystemColors;
use theme::Theme;
use theme::ThemeColors;
use theme::ThemeFamily;
use theme::ThemeRegistry;
use theme::ThemeSettingsProvider;
use theme::ThemeStyles;
use theme::default_color_scales;
use theme::try_parse_color;

pub use crate::schema::FontStyleContent;
pub use crate::schema::FontWeightContent;
pub use crate::schema::HighlightStyleContent;
pub use crate::schema::StatusColorsContent;
pub use crate::schema::ThemeColorsContent;
pub use crate::schema::ThemeContent;
pub use crate::schema::ThemeFamilyContent;
pub use crate::schema::ThemeStyleContent;
pub use crate::schema::WindowBackgroundContent;
pub use crate::schema::status_colors_refinement;
pub use crate::schema::syntax_overrides;
pub use crate::schema::theme_colors_refinement;
pub use crate::settings::BufferLineHeight;
pub use crate::settings::FontFamilyName;
pub use crate::settings::GitCommitBufferFontSize;
pub use crate::settings::MarkdownPreviewFontSize;
pub use crate::settings::ThemeAppearanceMode;
pub use crate::settings::ThemeName;
pub use crate::settings::ThemeSelection;
pub use crate::settings::ThemeSettings;
use crate::settings::adjust_buffer_font_size;
pub use crate::settings::adjust_git_commit_buffer_font_size;
pub use crate::settings::adjust_markdown_preview_font_size;
pub use crate::settings::adjust_ui_font_size;
pub use crate::settings::adjusted_font_size;
pub use crate::settings::appearance_to_mode;
pub use crate::settings::buffer_line_height_from_settings;
pub use crate::settings::clamp_font_size;
pub use crate::settings::default_theme;
pub use crate::settings::observe_buffer_font_size_adjustment;
pub use crate::settings::reset_buffer_font_size;
pub use crate::settings::reset_git_commit_buffer_font_size;
pub use crate::settings::reset_markdown_preview_font_size;
pub use crate::settings::reset_ui_font_size;
pub use crate::settings::set_mode;
pub use crate::settings::set_theme;
pub use crate::settings::setup_ui_font;

struct ThemeSettingsProviderImpl;

impl ThemeSettingsProvider for ThemeSettingsProviderImpl {
    fn ui_font<'a>(&'a self, cx: &'a App) -> &'a Font {
        &ThemeSettings::get_global(cx).ui_font
    }

    fn buffer_font<'a>(&'a self, cx: &'a App) -> &'a Font {
        &ThemeSettings::get_global(cx).buffer_font
    }

    fn ui_font_size(&self, cx: &App) -> Pixels {
        ThemeSettings::get_global(cx).ui_font_size(cx)
    }

    fn buffer_font_size(&self, cx: &App) -> Pixels {
        ThemeSettings::get_global(cx).buffer_font_size(cx)
    }
}

/// Initialize the theme system with settings integration.
///
/// This is the full initialization for the application. It calls [`theme::init`]
/// and then wires up settings observation for theme/font changes.
pub fn init(themes_to_load: LoadThemes, cx: &mut App) {
    let load_user_themes = matches!(&themes_to_load, LoadThemes::All(_));

    theme::init(themes_to_load, cx);
    theme::set_theme_settings_provider(Box::new(ThemeSettingsProviderImpl), cx);

    if load_user_themes {
        let registry = ThemeRegistry::global(cx);
        load_bundled_themes(&registry);
    }

    let theme = configured_theme(cx);
    GlobalTheme::update_theme(cx, theme);

    let settings = ThemeSettings::get_global(cx);

    let mut prev_buffer_font_size_settings = settings.buffer_font_size_settings();
    let mut prev_ui_font_size_settings = settings.ui_font_size_settings();
    let mut prev_git_commit_buffer_font_size_settings = settings.git_commit_buffer_font_size_settings();
    let mut prev_markdown_preview_font_size_settings = settings.markdown_preview_font_size_settings();
    let mut prev_theme_name = settings.theme.name(SystemAppearance::global(cx).0);
    let mut prev_theme_overrides = (settings.experimental_theme_overrides.clone(), settings.theme_overrides.clone());

    cx.observe_global::<SettingsStore>(move |cx| {
        let settings = ThemeSettings::get_global(cx);

        let buffer_font_size_settings = settings.buffer_font_size_settings();
        let ui_font_size_settings = settings.ui_font_size_settings();
        let git_commit_buffer_font_size_settings = settings.git_commit_buffer_font_size_settings();
        let markdown_preview_font_size_settings = settings.markdown_preview_font_size_settings();
        let theme_name = settings.theme.name(SystemAppearance::global(cx).0);
        let theme_overrides = (settings.experimental_theme_overrides.clone(), settings.theme_overrides.clone());

        if buffer_font_size_settings != prev_buffer_font_size_settings {
            prev_buffer_font_size_settings = buffer_font_size_settings;
            reset_buffer_font_size(cx);
        }

        if ui_font_size_settings != prev_ui_font_size_settings {
            prev_ui_font_size_settings = ui_font_size_settings;
            reset_ui_font_size(cx);
        }

        if git_commit_buffer_font_size_settings != prev_git_commit_buffer_font_size_settings {
            prev_git_commit_buffer_font_size_settings = git_commit_buffer_font_size_settings;
            reset_git_commit_buffer_font_size(cx);
        }

        if markdown_preview_font_size_settings != prev_markdown_preview_font_size_settings {
            prev_markdown_preview_font_size_settings = markdown_preview_font_size_settings;
            reset_markdown_preview_font_size(cx);
        }

        if theme_name != prev_theme_name || theme_overrides != prev_theme_overrides {
            prev_theme_name = theme_name;
            prev_theme_overrides = theme_overrides;
            reload_theme(cx);
        }
    })
    .detach();
}

fn configured_theme(cx: &mut App) -> Arc<Theme> {
    let themes = ThemeRegistry::default_global(cx);
    let theme_settings = ThemeSettings::get_global(cx);
    let system_appearance = SystemAppearance::global(cx);

    let theme_name = theme_settings.theme.name(*system_appearance);

    let theme = match themes.get(&theme_name.0) {
        Ok(theme) => theme,
        Err(err) => {
            log::error!("{err}");
            themes.get(default_theme(*system_appearance)).unwrap_or_else(|_| themes.get(DEFAULT_DARK_THEME).unwrap())
        }
    };
    theme_settings.apply_theme_overrides(theme)
}

/// Reloads the current theme from settings.
pub fn reload_theme(cx: &mut App) {
    let theme = configured_theme(cx);
    GlobalTheme::update_theme(cx, theme);
    cx.refresh_windows();
}

/// Loads the themes bundled with the ed binary into the registry.
pub fn load_bundled_themes(registry: &ThemeRegistry) {
    let theme_paths = registry
        .assets()
        .list("themes/")
        .expect("failed to list theme assets")
        .into_iter()
        .filter(|path| path.ends_with(".json"));

    for path in theme_paths {
        let Some(theme) = registry.assets().load(&path).log_err().flatten() else {
            continue;
        };

        let Some(theme_family) = serde_json::from_slice(&theme)
            .with_context(|| format!("failed to parse theme at path \"{path}\""))
            .log_err()
        else {
            continue;
        };

        let refined = refine_theme_family(theme_family);
        registry.insert_theme_families([refined]);
    }
}

/// Refines a [`ThemeFamilyContent`] and its [`ThemeContent`]s into a [`ThemeFamily`].
pub fn refine_theme_family(theme_family_content: ThemeFamilyContent) -> ThemeFamily {
    let id = uuid::Uuid::new_v4().to_string();
    let name = theme_family_content.name.clone();
    let author = theme_family_content.author.clone();

    let themes: Vec<Theme> = theme_family_content.themes.iter().map(refine_theme).collect();

    ThemeFamily { id, name: name.into(), author: author.into(), themes, scales: default_color_scales() }
}

/// Refines a [`ThemeContent`] into a [`Theme`].
pub fn refine_theme(theme: &ThemeContent) -> Theme {
    let appearance = match theme.appearance {
        AppearanceContent::Light => Appearance::Light,
        AppearanceContent::Dark => Appearance::Dark,
    };

    let mut refined_status_colors = match theme.appearance {
        AppearanceContent::Light => StatusColors::light(),
        AppearanceContent::Dark => StatusColors::dark(),
    };
    let mut status_colors_refinement = status_colors_refinement(&theme.style.status);
    theme::apply_status_color_defaults(&mut status_colors_refinement);
    refined_status_colors.refine(&status_colors_refinement);

    let mut refined_player_color = match theme.appearance {
        AppearanceContent::Light => PlayerColor::light(),
        AppearanceContent::Dark => PlayerColor::dark(),
    };
    merge_player_color(&mut refined_player_color, &theme.style.players);

    let mut refined_theme_colors = match theme.appearance {
        AppearanceContent::Light => ThemeColors::light(),
        AppearanceContent::Dark => ThemeColors::dark(),
    };
    let mut theme_colors_refinement = theme_colors_refinement(
        &theme.style.colors,
        &status_colors_refinement,
        theme.appearance == AppearanceContent::Light,
    );
    theme::apply_theme_color_defaults(&mut theme_colors_refinement, &refined_player_color);
    refined_theme_colors.refine(&theme_colors_refinement);

    let mut refined_accent_colors = match theme.appearance {
        AppearanceContent::Light => AccentColors::light(),
        AppearanceContent::Dark => AccentColors::dark(),
    };
    merge_accent_colors(&mut refined_accent_colors, &theme.style.accents);

    let syntax_highlights = theme.style.syntax.iter().map(|(syntax_token, highlight)| {
        (
            syntax_token.clone(),
            HighlightStyle {
                color: highlight.color.as_ref().and_then(|color| try_parse_color(color).ok()),
                background_color: highlight.background_color.as_ref().and_then(|color| try_parse_color(color).ok()),
                font_style: highlight.font_style.map(|s| s.into_gpui()),
                font_weight: highlight.font_weight.map(|w| w.into_gpui()),
                ..Default::default()
            },
        )
    });
    let syntax_theme = Arc::new(SyntaxTheme::new(syntax_highlights));

    let window_background_appearance =
        theme.style.window_background_appearance.map(|w| w.into_gpui()).unwrap_or_default();

    Theme {
        id: uuid::Uuid::new_v4().to_string(),
        name: theme.name.clone().into(),
        appearance,
        styles: ThemeStyles {
            system: SystemColors::default(),
            window_background_appearance,
            accents: refined_accent_colors,
            colors: refined_theme_colors,
            status: refined_status_colors,
            player: refined_player_color,
            syntax: syntax_theme,
        },
    }
}

/// Merges player color overrides into the given [`PlayerColor`].
///
/// Only the first entry is used: it is the local player's color.
pub fn merge_player_color(player_color: &mut PlayerColor, user_player_colors: &[::settings::PlayerColorContent]) {
    let Some(player) = user_player_colors.first() else {
        return;
    };
    if let Some(cursor) = player.cursor.as_ref().and_then(|color| try_parse_color(color).ok()) {
        player_color.cursor = cursor;
    }
    if let Some(selection) = player.selection.as_ref().and_then(|color| try_parse_color(color).ok()) {
        player_color.selection = selection;
    }
}

/// Merges accent color overrides into the given [`AccentColors`].
pub fn merge_accent_colors(accent_colors: &mut AccentColors, user_accent_colors: &[::settings::AccentContent]) {
    if user_accent_colors.is_empty() {
        return;
    }

    let colors = user_accent_colors
        .iter()
        .filter_map(|accent_color| accent_color.0.as_ref().and_then(|color| try_parse_color(color).ok()))
        .collect::<Vec<_>>();

    if !colors.is_empty() {
        accent_colors.0 = Arc::from(colors);
    }
}

/// Increases the buffer font size by 1 pixel, without persisting the result in the settings.
/// This will be effective until the app is restarted.
pub fn increase_buffer_font_size(cx: &mut App) {
    adjust_buffer_font_size(cx, |size| size + px(1.0));
}

/// Decreases the buffer font size by 1 pixel, without persisting the result in the settings.
/// This will be effective until the app is restarted.
pub fn decrease_buffer_font_size(cx: &mut App) {
    adjust_buffer_font_size(cx, |size| size - px(1.0));
}
