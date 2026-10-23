//! Gantt charts: tasks on a time axis, with sections, dependencies, excluded days, and
//! milestones. Port of Pele's `src/diagrams/gantt/index.ts`.
//!
//! Pele reads and writes dates in the host's local time zone. Ondine works in UTC, so it matches
//! Pele running with `TZ=UTC`. Pele schedules against the current time when the host gives no
//! `now`. The compiler reads no clock, so it schedules against the epoch instead.

pub(crate) mod axis;
pub(crate) mod dates;
pub(crate) mod db;
pub(crate) mod js_date;
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

/// The `gantt` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

/// Parses and draws a gantt chart.
fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let mut db = parse(input.source, input.config, input.title)?;
    render::render(&mut db, input.config, options)
}

/// The parsed gantt model: Pele's `GanttDb`.
pub(crate) use db::GanttDb as Model;

/// Parses preprocessed gantt `source`, as Pele's `gantt.parse` does. `config` is the diagram
/// configuration, of which `gantt.displayMode` is read. `title` comes from front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for a task with no
/// end or too many fields.
pub(crate) fn parse(source: &str, config: &Value, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::new();
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        db.title = Some(title.to_owned());
    }
    if let Some(Value::String(mode)) = property(config, "gantt").and_then(|g| property(g, "displayMode")) {
        mode.clone_into(&mut db.display_mode);
    }
    parser::parse_gantt(source, &mut db)?;
    Ok(db)
}

/// A property of a YAML mapping, as JavaScript's `value.key` reads it.
fn property<'v>(value: &'v Value, key: &str) -> Option<&'v Value> {
    let Value::Object(entries) = value else {
        return None;
    };
    entries.iter().find(|(own, _)| own == key).map(|(_, value)| value)
}
