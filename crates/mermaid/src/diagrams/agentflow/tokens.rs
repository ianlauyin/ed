//! Token kinds of the agentflow lexer and the names Mermaid's parser reports for them. Port of
//! Pele's `src/diagrams/agentflow/tokens.ts`.

/// A token kind. Variant order mirrors Pele's `T` enum and indexes [`NAMES`], so it is observable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum T {
    /// Pele's `T.END`.
    End,
    /// Pele's `T.ERROR`.
    Error,
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
    /// Pele's `T.SHAPE_DATA`.
    ShapeData,
    /// Pele's `T.CALLBACKNAME`.
    CallbackName,
    /// Pele's `T.CALLBACKARGS`.
    CallbackArgs,
    /// Pele's `T.MD_STR`.
    MdStr,
    /// Pele's `T.STR`.
    Str,
    /// Pele's `T.STYLE`.
    Style,
    /// Pele's `T.DEFAULT`.
    Default,
    /// Pele's `T.LINKSTYLE`.
    LinkStyle,
    /// Pele's `T.INTERPOLATE`.
    Interpolate,
    /// Pele's `T.CLASSDEF`.
    ClassDef,
    /// Pele's `T.CLASS`.
    Class,
    /// Pele's `T.HREF`.
    Href,
    /// Pele's `T.CLICK`.
    Click,
    /// Pele's `T.GRAPH`.
    Graph,
    /// Pele's `T.flow`.
    Flow,
    /// Pele's `T.connector`.
    Connector,
    /// Pele's `T.global`.
    Global,
    /// Pele's `T.end`.
    BlockEnd,
    /// Pele's `T.LINK_TARGET`.
    LinkTarget,
    /// Pele's `T.NODIR`.
    NoDir,
    /// Pele's `T.DIR`.
    Dir,
    /// Pele's `T.direction_tb`.
    DirectionTb,
    /// Pele's `T.direction_bt`.
    DirectionBt,
    /// Pele's `T.direction_rl`.
    DirectionRl,
    /// Pele's `T.direction_lr`.
    DirectionLr,
    /// Pele's `T.direction_td`.
    DirectionTd,
    /// Pele's `T.LINK_ID`.
    LinkId,
    /// Pele's `T.NUM`.
    Num,
    /// Pele's `T.BRKT`.
    Brkt,
    /// Pele's `T.STYLE_SEPARATOR`.
    StyleSeparator,
    /// Pele's `T.COLON`.
    Colon,
    /// Pele's `T.AMP`.
    Amp,
    /// Pele's `T.SEMI`.
    Semi,
    /// Pele's `T.COMMA`.
    Comma,
    /// Pele's `T.MULT`.
    Mult,
    /// Pele's `T.LINK`.
    Link,
    /// Pele's `T.START_LINK`.
    StartLink,
    /// Pele's `T.EDGE_TEXT`.
    EdgeText,
    /// Pele's `T.ELLIPSE_END`.
    EllipseEnd,
    /// Pele's `T.TEXT`.
    Text,
    /// Pele's `T.ELLIPSE_START`.
    EllipseStart,
    /// Pele's `T.STADIUMEND`.
    StadiumEnd,
    /// Pele's `T.STADIUMSTART`.
    StadiumStart,
    /// Pele's `T.SUBROUTINEEND`.
    SubroutineEnd,
    /// Pele's `T.SUBROUTINESTART`.
    SubroutineStart,
    /// Pele's `T.VERTEX_WITH_PROPS_START`.
    VertexWithPropsStart,
    /// Pele's `T.TAGEND`.
    TagEnd,
    /// Pele's `T.CYLINDEREND`.
    CylinderEnd,
    /// Pele's `T.CYLINDERSTART`.
    CylinderStart,
    /// Pele's `T.DOUBLECIRCLEEND`.
    DoubleCircleEnd,
    /// Pele's `T.DOUBLECIRCLESTART`.
    DoubleCircleStart,
    /// Pele's `T.TRAPEND`.
    TrapEnd,
    /// Pele's `T.INVTRAPEND`.
    InvTrapEnd,
    /// Pele's `T.TRAPSTART`.
    TrapStart,
    /// Pele's `T.INVTRAPSTART`.
    InvTrapStart,
    /// Pele's `T.TAGSTART`.
    TagStart,
    /// Pele's `T.UP`.
    Up,
    /// Pele's `T.SEP`.
    Sep,
    /// Pele's `T.DOWN`.
    Down,
    /// Pele's `T.COMMENT`.
    Comment,
    /// Pele's `T.NODE_STRING`.
    NodeString,
    /// Pele's `T.MINUS`.
    Minus,
    /// Pele's `T.UNICODE_TEXT`.
    UnicodeText,
    /// Pele's `T.PIPE`.
    Pipe,
    /// Pele's `T.PE`.
    Pe,
    /// Pele's `T.PS`.
    Ps,
    /// Pele's `T.SQE`.
    Sqe,
    /// Pele's `T.SQS`.
    Sqs,
    /// Pele's `T.DIAMOND_STOP`.
    DiamondStop,
    /// Pele's `T.DIAMOND_START`.
    DiamondStart,
    /// Pele's `T.NEWLINE`.
    Newline,
    /// Pele's `T.SPACE`.
    Space,
    /// Pele's `T.EOF`.
    Eof,
}

impl T {
    /// The name Mermaid's parser reports for this kind in `Expecting ...` messages.
    pub(crate) fn name(self) -> &'static str {
        NAMES.get(self as usize).copied().unwrap_or("")
    }
}

/// Token names, indexed by [`T`].
const NAMES: [&str; 80] = [
    "$end",
    "INVALID",
    "acc_title",
    "acc_title_value",
    "acc_descr",
    "acc_descr_value",
    "acc_descr_multiline_value",
    "SHAPE_DATA",
    "CALLBACKNAME",
    "CALLBACKARGS",
    "MD_STR",
    "STR",
    "STYLE",
    "DEFAULT",
    "LINKSTYLE",
    "INTERPOLATE",
    "CLASSDEF",
    "CLASS",
    "HREF",
    "CLICK",
    "GRAPH",
    "flow",
    "connector",
    "global",
    "end",
    "LINK_TARGET",
    "NODIR",
    "DIR",
    "direction_tb",
    "direction_bt",
    "direction_rl",
    "direction_lr",
    "direction_td",
    "LINK_ID",
    "NUM",
    "BRKT",
    "STYLE_SEPARATOR",
    "COLON",
    "AMP",
    "SEMI",
    "COMMA",
    "MULT",
    "LINK",
    "START_LINK",
    "EDGE_TEXT",
    "-)",
    "TEXT",
    "(-",
    "STADIUMEND",
    "STADIUMSTART",
    "SUBROUTINEEND",
    "SUBROUTINESTART",
    "VERTEX_WITH_PROPS_START",
    "TAGEND",
    "CYLINDEREND",
    "CYLINDERSTART",
    "DOUBLECIRCLEEND",
    "DOUBLECIRCLESTART",
    "TRAPEND",
    "INVTRAPEND",
    "TRAPSTART",
    "INVTRAPSTART",
    "TAGSTART",
    "UP",
    "SEP",
    "DOWN",
    "COMMENT",
    "NODE_STRING",
    "MINUS",
    "UNICODE_TEXT",
    "PIPE",
    "PE",
    "PS",
    "SQE",
    "SQS",
    "DIAMOND_STOP",
    "DIAMOND_START",
    "NEWLINE",
    "SPACE",
    "EOF",
];
