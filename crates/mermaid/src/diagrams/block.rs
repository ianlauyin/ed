//! Block diagrams: Mermaid's `block-beta` grid of blocks, composites, and links. Port of Pele's
//! `src/diagrams/block/index.ts`.

pub(crate) mod arrows;
pub(crate) mod db;
pub(crate) mod layout;
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

/// The `block` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    render::render(&model, input.config, options)
}

/// The parsed block diagram model: Pele's `BlockDb`.
pub(crate) use db::BlockDb as Model;

/// Parses preprocessed block diagram `source`, as Pele's `block.parse` does. `title` comes from
/// front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a limit error past 100,000 space blocks.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    db.title = title.map(str::to_owned);
    parser::parse_block(source, &mut db)?;
    Ok(db)
}
