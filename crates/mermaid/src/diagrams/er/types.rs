//! The records of an ER diagram. Port of Pele's `src/diagrams/er/types.ts`.

/// An entity's attribute.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ErAttribute {
    /// The comment, or empty.
    pub(crate) comment: String,
    /// Keys such as `PK`.
    pub(crate) keys: Vec<String>,
    /// The name.
    pub(crate) name: String,
    /// The type.
    pub(crate) r#type: String,
}

/// An entity.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ErEntity {
    /// The shown name in brackets, or empty.
    pub(crate) alias: String,
    /// Attributes in authored order.
    pub(crate) attributes: Vec<ErAttribute>,
    /// Class names, starting with `default`, separated by spaces.
    pub(crate) css_classes: String,
    /// Style declarations from `style` statements.
    pub(crate) css_styles: Vec<String>,
    /// `entity-`, the name, `-`, and the position among the entities.
    pub(crate) id: String,
    /// The name.
    pub(crate) label: String,
}

/// A cardinality, as Mermaid names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cardinality {
    /// `MD_PARENT`.
    MdParent,
    /// `ONE_OR_MORE`.
    OneOrMore,
    /// `ONLY_ONE`.
    OnlyOne,
    /// `ZERO_OR_MORE`.
    ZeroOrMore,
    /// `ZERO_OR_ONE`.
    ZeroOrOne,
}

impl Cardinality {
    /// Mermaid's name.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::MdParent => "MD_PARENT",
            Self::OneOrMore => "ONE_OR_MORE",
            Self::OnlyOne => "ONLY_ONE",
            Self::ZeroOrMore => "ZERO_OR_MORE",
            Self::ZeroOrOne => "ZERO_OR_ONE",
        }
    }

    /// The lower-case name, which names the marker drawn for it.
    pub(crate) fn marker(self) -> &'static str {
        match self {
            Self::MdParent => "md_parent",
            Self::OneOrMore => "one_or_more",
            Self::OnlyOne => "only_one",
            Self::ZeroOrMore => "zero_or_more",
            Self::ZeroOrOne => "zero_or_one",
        }
    }
}

/// The cardinalities and identification of a relationship. `card_b` sits at entity A's end of
/// the line and `card_a` at entity B's, as in Mermaid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RelSpec {
    /// The cardinality at entity B's end.
    pub(crate) card_a: Cardinality,
    /// The cardinality at entity A's end.
    pub(crate) card_b: Cardinality,
    /// Whether the relationship is identifying.
    pub(crate) identifying: bool,
}

/// A relationship. The ends hold an entity's id, or a subgraph's id when the name was a subgraph
/// at the time.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ErRelationship {
    /// The first end.
    pub(crate) entity_a: String,
    /// The second end.
    pub(crate) entity_b: String,
    /// The cardinalities and identification.
    pub(crate) rel_spec: RelSpec,
    /// The label.
    pub(crate) role_a: String,
}

/// A class definition from `classDef`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ErClassDef {
    /// The class name.
    pub(crate) id: String,
    /// Its declarations.
    pub(crate) styles: Vec<String>,
    /// Its declarations that mention a color, with `fill` read as `bgFill`.
    pub(crate) text_styles: Vec<String>,
}

/// A subgraph.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ErSubgraph {
    /// Class names from `class` statements.
    pub(crate) classes: Vec<String>,
    /// Style declarations from `style` statements.
    pub(crate) css_styles: Vec<String>,
    /// The direction inside, when set.
    pub(crate) dir: Option<&'static str>,
    /// The id.
    pub(crate) id: String,
    /// The names it holds that no earlier subgraph claimed.
    pub(crate) nodes: Vec<String>,
    /// The shown title.
    pub(crate) title: String,
}

/// An item of a subgraph's statement list, as Mermaid's grammar hands it over.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DocItem {
    /// A `direction` statement.
    Dir(&'static str),
    /// Any other statement's value.
    Name(String),
}
