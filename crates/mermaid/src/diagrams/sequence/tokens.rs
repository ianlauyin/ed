//! The sequence diagram token kinds. Port of Pele's `src/diagrams/sequence/tokens.ts`.

/// A token kind, in the order of Pele's `T` enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum T {
    /// `$end` in Mermaid's grammar.
    Eof,
    /// `INVALID` in Mermaid's grammar.
    Error,
    /// `INVALID` in Mermaid's grammar.
    Invalid,
    /// `NEWLINE` in Mermaid's grammar.
    Newline,
    /// `SD` in Mermaid's grammar.
    Sd,
    /// `create` in Mermaid's grammar.
    Create,
    /// `box` in Mermaid's grammar.
    Box,
    /// `restOfLine` in Mermaid's grammar.
    RestOfLine,
    /// `end` in Mermaid's grammar.
    End,
    /// `autonumber` in Mermaid's grammar.
    Autonumber,
    /// `NUM` in Mermaid's grammar.
    Num,
    /// `off` in Mermaid's grammar.
    Off,
    /// `activate` in Mermaid's grammar.
    Activate,
    /// `deactivate` in Mermaid's grammar.
    Deactivate,
    /// `title` in Mermaid's grammar.
    Title,
    /// `legacy_title` in Mermaid's grammar.
    LegacyTitle,
    /// `acc_title` in Mermaid's grammar.
    AccTitle,
    /// `acc_title_value` in Mermaid's grammar.
    AccTitleValue,
    /// `acc_descr` in Mermaid's grammar.
    AccDescr,
    /// `acc_descr_value` in Mermaid's grammar.
    AccDescrValue,
    /// `acc_descr_multiline_value` in Mermaid's grammar.
    AccDescrMultilineValue,
    /// `loop` in Mermaid's grammar.
    Loop,
    /// `rect` in Mermaid's grammar.
    Rect,
    /// `opt` in Mermaid's grammar.
    Opt,
    /// `alt` in Mermaid's grammar.
    Alt,
    /// `par` in Mermaid's grammar.
    Par,
    /// `par_over` in Mermaid's grammar.
    ParOver,
    /// `critical` in Mermaid's grammar.
    Critical,
    /// `break` in Mermaid's grammar.
    Break,
    /// `option` in Mermaid's grammar.
    Option,
    /// `and` in Mermaid's grammar.
    And,
    /// `else` in Mermaid's grammar.
    Else,
    /// `participant` in Mermaid's grammar.
    Participant,
    /// `AS` in Mermaid's grammar.
    As,
    /// `participant_actor` in Mermaid's grammar.
    ParticipantActor,
    /// `destroy` in Mermaid's grammar.
    Destroy,
    /// `note` in Mermaid's grammar.
    Note,
    /// `over` in Mermaid's grammar.
    Over,
    /// `links` in Mermaid's grammar.
    Links,
    /// `link` in Mermaid's grammar.
    Link,
    /// `properties` in Mermaid's grammar.
    Properties,
    /// `details` in Mermaid's grammar.
    Details,
    /// `,` in Mermaid's grammar.
    Comma,
    /// `left_of` in Mermaid's grammar.
    LeftOf,
    /// `right_of` in Mermaid's grammar.
    RightOf,
    /// `+` in Mermaid's grammar.
    Plus,
    /// `-` in Mermaid's grammar.
    Minus,
    /// `()` in Mermaid's grammar.
    Central,
    /// `ACTOR` in Mermaid's grammar.
    Actor,
    /// `CONFIG_START` in Mermaid's grammar.
    ConfigStart,
    /// `CONFIG_CONTENT` in Mermaid's grammar.
    ConfigContent,
    /// `CONFIG_END` in Mermaid's grammar.
    ConfigEnd,
    /// `TXT` in Mermaid's grammar.
    Txt,
    /// `SOLID_OPEN_ARROW` in Mermaid's grammar.
    SolidOpenArrow,
    /// `DOTTED_OPEN_ARROW` in Mermaid's grammar.
    DottedOpenArrow,
    /// `SOLID_ARROW` in Mermaid's grammar.
    SolidArrow,
    /// `BIDIRECTIONAL_SOLID_ARROW` in Mermaid's grammar.
    BidirectionalSolidArrow,
    /// `DOTTED_ARROW` in Mermaid's grammar.
    DottedArrow,
    /// `BIDIRECTIONAL_DOTTED_ARROW` in Mermaid's grammar.
    BidirectionalDottedArrow,
    /// `SOLID_CROSS` in Mermaid's grammar.
    SolidCross,
    /// `DOTTED_CROSS` in Mermaid's grammar.
    DottedCross,
    /// `SOLID_POINT` in Mermaid's grammar.
    SolidPoint,
    /// `DOTTED_POINT` in Mermaid's grammar.
    DottedPoint,
    /// `SOLID_ARROW_TOP` in Mermaid's grammar.
    SolidArrowTop,
    /// `SOLID_ARROW_BOTTOM` in Mermaid's grammar.
    SolidArrowBottom,
    /// `STICK_ARROW_TOP` in Mermaid's grammar.
    StickArrowTop,
    /// `STICK_ARROW_BOTTOM` in Mermaid's grammar.
    StickArrowBottom,
    /// `SOLID_ARROW_TOP_DOTTED` in Mermaid's grammar.
    SolidArrowTopDotted,
    /// `SOLID_ARROW_BOTTOM_DOTTED` in Mermaid's grammar.
    SolidArrowBottomDotted,
    /// `STICK_ARROW_TOP_DOTTED` in Mermaid's grammar.
    StickArrowTopDotted,
    /// `STICK_ARROW_BOTTOM_DOTTED` in Mermaid's grammar.
    StickArrowBottomDotted,
    /// `SOLID_ARROW_TOP_REVERSE` in Mermaid's grammar.
    SolidArrowTopReverse,
    /// `SOLID_ARROW_BOTTOM_REVERSE` in Mermaid's grammar.
    SolidArrowBottomReverse,
    /// `STICK_ARROW_TOP_REVERSE` in Mermaid's grammar.
    StickArrowTopReverse,
    /// `STICK_ARROW_BOTTOM_REVERSE` in Mermaid's grammar.
    StickArrowBottomReverse,
    /// `SOLID_ARROW_TOP_REVERSE_DOTTED` in Mermaid's grammar.
    SolidArrowTopReverseDotted,
    /// `SOLID_ARROW_BOTTOM_REVERSE_DOTTED` in Mermaid's grammar.
    SolidArrowBottomReverseDotted,
    /// `STICK_ARROW_TOP_REVERSE_DOTTED` in Mermaid's grammar.
    StickArrowTopReverseDotted,
    /// `STICK_ARROW_BOTTOM_REVERSE_DOTTED` in Mermaid's grammar.
    StickArrowBottomReverseDotted,
}

impl T {
    /// The name Mermaid's grammar gives the token, for error messages.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Eof => "$end",
            Self::Error | Self::Invalid => "INVALID",
            Self::Newline => "NEWLINE",
            Self::Sd => "SD",
            Self::Create => "create",
            Self::Box => "box",
            Self::RestOfLine => "restOfLine",
            Self::End => "end",
            Self::Autonumber => "autonumber",
            Self::Num => "NUM",
            Self::Off => "off",
            Self::Activate => "activate",
            Self::Deactivate => "deactivate",
            Self::Title => "title",
            Self::LegacyTitle => "legacy_title",
            Self::AccTitle => "acc_title",
            Self::AccTitleValue => "acc_title_value",
            Self::AccDescr => "acc_descr",
            Self::AccDescrValue => "acc_descr_value",
            Self::AccDescrMultilineValue => "acc_descr_multiline_value",
            Self::Loop => "loop",
            Self::Rect => "rect",
            Self::Opt => "opt",
            Self::Alt => "alt",
            Self::Par => "par",
            Self::ParOver => "par_over",
            Self::Critical => "critical",
            Self::Break => "break",
            Self::Option => "option",
            Self::And => "and",
            Self::Else => "else",
            Self::Participant => "participant",
            Self::As => "AS",
            Self::ParticipantActor => "participant_actor",
            Self::Destroy => "destroy",
            Self::Note => "note",
            Self::Over => "over",
            Self::Links => "links",
            Self::Link => "link",
            Self::Properties => "properties",
            Self::Details => "details",
            Self::Comma => ",",
            Self::LeftOf => "left_of",
            Self::RightOf => "right_of",
            Self::Plus => "+",
            Self::Minus => "-",
            Self::Central => "()",
            Self::Actor => "ACTOR",
            Self::ConfigStart => "CONFIG_START",
            Self::ConfigContent => "CONFIG_CONTENT",
            Self::ConfigEnd => "CONFIG_END",
            Self::Txt => "TXT",
            Self::SolidOpenArrow => "SOLID_OPEN_ARROW",
            Self::DottedOpenArrow => "DOTTED_OPEN_ARROW",
            Self::SolidArrow => "SOLID_ARROW",
            Self::BidirectionalSolidArrow => "BIDIRECTIONAL_SOLID_ARROW",
            Self::DottedArrow => "DOTTED_ARROW",
            Self::BidirectionalDottedArrow => "BIDIRECTIONAL_DOTTED_ARROW",
            Self::SolidCross => "SOLID_CROSS",
            Self::DottedCross => "DOTTED_CROSS",
            Self::SolidPoint => "SOLID_POINT",
            Self::DottedPoint => "DOTTED_POINT",
            Self::SolidArrowTop => "SOLID_ARROW_TOP",
            Self::SolidArrowBottom => "SOLID_ARROW_BOTTOM",
            Self::StickArrowTop => "STICK_ARROW_TOP",
            Self::StickArrowBottom => "STICK_ARROW_BOTTOM",
            Self::SolidArrowTopDotted => "SOLID_ARROW_TOP_DOTTED",
            Self::SolidArrowBottomDotted => "SOLID_ARROW_BOTTOM_DOTTED",
            Self::StickArrowTopDotted => "STICK_ARROW_TOP_DOTTED",
            Self::StickArrowBottomDotted => "STICK_ARROW_BOTTOM_DOTTED",
            Self::SolidArrowTopReverse => "SOLID_ARROW_TOP_REVERSE",
            Self::SolidArrowBottomReverse => "SOLID_ARROW_BOTTOM_REVERSE",
            Self::StickArrowTopReverse => "STICK_ARROW_TOP_REVERSE",
            Self::StickArrowBottomReverse => "STICK_ARROW_BOTTOM_REVERSE",
            Self::SolidArrowTopReverseDotted => "SOLID_ARROW_TOP_REVERSE_DOTTED",
            Self::SolidArrowBottomReverseDotted => "SOLID_ARROW_BOTTOM_REVERSE_DOTTED",
            Self::StickArrowTopReverseDotted => "STICK_ARROW_TOP_REVERSE_DOTTED",
            Self::StickArrowBottomReverseDotted => "STICK_ARROW_BOTTOM_REVERSE_DOTTED",
        }
    }
}
