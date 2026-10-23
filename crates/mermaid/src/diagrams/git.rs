//! Git graphs: Mermaid's `gitGraph`. Port of Pele's `src/diagrams/git/index.ts`.

pub(crate) mod db;
pub(crate) mod parser;
pub(crate) mod render;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::Options;
use crate::error::Error;
use crate::yaml::Value;

/// The `gitGraph` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let model = parse(input.source, input.config, input.title)?;
    Ok(render::render(&model, input.config, options))
}

/// The built git graph: Pele's `GitModel`.
pub(crate) use db::GitModel as Model;

/// Parses preprocessed git graph `source` with `config`, as Pele's `gitGraph.parse` does. `title`
/// comes from front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for an operation
/// Mermaid's database rejects.
pub(crate) fn parse(source: &str, config: &Value, title: Option<&str>) -> Result<Model, Error> {
    db::build_git(parser::parse_git(source)?, config, title)
}
