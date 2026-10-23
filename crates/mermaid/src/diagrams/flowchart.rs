//! Flowcharts: Mermaid's `graph` and `flowchart` diagrams, which swimlane diagrams share. Port of
//! Pele's `src/diagrams/flowchart/index.ts`.

pub(crate) mod db;
pub(crate) mod graph;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod render;
pub(crate) mod shapes;
pub(crate) mod tokens;
pub(crate) mod types;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::Options;
use crate::error::Error;
use crate::yaml::Value;

/// The `flowchart` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let db = parse(input.source, input.config, input.title, Some(input.max_edges))?;
    Ok(render::render(&db, input.config, options, None))
}

/// The parsed flowchart model: Pele's `FlowDb`.
pub(crate) use db::FlowDb as Model;

/// Parses preprocessed flowchart `source`, as Pele's `flowchart.parse` does. `config` is the
/// diagram configuration, of which `flowchart.inheritDir` is read. `title` comes from front
/// matter. `max_edges` bounds the edge count.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, a semantic error for unknown shapes and
/// out-of-range `linkStyle` indexes, and a limit error past `max_edges` edges.
pub(crate) fn parse(
    source: &str,
    config: &Value,
    title: Option<&str>,
    max_edges: Option<usize>,
) -> Result<Model, Error> {
    let inherit_dir =
        matches!(property(config, "flowchart").and_then(|flow| property(flow, "inheritDir")), Some(Value::Bool(true)));
    let mut db = Model::new(db::Options { inherit_dir, max_edges });
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        db.title = Some(title.to_owned());
    }
    parser::parse_flowchart(source, &mut db)?;
    Ok(db)
}

/// A property of a YAML mapping, as JavaScript's `value.key` reads it.
fn property<'v>(value: &'v Value, key: &str) -> Option<&'v Value> {
    let Value::Object(entries) = value else {
        return None;
    };
    entries.iter().find(|(own, _)| own == key).map(|(_, value)| value)
}
