//! Sankey diagrams: flows between named nodes, drawn as bands between bars. Port of Pele's
//! `src/diagrams/sankey/index.ts`.

pub(crate) mod db;
pub(crate) mod layout;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod render;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::Options;
use crate::error::Error;

/// The `sankey` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let db = parse(input.source, input.title)?;
    render::render(&db, input.config, options)
}

/// The parsed sankey model: Pele's `SankeyDb`.
pub(crate) use db::SankeyDb as Model;

/// Parses preprocessed sankey `source`, as Pele's `sankey.parse` does. `title` comes from front
/// matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        db.title = Some(title.to_owned());
    }
    parser::parse_sankey(&parser::prepare_text_for_parsing(source), &mut db)?;
    Ok(db)
}
