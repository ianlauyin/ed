//! The railroad model: rules whose definitions are trees of terminals, names, and the ways to
//! combine them. Port of Pele's `src/diagrams/railroad/types.ts`.

/// A node of a rule's definition: Pele's `RailroadNode`. Parts are indices into
/// [`RailroadModel::nodes`], so deep definitions need no recursion to build or drop.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RailroadNode {
    /// One of several alternatives.
    Choice(Vec<usize>),
    /// A reference to another rule.
    NonTerminal(String),
    /// An element that may be skipped.
    Optional(usize),
    /// An element repeated from `min` to `max` times. `max` is infinite when the count has no
    /// upper bound.
    Repetition {
        /// The repeated element.
        element: usize,
        /// The most repetitions.
        max: f64,
        /// The fewest repetitions.
        min: f64,
    },
    /// Elements one after another.
    Sequence(Vec<usize>),
    /// Text that stands for something the grammar does not spell out.
    Special(String),
    /// Literal text.
    Terminal(String),
}

impl RailroadNode {
    /// The node's parts, in order: Pele's `parts`.
    pub(crate) fn parts(&self) -> &[usize] {
        match self {
            Self::Choice(list) | Self::Sequence(list) => list,
            Self::Optional(element) | Self::Repetition { element, .. } => {
                std::slice::from_ref(element)
            }
            Self::NonTerminal(_) | Self::Special(_) | Self::Terminal(_) => &[],
        }
    }
}

/// A rule: Pele's `RailroadRule`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RailroadRule {
    /// The definition, as an index into [`RailroadModel::nodes`].
    pub(crate) definition: usize,
    /// The rule's name.
    pub(crate) name: String,
}

/// Which of Mermaid's four grammars the text was written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Notation {
    /// `railroad-abnf-beta`.
    Abnf,
    /// `railroad-ebnf-beta`.
    Ebnf,
    /// `railroad-peg-beta`.
    Peg,
    /// `railroad-beta`.
    Railroad,
}

impl Notation {
    /// The notation as Pele names it.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Abnf => "abnf",
            Self::Ebnf => "ebnf",
            Self::Peg => "peg",
            Self::Railroad => "railroad",
        }
    }
}

/// The parsed railroad diagram: Pele's `RailroadModel`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RailroadModel {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    /// Every node of every rule's definition.
    pub(crate) nodes: Vec<RailroadNode>,
    /// The grammar the text was written in.
    pub(crate) notation: Notation,
    /// Every rule in source order. A name defined twice appears twice.
    pub(crate) rules: Vec<RailroadRule>,
    /// The title.
    pub(crate) title: Option<String>,
}
