//! Sequence diagrams: participants on lifelines, the messages between them, notes, and blocks.
//! Port of Pele's `src/diagrams/sequence/index.ts`.

pub(crate) mod actors;
pub(crate) mod colors;
pub(crate) mod db;
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
use crate::yaml::Value;

/// The `sequence` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let db = parse(input.source, input.config, input.title)?;
    Ok(render::render(&db, input.config, options))
}

/// The parsed sequence diagram model: Pele's `SeqDb`.
pub(crate) use db::SeqDb as Model;

/// Parses preprocessed sequence diagram `source`, as Pele's `sequence.parse` does. `config` is
/// the diagram configuration, of which `wrap` and `sequence.wrap` are read. `title` comes from
/// front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects or a participant config that is not valid
/// YAML, and a semantic error for a participant in two boxes, a misplaced `create` or `destroy`,
/// and a deactivation of an inactive participant.
pub(crate) fn parse(source: &str, config: &Value, title: Option<&str>) -> Result<Model, Error> {
    let wrap = match member(config, "wrap") {
        Some(Value::Bool(wrap)) => Some(*wrap),
        _ => None,
    };
    let sequence_wrap = member(config, "sequence")
        .and_then(|section| member(section, "wrap"))
        .is_some_and(|wrap| *wrap == Value::Bool(true));
    let mut db = Model::new(wrap, sequence_wrap);
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        db.set_diagram_title(title);
    }
    parser::parse_sequence(source, &mut db)?;
    Ok(db)
}

/// The value under `key` of a YAML mapping.
fn member<'v>(value: &'v Value, key: &str) -> Option<&'v Value> {
    match value {
        Value::Object(entries) => entries.iter().find(|(own, _)| own == key).map(|(_, v)| v),
        _ => None,
    }
}
