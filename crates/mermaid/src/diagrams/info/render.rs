//! Draws an info diagram: the title and the library version. Port of Pele's
//! `src/diagrams/info/render.ts`.

use super::model::InfoModel;
use crate::Options;
use crate::diagrams::Rendered;
use crate::svg::builder::label_svg;
use crate::svg::root::svg_document;
use crate::text::label::layout_label;
use crate::text::measure::BOLD;

/// The text drawn under the title. Pele draws its own name and version, so this names Ondine's.
const VERSION: &str = concat!("Ondine v", env!("CARGO_PKG_VERSION"));

/// Draws `model`.
pub(crate) fn render(model: &InfoModel, options: &Options) -> Rendered {
    let size = options.font_size;
    let pad = options.padding;

    let title = layout_label(model.title.as_deref(), false, size, 4000.0, BOLD, None);
    let title_height = if title.height > 0.0 { title.height + 8.0 } else { 0.0 };
    let text = layout_label(Some(VERSION), false, size, 4000.0, 0, None);

    let width = (title.width.max(text.width) + 2.0 * pad).ceil();
    let height = (title_height + text.height + 2.0 * pad).ceil();
    let mut content = label_svg(
        &title,
        width / 2.0,
        pad + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str(&label_svg(
        &text,
        width / 2.0,
        pad + title_height + text.height / 2.0,
        " class=\"ondine-version\" fill=\"var(--_m)\"",
        None,
    ));

    let svg = svg_document("info", width, height, size, &content);
    Rendered { height, svg, width }
}
