//! Token kinds of the block lexer and the names Mermaid's parser reports for them. Port of Pele's
//! `src/diagrams/block/tokens.ts`.

/// A token kind, in the order of Pele's `T` enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum T {
    /// End of input, after `Eof`.
    End,
    /// Unrecognized text.
    Error,
    /// `block-beta` or `block`.
    Key,
    /// `block:`, which opens a composite with an id.
    IdBlock,
    /// `columns n` or `columns auto`.
    Columns,
    /// Markdown string text.
    MdStr,
    /// String text.
    Str,
    /// `space` or `space:n`.
    SpaceBlock,
    /// `default`.
    Default,
    /// `linkStyle`.
    LinkStyle,
    /// `interpolate`.
    Interpolate,
    /// `classDef`.
    ClassDef,
    /// `DEFAULT` after `classDef`.
    DefaultClassDefId,
    /// A class name after `classDef`.
    ClassDefId,
    /// The styles of a `classDef`.
    ClassDefStyleOpts,
    /// `class`.
    Class,
    /// The ids of a `class` statement.
    ClassEntityIds,
    /// The class name of a `class` statement.
    StyleClass,
    /// `style`.
    Style,
    /// The ids of a `style` statement.
    StyleEntityIds,
    /// The declarations of a `style` statement.
    StyleDefinitionData,
    /// `accTitle:`.
    AccTitle,
    /// The text of an `accTitle`.
    AccTitleValue,
    /// `accDescr:`.
    AccDescr,
    /// The text of an `accDescr`.
    AccDescrValue,
    /// The text of an `accDescr { }`.
    AccDescrMultilineValue,
    /// `end`.
    EndKeyword,
    /// The opening of a node shape.
    NodeDStart,
    /// The closing of a node shape.
    NodeDEnd,
    /// `<[`.
    BlockArrowStart,
    /// `]>(...)`.
    BlockArrowEnd,
    /// A direction of a block arrow.
    Dir,
    /// A node id.
    NodeId,
    /// A link.
    Link,
    /// The start of a link with a label.
    StartLink,
    /// The quote before a link label.
    LinkLabel,
    /// `:n` after a node.
    Size,
    /// End of input.
    Eof,
}

impl T {
    /// The name Mermaid's parser reports, from Pele's `TOKEN_NAMES`.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::End => "$end",
            Self::Error => "INVALID",
            Self::Key => "BLOCK_DIAGRAM_KEY",
            Self::IdBlock => "id-block",
            Self::Columns => "COLUMNS",
            Self::MdStr => "MD_STR",
            Self::Str => "STR",
            Self::SpaceBlock => "SPACE_BLOCK",
            Self::Default => "DEFAULT",
            Self::LinkStyle => "LINKSTYLE",
            Self::Interpolate => "INTERPOLATE",
            Self::ClassDef => "classDef",
            Self::DefaultClassDefId => "DEFAULT_CLASSDEF_ID",
            Self::ClassDefId => "CLASSDEF_ID",
            Self::ClassDefStyleOpts => "CLASSDEF_STYLEOPTS",
            Self::Class => "class",
            Self::ClassEntityIds => "CLASSENTITY_IDS",
            Self::StyleClass => "STYLECLASS",
            Self::Style => "style",
            Self::StyleEntityIds => "STYLE_ENTITY_IDS",
            Self::StyleDefinitionData => "STYLE_DEFINITION_DATA",
            Self::AccTitle => "acc_title",
            Self::AccTitleValue => "acc_title_value",
            Self::AccDescr => "acc_descr",
            Self::AccDescrValue => "acc_descr_value",
            Self::AccDescrMultilineValue => "acc_descr_multiline_value",
            Self::EndKeyword => "end",
            Self::NodeDStart => "NODE_DSTART",
            Self::NodeDEnd => "NODE_DEND",
            Self::BlockArrowStart => "BLOCK_ARROW_START",
            Self::BlockArrowEnd => "BLOCK_ARROW_END",
            Self::Dir => "DIR",
            Self::NodeId => "NODE_ID",
            Self::Link => "LINK",
            Self::StartLink => "START_LINK",
            Self::LinkLabel => "LINK_LABEL",
            Self::Size => "SIZE",
            Self::Eof => "EOF",
        }
    }
}
