//! The root SVG document around a diagram's content. Port of Pele's `src/svg/root.ts`.

use crate::svg::theme::ROOT_STYLE;
use crate::svg::theme::font_style;
use crate::yaml::push_number;

/// Wraps a diagram's content in the root `<svg>` document of the given fixed size, with its
/// theme, font at `font_size`, and diagram `type` class.
pub(crate) fn svg_document(r#type: &str, width: f64, height: f64, font_size: f64, content: &str) -> String {
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
    out.push_str(ROOT_STYLE);
    out.push_str(&font_style(font_size));
    out.push_str("\" fill=\"var(--_fg)\">");
    out.push_str(content);
    out.push_str("</svg>");
    out
}
