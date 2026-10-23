//! Railroad diagrams: grammar rules drawn as tracks, written in Mermaid's own notation, EBNF,
//! ABNF, or PEG. Port of Pele's `src/diagrams/railroad/index.ts`.

pub(crate) mod abnf;
pub(crate) mod common;
pub(crate) mod db;
pub(crate) mod ebnf;
pub(crate) mod ir;
pub(crate) mod peg;
pub(crate) mod render;
pub(crate) mod types;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::Options;
use crate::error::Error;

/// The `railroad` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

/// Parses and draws the diagram.
fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render(&model, input.config, options))
}

/// The parsed railroad diagram: Pele's `RailroadDb`.
pub(crate) use types::RailroadModel as Model;

/// Parses preprocessed railroad `source`, as Pele's `railroad.parse` does. `title` comes from
/// front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    db::parse_railroad(source, title)
}
