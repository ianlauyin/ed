//! The root SVG document around a diagram's content. Port of Pele's `src/svg/root.ts`.

use crate::svg::builder::{esc, esc_text};
use crate::svg::theme::{ROOT_STYLE, font_style};
use crate::yaml::push_number;

/// The host's options that the shared SVG modules read. A subset of Pele's `RenderOptions`;
/// diagram modules add the fields they read as they are ported. There are no font options, since
/// the font comes from the [`Theme`](crate::Theme) and every width is monospace.
#[derive(Clone, Debug, Default)]
pub(crate) struct RenderOptions {
    /// Whether a diagram that runs across may be drawn running down when it does not fit
    /// `max_width`. On unless `Some(false)`.
    pub(crate) auto_direction: Option<bool>,
    /// The width under which a diagram may turn to run down. 640 when absent or not positive.
    pub(crate) direction_breakpoint: Option<f64>,
    /// Font size in pixels. 16 when absent.
    pub(crate) font_size: Option<f64>,
    /// The prefix of element IDs, so several diagrams can share a page. `pele` when absent.
    pub(crate) id_prefix: Option<String>,
    /// Schemes an image may use besides a relative address.
    pub(crate) image_schemes: Option<Vec<String>>,
    /// Whether images are drawn. On unless `Some(false)`.
    pub(crate) images: Option<bool>,
    /// The most UTF-16 units of source to accept. 50,000 when absent.
    pub(crate) limit: Option<f64>,
    /// The `rel` of links. `noopener` when absent; empty omits it.
    pub(crate) link_rel: Option<String>,
    /// Schemes a link may use besides a relative address.
    pub(crate) link_schemes: Option<Vec<String>>,
    /// Whether links are drawn. On unless `Some(false)`.
    pub(crate) links: Option<bool>,
    /// The most edges a flowchart may have. 5,000 when absent.
    pub(crate) max_edges: Option<f64>,
    /// The width the host has for the diagram, in pixels. A type that can draw itself narrower
    /// does, so its text is not shrunk with it.
    pub(crate) max_width: Option<f64>,
    /// The time a gantt chart schedules against, in milliseconds since the epoch. The epoch
    /// when absent, since the compiler reads no clock.
    pub(crate) now: Option<f64>,
    /// The most UTF-16 units of SVG to produce. 4,000,000 when absent.
    pub(crate) output_limit: Option<f64>,
    /// Space around a diagram. 8 when absent.
    pub(crate) padding: Option<f64>,
    /// Whether the document scales down to its container. On unless `Some(false)`.
    pub(crate) responsive: Option<bool>,
}

/// A diagram's accessible title and description.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Accessible<'a> {
    /// The accessible description.
    pub(crate) acc_descr: Option<&'a str>,
    /// The accessible title.
    pub(crate) acc_title: Option<&'a str>,
}

/// Wraps a diagram's content in the root `<svg>` document of the given size, with its theme,
/// font at `font_size`, accessible title and description, and diagram `type` class.
pub(crate) fn svg_document(
    r#type: &str,
    width: f64,
    height: f64,
    font_size: f64,
    options: &RenderOptions,
    acc: Accessible<'_>,
    content: &str,
) -> String {
    let prefix = esc(options.id_prefix.as_deref().unwrap_or("ondine"));
    let mut head = String::new();
    let mut aria = String::new();
    if let Some(title) = acc.acc_title.filter(|title| !title.is_empty()) {
        head.push_str("<title id=\"");
        head.push_str(&prefix);
        head.push_str("-title\">");
        head.push_str(&esc_text(title));
        head.push_str("</title>");
        aria.push_str(" aria-labelledby=\"");
        aria.push_str(&prefix);
        aria.push_str("-title\"");
    }
    if let Some(descr) = acc.acc_descr.filter(|descr| !descr.is_empty()) {
        head.push_str("<desc id=\"");
        head.push_str(&prefix);
        head.push_str("-desc\">");
        head.push_str(&esc_text(descr));
        head.push_str("</desc>");
        aria.push_str(" aria-describedby=\"");
        aria.push_str(&prefix);
        aria.push_str("-desc\"");
    }

    let mut out = String::with_capacity(content.len() + 1024);
    out.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\" class=\"ondine ondine-");
    out.push_str(r#type);
    out.push_str("\" viewBox=\"0 0 ");
    push_number(&mut out, width);
    out.push(' ');
    push_number(&mut out, height);
    out.push_str("\" width=\"");
    push_number(&mut out, width);
    out.push_str("\" height=\"");
    push_number(&mut out, height);
    out.push_str("\" style=\"");
    if options.responsive != Some(false) {
        out.push_str("max-width:100%;height:auto;");
    }
    out.push_str(ROOT_STYLE);
    out.push_str(&font_style(font_size));
    out.push_str(
        "\" fill=\"var(--_fg)\" role=\"graphics-document document\" aria-roledescription=\"",
    );
    out.push_str(r#type);
    out.push('"');
    out.push_str(&aria);
    out.push('>');
    out.push_str(&head);
    out.push_str(content);
    out.push_str("</svg>");
    out
}
