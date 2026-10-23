//! Token kinds of the er lexer and the names Mermaid's parser reports for them. Port of
//! Pele's `src/diagrams/er/tokens.ts`.

/// A token kind. Variant order mirrors Pele's `T` enum and indexes [`NAMES`], so it is observable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub(crate) enum T {
    /// Pele's `T.END`.
    End,
    /// Pele's `T.ERROR`.
    Error,
    /// Pele's `T.ER_DIAGRAM`.
    ErDiagram,
    /// Pele's `T.EOF`.
    Eof,
    /// Pele's `T.NEWLINE`.
    Newline,
    /// Pele's `T.COLON`.
    Colon,
    /// Pele's `T.STYLE_SEPARATOR`.
    StyleSeparator,
    /// Pele's `T.BLOCK_START`.
    BlockStart,
    /// Pele's `T.BLOCK_STOP`.
    BlockStop,
    /// Pele's `T.SQS`.
    Sqs,
    /// Pele's `T.SQE`.
    Sqe,
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
    /// Pele's `T.END_KW`.
    EndKw,
    /// Pele's `T.SUBGRAPH`.
    Subgraph,
    /// Pele's `T.direction_tb`.
    DirectionTb,
    /// Pele's `T.direction_bt`.
    DirectionBt,
    /// Pele's `T.direction_rl`.
    DirectionRl,
    /// Pele's `T.direction_lr`.
    DirectionLr,
    /// Pele's `T.CLASSDEF`.
    Classdef,
    /// Pele's `T.UNICODE_TEXT`.
    UnicodeText,
    /// Pele's `T.STYLE_TEXT`.
    StyleText,
    /// Pele's `T.COMMA`.
    Comma,
    /// Pele's `T.CLASS`.
    Class,
    /// Pele's `T.STYLE`.
    Style,
    /// Pele's `T.SEMI`.
    Semi,
    /// Pele's `T.NUM`.
    Num,
    /// Pele's `T.BRKT`.
    Brkt,
    /// Pele's `T.ENTITY_NAME`.
    EntityName,
    /// Pele's `T.DECIMAL_NUM`.
    DecimalNum,
    /// Pele's `T.ENTITY_ONE`.
    EntityOne,
    /// Pele's `T.ATTRIBUTE_WORD`.
    AttributeWord,
    /// Pele's `T.ATTRIBUTE_KEY`.
    AttributeKey,
    /// Pele's `T.COMMENT`.
    Comment,
    /// Pele's `T.ZERO_OR_ONE`.
    ZeroOrOne,
    /// Pele's `T.ZERO_OR_MORE`.
    ZeroOrMore,
    /// Pele's `T.ONE_OR_MORE`.
    OneOrMore,
    /// Pele's `T.ONLY_ONE`.
    OnlyOne,
    /// Pele's `T.MD_PARENT`.
    MdParent,
    /// Pele's `T.NON_IDENTIFYING`.
    NonIdentifying,
    /// Pele's `T.IDENTIFYING`.
    Identifying,
    /// Pele's `T.WORD`.
    Word,
    /// Pele's `T.CHAR`.
    Char,
}

/// The names Mermaid's parser reports for each token kind, indexed by [`T`].
pub(crate) const NAMES: [&str; 46] = [
    "$end",
    "INVALID",
    "ER_DIAGRAM",
    "EOF",
    "NEWLINE",
    "COLON",
    "STYLE_SEPARATOR",
    "BLOCK_START",
    "BLOCK_STOP",
    "SQS",
    "SQE",
    "acc_title",
    "acc_title_value",
    "acc_descr",
    "acc_descr_value",
    "acc_descr_multiline_value",
    "END",
    "SUBGRAPH",
    "direction_tb",
    "direction_bt",
    "direction_rl",
    "direction_lr",
    "CLASSDEF",
    "UNICODE_TEXT",
    "STYLE_TEXT",
    "COMMA",
    "CLASS",
    "STYLE",
    "SEMI",
    "NUM",
    "BRKT",
    "ENTITY_NAME",
    "DECIMAL_NUM",
    "ENTITY_ONE",
    "ATTRIBUTE_WORD",
    "ATTRIBUTE_KEY",
    "COMMENT",
    "ZERO_OR_ONE",
    "ZERO_OR_MORE",
    "ONE_OR_MORE",
    "ONLY_ONE",
    "MD_PARENT",
    "NON_IDENTIFYING",
    "IDENTIFYING",
    "WORD",
    "CHAR",
];

impl T {
    /// The name Mermaid's parser reports for this kind.
    pub(crate) fn name(self) -> &'static str {
        NAMES.get(self as usize).copied().unwrap_or("")
    }
}
