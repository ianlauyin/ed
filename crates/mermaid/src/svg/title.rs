//! A diagram title above its content. Port of Pele's `src/svg/title.ts`.

use crate::svg::builder::{label_svg, push_num};
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
pub(crate) fn with_title(
    title: Option<&str>,
    size: f64,
    pad: f64,
    width: f64,
    height: f64,
    content: String,
) -> Titled {
    let label = layout_label(title, false, size, 4000.0, BOLD, None);
    if label.height == 0.0 {
        return Titled {
            content,
            height,
            width,
        };
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
    Titled {
        content: out,
        height: (height + shift).ceil(),
        width: full,
    }
}

#[cfg(test)]
mod tests {
    //! Expectations come from Pele's `withTitle` with its monospace measurer.

    use super::with_title;

    #[test]
    fn with_title_matches_pele() {
        for &(title, size, pad, width, height, w, h, content) in &[
            (None, 16.0, 8.0, 100.0, 50.0, 100.0, 50.0, "<g id=\"c\"/>"),
            (
                Some(""),
                16.0,
                8.0,
                100.0,
                50.0,
                100.0,
                50.0,
                "<g id=\"c\"/>",
            ),
            (
                Some("A diagram title"),
                16.0,
                8.0,
                100.0,
                50.5,
                160.0,
                87.0,
                "<text class=\"ondine-title\" font-weight=\"var(--_tw)\" text-anchor=\"middle\" xml:space=\"preserve\"><tspan x=\"80\" y=\"25.6\">A diagram title</tspan></text><g transform=\"translate(30,36)\"><g id=\"c\"/></g>",
            ),
            (
                Some("Wide title that is much wider than the content"),
                18.0,
                12.0,
                40.0,
                20.0,
                521.0,
                59.0,
                "<text class=\"ondine-title\" font-weight=\"var(--_tw)\" text-anchor=\"middle\" xml:space=\"preserve\"><tspan x=\"260.5\" y=\"31.8\">Wide title that is much wider than the content</tspan></text><g transform=\"translate(240.5,39)\"><g id=\"c\"/></g>",
            ),
            (
                Some("x &amp; y"),
                16.0,
                8.0,
                33.3,
                10.0,
                64.0,
                46.0,
                "<text class=\"ondine-title\" font-weight=\"var(--_tw)\" text-anchor=\"middle\" xml:space=\"preserve\"><tspan x=\"32\" y=\"25.6\">x &amp; y</tspan></text><g transform=\"translate(15.35,36)\"><g id=\"c\"/></g>",
            ),
        ] {
            let titled = with_title(title, size, pad, width, height, "<g id=\"c\"/>".to_owned());
            assert_eq!((titled.width, titled.height), (w, h), "{title:?}");
            assert_eq!(titled.content, content, "{title:?}");
        }
    }
}
