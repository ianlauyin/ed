//! Token types of Mermaid's use case lexer. Port of Pele's `src/diagrams/usecase/tokens.ts`.

/// Mermaid's token types. The order is observable: it makes the token categories ranges, so WORD
/// is `Usecase` to `Identifier`, and `LABEL_TEXT` is `Usecase` to `LabelSymbol`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum T {
    /// End of input.
    Eof,
    /// `usecase-beta`.
    Usecase,
    /// `actor`.
    Actor,
    /// `systemBoundary`.
    SystemBoundary,
    /// `end`.
    End,
    /// `direction`.
    Direction,
    /// `TD`.
    Td,
    /// `TB`.
    Tb,
    /// `BT`.
    Bt,
    /// `LR`.
    Lr,
    /// `RL`.
    Rl,
    /// `note`.
    Note,
    /// `for`.
    For,
    /// `json`.
    Json,
    /// `classDef`.
    ClassDef,
    /// `class`.
    Class,
    /// `style`.
    Style,
    /// `include`, in any case.
    Include,
    /// `extend`, in any case.
    Extend,
    /// `true`.
    True,
    /// `false`.
    False,
    /// A word.
    Identifier,
    /// A decimal number with a fraction and an optional unit.
    Number,
    /// `@`.
    At,
    /// `,`.
    Comma,
    /// `:`.
    Colon,
    /// `#` and hexadecimal digits.
    HashColor,
    /// A hyphenated word.
    CssIdentifier,
    /// `\,`.
    CssEscapedComma,
    /// `-`.
    Dash,
    /// `.`.
    Dot,
    /// `%`.
    Percent,
    /// Punctuation of a style value.
    CssPunctuation,
    /// Other punctuation.
    LabelPunctuation,
    /// A run of other characters.
    LabelSymbol,
    /// A quoted string.
    PlainString,
    /// A markdown string.
    MarkdownString,
    /// A markdown string that is never closed.
    UnclosedMarkdownString,
    /// A `%%` comment.
    Comment,
    /// A line break.
    Newline,
    /// `accDescr { ... }`.
    AccDescrBlock,
    /// `accTitle: ...`.
    AccTitleLine,
    /// `accDescr: ...`.
    AccDescrLine,
    /// `json id@`.
    JsonDeclarationStart,
    /// A JSON object.
    JsonObjectLiteral,
    /// A JSON object that is never closed.
    UnclosedJsonObjectLiteral,
    /// `--|>`.
    Generalization,
    /// `..>`.
    DependencyArrow,
    /// `<<`.
    StereotypeStart,
    /// `>>`.
    StereotypeEnd,
    /// The text of a stereotype.
    StereotypeText,
    /// The text of a stereotype that is never closed.
    UnclosedStereotypeText,
    /// `:::`.
    ClassSeparator,
    /// `-->`.
    ForwardSolid,
    /// `<--`.
    BackwardSolid,
    /// `--o`.
    ForwardCircle,
    /// `o--`.
    BackwardCircle,
    /// `--x`.
    ForwardCross,
    /// `x--`.
    BackwardCross,
    /// `--`.
    MarkerlessSolid,
    /// `@{`.
    MetadataStart,
    /// `{`.
    LBrace,
    /// `}`.
    RBrace,
    /// `[`.
    LBracket,
    /// `]`.
    RBracket,
    /// `(`.
    LParen,
    /// `)`.
    RParen,
}

/// Mermaid's name for `t`, as its errors print it.
pub(crate) fn name(t: T) -> &'static str {
    match t {
        T::Eof => "EOF",
        T::Usecase => "USECASE",
        T::Actor => "ACTOR",
        T::SystemBoundary => "SYSTEM_BOUNDARY",
        T::End => "END",
        T::Direction => "DIRECTION",
        T::Td => "TD",
        T::Tb => "TB",
        T::Bt => "BT",
        T::Lr => "LR",
        T::Rl => "RL",
        T::Note => "NOTE",
        T::For => "FOR",
        T::Json => "JSON",
        T::ClassDef => "CLASS_DEF",
        T::Class => "CLASS",
        T::Style => "STYLE",
        T::Include => "INCLUDE",
        T::Extend => "EXTEND",
        T::True => "TRUE",
        T::False => "FALSE",
        T::Identifier => "IDENTIFIER",
        T::Number => "NUMBER",
        T::At => "AT",
        T::Comma => "COMMA",
        T::Colon => "COLON",
        T::HashColor => "HASH_COLOR",
        T::CssIdentifier => "CSS_IDENTIFIER",
        T::CssEscapedComma => "CSS_ESCAPED_COMMA",
        T::Dash => "DASH",
        T::Dot => "DOT",
        T::Percent => "PERCENT",
        T::CssPunctuation => "CSS_PUNCTUATION",
        T::LabelPunctuation => "LABEL_PUNCTUATION",
        T::LabelSymbol => "LABEL_SYMBOL",
        T::PlainString => "PLAIN_STRING",
        T::MarkdownString => "MARKDOWN_STRING",
        T::UnclosedMarkdownString => "UNCLOSED_MARKDOWN_STRING",
        T::Comment => "COMMENT",
        T::Newline => "NEWLINE",
        T::AccDescrBlock => "ACC_DESCR_BLOCK",
        T::AccTitleLine => "ACC_TITLE_LINE",
        T::AccDescrLine => "ACC_DESCR_LINE",
        T::JsonDeclarationStart => "JSON_DECLARATION_START",
        T::JsonObjectLiteral => "JSON_OBJECT_LITERAL",
        T::UnclosedJsonObjectLiteral => "UNCLOSED_JSON_OBJECT_LITERAL",
        T::Generalization => "GENERALIZATION",
        T::DependencyArrow => "DEPENDENCY_ARROW",
        T::StereotypeStart => "STEREOTYPE_START",
        T::StereotypeEnd => "STEREOTYPE_END",
        T::StereotypeText => "STEREOTYPE_TEXT",
        T::UnclosedStereotypeText => "UNCLOSED_STEREOTYPE_TEXT",
        T::ClassSeparator => "CLASS_SEPARATOR",
        T::ForwardSolid => "FORWARD_SOLID",
        T::BackwardSolid => "BACKWARD_SOLID",
        T::ForwardCircle => "FORWARD_CIRCLE",
        T::BackwardCircle => "BACKWARD_CIRCLE",
        T::ForwardCross => "FORWARD_CROSS",
        T::BackwardCross => "BACKWARD_CROSS",
        T::MarkerlessSolid => "MARKERLESS_SOLID",
        T::MetadataStart => "METADATA_START",
        T::LBrace => "LBRACE",
        T::RBrace => "RBRACE",
        T::LBracket => "LBRACKET",
        T::RBracket => "RBRACKET",
        T::LParen => "LPAREN",
        T::RParen => "RPAREN",
    }
}
