//! The Ishikawa diagram model. Port of Pele's `src/diagrams/ishikawa/types.ts`.
//!
//! Pele nests nodes as objects. This port keeps them in one arena addressed by index, so a
//! deeply indented outline needs no recursion to build, walk, or drop.

/// A line of the outline: the effect, a category, or a cause.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct IshikawaNode {
    /// The indexes of the nodes under it, in authored order.
    pub(crate) children: Vec<usize>,
    /// The text, trimmed.
    pub(crate) text: String,
}
