//! The C4 diagram model. Port of Pele's `src/diagrams/c4/types.ts`.

/// A `$key="value"` argument.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct C4Named {
    /// The name after `$`.
    pub(crate) key: String,
    /// The quoted value.
    pub(crate) value: String,
}

/// An argument of a statement: positional text or a named argument.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum C4Attr {
    /// A `$key="value"` argument.
    Named(C4Named),
    /// A positional argument, trimmed.
    Text(String),
}

/// A shape or boundary. Pele's `C4Shape` and `C4Boundary` share this record: `kind` is set on
/// shapes and `node_type` on deployment nodes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct C4Element {
    /// The alias, or empty when a named argument was written in its place.
    pub(crate) alias: String,
    /// A named argument written where the alias belongs. It is applied to the field it names.
    pub(crate) alias_attr: Option<C4Named>,
    /// `$bgColor`.
    pub(crate) bg_color: Option<String>,
    /// `$borderColor`.
    pub(crate) border_color: Option<String>,
    /// The description.
    pub(crate) descr: Option<String>,
    /// Named arguments that set no known field, in the order they were first set.
    pub(crate) extra: Option<Vec<(String, String)>>,
    /// `$fontColor`.
    pub(crate) font_color: Option<String>,
    /// Mermaid's typeC4Shape of a shape: person, `external_system_db`, `container_queue` and so
    /// on. Empty on a boundary.
    pub(crate) kind: String,
    /// The label.
    pub(crate) label: String,
    /// A named argument written where the label belongs. It is applied to the field it names.
    pub(crate) label_attr: Option<C4Named>,
    /// `$legendSprite`.
    pub(crate) legend_sprite: Option<String>,
    /// `$legendText`.
    pub(crate) legend_text: Option<String>,
    /// The link.
    pub(crate) link: Option<String>,
    /// Set on deployment nodes: `node`, `nodeL` or `nodeR`.
    pub(crate) node_type: Option<String>,
    /// The index of the boundary it was declared in.
    pub(crate) parent: Option<usize>,
    /// `$shadowing`.
    pub(crate) shadowing: Option<String>,
    /// `$shape`.
    pub(crate) shape: Option<String>,
    /// The sprite.
    pub(crate) sprite: Option<String>,
    /// The tags.
    pub(crate) tags: Option<String>,
    /// The technology.
    pub(crate) techn: Option<String>,
    /// The type of a boundary.
    pub(crate) r#type: Option<String>,
}

/// A relation between two shapes or boundaries.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct C4Rel {
    /// The description.
    pub(crate) descr: Option<String>,
    /// Named arguments that set no known field, in the order they were first set.
    pub(crate) extra: Option<Vec<(String, String)>>,
    /// The alias it starts at, or empty when a named argument was written in its place.
    pub(crate) from: String,
    /// A named argument written where the start belongs.
    pub(crate) from_attr: Option<C4Named>,
    /// The label.
    pub(crate) label: String,
    /// A named argument written where the label belongs. It is applied to the field it names.
    pub(crate) label_attr: Option<C4Named>,
    /// `$lineColor`.
    pub(crate) line_color: Option<String>,
    /// The link.
    pub(crate) link: Option<String>,
    /// How far the label moves right, as JavaScript's `parseInt` read it.
    pub(crate) offset_x: Option<f64>,
    /// How far the label moves down, as JavaScript's `parseInt` read it.
    pub(crate) offset_y: Option<f64>,
    /// The sprite.
    pub(crate) sprite: Option<String>,
    /// The tags.
    pub(crate) tags: Option<String>,
    /// The technology.
    pub(crate) techn: Option<String>,
    /// `$textColor`.
    pub(crate) text_color: Option<String>,
    /// The alias it ends at, or empty when a named argument was written in its place.
    pub(crate) to: String,
    /// A named argument written where the end belongs.
    pub(crate) to_attr: Option<C4Named>,
    /// `rel`, `birel`, `rel_u`, `rel_d`, `rel_l`, `rel_r` or `rel_b`.
    pub(crate) r#type: String,
}
