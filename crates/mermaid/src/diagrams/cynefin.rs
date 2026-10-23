//! Cynefin diagrams: items sorted into the five Cynefin domains, with transitions between them.
//! Port of Pele's `src/diagrams/cynefin/index.ts`.

pub(crate) mod boundaries;
pub(crate) mod model;
pub(crate) mod parser;
pub(crate) mod render;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::Options;
use crate::error::Error;

/// The `cynefin` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render(&model, input.config, options))
}

/// The parsed Cynefin diagram: Pele's `CynefinModel`.
pub(crate) use model::CynefinModel as Model;

/// Parses preprocessed Cynefin `source`, as Pele's `cynefin.parse` does. `title` comes from
/// front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    Ok(model::build_cynefin(parser::parse_cynefin(source)?, title))
}
