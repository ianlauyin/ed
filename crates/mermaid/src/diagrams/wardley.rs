//! Wardley maps: Mermaid's `wardley-beta`, components placed by visibility and evolution. Port of
//! Pele's `src/diagrams/wardley/index.ts`.

pub(crate) mod builder;
pub(crate) mod model;
pub(crate) mod parser;
pub(crate) mod render;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::Options;
use crate::error::Error;

/// The `wardley` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render(&model, input.config, options))
}

/// The parsed map: Pele's `WardleyModel`.
pub(crate) use model::WardleyModel as Model;

/// Parses preprocessed Wardley `source`, as Pele's `wardley.parse` does. `title` comes from front
/// matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for a coordinate out of
/// range or a pipeline without its component.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    model::build_wardley(parser::parse_wardley(source)?, title)
}
