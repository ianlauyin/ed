//! Event models: frames in swimlanes along a timeline, with notes and given/when/then
//! specifications. Port of Pele's `src/diagrams/eventmodeling/index.ts`.

pub(crate) mod model;
pub(crate) mod parser;
pub(crate) mod render;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::Options;
use crate::error::Error;

/// The `eventmodeling` diagram type. It parses source with its `%%` comment lines kept.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: true, render: render_input };

/// Parses and draws the diagram.
fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render(&model, options))
}

/// The parsed event model: Pele's `EventModelingModel`.
pub(crate) use model::EventModelingModel as Model;

/// Parses preprocessed event modeling `source`, as Pele's `eventmodeling.parse` does. `title`
/// comes from front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for a repeated frame ID.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    model::build_event_model(&parser::parse_event_model(source)?, source, title)
}
