//! A diagram title above its content. Port of Pele's `src/svg/title.ts`.

use crate::svg::builder::label_svg;
use crate::svg::builder::push_num;
use crate::text::label::layout_label;
use crate::text::measure::BOLD;

/// Content with a title placed above it, and the size of both together.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Titled {
    /// The SVG content.
    pub(crate) content: String,
    /// Height of the whole.
    pub(crate) height: f64,
    /// Width of the whole.
    pub(crate) width: f64,
}

/// Puts a bold `title` at font `size` above content of `width` by `height` that was laid out
/// without one, `pad` from the top. Without a title, the content is returned as it is.
pub(crate) fn with_title(title: Option<&str>, size: f64, pad: f64, width: f64, height: f64, content: String) -> Titled {
    let label = layout_label(title, false, size, 4000.0, BOLD, None);
    if label.height == 0.0 {
        return Titled { content, height, width };
    }

    let shift = label.height + 12.0;
    let full = width.max((label.width + 2.0 * pad).ceil());
    let mut out = label_svg(
        &label,
        full / 2.0,
        pad + label.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    out.push_str("<g transform=\"translate(");
    push_num(&mut out, (full - width) / 2.0);
    out.push(',');
    push_num(&mut out, shift);
    out.push_str(")\">");
    out.push_str(&content);
    out.push_str("</g>");
    Titled { content: out, height: (height + shift).ceil(), width: full }
}
