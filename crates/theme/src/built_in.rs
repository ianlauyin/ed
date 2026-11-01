use std::sync::Arc;

use gpui::FontStyle;
use gpui::FontWeight;
use gpui::HighlightStyle;
use gpui::Hsla;
use gpui::SharedString;
use gpui::WindowBackgroundAppearance;
use gpui::rgba;

use crate::AccentColors;
use crate::Appearance;
use crate::PlayerColor;
use crate::StatusColors;
use crate::SyntaxTheme;
use crate::SystemColors;
use crate::Theme;
use crate::ThemeColors;
use crate::ThemeStyles;

/// Buffer and terminal line height, relative to the font size.
pub const BUFFER_LINE_HEIGHT: f32 = 1.618;
/// Opacity reduction applied to unnecessary (e.g. unused) code.
pub const UNNECESSARY_CODE_FADE: f32 = 0.3;

/// The only theme: Nord with the floating-card chrome. Edit these constructors to change the look.
pub fn theme() -> Theme {
    Theme {
        id: "nord".into(),
        name: SharedString::new_static("Nord"),
        appearance: Appearance::Dark,
        styles: ThemeStyles {
            window_background_appearance: WindowBackgroundAppearance::Opaque,
            system: system_colors(),
            accents: accent_colors(),
            colors: theme_colors(),
            status: status_colors(),
            player: player_color(),
            syntax: Arc::new(syntax_theme()),
        },
    }
}

/// Everything around the editor and terminal cards (gaps, title bar, status bar, side docks) is one
/// opaque chrome colour, the panel background; the cards are darker, and the active tab joins its card.
pub fn theme_colors() -> ThemeColors {
    ThemeColors {
        border: color(0x3b4252ff),
        border_variant: color(0x3b4252ff),
        border_focused: color(0x3b4252ff),
        border_selected: color(0x3b4252ff),
        border_transparent: color(0x00000000),
        border_disabled: color(0x4c566aff),
        elevated_surface_background: color(0x292e39ff),
        surface_background: color(0x20242cff),
        background: color(0x20242cff),
        element_background: color(0x3b4252ff),
        element_hover: color(0x3b425280),
        element_active: color(0x3b4252ff),
        element_selected: color(0x3b4252ff),
        element_selection_background: color(0x72819f80),
        element_disabled: color(0x3b4252ff),
        ghost_element_background: color(0x00000000),
        ghost_element_hover: color(0x434c5eff),
        ghost_element_active: color(0x434c5eff),
        ghost_element_selected: color(0x4c566aff),
        ghost_element_disabled: color(0x3b4252ff),
        text: color(0xeceff4ff),
        text_muted: color(0xacb1baff),
        text_placeholder: color(0xacb1baff),
        text_disabled: color(0xacb1baff),
        text_accent: color(0x88c0d0ff),
        icon: color(0xeceff4ff),
        icon_muted: color(0xacb1baff),
        icon_disabled: color(0xacb1baff),
        icon_placeholder: color(0x3b4252ff),
        icon_accent: color(0x88c0d0ff),
        status_bar_background: color(0x20242cff),
        title_bar_background: color(0x20242cff),
        title_bar_inactive_background: color(0x20242cff),
        toolbar_background: color(0x15181dff),
        tab_bar_background: color(0x20242cff),
        tab_inactive_background: color(0x20242cff),
        tab_active_background: color(0x15181dff),
        search_match_background: color(0xebcb8baa),
        search_active_match_background: color(0xbf616aaa),
        panel_background: color(0x20242cff),
        panel_focused_border: color(0x88c0d0ff),
        panel_indent_guide: color(0xfefef31b),
        panel_indent_guide_hover: color(0xfffaed2d),
        panel_indent_guide_active: color(0xfffaed2d),
        panel_overlay_background: color(0x20242cff),
        panel_overlay_hover: color(0x2e333fff),
        pane_focused_border: color(0x88c0d0ff),
        pane_group_border: color(0x3b4252ff),
        scrollbar_thumb_background: color(0x434c5e80),
        scrollbar_thumb_hover_background: color(0x434c5eaa),
        scrollbar_thumb_active_background: color(0x434c5eaa),
        scrollbar_thumb_border: color(0x00000000),
        scrollbar_track_background: color(0x20242cff),
        scrollbar_track_border: color(0x00000000),
        minimap_thumb_background: color(0x434c5e80),
        minimap_thumb_hover_background: color(0x434c5eaa),
        minimap_thumb_active_background: color(0x434c5eaa),
        minimap_thumb_border: color(0x00000000),
        editor_foreground: color(0xeceff4ff),
        editor_code_lens_foreground: None,
        editor_background: color(0x15181dff),
        editor_gutter_background: color(0x15181dff),
        editor_subheader_background: color(0x303643ff),
        editor_active_line_background: color(0x3b425280),
        editor_highlighted_line_background: color(0x434c5eff),
        editor_line_number: color(0x4c566aff),
        editor_active_line_number: color(0xeceff4ff),
        editor_hover_line_number: color(0xfffffded),
        editor_invisible: color(0x979ba3ff),
        editor_wrap_guide: color(0x4c566aff),
        editor_active_wrap_guide: color(0x4c566aff),
        editor_indent_guide: color(0xfefef31b),
        editor_indent_guide_active: color(0xfffaed2d),
        editor_document_highlight_read_background: color(0x434c5eff),
        editor_document_highlight_write_background: color(0x434c5eff),
        editor_document_highlight_bracket_background: color(0x434c5eff),
        editor_diff_hunk_added_background: color(0xa3be8c1f),
        editor_diff_hunk_added_hollow_background: color(0xa3be8c0f),
        editor_diff_hunk_added_hollow_border: color(0xa3be8c5c),
        editor_diff_hunk_deleted_background: color(0xbf616a1f),
        editor_diff_hunk_deleted_hollow_background: color(0xbf616a0f),
        editor_diff_hunk_deleted_hollow_border: color(0xbf616a5c),
        terminal_background: color(0x15181dff),
        terminal_foreground: color(0xd8dee9ff),
        terminal_bright_foreground: color(0xffffffe6),
        terminal_dim_foreground: color(0xffffffcc),
        terminal_ansi_background: color(0x040404ff),
        terminal_ansi_black: color(0x3b4252ff),
        terminal_ansi_bright_black: color(0x4c566aff),
        terminal_ansi_dim_black: color(0x000000cc),
        terminal_ansi_red: color(0xbf616aff),
        terminal_ansi_bright_red: color(0xe5747fff),
        terminal_ansi_dim_red: color(0xe5484dff),
        terminal_ansi_green: color(0xa3be8cff),
        terminal_ansi_bright_green: color(0xc3e4a8ff),
        terminal_ansi_dim_green: color(0x30a46cff),
        terminal_ansi_yellow: color(0xebcb8bff),
        terminal_ansi_bright_yellow: color(0xfff3a6ff),
        terminal_ansi_dim_yellow: color(0xffe629ff),
        terminal_ansi_blue: color(0x81a1c1ff),
        terminal_ansi_bright_blue: color(0x9ac1e7ff),
        terminal_ansi_dim_blue: color(0x0090ffff),
        terminal_ansi_magenta: color(0xb48eadff),
        terminal_ansi_bright_magenta: color(0xd8aacfff),
        terminal_ansi_dim_magenta: color(0x6e56cfff),
        terminal_ansi_cyan: color(0x88c0d0ff),
        terminal_ansi_bright_cyan: color(0x8fbcbbff),
        terminal_ansi_dim_cyan: color(0x00a2c7ff),
        terminal_ansi_white: color(0xe5e9f0ff),
        terminal_ansi_bright_white: color(0xeceff4ff),
        terminal_ansi_dim_white: color(0x7c7b74ff),
        link_text_hover: color(0x8fbcbbff),
        version_control_added: color(0xa3be8cff),
        version_control_deleted: color(0xbf616aff),
        version_control_modified: color(0xebcb8bff),
        version_control_renamed: color(0xd08770ff),
        version_control_conflict: color(0xbf616aff),
        version_control_ignored: color(0x8d7953ff),
        version_control_word_added: color(0x515f46ff),
        version_control_word_deleted: color(0x5f3035ff),
        version_control_conflict_marker_ours: color(0xa3be8c20),
        version_control_conflict_marker_theirs: color(0xbf616a20),
    }
}

pub fn status_colors() -> StatusColors {
    StatusColors {
        conflict: color(0xbf616aff),
        conflict_background: color(0xbf616a40),
        conflict_border: color(0xe5484dff),
        created: color(0xa3be8cff),
        created_background: color(0xa3be8c40),
        created_border: color(0x46a758ff),
        deleted: color(0xbf616aff),
        deleted_background: color(0xbf616a40),
        deleted_border: color(0xe5484dff),
        error: color(0xbf616aff),
        error_background: color(0xbf616a33),
        error_border: color(0xbf616aff),
        hidden: color(0x979ba3ff),
        hidden_background: color(0x979ba340),
        hidden_border: color(0x6f6d66ff),
        hint: color(0x979ba3ff),
        hint_background: color(0x292e39ff),
        hint_border: color(0x3b4252ff),
        ignored: color(0x8d7953ff),
        ignored_background: color(0x6f6d66ff),
        ignored_border: color(0x6f6d66ff),
        info: color(0x979ba3ff),
        info_background: color(0x292e39ff),
        info_border: color(0x3b4252ff),
        modified: color(0xebcb8bff),
        modified_background: color(0xebcb8b40),
        modified_border: color(0xffe629ff),
        predictive: color(0x5b677fff),
        predictive_background: color(0xfffae965),
        predictive_border: color(0xfffae965),
        renamed: color(0xd08770ff),
        renamed_background: color(0x0090ffff),
        renamed_border: color(0x0090ffff),
        success: color(0xa3be8cff),
        success_background: color(0x292e39ff),
        success_border: color(0x3b4252ff),
        unreachable: color(0xacb1baff),
        unreachable_background: color(0x7c7b74ff),
        unreachable_border: color(0x7c7b74ff),
        warning: color(0xebcb8bff),
        warning_background: color(0xebcb8b33),
        warning_border: color(0xebcb8bff),
    }
}

/// The local cursor and selection.
pub fn player_color() -> PlayerColor {
    PlayerColor { cursor: color(0xeceff4ff), selection: color(0x72819f80) }
}

pub fn accent_colors() -> AccentColors {
    AccentColors(Arc::from([
        color(0x81a1c1ff),
        color(0x88c0d0ff),
        color(0xbf616aff),
        color(0xd08770ff),
        color(0xebcb8bff),
        color(0xa3be8cff),
        color(0xb48eadff),
    ]))
}

pub fn system_colors() -> SystemColors {
    SystemColors {
        transparent: color(0x00000000),
        mac_os_traffic_light_red: color(0xec6b5fff),
        mac_os_traffic_light_yellow: color(0xf4bf4eff),
        mac_os_traffic_light_green: color(0x62c454ff),
    }
}

pub fn syntax_theme() -> SyntaxTheme {
    SyntaxTheme::new(
        [
            ("attribute", HighlightStyle { color: Some(color(0x8fbcbbff)), ..HighlightStyle::default() }),
            ("boolean", HighlightStyle { color: Some(color(0x81a1c1ff)), ..HighlightStyle::default() }),
            ("comment", HighlightStyle { color: Some(color(0x979ba3ff)), ..HighlightStyle::default() }),
            ("comment.doc", HighlightStyle { color: Some(color(0x979ba3ff)), ..HighlightStyle::default() }),
            ("constant", HighlightStyle { color: Some(color(0xd8dee9ff)), ..HighlightStyle::default() }),
            ("constructor", HighlightStyle { color: Some(color(0xd08770ff)), ..HighlightStyle::default() }),
            ("definition.function", HighlightStyle { color: Some(color(0xd08770ff)), ..HighlightStyle::default() }),
            ("embedded", HighlightStyle { color: Some(color(0xd08770ff)), ..HighlightStyle::default() }),
            (
                "emphasis",
                HighlightStyle {
                    color: Some(color(0x81a1c1ff)),
                    font_style: Some(FontStyle::Italic),
                    ..HighlightStyle::default()
                },
            ),
            (
                "emphasis.strong",
                HighlightStyle {
                    color: Some(color(0x81a1c1ff)),
                    font_weight: Some(FontWeight::BOLD),
                    ..HighlightStyle::default()
                },
            ),
            ("enum", HighlightStyle { color: Some(color(0xd08770ff)), ..HighlightStyle::default() }),
            ("function", HighlightStyle { color: Some(color(0xd8dee9ff)), ..HighlightStyle::default() }),
            ("function.method", HighlightStyle { color: Some(color(0xd8dee9ff)), ..HighlightStyle::default() }),
            ("hint", HighlightStyle { color: Some(color(0x5b677fff)), ..HighlightStyle::default() }),
            ("keyword", HighlightStyle { color: Some(color(0x81a1c1ff)), ..HighlightStyle::default() }),
            ("label", HighlightStyle { color: Some(color(0x81a1c1ff)), ..HighlightStyle::default() }),
            (
                "link_text",
                HighlightStyle {
                    color: Some(color(0x81a1c1ff)),
                    font_style: Some(FontStyle::Normal),
                    ..HighlightStyle::default()
                },
            ),
            ("link_uri", HighlightStyle { color: Some(color(0x81a1c1ff)), ..HighlightStyle::default() }),
            ("number", HighlightStyle { color: Some(color(0xb48eadff)), ..HighlightStyle::default() }),
            ("operator", HighlightStyle { color: Some(color(0x979ba3ff)), ..HighlightStyle::default() }),
            ("predictive", HighlightStyle { color: Some(color(0x5b677fff)), ..HighlightStyle::default() }),
            ("preproc", HighlightStyle { color: Some(color(0x5e81acff)), ..HighlightStyle::default() }),
            ("primary", HighlightStyle { color: Some(color(0xeceff4ff)), ..HighlightStyle::default() }),
            ("property", HighlightStyle { color: Some(color(0xd8dee9ff)), ..HighlightStyle::default() }),
            ("punctuation", HighlightStyle { color: Some(color(0x979ba3ff)), ..HighlightStyle::default() }),
            ("punctuation.bracket", HighlightStyle { color: Some(color(0x979ba3ff)), ..HighlightStyle::default() }),
            ("punctuation.delimiter", HighlightStyle { color: Some(color(0x979ba3ff)), ..HighlightStyle::default() }),
            ("punctuation.list_marker", HighlightStyle { color: Some(color(0x979ba3ff)), ..HighlightStyle::default() }),
            ("punctuation.special", HighlightStyle { color: Some(color(0x979ba3ff)), ..HighlightStyle::default() }),
            ("string", HighlightStyle { color: Some(color(0xa3be8cff)), ..HighlightStyle::default() }),
            ("string.escape", HighlightStyle { color: Some(color(0xebcb8bff)), ..HighlightStyle::default() }),
            ("string.regex", HighlightStyle { color: Some(color(0xa3be8cff)), ..HighlightStyle::default() }),
            ("string.special", HighlightStyle { color: Some(color(0xa3be8cff)), ..HighlightStyle::default() }),
            ("tag", HighlightStyle { color: Some(color(0x81a1c1ff)), ..HighlightStyle::default() }),
            ("text.literal", HighlightStyle { color: Some(color(0xa3be8cff)), ..HighlightStyle::default() }),
            (
                "title",
                HighlightStyle {
                    color: Some(color(0x88c0d0ff)),
                    font_weight: Some(FontWeight::BOLD),
                    ..HighlightStyle::default()
                },
            ),
            ("type", HighlightStyle { color: Some(color(0x88c0d0ff)), ..HighlightStyle::default() }),
            ("type.builtin", HighlightStyle { color: Some(color(0x88c0d0ff)), ..HighlightStyle::default() }),
            ("type.super", HighlightStyle { color: Some(color(0x88c0d0ff)), ..HighlightStyle::default() }),
            ("variable", HighlightStyle { color: Some(color(0xd8dee9ff)), ..HighlightStyle::default() }),
            ("variable.parameter", HighlightStyle { color: Some(color(0xd8dee9ff)), ..HighlightStyle::default() }),
            ("variable.special", HighlightStyle { color: Some(color(0x81a1c1ff)), ..HighlightStyle::default() }),
        ]
        .map(|(name, style): (&str, HighlightStyle)| (name.to_string(), style)),
    )
}

fn color(hex: u32) -> Hsla {
    Hsla::from(rgba(hex))
}

#[cfg(test)]
mod tests {
    use gpui::FontWeight;
    use gpui::Rgba;

    use super::color;
    use super::theme;
    use crate::Appearance;

    #[test]
    fn test_color_parses_rgba_hex() {
        for hex in [0x88c0d0ff, 0x3b425280, 0x00000000] {
            let rgba = Rgba::from(color(hex));
            let channels = [rgba.r, rgba.g, rgba.b, rgba.a].map(|channel| (channel * 255.).round() as u32);
            assert_eq!(u32::from_be_bytes(channels.map(|channel| channel as u8)), hex);
        }
    }

    #[test]
    fn test_theme_is_dark_nord_with_floating_chrome() {
        let theme = theme();
        assert_eq!(theme.name.as_ref(), "Nord");
        assert_eq!(theme.appearance, Appearance::Dark);

        let colors = theme.colors();
        assert_eq!(colors.background, colors.panel_background);
        assert_eq!(colors.title_bar_background, colors.panel_background);
        assert_eq!(colors.status_bar_background, colors.panel_background);
        assert_eq!(colors.tab_bar_background, colors.panel_background);
        assert_eq!(colors.tab_active_background, colors.editor_background);
        assert!(colors.editor_background.l < colors.panel_background.l);
        assert_eq!(colors.element_selection_background, theme.player().selection);
    }

    #[test]
    fn test_syntax_theme_resolves_capture_names() {
        let theme = theme();
        let syntax = theme.syntax();
        assert_eq!(syntax.style_for_name("keyword").and_then(|style| style.color), Some(color(0x81a1c1ff)));
        assert_eq!(syntax.style_for_name("title").and_then(|style| style.font_weight), Some(FontWeight::BOLD));
        assert!(syntax.highlight_id("string.escape").is_some());
        assert!(syntax.style_for_name("missing").is_none());
    }

    #[test]
    fn test_accent_colors_cycle() {
        let accents = theme().accents().clone();
        assert_eq!(accents.color_for_index(0), color(0x81a1c1ff));
        assert_eq!(accents.color_for_index(accents.0.len() as u32), accents.color_for_index(0));
    }
}
