//! The sequence diagram model. Port of Pele's `src/diagrams/sequence/types.ts`.

/// The message kinds, numbered as in Mermaid. The numbers reach the model, so they are kept.
pub(crate) mod line {
    /// `->>`.
    pub(crate) const SOLID: u8 = 0;
    /// `-->>`.
    pub(crate) const DOTTED: u8 = 1;
    /// A note.
    pub(crate) const NOTE: u8 = 2;
    /// `-x`.
    pub(crate) const SOLID_CROSS: u8 = 3;
    /// `--x`.
    pub(crate) const DOTTED_CROSS: u8 = 4;
    /// `->`.
    pub(crate) const SOLID_OPEN: u8 = 5;
    /// `-->`.
    pub(crate) const DOTTED_OPEN: u8 = 6;
    /// `loop`.
    pub(crate) const LOOP_START: u8 = 10;
    /// The `end` of a `loop`.
    pub(crate) const LOOP_END: u8 = 11;
    /// `alt`.
    pub(crate) const ALT_START: u8 = 12;
    /// `else`.
    pub(crate) const ALT_ELSE: u8 = 13;
    /// The `end` of an `alt`.
    pub(crate) const ALT_END: u8 = 14;
    /// `opt`.
    pub(crate) const OPT_START: u8 = 15;
    /// The `end` of an `opt`.
    pub(crate) const OPT_END: u8 = 16;
    /// `activate` or a `+` mark.
    pub(crate) const ACTIVE_START: u8 = 17;
    /// `deactivate` or a `-` mark.
    pub(crate) const ACTIVE_END: u8 = 18;
    /// `par`.
    pub(crate) const PAR_START: u8 = 19;
    /// `and`.
    pub(crate) const PAR_AND: u8 = 20;
    /// The `end` of a `par`.
    pub(crate) const PAR_END: u8 = 21;
    /// `rect`.
    pub(crate) const RECT_START: u8 = 22;
    /// The `end` of a `rect`.
    pub(crate) const RECT_END: u8 = 23;
    /// `-)`.
    pub(crate) const SOLID_POINT: u8 = 24;
    /// `--)`.
    pub(crate) const DOTTED_POINT: u8 = 25;
    /// `autonumber`.
    pub(crate) const AUTONUMBER: u8 = 26;
    /// `critical`.
    pub(crate) const CRITICAL_START: u8 = 27;
    /// `option`.
    pub(crate) const CRITICAL_OPTION: u8 = 28;
    /// The `end` of a `critical`.
    pub(crate) const CRITICAL_END: u8 = 29;
    /// `break`.
    pub(crate) const BREAK_START: u8 = 30;
    /// The `end` of a `break`.
    pub(crate) const BREAK_END: u8 = 31;
    /// `par_over`.
    pub(crate) const PAR_OVER_START: u8 = 32;
    /// `<<->>`.
    pub(crate) const BIDIRECTIONAL_SOLID: u8 = 33;
    /// `<<-->>`.
    pub(crate) const BIDIRECTIONAL_DOTTED: u8 = 34;
    /// `-|\`.
    pub(crate) const SOLID_TOP: u8 = 41;
    /// `-|/`.
    pub(crate) const SOLID_BOTTOM: u8 = 42;
    /// `-\\`.
    pub(crate) const STICK_TOP: u8 = 43;
    /// `-//`.
    pub(crate) const STICK_BOTTOM: u8 = 44;
    /// `/|-`.
    pub(crate) const SOLID_ARROW_TOP_REVERSE: u8 = 45;
    /// `\|-`.
    pub(crate) const SOLID_ARROW_BOTTOM_REVERSE: u8 = 46;
    /// `//-`.
    pub(crate) const STICK_ARROW_TOP_REVERSE: u8 = 47;
    /// `\\-`.
    pub(crate) const STICK_ARROW_BOTTOM_REVERSE: u8 = 48;
    /// `--|\`.
    pub(crate) const SOLID_TOP_DOTTED: u8 = 51;
    /// `--|/`.
    pub(crate) const SOLID_BOTTOM_DOTTED: u8 = 52;
    /// `--\\`.
    pub(crate) const STICK_TOP_DOTTED: u8 = 53;
    /// `--//`.
    pub(crate) const STICK_BOTTOM_DOTTED: u8 = 54;
    /// `/|--`.
    pub(crate) const SOLID_ARROW_TOP_REVERSE_DOTTED: u8 = 55;
    /// `\|--`.
    pub(crate) const SOLID_ARROW_BOTTOM_REVERSE_DOTTED: u8 = 56;
    /// `//--`.
    pub(crate) const STICK_ARROW_TOP_REVERSE_DOTTED: u8 = 57;
    /// `\\--`.
    pub(crate) const STICK_ARROW_BOTTOM_REVERSE_DOTTED: u8 = 58;
    /// A `()` at the receiving end.
    pub(crate) const CENTRAL_CONNECTION: u8 = 59;
    /// A `()` at the sending end.
    pub(crate) const CENTRAL_CONNECTION_REVERSE: u8 = 60;
    /// A `()` at both ends.
    pub(crate) const CENTRAL_CONNECTION_DUAL: u8 = 61;
}

/// Where a note sits, numbered as in Mermaid.
pub(crate) mod placement {
    /// `left of`.
    pub(crate) const LEFT_OF: u8 = 0;
    /// `right of`.
    pub(crate) const RIGHT_OF: u8 = 1;
    /// `over`.
    pub(crate) const OVER: u8 = 2;
}

/// Text with its `wrap:` or `nowrap:` choice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SeqText {
    /// The text.
    pub(crate) text: String,
    /// `Some(true)` after `wrap:`, `Some(false)` after `nowrap:`.
    pub(crate) wrap: Option<bool>,
}

/// What a `box` line says: its color and title.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SeqBoxData {
    /// The fill color, or `transparent`.
    pub(crate) color: String,
    /// The title.
    pub(crate) text: Option<String>,
    /// The title's wrap choice.
    pub(crate) wrap: Option<bool>,
}

/// A box around participants.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SeqBox {
    /// The ids of the participants, in order.
    pub(crate) actor_keys: Vec<String>,
    /// The fill color, or `transparent`.
    pub(crate) fill: String,
    /// The title.
    pub(crate) name: Option<String>,
    /// Whether the title wraps.
    pub(crate) wrap: bool,
}

/// A JSON value from a `links` or `properties` statement, in `JSON.parse`'s shapes.
pub(crate) use crate::yaml::Value as MenuValue;

/// A participant.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SeqActor {
    /// The index of its box in [`super::db::SeqDb::boxes`].
    pub(crate) r#box: Option<usize>,
    /// The label.
    pub(crate) description: String,
    /// The id of the page element named by a `details` statement. Pele has no page to read it from.
    pub(crate) details: Option<String>,
    /// Menu links, in insertion order.
    pub(crate) links: Vec<(String, MenuValue)>,
    /// The id.
    pub(crate) name: String,
    /// The id of the participant declared after it.
    pub(crate) next_actor: Option<String>,
    /// The id of the participant declared before it.
    pub(crate) prev_actor: Option<String>,
    /// Properties, in insertion order.
    pub(crate) properties: Vec<(String, MenuValue)>,
    /// The kind: `participant`, `actor`, `database`, and so on.
    pub(crate) r#type: String,
    /// Whether the label wraps.
    pub(crate) wrap: bool,
}

/// The numbering an `autonumber` statement sets.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SeqNumbering {
    /// The first number.
    pub(crate) start: Option<f64>,
    /// The increment.
    pub(crate) step: Option<f64>,
    /// Whether numbers are shown.
    pub(crate) visible: bool,
}

/// A message's text, or the numbering of an `autonumber` entry.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum MessageText {
    /// An `autonumber` entry's numbering.
    Numbering(SeqNumbering),
    /// Text.
    Text(String),
}

/// One entry of the message list: a message, a note, or a block or activation marker.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SeqMessage {
    /// Whether the message activates its receiver. Absent for notes and numbering.
    pub(crate) activate: Option<bool>,
    /// The kind of `()` connection. Absent for notes and numbering.
    pub(crate) central_connection: Option<u8>,
    /// The sender.
    pub(crate) from: Option<String>,
    /// The position in the list, as a string.
    pub(crate) id: String,
    /// The text or numbering.
    pub(crate) message: MessageText,
    /// A note's placement.
    pub(crate) placement: Option<u8>,
    /// The receiver.
    pub(crate) to: Option<String>,
    /// The kind, from [`line`].
    pub(crate) r#type: Option<u8>,
    /// Whether the text wraps.
    pub(crate) wrap: bool,
}

/// The participants a note is attached to: one beside it, or one or two under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NoteActor {
    /// The two participants of an `over` note, the same one twice for a single one.
    Over(String, String),
    /// The participant of a `left of` or `right of` note.
    Single(String),
}

/// A note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SeqNote {
    /// The participants.
    pub(crate) actor: NoteActor,
    /// The text.
    pub(crate) message: String,
    /// The placement, from [`placement`].
    pub(crate) placement: u8,
    /// Whether the text wraps.
    pub(crate) wrap: bool,
}
