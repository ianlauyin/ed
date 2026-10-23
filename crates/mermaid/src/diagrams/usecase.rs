//! Use case diagrams: actors, use cases, notes, JSON tables, and system boundaries, joined by
//! relationships. Port of Pele's `src/diagrams/usecase/index.ts`.

pub(crate) mod db;
pub(crate) mod graph;
pub(crate) mod json;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod render;
pub(crate) mod tokens;
pub(crate) mod types;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::Options;
use crate::error::Error;

/// The `usecase` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

/// Parses and draws the diagram.
fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render(&model, input.config, options))
}

/// The parsed use case model: Pele's `UsecaseModel`.
pub(crate) use types::UsecaseModel as Model;

/// Parses preprocessed use case `source`, as Pele's `usecase.parse` does. `title` comes from
/// front matter.
///
/// # Errors
///
/// Returns Mermaid's lexing, parsing, and JSON errors as syntax errors, and its model errors as
/// semantic errors.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    parser::parse_usecase(source, title)
}
