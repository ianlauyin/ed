// for a very big json! macro
#![recursion_limit = "256"]

//! Crate for rendering Mermaid diagram strings to SVG strings.
//!
//! The entrypoint to this crate is [`render_to_svg`].
//!
//! It takes a `&str` and a [`MermaidTheme`]. The output is an SVG with the
//! following properties:
//! - The style matches the provided theme
//! - Nodes are given accent colors, even if none are provided in the mermaid
//!   source.
//! - The SVG has been tweaked based on the assumption that it will be rasterized
//!   using `usvg`/`resvg`. Some bugs/quirks of `usvg`/`resvg` are accounted for
//!   in this crate.
//!
//! This module uses the [`merman`] crate for rendering, rather than
//! `mermaid-rs`, which was used in the previous implementation of mermaid
//! rendering in Zed.
//!
//! Historically, this crate also carried generic `usvg`/`resvg` cleanup for SVG
//! constructs that merman's parity output could emit, such as HTML labels in
//! `<foreignObject>` and CSS/attribute forms that rasterizers do not handle.
//! Since merman 0.6, that generic cleanup is exposed as merman's raster-safe SVG
//! pipeline. Zed opts into that pipeline during rendering, then keeps
//! editor-specific theme and accent color rules in this crate. The [`gpui`]
//! dependency is only needed for the [`Hsla`] and [`Rgba`] color types.
//!
//! The [`render_to_svg`] function operates in two stages:
//! - [`render`] the mermaid text to raster-safe SVG using [`merman`].
//! - [`postprocess`] the SVG to add Zed theme and accent styling.
//!
//! Zed's postprocessing is split up into stages. We parse the generated SVG
//! using [`quick_xml`], which produces an iterator of
//! [`Event<'_>`](quick_xml::events::Event)s. This iterator is then repeatedly
//! transformed, and finally collected back into an SVG string.
//!
//! This approach:
//! - Avoids doing multiple expensive string insertions.
//! - Avoids parsing the SVG multiple times (without needing to put all the
//!   logic in one huge function).
//! - But is quite a bit more complex.
//!
//! I think this complexity is justified because of the drastic performance
//! impact, as well as the low-risk nature; this code cannot panic, and errors
//! in the output just produce weird-looking diagrams.
//!
//! ## Color handling
//!
//! We try to match the users theme, and also apply accent colors to diagrams to
//! make them more visually interesting. Accent colors are derived from the
//! `accents` in the Zed theme.
//!
//! There are three parts to color handling:
//!
//! 1. A [`merman::MermaidConfig`] is passed when initially rendering the
//!    diagram. This sets most "normal" colors (background, text, etc.). However,
//!    it's not possible to color nodes individually, and not all parts of the
//!    diagrams are correctly themed.
//! 2. `postprocess::accent_colors` injects custom CSS classes (e.g.
//!    `zed-accent-0`) to specific elements, based on the diagram type and
//!    node.
//! 3. `postprocess::inject_css` injects CSS rules for the classes applied by
//!    `accent_colors`

mod postprocess;
mod render;

use anyhow::Result;
use gpui::{Hsla, Rgba};

#[derive(Debug, Clone, Copy)]
pub struct AccentColor {
    pub foreground: Hsla,
    pub background: Hsla,
}

#[derive(Debug, Clone)]
pub struct MermaidTheme {
    pub dark_mode: bool,
    pub font_family: String,
    pub background: Hsla,
    pub primary_color: Hsla,
    pub primary_text_color: Hsla,
    pub primary_border_color: Hsla,
    pub secondary_color: Hsla,
    pub tertiary_color: Hsla,
    pub line_color: Hsla,
    pub text_color: Hsla,
    pub edge_label_background: Hsla,
    pub cluster_background: Hsla,
    pub cluster_border: Hsla,
    pub note_background: Hsla,
    pub note_border: Hsla,
    pub actor_background: Hsla,
    pub actor_border: Hsla,
    pub activation_background: Hsla,
    pub activation_border: Hsla,
    pub git_branch_colors: [Hsla; 8],
    pub git_branch_label_colors: [Hsla; 8],
    pub er_attr_bg_odd: Hsla,
    pub er_attr_bg_even: Hsla,
    pub error_color: Hsla,
    pub warning_color: Hsla,
    pub accent_colors: Vec<AccentColor>,
}

/// Formats a color as a CSS hex color for embedding in SVG/CSS.
///
/// Emits `#rrggbb` for fully opaque colors and `#rrggbbaa` when the input
/// has any transparency, so translucent theme colors (e.g. `ghost_element_hover`
/// from Zed's UI palette) round-trip without silently losing their alpha.
pub(crate) fn css_color(color: Hsla) -> String {
    let rgba = Rgba::from(color);
    let r = (rgba.r.clamp(0.0, 1.0) * 255.0).round() as u8;
    let g = (rgba.g.clamp(0.0, 1.0) * 255.0).round() as u8;
    let b = (rgba.b.clamp(0.0, 1.0) * 255.0).round() as u8;
    let a = (rgba.a.clamp(0.0, 1.0) * 255.0).round() as u8;
    if a == 0xff {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

pub use postprocess::util::text_color_for_background;

/// See the [module-level docs][crate] for more info.
pub fn render_to_svg(source: &str, theme: &MermaidTheme) -> Result<String> {
    let svg = render::render_mermaid(source, theme)?;
    let svg = postprocess::postprocess(&svg, theme)?;
    Ok(svg)
}
