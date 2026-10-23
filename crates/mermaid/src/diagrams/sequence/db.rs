//! The sequence diagram database. Port of Pele's `src/diagrams/sequence/db.ts`.

use std::collections::BTreeMap;

use super::colors::is_css_color;
use super::types::{
    MenuValue, MessageText, NoteActor, SeqActor, SeqBox, SeqBoxData, SeqMessage, SeqNote,
    SeqNumbering, SeqText, line,
};
use crate::error::{Code, Error, push_u32};
use crate::preprocess::json;
use crate::text::label::{trim, trim_start};
use crate::util::chars::{is_space, is_word};
use crate::yaml::{self, Value};

/// A semantic error of a sequence diagram.
#[cold]
fn fail(message: String) -> Error {
    let mut error = Error::new(Code::Semantic, message);
    error.r#type = Some("sequence");
    error
}

/// Whether `text` starts with `wrap:` or, when `no` is set, `nowrap:`, after an optional `:`.
/// Returns the length of the prefix.
fn wrap_prefix(text: &str, no: bool) -> Option<usize> {
    let colon = usize::from(text.starts_with(':'));
    let rest = text.get(colon..)?;
    let word = if no { "nowrap:" } else { "wrap:" };
    rest.starts_with(word).then_some(colon + word.len())
}

/// Mermaid's `parseMessage`: trims the text and reads a leading `wrap:` or `nowrap:`.
pub(crate) fn extract_wrap(raw: &str) -> SeqText {
    let text = trim(raw);
    let (wrap, cut) = match (wrap_prefix(text, false), wrap_prefix(text, true)) {
        (Some(len), _) => (Some(true), len),
        (None, Some(len)) => (Some(false), len),
        (None, None) => (None, 0),
    };
    SeqText {
        text: trim(text.get(cut..).unwrap_or("")).to_owned(),
        wrap,
    }
}

/// Splits a box line into a leading color and the rest, as Mermaid's
/// `/^((?:rgba?|hsla?)\s*\(.*\)|\w*)(.*)$/` does. `.` stops at a line terminator, so a line that
/// holds one outside the gap before the parenthesis does not match at all.
fn split_box(text: &str) -> Option<(&str, &str)> {
    let has_terminator = |s: &str| {
        s.chars()
            .any(|c| c == '\r' || c == '\u{2028}' || c == '\u{2029}')
    };
    let name = ["rgba", "rgb", "hsla", "hsl"]
        .into_iter()
        .find(|name| text.starts_with(name));
    if let Some(name) = name {
        let rest = text.get(name.len()..).unwrap_or("");
        let gap = rest.len() - trim_start(rest).len();
        let head = name.len() + gap;
        if text.get(head..).is_some_and(|rest| rest.starts_with('(')) {
            let head = head + 1;
            if let Some(close) = text.rfind(')').filter(|&close| close >= head) {
                if has_terminator(text.get(head..).unwrap_or("")) {
                    return None;
                }
                return Some((
                    text.get(..=close).unwrap_or(""),
                    text.get(close + 1..).unwrap_or(""),
                ));
            }
        }
    }
    if has_terminator(text) {
        return None;
    }
    let word = text
        .char_indices()
        .find(|&(_, c)| !is_word(u32::from(c)))
        .map_or(text.len(), |(at, _)| at);
    Some((
        text.get(..word).unwrap_or(""),
        text.get(word..).unwrap_or(""),
    ))
}

/// The key and value pairs of a parsed `links` or `properties` text, as Pele's `entries` lists
/// them: the characters of a string, the members of an object or array, and nothing otherwise.
fn entries(value: Value) -> Vec<(String, MenuValue)> {
    let key = |i: usize| {
        let mut key = String::new();
        push_u32(&mut key, u32::try_from(i).unwrap_or(u32::MAX));
        key
    };
    match value {
        Value::String(text) => text
            .chars()
            .enumerate()
            .map(|(i, c)| (key(i), Value::String(String::from(c))))
            .collect(),
        Value::Object(members) => members,
        Value::Array(items) => items
            .into_iter()
            .enumerate()
            .map(|(i, item)| (key(i), item))
            .collect(),
        Value::Bool(_) | Value::Null | Value::Number(_) => Vec::new(),
    }
}

/// Sets `key` as `Map.prototype.set` does: an existing key keeps its place.
fn map_set<V>(map: &mut Vec<(String, V)>, key: String, value: V) {
    match map.iter_mut().find(|(own, _)| *own == key) {
        Some(entry) => entry.1 = value,
        None => map.push((key, value)),
    }
}

/// `text.slice(start, end)` in UTF-16 units, with JavaScript's negative indices.
fn js_slice(text: &str, start: i64, end: Option<i64>) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let len = i64::try_from(units.len()).unwrap_or(i64::MAX);
    let clamp = |at: i64| {
        let at = if at < 0 {
            (len + at).max(0)
        } else {
            at.min(len)
        };
        usize::try_from(at).unwrap_or(0)
    };
    let from = clamp(start);
    let to = clamp(end.unwrap_or(len));
    if from >= to {
        return String::new();
    }
    String::from_utf16_lossy(units.get(from..to).unwrap_or(&[]))
}

/// A menu statement's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Menu {
    /// `link`.
    ALink,
    /// `details`.
    Details,
    /// `links`.
    Links,
    /// `properties`.
    Properties,
}

/// How a participant statement declares its participant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Declare {
    /// Declared, or mentioned by a message.
    Add,
    /// `create`.
    Create,
    /// `destroy`.
    Destroy,
}

/// One statement of the list the parser hands to [`SeqDb::apply`], in document order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Statement {
    /// A block's start or divider, with its text, or its end without one.
    Block {
        /// The kind, from [`line`].
        signal_type: u8,
        /// The text.
        text: Option<SeqText>,
    },
    /// The end of a `box`.
    BoxEnd,
    /// The start of a `box`.
    BoxStart(SeqBoxData),
    /// An activation or connection marker on one participant.
    Marker {
        /// The participant.
        actor: String,
        /// The kind, from [`line`].
        signal_type: u8,
    },
    /// A menu statement.
    Menu {
        /// The participant.
        actor: String,
        /// The kind.
        kind: Menu,
        /// The text.
        text: SeqText,
    },
    /// A message.
    Message {
        /// Whether it activates its receiver.
        activate: bool,
        /// The kind of `()` connection.
        central_connection: Option<u8>,
        /// The sender.
        from: String,
        /// The text.
        msg: SeqText,
        /// The kind, from [`line`].
        signal_type: u8,
        /// The receiver.
        to: String,
    },
    /// A note.
    Note {
        /// The participants.
        actor: NoteActor,
        /// The placement.
        placement: u8,
        /// The text.
        text: SeqText,
    },
    /// A participant.
    Participant {
        /// The id.
        actor: String,
        /// The `@{ ... }` config.
        config: Option<String>,
        /// The `as` label.
        description: Option<SeqText>,
        /// `participant` or `actor`, or `None` for a participant a message mentions.
        draw: Option<&'static str>,
        /// How it is declared.
        how: Declare,
    },
    /// `autonumber`.
    SequenceIndex(SeqNumbering),
}

/// The parsed sequence diagram: Pele's `SeqDb`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SeqDb {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    active: BTreeMap<String, i64>,
    /// Participants, in insertion order.
    pub(crate) actors: Vec<SeqActor>,
    actor_at: BTreeMap<String, usize>,
    /// Boxes, in order.
    pub(crate) boxes: Vec<SeqBox>,
    /// The message count when each created participant was declared, in insertion order.
    pub(crate) created_actors: Vec<(String, usize)>,
    current_box: Option<usize>,
    /// The message count when each destroyed participant was declared, in insertion order.
    pub(crate) destroyed_actors: Vec<(String, usize)>,
    last_created: Option<String>,
    last_destroyed: Option<String>,
    /// Messages, notes, and markers, in order.
    pub(crate) messages: Vec<SeqMessage>,
    /// Notes, in order.
    pub(crate) notes: Vec<SeqNote>,
    prev_actor: Option<String>,
    sequence_wrap: bool,
    /// The title.
    pub(crate) title: Option<String>,
    wrap_enabled: Option<bool>,
}

impl SeqDb {
    /// A database for the top-level `wrap` setting and `sequence.wrap`.
    pub(crate) fn new(wrap: Option<bool>, sequence_wrap: bool) -> Self {
        Self {
            sequence_wrap,
            wrap_enabled: wrap,
            ..Self::default()
        }
    }

    fn auto_wrap(&self) -> bool {
        self.wrap_enabled.unwrap_or(self.sequence_wrap)
    }

    /// The participant with `id`.
    pub(crate) fn actor(&self, id: &str) -> Option<&SeqActor> {
        self.actor_at.get(id).and_then(|&at| self.actors.get(at))
    }

    fn actor_mut(&mut self, id: &str) -> Option<&mut SeqActor> {
        self.actor_at
            .get(id)
            .copied()
            .and_then(|at| self.actors.get_mut(at))
    }

    /// Sets the accessible title, without leading whitespace.
    pub(crate) fn set_acc_title(&mut self, text: &str) {
        self.acc_title = Some(trim_start(text).to_owned());
    }

    /// Sets the accessible description, without the indentation of its lines.
    pub(crate) fn set_acc_description(&mut self, text: &str) {
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            out.push(c);
            if c == '\n' && chars.peek().is_some_and(|&c| is_space(u32::from(c))) {
                // `/\n\s+/` takes the newline and every whitespace character after it.
                while chars.next_if(|&c| is_space(u32::from(c))).is_some() {}
            }
        }
        self.acc_descr = Some(out);
    }

    /// Sets the title.
    pub(crate) fn set_diagram_title(&mut self, text: &str) {
        self.title = Some(text.to_owned());
    }

    /// Mermaid's `parseBoxData`: a leading color, then a title.
    pub(crate) fn parse_box_data(text: &str) -> SeqBoxData {
        let split = split_box(text);
        let mut color = match split {
            Some((color, _)) if !color.is_empty() => trim(color).to_owned(),
            _ => "transparent".to_owned(),
        };
        let mut title = match split {
            Some((_, rest)) if !rest.is_empty() => Some(trim(rest)),
            _ => None,
        };
        if !is_css_color(&color) {
            "transparent".clone_into(&mut color);
            title = Some(trim(text));
        }
        let Some(title) = title else {
            return SeqBoxData {
                color,
                text: None,
                wrap: None,
            };
        };
        let SeqText { text, wrap } = extract_wrap(title);
        SeqBoxData {
            color,
            text: (!text.is_empty()).then_some(text),
            wrap,
        }
    }

    fn add_box(&mut self, data: SeqBoxData) {
        self.current_box = Some(self.boxes.len());
        self.boxes.push(SeqBox {
            actor_keys: Vec::new(),
            fill: data.color,
            name: data.text,
            wrap: data.wrap.unwrap_or(self.auto_wrap()),
        });
    }

    fn box_name(&self, at: usize) -> &str {
        self.boxes
            .get(at)
            .and_then(|b| b.name.as_deref())
            .unwrap_or("undefined")
    }

    fn add_actor(
        &mut self,
        id: &str,
        description: Option<SeqText>,
        r#type: Option<&str>,
        metadata: Option<&str>,
    ) -> Result<(), Error> {
        let name = id;
        let mut description = description;
        let mut r#type = r#type.map(str::to_owned);
        let mut assigned_box = self.current_box;
        let mut doc = None;
        if let Some(metadata) = metadata {
            let src = if metadata.contains('\n') {
                [metadata, "\n"].concat()
            } else {
                ["{\n", metadata, "\n}"].concat()
            };
            match yaml::parse(&src) {
                Ok(Value::Object(members)) => doc = Some(members),
                Ok(_) => {}
                Err(message) => {
                    let mut error = Error::new(
                        Code::Syntax,
                        [
                            "Config of participant \"",
                            id,
                            "\" is not valid YAML. ",
                            &message,
                        ]
                        .concat(),
                    );
                    error.r#type = Some("sequence");
                    return Err(error);
                }
            }
        }
        let member = |key: &str| {
            doc.as_ref()
                .and_then(|doc| doc.iter().find(|(own, _)| own == key).map(|(_, v)| v))
        };
        if let Some(doc_type) = member("type").filter(|v| !matches!(v, Value::Null)) {
            let mut text = String::new();
            doc_type.push_js_string(&mut text);
            r#type = Some(text);
        }
        if let Some(alias) = member("alias").filter(|v| v.truthy())
            && description.as_ref().is_none_or(|d| d.text == name)
        {
            let mut text = String::new();
            alias.push_js_string(&mut text);
            description = Some(SeqText {
                text,
                wrap: description.as_ref().and_then(|d| d.wrap),
            });
        }

        if let Some(&at) = self.actor_at.get(id) {
            let old_box = self.actors.get(at).and_then(|old| old.r#box);
            if let (Some(current), Some(old_box)) = (self.current_box, old_box)
                && current != old_box
            {
                return Err(fail(
                    [
                        "A same participant should only be defined in one Box: ",
                        name,
                        " can't be in '",
                        self.box_name(old_box),
                        "' and in '",
                        self.box_name(current),
                        "' at the same time.",
                    ]
                    .concat(),
                ));
            }
            assigned_box = old_box.or(self.current_box);
            if let Some(old) = self.actors.get_mut(at) {
                old.r#box = assigned_box;
            }
            if description.is_none() {
                return Ok(());
            }
        }
        // A participant that is only mentioned in a message has no kind and takes its name as its
        // label.
        let description = match (description, &r#type) {
            (Some(description), Some(_)) => description,
            _ => SeqText {
                text: name.to_owned(),
                wrap: None,
            },
        };

        let actor = SeqActor {
            r#box: assigned_box,
            description: description.text,
            details: None,
            links: Vec::new(),
            name: name.to_owned(),
            next_actor: None,
            prev_actor: self.prev_actor.clone(),
            properties: Vec::new(),
            r#type: r#type.unwrap_or_else(|| "participant".to_owned()),
            wrap: description.wrap.unwrap_or(self.auto_wrap()),
        };
        // A redeclared participant keeps its place, as `Map.prototype.set` keeps a key's.
        if let Some(slot) = self
            .actor_at
            .get(id)
            .copied()
            .and_then(|at| self.actors.get_mut(at))
        {
            *slot = actor;
        } else {
            self.actor_at.insert(id.to_owned(), self.actors.len());
            self.actors.push(actor);
        }
        if let Some(prev) = self.prev_actor.clone().filter(|prev| !prev.is_empty())
            && let Some(prev) = self.actor_mut(&prev)
        {
            prev.next_actor = Some(id.to_owned());
        }
        if let Some(current) = self.current_box.and_then(|at| self.boxes.get_mut(at)) {
            current.actor_keys.push(id.to_owned());
        }
        self.prev_actor = Some(id.to_owned());
        Ok(())
    }

    fn add_signal(
        &mut self,
        from: Option<&str>,
        to: Option<&str>,
        message: Option<SeqText>,
        r#type: u8,
        activate: bool,
        central_connection: Option<u8>,
    ) -> Result<(), Error> {
        // Mermaid recounts every earlier message here. A running count per participant says the
        // same.
        if r#type == line::ACTIVE_END {
            let count = from
                .filter(|from| !from.is_empty())
                .map_or(0, |from| self.active.get(from).copied().unwrap_or(0));
            if count < 1 {
                return Err(fail(
                    [
                        "Trying to inactivate an inactive participant (",
                        from.unwrap_or("undefined"),
                        ")",
                    ]
                    .concat(),
                ));
            }
            if let Some(from) = from {
                self.active.insert(from.to_owned(), count - 1);
            }
        } else if r#type == line::ACTIVE_START
            && let Some(from) = from.filter(|from| !from.is_empty())
        {
            *self.active.entry(from.to_owned()).or_insert(0) += 1;
        }
        let (text, wrap) = match message {
            Some(message) => (message.text, message.wrap),
            None => (String::new(), None),
        };
        self.messages.push(SeqMessage {
            activate: Some(activate),
            central_connection: Some(central_connection.unwrap_or(0)),
            from: from.map(str::to_owned),
            id: self.next_id(),
            message: MessageText::Text(text),
            placement: None,
            to: to.map(str::to_owned),
            r#type: Some(r#type),
            wrap: wrap.unwrap_or(self.auto_wrap()),
        });
        Ok(())
    }

    fn next_id(&self) -> String {
        let mut id = String::new();
        push_u32(
            &mut id,
            u32::try_from(self.messages.len()).unwrap_or(u32::MAX),
        );
        id
    }

    fn add_note(&mut self, actor: NoteActor, placement: u8, message: SeqText) {
        let wrap = message.wrap.unwrap_or(self.auto_wrap());
        let (from, to) = match &actor {
            NoteActor::Single(id) => (id.clone(), id.clone()),
            NoteActor::Over(a, b) => (a.clone(), b.clone()),
        };
        self.notes.push(SeqNote {
            actor,
            message: message.text.clone(),
            placement,
            wrap,
        });
        self.messages.push(SeqMessage {
            activate: None,
            central_connection: None,
            from: Some(from),
            id: self.next_id(),
            message: MessageText::Text(message.text),
            placement: Some(placement),
            to: Some(to),
            r#type: Some(line::NOTE),
            wrap,
        });
    }

    // Mermaid logs and ignores menu text that is not valid JSON. It also undoes the `&amp;` and
    // `&equals;` its sanitizer writes; Pele has no sanitizer, and a `;` cannot occur in the text.
    fn add_menu(&mut self, actor_id: &str, kind: Menu, text: &str) {
        let Some(actor) = self.actor_mut(actor_id) else {
            return;
        };
        match kind {
            Menu::Links | Menu::Properties => {
                let Some(parsed) = json(text) else {
                    return;
                };
                let map = if kind == Menu::Links {
                    &mut actor.links
                } else {
                    &mut actor.properties
                };
                for (key, value) in entries(parsed) {
                    map_set(map, key, value);
                }
            }
            Menu::ALink => {
                let sep = text.find('@').map_or(-1, |at| {
                    i64::try_from(text.get(..at).unwrap_or("").encode_utf16().count())
                        .unwrap_or(i64::MAX)
                });
                let key = js_slice(text, 0, Some(sep - 1));
                let value = js_slice(text, sep + 1, None);
                map_set(
                    &mut actor.links,
                    trim(&key).to_owned(),
                    Value::String(trim(&value).to_owned()),
                );
            }
            Menu::Details => actor.details = Some(text.to_owned()),
        }
    }

    /// Applies the parsed statements in order.
    ///
    /// # Errors
    ///
    /// Returns a semantic error for a participant in two boxes, a created participant declared
    /// twice or not created by the next message, a destroyed participant not in the next message,
    /// and a deactivation of an inactive participant, and a syntax error for a participant config
    /// that is not valid YAML.
    pub(crate) fn apply(&mut self, statements: Vec<Statement>) -> Result<(), Error> {
        for statement in statements {
            self.apply_one(statement)?;
        }
        Ok(())
    }

    fn apply_one(&mut self, statement: Statement) -> Result<(), Error> {
        match statement {
            Statement::SequenceIndex(numbering) => {
                let id = self.next_id();
                self.messages.push(SeqMessage {
                    activate: None,
                    central_connection: None,
                    from: None,
                    id,
                    message: MessageText::Numbering(numbering),
                    placement: None,
                    to: None,
                    r#type: Some(line::AUTONUMBER),
                    wrap: false,
                });
            }
            Statement::Participant {
                actor,
                config,
                description,
                draw,
                how,
            } => match how {
                Declare::Add => self.add_actor(&actor, description, draw, config.as_deref())?,
                Declare::Create => {
                    if self.actor_at.contains_key(&actor) {
                        return Err(fail("It is not possible to have actors with the same id, even if one is destroyed before the next is created. Use 'AS' aliases to simulate the behavior".to_owned()));
                    }
                    self.last_created = Some(actor.clone());
                    self.add_actor(&actor, description, draw, config.as_deref())?;
                    let count = self.messages.len();
                    map_set(&mut self.created_actors, actor, count);
                }
                Declare::Destroy => {
                    self.last_destroyed = Some(actor.clone());
                    let count = self.messages.len();
                    map_set(&mut self.destroyed_actors, actor, count);
                }
            },
            Statement::Marker { actor, signal_type } => {
                self.add_signal(Some(&actor), None, None, signal_type, false, None)?;
            }
            Statement::Note {
                actor,
                placement,
                text,
            } => self.add_note(actor, placement, text),
            Statement::Menu { actor, kind, text } => self.add_menu(&actor, kind, &text.text),
            Statement::Message {
                activate,
                central_connection,
                from,
                msg,
                signal_type,
                to,
            } => {
                if let Some(created) = self.last_created.take() {
                    if to != created {
                        return Err(fail(["The created participant ", &created, " does not have an associated creating message after its declaration. Please check the sequence diagram."].concat()));
                    }
                } else if let Some(destroyed) = self.last_destroyed.take()
                    && to != destroyed
                    && from != destroyed
                {
                    return Err(fail(["The destroyed participant ", &destroyed, " does not have an associated destroying message after its declaration. Please check the sequence diagram."].concat()));
                }
                self.add_signal(
                    Some(&from),
                    Some(&to),
                    Some(msg),
                    signal_type,
                    activate,
                    central_connection,
                )?;
            }
            Statement::BoxStart(data) => self.add_box(data),
            Statement::BoxEnd => self.current_box = None,
            Statement::Block { signal_type, text } => {
                self.add_signal(None, None, text, signal_type, false, None)?;
            }
        }
        Ok(())
    }
}
