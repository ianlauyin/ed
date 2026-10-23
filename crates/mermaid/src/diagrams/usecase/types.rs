//! The use case model. Port of Pele's `src/diagrams/usecase/types.ts`.
//!
//! Pele also records each statement's source positions for Mermaid's syntax tree. Nothing Ondine
//! draws reads them, so they are left out.

use super::json::JsonDoc;

/// The kind of an actor's figure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActorType {
    /// A filled figure.
    Awesome,
    /// A hollow figure.
    Hollow,
    /// An icon in a box.
    Icon,
    /// A stick figure.
    Normal,
}

impl ActorType {
    /// The type as Pele names it.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Awesome => "awesome",
            Self::Hollow => "hollow",
            Self::Icon => "icon",
            Self::Normal => "normal",
        }
    }
}

/// The outline of a use case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    /// An ellipse.
    Ellipse,
    /// A rectangle.
    Rect,
}

impl Shape {
    /// The shape as Pele names it.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Ellipse => "ellipse",
            Self::Rect => "rect",
        }
    }
}

/// An actor.
#[derive(Clone, Debug)]
pub(crate) struct Actor {
    /// Whether it is a business actor.
    pub(crate) business: bool,
    /// Its classes.
    pub(crate) classes: Vec<String>,
    /// The icon of an `icon` actor.
    pub(crate) icon: Option<String>,
    /// Its id.
    pub(crate) id: String,
    /// Its label.
    pub(crate) label: String,
    /// Whether the label is markdown.
    pub(crate) markdown: bool,
    /// The boundary it is in.
    pub(crate) parent_id: Option<String>,
    /// Its stereotype.
    pub(crate) stereotype: Option<String>,
    /// Its inline styles.
    pub(crate) styles: Vec<String>,
    /// Its figure.
    pub(crate) r#type: ActorType,
}

/// A use case.
#[derive(Clone, Debug)]
pub(crate) struct UseCase {
    /// Whether it is a business use case.
    pub(crate) business: bool,
    /// Its classes.
    pub(crate) classes: Vec<String>,
    /// Its id.
    pub(crate) id: String,
    /// Its label.
    pub(crate) label: String,
    /// Whether the label is markdown.
    pub(crate) markdown: bool,
    /// The boundary it is in.
    pub(crate) parent_id: Option<String>,
    /// Its outline.
    pub(crate) shape: Shape,
    /// Its stereotype.
    pub(crate) stereotype: Option<String>,
    /// Its inline styles.
    pub(crate) styles: Vec<String>,
}

/// A system boundary.
#[derive(Clone, Debug)]
pub(crate) struct SystemBoundary {
    /// Its classes.
    pub(crate) classes: Vec<String>,
    /// Its id.
    pub(crate) id: String,
    /// Its title.
    pub(crate) label: String,
    /// Whether the title is markdown.
    pub(crate) markdown: bool,
    /// The ids of the elements in it.
    pub(crate) members: Vec<String>,
    /// Whether it is drawn as a package rather than a rectangle.
    pub(crate) package: bool,
    /// Its inline styles.
    pub(crate) styles: Vec<String>,
}

/// The kind of a relationship.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RelationshipType {
    /// A plain line between two ends.
    Association,
    /// `..> : extend`.
    Extend,
    /// `--|>`.
    Generalization,
    /// `..> : include`.
    Include,
}

impl RelationshipType {
    /// The type as Pele names it.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Association => "association",
            Self::Extend => "extend",
            Self::Generalization => "generalization",
            Self::Include => "include",
        }
    }
}

/// Arrow types, in the order of Pele's `Arrow` enum, whose values the model keeps.
pub(crate) mod arrow {
    /// An arrowhead at the target.
    pub(crate) const SOLID_ARROW: u8 = 0;
    /// An arrowhead at the source.
    pub(crate) const BACK_ARROW: u8 = 1;
    /// No arrowheads.
    pub(crate) const LINE_SOLID: u8 = 2;
    /// A circle at the target.
    pub(crate) const CIRCLE_ARROW: u8 = 3;
    /// A cross at the target.
    pub(crate) const CROSS_ARROW: u8 = 4;
    /// A circle at the source.
    pub(crate) const CIRCLE_ARROW_REVERSED: u8 = 5;
    /// A cross at the source.
    pub(crate) const CROSS_ARROW_REVERSED: u8 = 6;
}

/// A relationship between two elements.
#[derive(Clone, Debug)]
pub(crate) struct Relationship {
    /// Whether it is drawn animated.
    pub(crate) animate: bool,
    /// `fast` or `slow`.
    pub(crate) animation: Option<&'static str>,
    /// One of [`arrow`].
    pub(crate) arrow_type: u8,
    /// Its classes.
    pub(crate) classes: Vec<String>,
    /// Whether the author named it.
    pub(crate) explicit_id: bool,
    /// Its id.
    pub(crate) id: String,
    /// Its label and whether the label is markdown.
    pub(crate) label: Option<(String, bool)>,
    /// The least number of ranks between its ends.
    pub(crate) minlen: u32,
    /// The source id.
    pub(crate) source: String,
    /// Its inline styles.
    pub(crate) styles: Vec<String>,
    /// The target id.
    pub(crate) target: String,
    /// Its kind.
    pub(crate) r#type: RelationshipType,
}

/// A note about an actor or use case.
#[derive(Clone, Debug)]
pub(crate) struct Note {
    /// Its id.
    pub(crate) id: String,
    /// Its text.
    pub(crate) label: String,
    /// Whether the text is markdown.
    pub(crate) markdown: bool,
    /// The id it is about.
    pub(crate) target: String,
}

/// A JSON node.
#[derive(Clone, Debug)]
pub(crate) struct JsonNode {
    /// Its classes.
    pub(crate) classes: Vec<String>,
    /// Its id.
    pub(crate) id: String,
    /// Its inline styles.
    pub(crate) styles: Vec<String>,
    /// Its value, with keys in the order they were written.
    pub(crate) value: JsonDoc,
}

/// A class definition.
#[derive(Clone, Debug)]
pub(crate) struct ClassDef {
    /// Its name.
    pub(crate) id: String,
    /// Its style declarations.
    pub(crate) styles: Vec<String>,
}

/// A parsed use case diagram: Pele's `UsecaseModel`. Collections keep Pele's `Map` order.
#[derive(Clone, Debug, Default)]
pub(crate) struct UsecaseModel {
    /// The accessible description, its lines' indentation dropped.
    pub(crate) acc_descr: String,
    /// The accessible description as written.
    pub(crate) acc_description: String,
    /// The accessible title.
    pub(crate) acc_title: String,
    /// Actors.
    pub(crate) actors: Vec<Actor>,
    /// Class definitions.
    pub(crate) class_defs: Vec<ClassDef>,
    /// `TB`, `BT`, `LR`, or `RL`.
    pub(crate) direction: &'static str,
    /// JSON nodes.
    pub(crate) json_nodes: Vec<JsonNode>,
    /// Notes.
    pub(crate) notes: Vec<Note>,
    /// Relationships in source order.
    pub(crate) relationships: Vec<Relationship>,
    /// System boundaries.
    pub(crate) system_boundaries: Vec<SystemBoundary>,
    /// The front matter title.
    pub(crate) title: Option<String>,
    /// Use cases.
    pub(crate) use_cases: Vec<UseCase>,
}
