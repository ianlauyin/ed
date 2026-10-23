//! Class diagrams: classes with members, namespaces, notes, and relations. Port of Pele's
//! `src/diagrams/class/index.ts`.

pub(crate) mod db;
pub(crate) mod graph;
pub(crate) mod lexer;
pub(crate) mod markers;
pub(crate) mod members;
pub(crate) mod parser;
pub(crate) mod render;
pub(crate) mod tokens;
pub(crate) mod types;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::Options;
use crate::error::Error;

/// The `class` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

/// Parses and draws a class diagram.
fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let db = parse(input.source, input.title)?;
    Ok(render::render(&db, input.config, options))
}

/// The parsed class model: Pele's `ClassDb`.
pub(crate) use db::ClassDb as Model;

/// Parses preprocessed class diagram `source`, as Pele's `classDiagram.parse` does. `title`
/// comes from front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        db.title = Some(title.to_owned());
    }
    parser::parse_class_diagram(source, &mut db)?;
    Ok(db)
}
