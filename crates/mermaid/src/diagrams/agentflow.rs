//! Agentflows: flowcharts of agent steps, tools, decisions, and references, grouped in flows,
//! with source positions for each statement and diagnostics about shapes and nesting. Port of
//! Pele's `src/diagrams/agentflow/index.ts`.
//!
//! Pele's `diagnostics.ts` only names the diagnostic vocabulary for tests, so it is not ported.

pub(crate) mod color_slots;
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

/// The `agentflow` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: true, render: render_input };

/// The parsed agentflow: Pele's `AgentflowDb`, with the graph Pele's `agentGraph` builds once per
/// model while parsing.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Model {
    /// The model.
    pub(crate) db: db::AgentflowDb,
    /// The nodes and edges to draw.
    pub(crate) graph: graph::AgentGraph,
}

fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let model = parse(input.source, input.config, input.title, input.line_offset, Some(input.max_edges))?;
    Ok(render::render(&model.db, &model.graph, input.config, options))
}

/// A property of a YAML mapping, as JavaScript's `value.key` reads it.
fn property<'v>(value: &'v Value, key: &str) -> Option<&'v Value> {
    let Value::Object(entries) = value else {
        return None;
    };
    entries.iter().find(|(own, _)| own == key).map(|(_, value)| value)
}

/// Parses preprocessed agentflow `source`, which keeps its `%%` comment lines, as Pele's
/// `agentflow.parse` does, and builds the graph to draw. `config` is the diagram configuration,
/// of which `direction`, `flowchart.inheritDir`, and `flowchart.curve` are read. `title` comes
/// from front matter, and `line_offset` counts the front matter lines. `max_edges` bounds the
/// edge count.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects and for metadata that is not YAML, a
/// semantic error for unknown shapes and out-of-range `linkStyle` indexes, and a limit error past
/// `max_edges` edges.
pub(crate) fn parse(
    source: &str,
    config: &Value,
    title: Option<&str>,
    line_offset: u32,
    max_edges: Option<usize>,
) -> Result<Model, Error> {
    // Mermaid reads these settings from the flowchart section for agentflow too.
    let flow = property(config, "flowchart");
    let inherit_dir = matches!(flow.and_then(|flow| property(flow, "inheritDir")), Some(Value::Bool(true)));
    let direction = match property(config, "direction") {
        Some(Value::String(direction)) => Some(direction.clone()),
        _ => None,
    };
    let mut db = db::AgentflowDb::new(db::Options { direction, inherit_dir, line_offset, max_edges });
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        db.title = Some(title.to_owned());
    }
    parser::parse_agentflow(source, &mut db)?;

    let curve = match flow.and_then(|flow| property(flow, "curve")) {
        Some(Value::String(curve)) => Some(curve.as_str()),
        _ => None,
    };
    let graph = graph::build_agent_graph(&mut db, render::PALETTE, curve);
    Ok(Model { db, graph })
}
