//! Token kinds of the state lexer and the names Mermaid's parser reports for them. Port of Pele's
//! `src/diagrams/state/tokens.ts`.

/// A token kind. Each variant is the Pele `T` member it documents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum T {
    /// Pele's `T.acc_descr`.
    AccDescr,
    /// Pele's `T.acc_descr_multiline_value`.
    AccDescrMultilineValue,
    /// Pele's `T.acc_descr_value`.
    AccDescrValue,
    /// Pele's `T.acc_title`.
    AccTitle,
    /// Pele's `T.acc_title_value`.
    AccTitleValue,
    /// Pele's `T.ARROW`.
    Arrow,
    /// Pele's `T.AS`.
    As,
    /// Pele's `T.CHOICE`.
    Choice,
    /// Pele's `T.class`.
    Class,
    /// Pele's `T.classDef`.
    ClassDef,
    /// Pele's `T.CLASSDEF_ID`.
    ClassDefId,
    /// Pele's `T.CLASSDEF_STYLEOPTS`.
    ClassDefStyleOpts,
    /// Pele's `T.CLASSENTITY_IDS`.
    ClassEntityIds,
    /// Pele's `T.CLICK`.
    Click,
    /// Pele's `T.COMPOSIT_STATE`.
    CompositState,
    /// Pele's `T.CONCURRENT`.
    Concurrent,
    /// Pele's `T.DEFAULT`.
    Default,
    /// Pele's `T.DEFAULT_CLASSDEF_ID`.
    DefaultClassDefId,
    /// Pele's `T.DESCR`.
    Descr,
    /// Pele's `T.direction_bt`.
    DirectionBt,
    /// Pele's `T.direction_lr`.
    DirectionLr,
    /// Pele's `T.direction_rl`.
    DirectionRl,
    /// Pele's `T.direction_tb`.
    DirectionTb,
    /// Pele's `T.EDGE_STATE`.
    EdgeState,
    /// Pele's `T.END`.
    End,
    /// Pele's `T.ERROR`.
    Error,
    /// Pele's `T.FORK`.
    Fork,
    /// Pele's `T.HIDE_EMPTY`.
    HideEmpty,
    /// Pele's `T.HREF`.
    Href,
    /// Pele's `T.ID`.
    Id,
    /// Pele's `T.INVALID`.
    Invalid,
    /// Pele's `T.JOIN`.
    Join,
    /// Pele's `T.left_of`.
    LeftOf,
    /// Pele's `T.NL`.
    Nl,
    /// Pele's `T.note`.
    Note,
    /// Pele's `T.NOTE_TEXT`.
    NoteText,
    /// Pele's `T.right_of`.
    RightOf,
    /// Pele's `T.scale`.
    Scale,
    /// Pele's `T.SD`.
    Sd,
    /// Pele's `T.STATE_DESCR`.
    StateDescr,
    /// Pele's `T.STRING`.
    String,
    /// Pele's `T.STRUCT_START`.
    StructStart,
    /// Pele's `T.STRUCT_STOP`.
    StructStop,
    /// Pele's `T.style`.
    Style,
    /// Pele's `T.STYLE_IDS`.
    StyleIds,
    /// Pele's `T.STYLE_SEPARATOR`.
    StyleSeparator,
    /// Pele's `T.STYLECLASS`.
    StyleClass,
    /// Pele's `T.STYLEDEF_STYLEOPTS`.
    StyleDefStyleOpts,
    /// Pele's `T.WIDTH`.
    Width,
}

impl T {
    /// The name Mermaid's parser reports for the token, as Pele's `TOKEN_NAMES` lists it.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::AccDescr => "acc_descr",
            Self::AccDescrMultilineValue => "acc_descr_multiline_value",
            Self::AccDescrValue => "acc_descr_value",
            Self::AccTitle => "acc_title",
            Self::AccTitleValue => "acc_title_value",
            Self::Arrow => "-->",
            Self::As => "AS",
            Self::Choice => "CHOICE",
            Self::Class => "class",
            Self::ClassDef => "classDef",
            Self::ClassDefId => "CLASSDEF_ID",
            Self::ClassDefStyleOpts => "CLASSDEF_STYLEOPTS",
            Self::ClassEntityIds => "CLASSENTITY_IDS",
            Self::Click => "CLICK",
            Self::CompositState => "COMPOSIT_STATE",
            Self::Concurrent => "CONCURRENT",
            Self::Default => "DEFAULT",
            Self::DefaultClassDefId => "DEFAULT_CLASSDEF_ID",
            Self::Descr => "DESCR",
            Self::DirectionBt => "direction_bt",
            Self::DirectionLr => "direction_lr",
            Self::DirectionRl => "direction_rl",
            Self::DirectionTb => "direction_tb",
            Self::EdgeState => "EDGE_STATE",
            Self::End => "$end",
            Self::Error | Self::Invalid => "INVALID",
            Self::Fork => "FORK",
            Self::HideEmpty => "HIDE_EMPTY",
            Self::Href => "HREF",
            Self::Id => "ID",
            Self::Join => "JOIN",
            Self::LeftOf => "left_of",
            Self::Nl => "NL",
            Self::Note => "note",
            Self::NoteText => "NOTE_TEXT",
            Self::RightOf => "right_of",
            Self::Scale => "scale",
            Self::Sd => "SD",
            Self::StateDescr => "STATE_DESCR",
            Self::String => "STRING",
            Self::StructStart => "STRUCT_START",
            Self::StructStop => "STRUCT_STOP",
            Self::Style => "style",
            Self::StyleIds => "STYLE_IDS",
            Self::StyleSeparator => "STYLE_SEPARATOR",
            Self::StyleClass => "STYLECLASS",
            Self::StyleDefStyleOpts => "STYLEDEF_STYLEOPTS",
            Self::Width => "WIDTH",
        }
    }
}
