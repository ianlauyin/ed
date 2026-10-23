//! Quadrant charts: points placed on two axes split into four labelled quadrants. Port of Pele's
//! `src/diagrams/quadrant/index.ts`.

pub(crate) mod db;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod render;
pub(crate) mod tokens;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::Options;
use crate::error::Error;

/// The `quadrantChart` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render(&model, input.config, options))
}

/// The parsed quadrant chart model: Pele's `QuadrantDb`.
pub(crate) use db::QuadrantDb as Model;

/// Parses preprocessed quadrant chart `source`, as Pele's `quadrantChart.parse` does. `title`
/// comes from front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for an invalid point
/// style.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model { title: title.map(str::to_owned), ..Model::default() };
    parser::parse_quadrant(source, &mut db)?;
    Ok(db)
}
