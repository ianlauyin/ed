//! Ishikawa diagrams: an effect and its causes, drawn as a fishbone. Port of Pele's
//! `src/diagrams/ishikawa/index.ts`.

pub(crate) mod db;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod render;
pub(crate) mod types;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::Options;
use crate::error::Error;

/// The `ishikawa` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render(&model, options))
}

/// The parsed Ishikawa model: Pele's `IshikawaDb`.
pub(crate) use db::IshikawaDb as Model;

/// Parses preprocessed Ishikawa `source`, as Pele's `ishikawa.parse` does. `title` comes from
/// front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    db.title = title.map(str::to_owned);
    parser::parse_ishikawa(source, &mut db)?;
    Ok(db)
}
