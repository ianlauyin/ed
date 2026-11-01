use std::sync::Arc;

use gpui::AbsoluteLength;
use gpui::BorderStyle;
use gpui::DefiniteLength;
use gpui::EdgesRefinement;
use gpui::FontWeight;
use gpui::Global;
use gpui::Length;
use gpui::StyleRefinement;
use gpui::TextStyleRefinement;
use gpui::UnderlineStyle;
use gpui::Window;
use gpui::point;
use gpui::px;
use gpui::relative;
use gpui::rems;
use markdown::BlockQuoteKindColors;
use markdown::HeadingLevelStyles;
use markdown::MarkdownStyle;
use theme::SyntaxTheme;
use theme::ThemeColors;
use theme::built_in::BUFFER_LINE_HEIGHT;
use ui::Rems;
use ui::prelude::*;

/// In-memory zoom of the preview font size, independent of UI and buffer zoom.
struct MarkdownPreviewFontSize(Pixels);

impl Global for MarkdownPreviewFontSize {}

/// The preview font size: the default UI font size, unless zoomed.
pub(crate) fn preview_font_size(cx: &App) -> Pixels {
    cx.try_global::<MarkdownPreviewFontSize>().map_or(ui::font::UI_FONT_SIZE, |size| size.0)
}

pub(crate) fn adjust_preview_font_size(cx: &mut App, f: impl FnOnce(Pixels) -> Pixels) {
    let size = ui::font::clamp_font_size(f(preview_font_size(cx)));
    cx.set_global(MarkdownPreviewFontSize(size));
    cx.refresh_windows();
}

pub(crate) fn reset_preview_font_size(cx: &mut App) {
    if cx.has_global::<MarkdownPreviewFontSize>() {
        cx.remove_global::<MarkdownPreviewFontSize>();
        cx.refresh_windows();
    }
}

/// The markdown style of the preview, rendered with `colors` and `syntax`
/// (which may come from a theme other than the active editor theme).
pub(crate) fn preview_markdown_style(
    colors: &ThemeColors,
    syntax: &Arc<SyntaxTheme>,
    window: &Window,
    cx: &App,
) -> MarkdownStyle {
    let buffer_font_weight = ui::font::buffer_font().weight;
    let buffer_font_size = preview_font_size(cx);
    let code_font_family = ui::font::buffer_font().family.clone();

    let body_font_size = rems(1.0);
    let mut base_text_style = window.text_style();
    base_text_style.refine(&TextStyleRefinement {
        font_family: Some(ui::font::ui_font().family.clone()),
        font_fallbacks: ui::font::ui_font().fallbacks.clone(),
        font_features: Some(ui::font::ui_font().features.clone()),
        font_size: Some(body_font_size.into()),
        line_height: Some(relative(1.5)),
        color: Some(colors.text),
        ..Default::default()
    });

    let code_block_padding = DefiniteLength::Absolute(AbsoluteLength::Pixels(px(12.)));
    let code_block_border = AbsoluteLength::Pixels(px(1.));
    let code_block_corner_radius = AbsoluteLength::Pixels(px(6.));
    let mut code_block = StyleRefinement {
        padding: EdgesRefinement {
            top: Some(code_block_padding),
            left: Some(code_block_padding),
            right: Some(code_block_padding),
            bottom: Some(code_block_padding),
        },
        margin: EdgesRefinement {
            top: Some(Length::Definite(px(16.).into())),
            left: Some(Length::Definite(px(0.).into())),
            right: Some(Length::Definite(px(0.).into())),
            bottom: Some(Length::Definite(px(16.).into())),
        },
        border_style: Some(BorderStyle::Solid),
        border_widths: EdgesRefinement {
            top: Some(code_block_border),
            left: Some(code_block_border),
            right: Some(code_block_border),
            bottom: Some(code_block_border),
        },
        border_color: Some(colors.border_variant),
        background: Some(colors.editor_background.into()),
        text: TextStyleRefinement {
            font_family: Some(code_font_family.clone()),
            font_fallbacks: ui::font::buffer_font().fallbacks.clone(),
            font_features: Some(ui::font::buffer_font().features.clone()),
            font_size: Some(buffer_font_size.into()),
            font_weight: Some(buffer_font_weight),
            line_height: Some(relative(BUFFER_LINE_HEIGHT)),
            ..Default::default()
        },
        ..Default::default()
    };
    code_block.corner_radii.top_left = Some(code_block_corner_radius);
    code_block.corner_radii.top_right = Some(code_block_corner_radius);
    code_block.corner_radii.bottom_left = Some(code_block_corner_radius);
    code_block.corner_radii.bottom_right = Some(code_block_corner_radius);

    let mut heading = StyleRefinement::default();
    heading.text.color = Some(colors.text);
    heading.margin.top = Some(Length::Definite(px(24.).into()));
    heading.margin.bottom = Some(Length::Definite(px(12.).into()));

    let heading_text_style = |font_size: Rems| TextStyleRefinement {
        font_size: Some(font_size.into()),
        font_weight: Some(FontWeight::SEMIBOLD),
        line_height: Some(relative(1.25)),
        ..Default::default()
    };

    let mut container_style = StyleRefinement::default();
    container_style.text.font_size = Some(body_font_size.into());

    let status = cx.theme().status();
    MarkdownStyle {
        base_text_style,
        container_style,
        syntax: syntax.clone(),
        selection_background_color: colors.element_selection_background,
        highlight_base_background: colors.editor_background,
        rule_color: colors.border,
        block_quote_border_color: colors.border,
        block_quote_kind_colors: BlockQuoteKindColors {
            note: status.info,
            tip: status.success,
            important: status.info,
            warning: status.warning,
            caution: status.error,
        },
        block_quote: TextStyleRefinement { color: Some(colors.text_muted), ..Default::default() },
        code_block_overflow_x_scroll: true,
        code_block,
        inline_code: TextStyleRefinement {
            font_family: Some(code_font_family),
            font_fallbacks: ui::font::buffer_font().fallbacks.clone(),
            font_features: Some(ui::font::buffer_font().features.clone()),
            font_size: Some(rems(0.875).into()),
            font_weight: Some(buffer_font_weight),
            color: Some(colors.text),
            background_color: Some(colors.editor_foreground.opacity(0.08)),
            ..Default::default()
        },
        link: TextStyleRefinement {
            color: Some(colors.text_accent),
            underline: Some(UnderlineStyle {
                color: Some(colors.text_accent.opacity(0.5)),
                thickness: px(1.),
                ..Default::default()
            }),
            ..Default::default()
        },
        heading,
        heading_level_styles: Some(HeadingLevelStyles {
            h1: Some(heading_text_style(rems(1.75))),
            h2: Some(heading_text_style(rems(1.4))),
            h3: Some(heading_text_style(rems(1.2))),
            h4: Some(heading_text_style(rems(1.0))),
            h5: Some(heading_text_style(rems(0.875))),
            h6: Some(TextStyleRefinement { color: Some(colors.text_muted), ..heading_text_style(rems(0.85)) }),
        }),
        heading_border_color: Some(colors.border_variant),
        paragraph_spacing: px(16.),
        paragraph_line_height: relative(1.5),
        list_spacing: px(12.),
        table_cell_padding: point(px(10.), px(4.)),
        ..Default::default()
    }
}
