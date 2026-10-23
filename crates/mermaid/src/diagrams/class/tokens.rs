//! Token kinds of the class lexer and the names Mermaid's parser reports for them. Port of
//! Pele's `src/diagrams/class/tokens.ts`.

/// A token kind. Variant order mirrors Pele's `T` enum and indexes [`NAMES`], so it is observable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub(crate) enum T {
    /// Pele's `T.END`.
    End,
    /// Pele's `T.ERROR`.
    Error,
    /// Pele's `T.CLASS_DIAGRAM`.
    ClassDiagram,
    /// Pele's `T.NEWLINE`.
    Newline,
    /// Pele's `T.EOF`.
    Eof,
    /// Pele's `T.SQS`.
    Sqs,
    /// Pele's `T.STR`.
    Str,
    /// Pele's `T.SQE`.
    Sqe,
    /// Pele's `T.DOT`.
    Dot,
    /// Pele's `T.GENERICTYPE`.
    Generictype,
    /// Pele's `T.LABEL`.
    Label,
    /// Pele's `T.acc_title`.
    AccTitle,
    /// Pele's `T.acc_title_value`.
    AccTitleValue,
    /// Pele's `T.acc_descr`.
    AccDescr,
    /// Pele's `T.acc_descr_value`.
    AccDescrValue,
    /// Pele's `T.acc_descr_multiline_value`.
    AccDescrMultilineValue,
    /// Pele's `T.STRUCT_START`.
    StructStart,
    /// Pele's `T.STRUCT_STOP`.
    StructStop,
    /// Pele's `T.NAMESPACE`.
    Namespace,
    /// Pele's `T.STYLE_SEPARATOR`.
    StyleSeparator,
    /// Pele's `T.ANNOTATION_START`.
    AnnotationStart,
    /// Pele's `T.ANNOTATION_END`.
    AnnotationEnd,
    /// Pele's `T.CLASS`.
    Class,
    /// Pele's `T.MEMBER`.
    Member,
    /// Pele's `T.NOTE_FOR`.
    NoteFor,
    /// Pele's `T.NOTE`.
    Note,
    /// Pele's `T.CLASSDEF`.
    Classdef,
    /// Pele's `T.ALPHA`.
    Alpha,
    /// Pele's `T.COMMA`.
    Comma,
    /// Pele's `T.direction_tb`.
    DirectionTb,
    /// Pele's `T.direction_bt`.
    DirectionBt,
    /// Pele's `T.direction_rl`.
    DirectionRl,
    /// Pele's `T.direction_lr`.
    DirectionLr,
    /// Pele's `T.AGGREGATION`.
    Aggregation,
    /// Pele's `T.EXTENSION`.
    Extension,
    /// Pele's `T.COMPOSITION`.
    Composition,
    /// Pele's `T.DEPENDENCY`.
    Dependency,
    /// Pele's `T.LOLLIPOP`.
    Lollipop,
    /// Pele's `T.LINE`.
    Line,
    /// Pele's `T.DOTTED_LINE`.
    DottedLine,
    /// Pele's `T.CALLBACK`.
    Callback,
    /// Pele's `T.LINK`.
    Link,
    /// Pele's `T.LINK_TARGET`.
    LinkTarget,
    /// Pele's `T.CLICK`.
    Click,
    /// Pele's `T.CALLBACK_NAME`.
    CallbackName,
    /// Pele's `T.CALLBACK_ARGS`.
    CallbackArgs,
    /// Pele's `T.HREF`.
    Href,
    /// Pele's `T.STYLE`.
    Style,
    /// Pele's `T.CSSCLASS`.
    Cssclass,
    /// Pele's `T.COLON`.
    Colon,
    /// Pele's `T.BRKT`.
    Brkt,
    /// Pele's `T.PCT`.
    Pct,
    /// Pele's `T.MINUS`.
    Minus,
    /// Pele's `T.UNICODE_TEXT`.
    UnicodeText,
    /// Pele's `T.BQUOTE_STR`.
    BquoteStr,
    /// Pele's `T.EDGE_STATE`.
    EdgeState,
    /// Pele's `T.EOF_IN_STRUCT`.
    EofInStruct,
    /// Pele's `T.OPEN_IN_STRUCT`.
    OpenInStruct,
    /// Pele's `T.PLUS`.
    Plus,
    /// Pele's `T.EQUALS`.
    Equals,
    /// Pele's `T.PUNCTUATION`.
    Punctuation,
}

/// The names Mermaid's parser reports for each token kind, indexed by [`T`].
pub(crate) const NAMES: [&str; 61] = [
    "$end",
    "INVALID",
    "CLASS_DIAGRAM",
    "NEWLINE",
    "EOF",
    "SQS",
    "STR",
    "SQE",
    "DOT",
    "GENERICTYPE",
    "LABEL",
    "acc_title",
    "acc_title_value",
    "acc_descr",
    "acc_descr_value",
    "acc_descr_multiline_value",
    "STRUCT_START",
    "STRUCT_STOP",
    "NAMESPACE",
    "STYLE_SEPARATOR",
    "ANNOTATION_START",
    "ANNOTATION_END",
    "CLASS",
    "MEMBER",
    "NOTE_FOR",
    "NOTE",
    "CLASSDEF",
    "ALPHA",
    "COMMA",
    "direction_tb",
    "direction_bt",
    "direction_rl",
    "direction_lr",
    "AGGREGATION",
    "EXTENSION",
    "COMPOSITION",
    "DEPENDENCY",
    "LOLLIPOP",
    "LINE",
    "DOTTED_LINE",
    "CALLBACK",
    "LINK",
    "LINK_TARGET",
    "CLICK",
    "CALLBACK_NAME",
    "CALLBACK_ARGS",
    "HREF",
    "STYLE",
    "CSSCLASS",
    "COLON",
    "BRKT",
    "PCT",
    "MINUS",
    "UNICODE_TEXT",
    "BQUOTE_STR",
    "EDGE_STATE",
    "EOF_IN_STRUCT",
    "OPEN_IN_STRUCT",
    "PLUS",
    "EQUALS",
    "PUNCTUATION",
];

impl T {
    /// The name Mermaid's parser reports for this kind.
    pub(crate) fn name(self) -> &'static str {
        NAMES.get(self as usize).copied().unwrap_or("")
    }
}
