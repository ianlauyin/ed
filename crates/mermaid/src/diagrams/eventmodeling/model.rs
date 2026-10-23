//! The event model: frames as boxes in lanes, the relations between them, notes, and
//! specifications. Port of Pele's `src/diagrams/eventmodeling/model.ts`.

use std::collections::BTreeSet;

use super::parser::EventModelAst;
use crate::diagrams::common::tokens::{acc_descr_value, acc_title_value, title_value};
use crate::error::{Code, Error, locate, push_u32};
use crate::text::label::{trim, trim_end, trim_start};
use crate::util::chars::is_space;

/// What a frame draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntityKind {
    /// A command.
    Command,
    /// An event.
    Event,
    /// An automated processor.
    Processor,
    /// A read model.
    Readmodel,
    /// A screen.
    Ui,
}

impl EntityKind {
    /// The kind as Pele names it.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Event => "event",
            Self::Processor => "processor",
            Self::Readmodel => "readmodel",
            Self::Ui => "ui",
        }
    }
}

/// The group of lanes a kind of entity goes in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LaneGroup {
    /// Commands and read models.
    State,
    /// Events.
    Stream,
    /// Screens and processors.
    Trigger,
}

impl LaneGroup {
    /// The group as Pele names it.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::State => "state",
            Self::Stream => "stream",
            Self::Trigger => "trigger",
        }
    }

    /// The group's first index, its label, and its label prefix for a namespace.
    const fn meta(self) -> (u32, &'static str, &'static str) {
        match self {
            Self::Trigger => (0, "UI/Automation", "UI/A: "),
            Self::State => (100, "Command/Read Model", "C/RM: "),
            Self::Stream => (200, "Events", "Stream: "),
        }
    }
}

/// A lane: Pele's `EmLane`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EmLane {
    /// The lane's group.
    pub(crate) group: LaneGroup,
    /// Mermaid's ordering key: 0 for UI/Automation, 100 for Command/Read Model, 200 for Events,
    /// and the next free number after those for each namespace.
    pub(crate) index: u32,
    /// The heading.
    pub(crate) label: String,
    /// The namespace, for a lane of one.
    pub(crate) namespace: Option<String>,
}

/// A frame's box: Pele's `EmBox`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EmBox {
    /// Example data from the frame or from the data block it refers to.
    pub(crate) data: Option<String>,
    /// The data type of inline data.
    pub(crate) data_type: Option<String>,
    /// The entity's qualified name.
    pub(crate) entity: String,
    /// The frame number as written, such as "01".
    pub(crate) id: String,
    /// What the box draws.
    pub(crate) kind: EntityKind,
    /// The box's lane, as an index into [`EventModelingModel::lanes`].
    pub(crate) lane: usize,
    /// The entity's name without its namespace.
    pub(crate) name: String,
    /// The namespace of a two-part name.
    pub(crate) namespace: Option<String>,
    /// Whether the frame is a reset frame.
    pub(crate) reset: bool,
}

/// A note: Pele's `EmNote`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EmNote {
    /// The data type.
    pub(crate) data_type: Option<String>,
    /// The frame the note belongs to.
    pub(crate) frame: String,
    /// The note's text.
    pub(crate) text: String,
}

/// A given/when/then specification: Pele's `EmSpecification`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EmSpecification {
    /// The frame the specification belongs to.
    pub(crate) frame: String,
    /// The `given` entities.
    pub(crate) given: Vec<String>,
    /// The `then` entities.
    pub(crate) then: Vec<String>,
    /// The `when` entities.
    pub(crate) when: Vec<String>,
}

/// The parsed event model: Pele's `EventModelingModel`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EventModelingModel {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    /// Boxes in frame order.
    pub(crate) boxes: Vec<EmBox>,
    /// Model entity names.
    pub(crate) entities: Vec<String>,
    /// Lanes from top to bottom.
    pub(crate) lanes: Vec<EmLane>,
    /// Notes.
    pub(crate) notes: Vec<EmNote>,
    /// Relations as indices into `boxes`: the source and the target.
    pub(crate) relations: Vec<(usize, usize)>,
    /// Specifications.
    pub(crate) specifications: Vec<EmSpecification>,
    /// The title.
    pub(crate) title: Option<String>,
}

/// The kind of entity an entity type keyword names. Unknown types are events.
fn kind_of(keyword: &str) -> EntityKind {
    match keyword {
        "ui" => EntityKind::Ui,
        "pcr" | "processor" => EntityKind::Processor,
        "rmo" | "readmodel" => EntityKind::Readmodel,
        "cmd" | "command" => EntityKind::Command,
        _ => EntityKind::Event,
    }
}

/// The group of lanes `kind` goes in.
fn group_of(kind: EntityKind) -> LaneGroup {
    match kind {
        EntityKind::Ui | EntityKind::Processor => LaneGroup::Trigger,
        EntityKind::Event => LaneGroup::Stream,
        EntityKind::Command | EntityKind::Readmodel => LaneGroup::State,
    }
}

/// `text.trim() === ''`.
fn blank(text: &str) -> bool {
    trim(text).is_empty()
}

/// The text between the outer braces, without the indentation its lines share.
pub(crate) fn block_text(raw: &str) -> String {
    let open = raw.find('{');
    let close = raw.rfind('}');
    let inner = match (open, close) {
        (Some(open), Some(close)) if close > open => raw.get(open + 1..close).unwrap_or(""),
        (None, _) if raw.encode_utf16().nth(1).is_some() => {
            // Without braces the value is a quoted string, whose quotes are single units.
            let mut chars = raw.chars();
            chars.next();
            chars.next_back();
            chars.as_str()
        }
        _ => "",
    };

    let mut unreturned = String::with_capacity(inner.len());
    unreturned.extend(inner.chars().filter(|&c| c != '\r'));
    let mut lines: Vec<&str> = unreturned.split('\n').collect();
    while lines.first().is_some_and(|line| blank(line)) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| blank(line)) {
        lines.pop();
    }

    // Leading whitespace is counted in characters, each one UTF-16 unit as JavaScript counts it.
    let lead = |line: &str| line.chars().take_while(|&c| is_space(u32::from(c))).count();
    let indent = lines
        .iter()
        .filter(|line| !blank(line))
        .map(|line| lead(line))
        .min()
        .unwrap_or(usize::MAX);
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let mut chars = line.chars();
        for _ in 0..indent.min(lead(line)) {
            chars.next();
        }
        out.push_str(trim_end(chars.as_str()));
    }
    out
}

/// `text.replace(/\n\s+/g, '\n')`.
fn dedent_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\n' {
            while chars.next_if(|&c| is_space(u32::from(c))).is_some() {}
        }
    }
    out
}

/// A duplicate frame ID on `line`.
#[cold]
fn duplicate(name: &str, line: u32) -> Error {
    let mut message = String::from("Duplicate event modeling frame ID \"");
    message.push_str(name);
    message.push_str("\" on line ");
    push_u32(&mut message, line);
    let mut error = Error::new(Code::Semantic, message);
    error.line = line;
    error.r#type = Some("eventmodeling");
    error
}

/// Builds the model from `ast` of `source`, as Pele's `buildEventModel` does. `title` comes from
/// front matter.
///
/// # Errors
///
/// Returns a semantic error when two frames share an ID.
pub(crate) fn build_event_model(
    ast: &EventModelAst<'_>,
    source: &str,
    title: Option<&str>,
) -> Result<EventModelingModel, Error> {
    let mut ids = BTreeSet::new();
    for frame in &ast.frames {
        if !ids.insert(frame.name) {
            return Err(duplicate(frame.name, locate(source, frame.offset).line));
        }
    }

    // One lane per group, and one more per namespace within a group, in the order they are met.
    let mut lanes: Vec<EmLane> = Vec::new();
    let mut next: Vec<(LaneGroup, u32)> = Vec::new();
    let mut lane = |kind: EntityKind, namespace: Option<&str>| -> usize {
        let group = group_of(kind);
        if let Some(found) = lanes
            .iter()
            .position(|lane| lane.group == group && lane.namespace.as_deref() == namespace)
        {
            return found;
        }
        let (first, label, prefix) = group.meta();
        let index = if namespace.is_none() {
            first
        } else {
            let at = next.iter().position(|&(known, _)| known == group);
            let index = at
                .and_then(|at| next.get(at))
                .map_or(first + 1, |&(_, index)| index);
            match at.and_then(|at| next.get_mut(at)) {
                Some(entry) => entry.1 = index + 1,
                None => next.push((group, index + 1)),
            }
            index
        };
        lanes.push(EmLane {
            group,
            index,
            label: namespace.map_or_else(
                || label.to_owned(),
                |namespace| {
                    let mut text = String::from(prefix);
                    text.push_str(namespace);
                    text
                },
            ),
            namespace: namespace.map(str::to_owned),
        });
        lanes.len() - 1
    };

    let mut boxes: Vec<EmBox> = Vec::new();
    let mut relations: Vec<(usize, usize)> = Vec::new();
    for (index, frame) in ast.frames.iter().enumerate() {
        // Only a two-part name has a namespace.
        let parts: Vec<&str> = frame.entity_identifier.split('.').collect();
        let pair = match parts.as_slice() {
            [namespace, name] => Some((*namespace, *name)),
            _ => None,
        };
        let kind = kind_of(frame.model_entity_type);
        let mut text = frame.data_inline_value.map(block_text);
        if let Some(reference) = frame.data_reference
            && let Some(block) = ast
                .data_entities
                .iter()
                .find(|entity| entity.name == reference)
        {
            text = Some(block_text(block.data_block_value));
        }
        let lane = lane(kind, pair.map(|(namespace, _)| namespace));
        let target = boxes.len();
        boxes.push(EmBox {
            data: text,
            data_type: frame.data_type.map(str::to_owned),
            entity: frame.entity_identifier.clone(),
            id: frame.name.to_owned(),
            kind,
            lane,
            name: pair.map_or_else(
                || frame.entity_identifier.clone(),
                |(_, name)| name.to_owned(),
            ),
            namespace: pair.map(|(namespace, _)| namespace.to_owned()),
            reset: frame.reset,
        });

        if !frame.source_frames.is_empty() {
            // Sources connect in frame order, and only frames that already have a box.
            for (source, candidate) in boxes.iter().enumerate() {
                if frame.source_frames.contains(&candidate.id.as_str()) {
                    relations.push((source, target));
                }
            }
        } else if index > 0 && !frame.reset {
            // Otherwise the nearest earlier frame in another lane feeds this one.
            if let Some(source) = (0..target)
                .rev()
                .find(|&i| boxes.get(i).is_some_and(|other| other.lane != lane))
            {
                relations.push((source, target));
            }
        }
    }

    // Lanes go top to bottom by index; boxes follow their lanes to the sorted positions.
    let mut order: Vec<usize> = (0..lanes.len()).collect();
    order.sort_by_key(|&i| lanes.get(i).map_or(u32::MAX, |lane| lane.index));
    let mut position = vec![0; lanes.len()];
    for (to, &from) in order.iter().enumerate() {
        if let Some(slot) = position.get_mut(from) {
            *slot = to;
        }
    }
    for item in &mut boxes {
        item.lane = position.get(item.lane).copied().unwrap_or(item.lane);
    }
    let sorted: Vec<EmLane> = order
        .iter()
        .filter_map(|&i| lanes.get(i).cloned())
        .collect();

    let names = |list: &[&str]| list.iter().map(|&name| name.to_owned()).collect();
    Ok(EventModelingModel {
        acc_descr: ast
            .acc_descr
            .filter(|text| !text.is_empty())
            .map(|text| dedent_lines(&acc_descr_value(text))),
        acc_title: ast
            .acc_title
            .filter(|text| !text.is_empty())
            .map(|text| trim_start(&acc_title_value(text)).to_owned()),
        boxes,
        entities: ast.model_entities.clone(),
        lanes: sorted,
        notes: ast
            .note_entities
            .iter()
            .map(|note| EmNote {
                data_type: note.data_type.map(str::to_owned),
                frame: note.source_frame.to_owned(),
                text: block_text(note.data_block_value),
            })
            .collect(),
        relations,
        specifications: ast
            .gwt_entities
            .iter()
            .map(|gwt| EmSpecification {
                frame: gwt.source_frame.to_owned(),
                given: names(&gwt.given),
                then: names(&gwt.then),
                when: names(&gwt.when),
            })
            .collect(),
        title: ast
            .title
            .map(title_value)
            .filter(|text| !text.is_empty())
            .or_else(|| title.map(str::to_owned)),
    })
}
