//! Requirement diagrams: requirements and elements with their fields, joined by relationships.
//! Port of Pele's `src/diagrams/requirement/index.ts`.

pub(crate) mod db;
pub(crate) mod graph;
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

/// The `requirement` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let db = parse(input.source, input.title)?;
    Ok(render::render(&db, options))
}

/// The parsed requirement model: Pele's `RequirementDb`.
pub(crate) use db::RequirementDb as Model;

/// Parses preprocessed requirement `source`, as Pele's `requirement.parse` does. `title` comes
/// from front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a limit error when classes copy too
/// many style declarations.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        db.title = Some(title.to_owned());
    }
    parser::parse_requirement(source, &mut db)?;
    Ok(db)
}
