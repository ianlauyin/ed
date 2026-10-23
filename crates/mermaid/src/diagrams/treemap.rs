//! Treemaps: nested sections and valued leaves drawn as nested rectangles. Port of Pele's
//! `src/diagrams/treemap/index.ts`.

pub(crate) mod format;
pub(crate) mod layout;
pub(crate) mod model;
pub(crate) mod parser;
pub(crate) mod render;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::Options;
use crate::error::Error;

/// The `treemap` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render(&model, input.config, options))
}

/// The parsed treemap: Pele's `TreemapModel`.
pub(crate) use model::TreemapModel as Model;

/// Parses preprocessed treemap `source`, as Pele's `treemap.parse` does. `title` comes from
/// front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    Ok(model::build_treemap(parser::parse_treemap(source)?, title))
}
