//! Quadrant chart token kinds. Port of Pele's `src/diagrams/quadrant/tokens.ts`.

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
    /// The text of a title.
    TitleValue,
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
    /// `x-axis`.
    XAxis,
    /// `y-axis`.
    YAxis,
    /// `-->` between axis texts.
    Delimiter,
    /// `quadrant-1`.
    Quadrant1,
    /// `quadrant-2`.
    Quadrant2,
    /// `quadrant-3`.
    Quadrant3,
    /// `quadrant-4`.
    Quadrant4,
    /// `classDef`.
    Classdef,
    /// The text of a markdown string.
    MdStr,
    /// The text of a string.
    Str,
    /// A class name after `:::`.
    ClassName,
    /// `: [` before a point.
    PointStart,
    /// A point's x.
    PointX,
    /// A point's y.
    PointY,
    /// `quadrantChart`.
    Quadrant,
    /// Letters.
    Alpha,
    /// Non-ASCII text.
    UnicodeText,
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
    /// A space or tab.
    Space,
    /// `;`.
    Semi,
    /// Other punctuation.
    Punctuation,
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
            Self::TitleValue => "title_value",
            Self::AccTitle => "acc_title",
            Self::AccTitleValue => "acc_title_value",
            Self::AccDescr => "acc_descr",
            Self::AccDescrValue => "acc_descr_value",
            Self::AccDescrMultilineValue => "acc_descr_multiline_value",
            Self::XAxis => "X-AXIS",
            Self::YAxis => "Y-AXIS",
            Self::Delimiter => "AXIS-TEXT-DELIMITER",
            Self::Quadrant1 => "QUADRANT_1",
            Self::Quadrant2 => "QUADRANT_2",
            Self::Quadrant3 => "QUADRANT_3",
            Self::Quadrant4 => "QUADRANT_4",
            Self::Classdef => "CLASSDEF",
            Self::MdStr => "MD_STR",
            Self::Str => "STR",
            Self::ClassName => "class_name",
            Self::PointStart => "point_start",
            Self::PointX => "point_x",
            Self::PointY => "point_y",
            Self::Quadrant => "QUADRANT",
            Self::Alpha => "ALPHA",
            Self::UnicodeText => "UNICODE_TEXT",
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
            Self::Space => "SPACE",
            Self::Semi => "SEMI",
            Self::Punctuation => "PUNCTUATION",
            Self::Eof => "EOF",
        }
    }
}
