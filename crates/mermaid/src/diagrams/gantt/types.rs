//! The gantt task record. Port of Pele's `src/diagrams/gantt/types.ts`. The model itself is
//! [`super::db::GanttDb`].

use super::js_date::Date;

/// A task's data as written: the whole data text and its start and end fields.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Raw {
    /// The data text, with its leading `:`.
    pub(crate) data: String,
    /// The end date, duration, or `until` reference.
    pub(crate) end: String,
    /// The start date or `after` reference, when one was given.
    pub(crate) start: Option<String>,
}

/// A task or vertical marker.
#[derive(Clone, Debug, Default, PartialEq)]
#[expect(clippy::struct_excessive_bools, reason = "mirrors Pele's task record")]
pub(crate) struct GanttTask {
    /// Whether the task is in progress.
    pub(crate) active: bool,
    /// Classes, such as `clickable`.
    pub(crate) classes: Vec<String>,
    /// Whether the task is critical.
    pub(crate) crit: bool,
    /// Whether the task is finished.
    pub(crate) done: bool,
    /// Unset until scheduled, and still unset when what it refers to never resolves.
    pub(crate) end_time: Option<Date>,
    /// The task's id, given or generated.
    pub(crate) id: String,
    /// Whether the end was given as a date.
    pub(crate) manual_end_time: bool,
    /// Whether the task is a milestone.
    pub(crate) milestone: bool,
    /// Row index in source order, or -1 for vertical markers, which take no row.
    pub(crate) order: i32,
    /// The id of the task written before this one.
    pub(crate) prev_task_id: Option<String>,
    /// Whether scheduling resolved the task.
    pub(crate) processed: bool,
    /// The task's data as written.
    pub(crate) raw: Raw,
    /// Where the bar ends when excluded days pushed the task's end out. `None` when the end was
    /// given as a date.
    pub(crate) render_end_time: Option<Date>,
    /// The section the task belongs to.
    pub(crate) section: String,
    /// Unset until scheduled.
    pub(crate) start_time: Option<Date>,
    /// The task's label, under the name Mermaid gives it.
    pub(crate) task: String,
    /// Whether the task is a vertical marker.
    pub(crate) vert: bool,
}
