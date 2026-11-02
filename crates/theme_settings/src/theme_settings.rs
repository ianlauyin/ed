mod schema;
mod settings;

use std::sync::Arc;

use ::settings::IntoGpui;
use gpui::App;
use gpui::HighlightStyle;
use gpui::Refineable;
use theme::AccentColors;
use theme::Appearance;
use theme::AppearanceContent;
use theme::GlobalTheme;
use theme::PlayerColor;
use theme::StatusColors;
use theme::SyntaxTheme;
use theme::SystemColors;
use theme::Theme;
use theme::ThemeColors;
use theme::ThemeStyles;
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
pub use crate::schema::theme_colors_refinement;
pub use crate::settings::BufferLineHeight;
pub use crate::settings::FontFamilyName;
pub use crate::settings::ThemeSettings;
use crate::settings::apply_floating_chrome;
pub use crate::settings::buffer_line_height_from_settings;

/// The only theme: Nord, embedded in the binary.
const NORD_THEME_JSON: &[u8] = include_bytes!("../../../assets/themes/nord/nord.json");

/// Initializes the theme system with the constant Nord theme.
pub fn init(cx: &mut App) {
    cx.set_global(GlobalTheme::new(Arc::new(nord_theme())));
}

/// Builds the Nord theme with the floating-card chrome applied.
pub fn nord_theme() -> Theme {
    let family: ThemeFamilyContent = serde_json::from_slice(NORD_THEME_JSON).expect("embedded Nord theme is valid");
    let mut theme = refine_theme(&family.themes[0]);
    apply_floating_chrome(&mut theme);
    theme
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

#[cfg(test)]
mod tests {
    use theme::Appearance;

    use super::nord_theme;

    #[test]
    fn test_nord_theme_is_dark_with_floating_chrome() {
        let theme = nord_theme();
        assert_eq!(theme.name.as_ref(), "Nord");
        assert_eq!(theme.appearance, Appearance::Dark);

        let colors = theme.colors();
        assert_eq!(colors.background, colors.panel_background);
        assert_eq!(colors.title_bar_background, colors.panel_background);
        assert_eq!(colors.tab_bar_background, colors.panel_background);
        assert_eq!(colors.tab_active_background, colors.editor_background);
        assert!(theme.syntax().style_for_name("keyword").is_some());
    }
}
