//! Mindmaps: an indented outline of nodes around one root, drawn as branches to both sides or as
//! an outline running down. Port of Pele's `src/diagrams/mindmap/index.ts`. Pele's
//! `parser.ts` only forwards to the shared outline grammar, so [`parse`] calls it directly.

pub(crate) mod db;
pub(crate) mod render;

use super::Diagram;
use super::Input;
use super::Rendered;
use super::common::outline::parse_outline;
use crate::Options;
use crate::error::Error;

/// The `mindmap` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    // Icons need a host resolver, which WebAssembly hosts do not pass yet.
    Ok(render::render(&model, input.config, options, None))
}

/// The parsed mindmap model: Pele's `MindmapDb`.
pub(crate) use db::MindmapDb as Model;

/// Parses preprocessed mindmap `source`, as Pele's `mindmap.parse` does. `title` comes from front
/// matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for a second root.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    db.title = title.map(str::to_owned);
    let parsed = parse_outline(source, &mut db, false);
    // An action's error came first: Pele throws it before the parser reads further.
    if let Some(error) = db.error.take() {
        return Err(error);
    }
    parsed?;
    Ok(db)
}
