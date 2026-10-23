//! Pie charts: labeled values drawn as slices of a circle. Port of Pele's
//! `src/diagrams/pie/index.ts`.

pub(crate) mod model;
pub(crate) mod parser;
pub(crate) mod render;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::Options;
use crate::error::Error;

/// The `pie` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render(&model, input.config, options))
}

/// The parsed pie chart: Pele's `PieModel`.
pub(crate) use model::PieModel as Model;

/// Parses preprocessed pie `source`, as Pele's `pie.parse` does. `title` comes from front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for a negative slice.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    model::build_pie(parser::parse_pie(source)?, title)
}
