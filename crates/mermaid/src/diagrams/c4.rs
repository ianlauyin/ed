//! C4 diagrams: people, systems, containers and components in nested boundaries, joined by
//! relations. Port of Pele's `src/diagrams/c4/index.ts`.

pub(crate) mod db;
pub(crate) mod layout;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod render;
pub(crate) mod route;
pub(crate) mod tokens;
pub(crate) mod types;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::Options;
use crate::error::Error;

/// The `c4` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    render::render(&model, options)
}

/// The parsed C4 model: Pele's `C4Db`.
pub(crate) use db::C4Db as Model;

/// Parses preprocessed C4 `source`, as Pele's `c4.parse` does. `title` comes from front matter
/// and is kept when it is not empty.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        db.title = Some(title.to_owned());
    }
    parser::parse_c4(source, &mut db)?;
    Ok(db)
}
