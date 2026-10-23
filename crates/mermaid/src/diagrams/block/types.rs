//! The block diagram model. Port of Pele's `src/diagrams/block/types.ts`.

/// One shape for blocks, edges, and the statements that set columns, classes, and styles, as in
/// Mermaid. Blocks live in the model's arena and name their children by index.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Block {
    /// The marker at the end of an edge.
    pub(crate) arrow_type_end: Option<&'static str>,
    /// The marker at the start of an edge.
    pub(crate) arrow_type_start: Option<&'static str>,
    /// Child blocks, by arena index.
    pub(crate) children: Option<Vec<usize>>,
    /// Class names applied with `class`.
    pub(crate) classes: Option<Vec<String>>,
    /// The order of a composite among composites.
    pub(crate) color_index: Option<u32>,
    /// Grid width: a count, or -1 to put every child on one row.
    pub(crate) columns: Option<f64>,
    /// The declarations of a `classDef` statement.
    pub(crate) css: Option<String>,
    /// The directions of a block arrow, or those of the block an edge ends at.
    pub(crate) directions: Option<Vec<String>>,
    /// The id an edge ends at.
    pub(crate) end: Option<String>,
    /// The id. A column setting has none, and neither has a composite whose header was a link.
    pub(crate) id: Option<String>,
    /// The label.
    pub(crate) label: Option<String>,
    /// `dotted` or `solid`, for an edge.
    pub(crate) pattern: Option<&'static str>,
    /// The id an edge starts at.
    pub(crate) start: Option<String>,
    /// The class name of a `class` statement.
    pub(crate) style_class: Option<String>,
    /// Declarations applied with `style`.
    pub(crate) styles: Option<Vec<String>>,
    /// The declarations of a `style` statement.
    pub(crate) styles_str: Option<String>,
    /// `thick` or `normal`, for an edge.
    pub(crate) thickness: Option<&'static str>,
    /// The kind of block or statement, such as `square`, `composite`, `edge`, or `classDef`.
    pub(crate) r#type: &'static str,
    /// The number of cells a `space` statement asked for.
    pub(crate) width: Option<f64>,
    /// The number of columns the block spans.
    pub(crate) width_in_columns: Option<f64>,
}

/// A `classDef`: declarations for shapes and for text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct BlockClassDef {
    /// The class name.
    pub(crate) id: String,
    /// Declarations for shapes.
    pub(crate) styles: Vec<String>,
    /// Declarations for text.
    pub(crate) text_styles: Vec<String>,
}
