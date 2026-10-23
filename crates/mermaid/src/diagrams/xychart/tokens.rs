//! XY chart token kinds. Port of Pele's `src/diagrams/xychart/tokens.ts`.

/// A token kind. The order mirrors Pele's `T` enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum T {
    /// After the end of input.
    End,
    /// Unrecognized text.
    Error,
    /// Line breaks.
    Newline,
    /// `title`.
    Title,
    /// `accTitle:`.
    AccTitle,
    /// The text of an accessible title.
    AccTitleValue,
    /// `accDescr:`.
    AccDescr,
    /// The text of an accessible description.
    AccDescrValue,
    /// The text of a braced accessible description.
    AccDescrMultilineValue,
    /// `xychart` or `xychart-beta`.
    Xychart,
    /// `vertical` or `horizontal`.
    ChartOrientation,
    /// `x-axis`.
    XAxis,
    /// `y-axis`.
    YAxis,
    /// `-->` in an axis range.
    ArrowDelimiter,
    /// `line`.
    Line,
    /// `bar`.
    Bar,
    /// A number in a range or a series.
    NumberWithDecimal,
    /// `[`.
    SquareBracesStart,
    /// `]`.
    SquareBracesEnd,
    /// The text of a string.
    Str,
    /// Letters.
    Alpha,
    /// `:`.
    Colon,
    /// `+`.
    Plus,
    /// `,`.
    Comma,
    /// `=`.
    Equals,
    /// `*`.
    Mult,
    /// `#`.
    Brkt,
    /// `_`.
    Underscore,
    /// `.`.
    Dot,
    /// `&`.
    Amp,
    /// `-`.
    Minus,
    /// Digits.
    Num,
    /// `;`.
    Semi,
    /// The end of input.
    Eof,
}

impl T {
    /// The name Mermaid's parser reports, from Pele's `TOKEN_NAMES`.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::End => "$end",
            Self::Error => "INVALID",
            Self::Newline => "NEWLINE",
            Self::Title => "title",
            Self::AccTitle => "acc_title",
            Self::AccTitleValue => "acc_title_value",
            Self::AccDescr => "acc_descr",
            Self::AccDescrValue => "acc_descr_value",
            Self::AccDescrMultilineValue => "acc_descr_multiline_value",
            Self::Xychart => "XYCHART",
            Self::ChartOrientation => "CHART_ORIENTATION",
            Self::XAxis => "X_AXIS",
            Self::YAxis => "Y_AXIS",
            Self::ArrowDelimiter => "ARROW_DELIMITER",
            Self::Line => "LINE",
            Self::Bar => "BAR",
            Self::NumberWithDecimal => "NUMBER_WITH_DECIMAL",
            Self::SquareBracesStart => "SQUARE_BRACES_START",
            Self::SquareBracesEnd => "SQUARE_BRACES_END",
            Self::Str => "STR",
            Self::Alpha => "ALPHA",
            Self::Colon => "COLON",
            Self::Plus => "PLUS",
            Self::Comma => "COMMA",
            Self::Equals => "EQUALS",
            Self::Mult => "MULT",
            Self::Brkt => "BRKT",
            Self::Underscore => "UNDERSCORE",
            Self::Dot => "DOT",
            Self::Amp => "AMP",
            Self::Minus => "MINUS",
            Self::Num => "NUM",
            Self::Semi => "SEMI",
            Self::Eof => "EOF",
        }
    }
}
