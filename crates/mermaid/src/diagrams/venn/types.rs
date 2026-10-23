//! The venn diagram model. Port of Pele's `src/diagrams/venn/types.ts`.

/// A set (one id) or an overlap of sets (two or more ids, sorted), with its relative area.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct VennSubset {
    /// The label.
    pub(crate) label: Option<String>,
    /// The ids of the sets.
    pub(crate) sets: Vec<String>,
    /// The relative area.
    pub(crate) size: f64,
}

/// A line of text shown inside the region of the given sets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct VennText {
    /// The id.
    pub(crate) id: String,
    /// The label, shown in place of the id.
    pub(crate) label: Option<String>,
    /// The ids of the sets of the region.
    pub(crate) sets: Vec<String>,
}

/// Declarations for a set, an overlap, or a text. Targets are the sorted ids of a set or overlap,
/// or the id of a text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct VennStyle {
    /// Property and value pairs, in insertion order.
    pub(crate) styles: Vec<(String, String)>,
    /// The ids the declarations apply to.
    pub(crate) targets: Vec<String>,
}
