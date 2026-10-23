//! Kanban boards: columns of cards written as an indented outline, with `@{ ... }` metadata.
//! Port of Pele's `src/diagrams/kanban/index.ts`. Pele's `parser.ts` only forwards to the shared
//! outline grammar, so [`parse`] calls it directly.

pub(crate) mod db;
pub(crate) mod render;

use super::Diagram;
use super::Input;
use super::Rendered;
use super::common::outline::parse_outline;
use crate::Options;
use crate::error::Error;

/// The `kanban` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    // Icons need a host resolver, which WebAssembly hosts do not pass yet.
    Ok(render::render(&model, input.config, options, None))
}

/// The parsed kanban model: Pele's `KanbanDb`.
pub(crate) use db::KanbanDb as Model;

/// Parses preprocessed kanban `source`, as Pele's `kanban.parse` does. `title` comes from front
/// matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects or metadata that is not YAML, and a semantic
/// error for a shape name that is not lowercase or a card before any column.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    db.title = title.map(str::to_owned);
    let parsed = parse_outline(source, &mut db, true);
    // An action's error came first: Pele throws it before the parser reads further.
    if let Some(error) = db.error.take() {
        return Err(error);
    }
    parsed?;
    Ok(db)
}
