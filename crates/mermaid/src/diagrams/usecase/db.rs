//! Resolves what the parser read into a model, once the whole diagram is read. Port of Pele's
//! `src/diagrams/usecase/db.ts`, which ports Mermaid's `UsecaseModelBuilder` and its messages.

use std::collections::BTreeMap;

use super::json::JsonDoc;
use super::lexer::{error, location_text};
use super::types::{
    Actor, ActorType, ClassDef, JsonNode, Note, Relationship, RelationshipType, Shape,
    SystemBoundary, UseCase, UsecaseModel,
};
use crate::error::{Code, Error};
use crate::util::chars::is_space;

/// Start and end byte offsets in the source, end exclusive.
pub(crate) type Span = (usize, usize);

/// A label as written, and where.
#[derive(Clone, Debug, Default)]
pub(crate) struct DraftLabel {
    /// Whether it is a markdown string.
    pub(crate) markdown: bool,
    /// Where it is.
    pub(crate) span: Span,
    /// The text.
    pub(crate) text: String,
}

/// A metadata value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PropValue {
    /// `true` or `false`.
    Bool(bool),
    /// A word or a string.
    Str(String),
}

/// A metadata property. Errors about it point at its key.
#[derive(Clone, Debug)]
pub(crate) struct DraftProperty {
    /// The key.
    pub(crate) key: String,
    /// Where the key is.
    pub(crate) key_span: Span,
    /// The value.
    pub(crate) value: PropValue,
}

/// What every mention of an id carries: where it is, and whether the id was made from a label.
#[derive(Clone, Debug, Default)]
pub(crate) struct Named {
    /// Whether the id was made from the label.
    pub(crate) generated: bool,
    /// The id.
    pub(crate) id: String,
    /// The label.
    pub(crate) label: DraftLabel,
    /// Where the id is.
    pub(crate) span: Span,
}

/// An actor or use case as one statement declares it.
#[derive(Clone, Debug)]
pub(crate) struct DraftElement {
    /// Whether it is an actor.
    pub(crate) actor: bool,
    /// Its `:::` classes.
    pub(crate) classes: Vec<String>,
    /// Its inline metadata.
    pub(crate) metadata: Option<Vec<DraftProperty>>,
    /// Its id and label.
    pub(crate) named: Named,
    /// The boundary around the statement, by index.
    pub(crate) parent: Option<usize>,
    /// Its shape.
    pub(crate) shape: Option<Shape>,
    /// Its stereotype.
    pub(crate) stereotype: Option<String>,
}

/// A system boundary.
#[derive(Clone, Debug)]
pub(crate) struct DraftBoundary {
    /// Its `:::` classes.
    pub(crate) classes: Vec<String>,
    /// Its id and title.
    pub(crate) named: Named,
}

/// A JSON node.
#[derive(Clone, Debug)]
pub(crate) struct DraftJson {
    /// Its `:::` classes.
    pub(crate) classes: Vec<String>,
    /// Its id.
    pub(crate) id: String,
    /// Where its id is.
    pub(crate) span: Span,
    /// Its value.
    pub(crate) value: JsonDoc,
}

/// One end of a relationship.
#[derive(Clone, Debug)]
pub(crate) struct DraftEndpoint {
    /// Whether it carries a `:::` suffix.
    pub(crate) classed: bool,
    /// Whether this mention declares the element.
    pub(crate) declares: bool,
    /// Its id and label.
    pub(crate) named: Named,
}

/// A relationship.
#[derive(Clone, Debug)]
pub(crate) struct DraftRelationship {
    /// One of the arrow types.
    pub(crate) arrow_type: u8,
    /// The id the author gave it, and where.
    pub(crate) id: Option<(String, Span)>,
    /// Its label.
    pub(crate) label: Option<DraftLabel>,
    /// The least number of ranks between its ends.
    pub(crate) minlen: u32,
    /// Its source.
    pub(crate) source: DraftEndpoint,
    /// Where the arrow and target are.
    pub(crate) span: Span,
    /// Its target.
    pub(crate) target: DraftEndpoint,
    /// Its kind.
    pub(crate) r#type: RelationshipType,
}

/// A note.
#[derive(Clone, Debug)]
pub(crate) struct DraftNote {
    /// Its text.
    pub(crate) label: DraftLabel,
    /// Where its target is.
    pub(crate) span: Span,
    /// The id it is about.
    pub(crate) target: String,
}

/// A metadata statement, or a boundary's inline metadata.
#[derive(Clone, Debug)]
pub(crate) struct DraftMetadata {
    /// Its properties.
    pub(crate) properties: Vec<DraftProperty>,
    /// Where its target is.
    pub(crate) span: Span,
    /// The id it is about.
    pub(crate) target: String,
}

/// A `class` statement.
#[derive(Clone, Debug)]
pub(crate) struct DraftClasses {
    /// The classes.
    pub(crate) classes: Vec<String>,
    /// The ids, and where they are.
    pub(crate) targets: Vec<(String, Span)>,
}

/// A `style` statement.
#[derive(Clone, Debug)]
pub(crate) struct DraftStyle {
    /// Where the target is.
    pub(crate) span: Span,
    /// The declarations.
    pub(crate) styles: Vec<String>,
    /// The id.
    pub(crate) target: String,
}

/// A `classDef` statement.
#[derive(Clone, Debug)]
pub(crate) struct DraftClassDef {
    /// The class names.
    pub(crate) ids: Vec<String>,
    /// The declarations.
    pub(crate) styles: Vec<String>,
}

/// Everything the parser read, in source order, before any of it is resolved.
#[derive(Clone, Debug, Default)]
pub(crate) struct Drafts {
    /// The accessible description as written.
    pub(crate) acc_description: String,
    /// The accessible title.
    pub(crate) acc_title: String,
    /// System boundaries.
    pub(crate) boundaries: Vec<DraftBoundary>,
    /// `classDef` statements.
    pub(crate) class_defs: Vec<DraftClassDef>,
    /// `class` statements.
    pub(crate) classes: Vec<DraftClasses>,
    /// The direction, when set.
    pub(crate) direction: Option<&'static str>,
    /// Actor and use case declarations.
    pub(crate) elements: Vec<DraftElement>,
    /// JSON nodes.
    pub(crate) jsons: Vec<DraftJson>,
    /// Metadata statements.
    pub(crate) metadata: Vec<DraftMetadata>,
    /// Notes.
    pub(crate) notes: Vec<DraftNote>,
    /// Relationships.
    pub(crate) relationships: Vec<DraftRelationship>,
    /// `style` statements.
    pub(crate) styles: Vec<DraftStyle>,
}

/// What an id names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SymbolKind {
    /// An actor.
    Actor,
    /// A system boundary.
    Boundary,
    /// A relationship the author named.
    Edge,
    /// A JSON node.
    Json,
    /// A use case.
    Usecase,
}

impl SymbolKind {
    /// The kind as Mermaid names it in messages.
    fn as_str(self) -> &'static str {
        match self {
            Self::Actor => "actor",
            Self::Boundary => "boundary",
            Self::Edge => "edge",
            Self::Json => "json",
            Self::Usecase => "usecase",
        }
    }
}

/// Where an id was first declared.
struct Origin {
    /// Whether the id was made from a label.
    generated: bool,
    /// What the id names.
    kind: SymbolKind,
    /// The label a generated id was derived from.
    label: Option<String>,
    /// Where the declaration is.
    span: Span,
}

/// An actor or use case as the statements so far describe it.
struct ElementState {
    /// Whether it is an actor.
    actor: bool,
    /// The figure metadata asked for.
    actor_type: Option<ActorType>,
    /// Whether metadata made it a business element.
    business: Option<bool>,
    /// Its `:::` classes.
    classes: Vec<String>,
    /// The icon metadata asked for.
    icon: Option<String>,
    /// Its id and label, as first declared.
    named: Named,
    /// The boundary it is in, by index.
    parent: Option<usize>,
    /// Its shape.
    shape: Option<Shape>,
    /// Its stereotype.
    stereotype: Option<String>,
    /// Where its stereotype was declared.
    stereotype_span: Option<Span>,
}

/// A system boundary as the statements so far describe it.
struct BoundaryState {
    /// Its `:::` classes.
    classes: Vec<String>,
    /// The ids of the elements in it.
    members: Vec<String>,
    /// Its id and title.
    named: Named,
    /// Whether metadata made it a package.
    package: Option<bool>,
}

/// A declaration, by its index among the drafts of its kind. Declarations run in source order,
/// whatever kind they are.
enum Declaration {
    /// A system boundary.
    Boundary(usize),
    /// A relationship id.
    Edge(usize),
    /// An actor or use case.
    Element(usize),
    /// A JSON node.
    Json(usize),
}

/// Records in insertion order with an index by id, as Pele's `Map` keeps them.
struct Ordered<V> {
    /// Positions by id. Only looked up, never iterated.
    index: BTreeMap<String, usize>,
    /// The records in insertion order.
    values: Vec<V>,
}

impl<V> Ordered<V> {
    /// An empty collection.
    fn new() -> Self {
        Self {
            index: BTreeMap::new(),
            values: Vec::new(),
        }
    }

    /// The position of `id`.
    fn position(&self, id: &str) -> Option<usize> {
        self.index.get(id).copied()
    }

    /// The record for `id`.
    fn get_mut(&mut self, id: &str) -> Option<&mut V> {
        let at = self.position(id)?;
        self.values.get_mut(at)
    }

    /// Adds a record for a new `id`.
    fn insert(&mut self, id: String, value: V) {
        self.index.insert(id, self.values.len());
        self.values.push(value);
    }
}

/// Appends each of `values` that `target` lacks.
fn push_unique(target: &mut Vec<String>, values: &[String]) {
    for value in values {
        if !target.contains(value) {
            target.push(value.clone());
        }
    }
}

/// Which kind of element the keys of a metadata block could only belong to, for the message.
fn infer_kind(properties: &[DraftProperty]) -> Option<&'static str> {
    /// The kinds, in the order Pele adds them to its set.
    const KINDS: [&str; 4] = ["actor", "usecase", "boundary", "edge"];
    let mut possible = [true; 4];
    for property in properties {
        let value = match &property.value {
            PropValue::Str(value) => Some(value.as_str()),
            PropValue::Bool(_) => None,
        };
        match property.key.as_str() {
            "icon" => possible = [true, false, false, false],
            "animate" | "animation" => possible = [false, false, false, true],
            "type" => {
                possible = match value {
                    Some("rect" | "package") => [false, false, true, false],
                    Some("normal" | "hollow" | "awesome") => [true, false, false, false],
                    _ => [false; 4],
                };
            }
            "business" => {
                possible[2] = false;
                possible[3] = false;
            }
            _ => return None,
        }
    }
    let mut kinds = KINDS.iter().zip(possible).filter(|(_, on)| *on);
    match (kinds.next(), kinds.next()) {
        (Some((kind, _)), None) => Some(kind),
        _ => None,
    }
}

/// `text` with every line break and the whitespace after it turned into one line break, as
/// `replace(/\n\s+/g, '\n')` does.
fn drop_indentation(text: &str) -> String {
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

/// A semantic error: `message` at `span`.
#[cold]
fn fail(src: &str, message: &str, span: Span) -> Error {
    let mut text = String::from(message);
    text.push_str(" at ");
    text.push_str(&location_text(src, span.0, span.1));
    error(src, Code::Semantic, text, span.0)
}

/// A semantic error about two declarations, each with the label its id was made from.
#[cold]
fn conflict(
    src: &str,
    message: &str,
    current: Span,
    previous: Span,
    current_label: Option<&str>,
    previous_label: Option<&str>,
) -> Error {
    let suffix = |out: &mut String, label: Option<&str>| {
        if let Some(label) = label {
            out.push_str(" (label \"");
            out.push_str(label);
            out.push_str("\")");
        }
    };
    let mut text = String::from(message);
    text.push_str(" at ");
    text.push_str(&location_text(src, current.0, current.1));
    suffix(&mut text, current_label);
    text.push_str("; previous declaration at ");
    text.push_str(&location_text(src, previous.0, previous.1));
    suffix(&mut text, previous_label);
    error(src, Code::Semantic, text, current.0)
}

/// `Metadata property '<key>' is invalid for <kind> '<id>'`.
#[cold]
fn invalid(src: &str, id: &str, kind: &str, property: &DraftProperty) -> Error {
    let mut message = String::from("Metadata property '");
    message.push_str(&property.key);
    message.push_str("' is invalid for ");
    message.push_str(kind);
    message.push_str(" '");
    message.push_str(id);
    message.push('\'');
    fail(src, &message, property.key_span)
}

/// A message of literal parts and values.
fn text(parts: &[&str]) -> String {
    parts.concat()
}

/// Resolves what the parser read into a model: which ids are actors, use cases, boundaries,
/// JSON nodes, or edges, in what order they appear, and whether the statements about them agree.
///
/// # Errors
///
/// Returns Mermaid's semantic error for the first statement that disagrees with another.
#[expect(clippy::too_many_lines, reason = "mirrors Pele's resolveUsecase")]
pub(crate) fn resolve_usecase(
    src: &str,
    drafts: Drafts,
    title: Option<&str>,
) -> Result<UsecaseModel, Error> {
    let mut symbols: BTreeMap<String, Origin> = BTreeMap::new();
    let mut elements: Ordered<ElementState> = Ordered::new();
    let mut boundaries: Ordered<BoundaryState> = Ordered::new();
    // The offset at which each id is first mentioned, which decides the order of the model.
    let mut first: BTreeMap<String, usize> = BTreeMap::new();

    let mention = |first: &mut BTreeMap<String, usize>, id: &str, offset: usize| {
        let entry = first.entry(id.to_owned()).or_insert(offset);
        if offset < *entry {
            *entry = offset;
        }
    };
    let register = |symbols: &mut BTreeMap<String, Origin>, named: &Named, kind: SymbolKind| {
        symbols.insert(
            named.id.clone(),
            Origin {
                generated: named.generated,
                kind,
                label: Some(named.label.text.clone()),
                span: named.span,
            },
        );
    };

    // An id means one thing in the whole diagram, and an id made from a label may be declared
    // once.
    let claim = |symbols: &BTreeMap<String, Origin>,
                 draft: &Named,
                 kind: SymbolKind|
     -> Result<(), Error> {
        let Some(origin) = symbols.get(&draft.id) else {
            return Ok(());
        };
        let label = draft.generated.then_some(draft.label.text.as_str());
        let origin_label = if origin.generated {
            origin.label.as_deref()
        } else {
            None
        };
        if origin.kind != kind {
            return Err(conflict(
                src,
                &text(&[
                    "ID '",
                    &draft.id,
                    "' is declared as both ",
                    origin.kind.as_str(),
                    " and ",
                    kind.as_str(),
                ]),
                draft.span,
                origin.span,
                label,
                origin_label,
            ));
        }
        if origin.generated || draft.generated {
            return Err(conflict(
                src,
                &text(&[
                    "Generated ID '",
                    &draft.id,
                    "' collides with another declaration",
                ]),
                draft.span,
                origin.span,
                label,
                origin_label,
            ));
        }
        Ok(())
    };

    let unique = |symbols: &mut BTreeMap<String, Origin>,
                  id: &str,
                  kind: SymbolKind,
                  span: Span|
     -> Result<(), Error> {
        if let Some(previous) = symbols.get(id) {
            return Err(conflict(
                src,
                &text(&[
                    "ID '",
                    id,
                    "' is declared more than once (",
                    previous.kind.as_str(),
                    " and ",
                    kind.as_str(),
                    ")",
                ]),
                span,
                previous.span,
                None,
                if previous.generated {
                    previous.label.as_deref()
                } else {
                    None
                },
            ));
        }
        symbols.insert(
            id.to_owned(),
            Origin {
                generated: false,
                kind,
                label: None,
                span,
            },
        );
        Ok(())
    };

    // A metadata statement replaces what a declaration said; two declarations have to agree.
    let element_metadata = |state: &mut ElementState,
                            properties: Option<&Vec<DraftProperty>>,
                            replace: bool|
     -> Result<(), Error> {
        for property in properties.into_iter().flatten() {
            let id = state.named.id.as_str();
            let key = property.key.as_str();
            if state.actor {
                let clash = |previous: bool, same: bool| {
                    if !replace && previous && !same {
                        Err(conflict(
                            src,
                            &text(&["Actor '", id, "' has conflicting ", key, " metadata"]),
                            property.key_span,
                            state.named.span,
                            None,
                            None,
                        ))
                    } else {
                        Ok(())
                    }
                };
                match (key, &property.value) {
                    ("type", PropValue::Str(value))
                        if matches!(value.as_str(), "normal" | "hollow" | "awesome") =>
                    {
                        let value = match value.as_str() {
                            "hollow" => ActorType::Hollow,
                            "awesome" => ActorType::Awesome,
                            _ => ActorType::Normal,
                        };
                        clash(state.actor_type.is_some(), state.actor_type == Some(value))?;
                        state.actor_type = Some(value);
                    }
                    ("icon", PropValue::Str(value)) => {
                        clash(state.icon.is_some(), state.icon.as_ref() == Some(value))?;
                        state.icon = Some(value.clone());
                    }
                    ("business", PropValue::Bool(value)) => {
                        clash(state.business.is_some(), state.business == Some(*value))?;
                        state.business = Some(*value);
                    }
                    _ => return Err(invalid(src, id, "actor", property)),
                }
            } else if let ("business", PropValue::Bool(value)) = (key, &property.value) {
                if !replace && state.business.is_some_and(|business| business != *value) {
                    return Err(conflict(
                        src,
                        &text(&["Use case '", id, "' has conflicting business metadata"]),
                        property.key_span,
                        state.named.span,
                        None,
                        None,
                    ));
                }
                state.business = Some(*value);
            } else {
                return Err(invalid(src, id, "usecase", property));
            }
        }
        Ok(())
    };

    let mut declarations: Vec<(usize, Declaration)> = Vec::new();
    for relationship in &drafts.relationships {
        mention(
            &mut first,
            &relationship.source.named.id,
            relationship.source.named.span.0,
        );
        mention(
            &mut first,
            &relationship.target.named.id,
            relationship.target.named.span.0,
        );
    }
    for (i, draft) in drafts.elements.iter().enumerate() {
        mention(&mut first, &draft.named.id, draft.named.span.0);
        declarations.push((draft.named.span.0, Declaration::Element(i)));
    }
    for (i, draft) in drafts.boundaries.iter().enumerate() {
        mention(&mut first, &draft.named.id, draft.named.span.0);
        declarations.push((draft.named.span.0, Declaration::Boundary(i)));
    }
    for (i, draft) in drafts.jsons.iter().enumerate() {
        mention(&mut first, &draft.id, draft.span.0);
        declarations.push((draft.span.0, Declaration::Json(i)));
    }
    for (i, relationship) in drafts.relationships.iter().enumerate() {
        if let Some((_, span)) = &relationship.id {
            declarations.push((span.0, Declaration::Edge(i)));
        }
    }
    declarations.sort_by_key(|(offset, _)| *offset);

    let boundary_named = |index: Option<usize>| {
        index
            .and_then(|at| drafts.boundaries.get(at))
            .map(|boundary| &boundary.named)
    };

    for (_, declaration) in &declarations {
        match *declaration {
            Declaration::Element(i) => {
                let Some(draft) = drafts.elements.get(i) else {
                    continue;
                };
                let kind = if draft.actor {
                    SymbolKind::Actor
                } else {
                    SymbolKind::Usecase
                };
                claim(&symbols, &draft.named, kind)?;
                let parent = draft
                    .parent
                    .filter(|_| boundary_named(draft.parent).is_some_and(|b| !b.id.is_empty()));
                let Some(existing) = elements.get_mut(&draft.named.id) else {
                    let mut state = ElementState {
                        actor: draft.actor,
                        actor_type: None,
                        business: None,
                        classes: draft.classes.clone(),
                        icon: None,
                        named: draft.named.clone(),
                        parent,
                        shape: draft.shape,
                        stereotype: draft.stereotype.clone(),
                        stereotype_span: draft.stereotype.as_ref().map(|_| draft.named.span),
                    };
                    element_metadata(&mut state, draft.metadata.as_ref(), false)?;
                    elements.insert(draft.named.id.clone(), state);
                    register(&mut symbols, &draft.named, kind);
                    continue;
                };
                let id = draft.named.id.as_str();
                if existing.named.label.text != draft.named.label.text
                    || existing.named.label.markdown != draft.named.label.markdown
                {
                    return Err(conflict(
                        src,
                        &text(&["ID '", id, "' has conflicting labels"]),
                        draft.named.span,
                        existing.named.span,
                        None,
                        None,
                    ));
                }
                if let (Some(shape), Some(own)) = (draft.shape, existing.shape)
                    && shape != own
                {
                    return Err(conflict(
                        src,
                        &text(&["Use case '", id, "' has conflicting shapes"]),
                        draft.named.span,
                        existing.named.span,
                        None,
                        None,
                    ));
                }
                // A boundary whose title makes an empty id is no parent, but it still takes the
                // place of one.
                if let (Some(parent), Some(own)) =
                    (boundary_named(parent), boundary_named(existing.parent))
                    && !own.id.is_empty()
                    && parent.id != own.id
                {
                    return Err(conflict(
                        src,
                        &text(&[
                            "Element '",
                            id,
                            "' belongs to more than one system boundary",
                        ]),
                        parent.span,
                        own.span,
                        None,
                        None,
                    ));
                }
                if let (Some(stereotype), Some(own)) = (&draft.stereotype, &existing.stereotype)
                    && stereotype != own
                {
                    return Err(conflict(
                        src,
                        &text(&["Element '", id, "' has conflicting stereotypes"]),
                        draft.named.span,
                        existing.stereotype_span.unwrap_or(existing.named.span),
                        None,
                        None,
                    ));
                }
                existing.shape = existing.shape.or(draft.shape);
                existing.parent = existing.parent.or(draft.parent);
                if existing.stereotype.is_none() {
                    existing.stereotype.clone_from(&draft.stereotype);
                }
                if existing.stereotype_span.is_none() {
                    existing.stereotype_span = draft.stereotype.as_ref().map(|_| draft.named.span);
                }
                push_unique(&mut existing.classes, &draft.classes);
                element_metadata(existing, draft.metadata.as_ref(), false)?;
            }
            Declaration::Boundary(i) => {
                let Some(draft) = drafts.boundaries.get(i) else {
                    continue;
                };
                claim(&symbols, &draft.named, SymbolKind::Boundary)?;
                if let Some(existing) = boundaries.get_mut(&draft.named.id) {
                    if existing.named.label.text != draft.named.label.text
                        || existing.named.label.markdown != draft.named.label.markdown
                    {
                        return Err(conflict(
                            src,
                            &text(&["Boundary '", &draft.named.id, "' has conflicting titles"]),
                            draft.named.span,
                            existing.named.span,
                            None,
                            None,
                        ));
                    }
                    push_unique(&mut existing.classes, &draft.classes);
                    continue;
                }
                boundaries.insert(
                    draft.named.id.clone(),
                    BoundaryState {
                        classes: draft.classes.clone(),
                        members: Vec::new(),
                        named: draft.named.clone(),
                        package: None,
                    },
                );
                register(&mut symbols, &draft.named, SymbolKind::Boundary);
            }
            Declaration::Json(i) => {
                if let Some(draft) = drafts.jsons.get(i) {
                    unique(&mut symbols, &draft.id, SymbolKind::Json, draft.span)?;
                }
            }
            Declaration::Edge(i) => {
                if let Some((id, span)) = drafts.relationships.get(i).and_then(|r| r.id.as_ref()) {
                    unique(&mut symbols, id, SymbolKind::Edge, *span)?;
                }
            }
        }
    }

    // An end of a relationship that nothing declares becomes a use case.
    let mut relationships: Vec<Relationship> = Vec::new();
    let mut edges: BTreeMap<String, usize> = BTreeMap::new();
    let mut anonymous = 0usize;
    for draft in &drafts.relationships {
        let mut endpoint = |end: &DraftEndpoint| -> Result<SymbolKind, Error> {
            if end.classed && !end.declares {
                return Err(fail(
                    src,
                    &text(&[
                        "Relationship endpoint '",
                        &end.named.id,
                        "' uses ::: without declaring the node",
                    ]),
                    end.named.span,
                ));
            }
            if let Some(origin) = symbols.get(&end.named.id) {
                return Ok(origin.kind);
            }
            elements.insert(
                end.named.id.clone(),
                ElementState {
                    actor: false,
                    actor_type: None,
                    business: None,
                    classes: Vec::new(),
                    icon: None,
                    named: end.named.clone(),
                    parent: None,
                    shape: Some(Shape::Ellipse),
                    stereotype: None,
                    stereotype_span: None,
                },
            );
            register(&mut symbols, &end.named, SymbolKind::Usecase);
            mention(&mut first, &end.named.id, end.named.span.0);
            Ok(SymbolKind::Usecase)
        };
        let source_kind = endpoint(&draft.source)?;
        let target_kind = endpoint(&draft.target)?;
        let related = |kind: SymbolKind| {
            matches!(
                kind,
                SymbolKind::Actor | SymbolKind::Usecase | SymbolKind::Json
            )
        };
        let origin_span = |id: &str| symbols.get(id).map_or((0, 0), |origin| origin.span);
        for (end, kind, role) in [
            (&draft.source, source_kind, "source"),
            (&draft.target, target_kind, "target"),
        ] {
            if !related(kind) {
                return Err(conflict(
                    src,
                    &text(&[
                        "Relationship ",
                        role,
                        " '",
                        &end.named.id,
                        "' cannot be ",
                        kind.as_str(),
                    ]),
                    end.named.span,
                    origin_span(&end.named.id),
                    None,
                    None,
                ));
            }
        }
        let r#type = draft.r#type;
        if matches!(r#type, RelationshipType::Include | RelationshipType::Extend)
            && (source_kind != SymbolKind::Usecase || target_kind != SymbolKind::Usecase)
        {
            return Err(fail(
                src,
                &text(&[r#type.as_str(), " relationship requires use-case endpoints"]),
                draft.span,
            ));
        }
        if r#type == RelationshipType::Generalization
            && (source_kind == SymbolKind::Json || source_kind != target_kind)
        {
            return Err(fail(
                src,
                "Generalization requires actor-to-actor or use-case-to-use-case endpoints",
                draft.span,
            ));
        }
        if r#type == RelationshipType::Association
            && (source_kind == SymbolKind::Json || target_kind == SymbolKind::Json)
            && draft.arrow_type > 2
        {
            return Err(fail(
                src,
                &text(&[
                    "JSON relationship '",
                    &draft.source.named.id,
                    "' to '",
                    &draft.target.named.id,
                    "' permits only point, reversed-point, or markerless solid association",
                ]),
                draft.span,
            ));
        }
        let id = if let Some((id, _)) = &draft.id {
            id.clone()
        } else {
            let mut id = String::from("edge-");
            super::lexer::push_usize(&mut id, anonymous);
            anonymous += 1;
            id
        };
        edges.insert(id.clone(), relationships.len());
        relationships.push(Relationship {
            animate: false,
            animation: None,
            arrow_type: draft.arrow_type,
            classes: Vec::new(),
            explicit_id: draft.id.is_some(),
            id,
            label: draft
                .label
                .as_ref()
                .map(|label| (label.text.clone(), label.markdown)),
            minlen: draft.minlen,
            source: draft.source.named.id.clone(),
            styles: Vec::new(),
            target: draft.target.named.id.clone(),
            r#type,
        });
    }

    for metadata in &drafts.metadata {
        let target = metadata.target.as_str();
        let Some(origin) = symbols.get(target) else {
            let mut message = text(&["Metadata target '", target, "' is unresolved"]);
            if let Some(kind) = infer_kind(&metadata.properties) {
                message.push_str(" (metadata implies ");
                message.push_str(kind);
                message.push(')');
            }
            return Err(fail(src, &message, metadata.span));
        };
        let kind = origin.kind;
        if matches!(kind, SymbolKind::Actor | SymbolKind::Usecase) {
            if let Some(state) = elements.get_mut(target) {
                element_metadata(state, Some(&metadata.properties), true)?;
            }
            continue;
        }
        let edge = edges.get(target).copied();
        for property in &metadata.properties {
            let key = property.key.as_str();
            match (kind, key, &property.value) {
                (SymbolKind::Boundary, "type", PropValue::Str(value))
                    if matches!(value.as_str(), "rect" | "package")
                        && boundaries.position(target).is_some() =>
                {
                    if let Some(boundary) = boundaries.get_mut(target) {
                        boundary.package = Some(value == "package");
                    }
                }
                (SymbolKind::Edge, "animate", PropValue::Bool(value)) if edge.is_some() => {
                    if let Some(edge) = edge.and_then(|at| relationships.get_mut(at)) {
                        edge.animate = *value;
                    }
                }
                (SymbolKind::Edge, "animation", PropValue::Str(value))
                    if edge.is_some() && matches!(value.as_str(), "fast" | "slow") =>
                {
                    if let Some(edge) = edge.and_then(|at| relationships.get_mut(at)) {
                        edge.animation = Some(if value == "fast" { "fast" } else { "slow" });
                        edge.animate = true;
                    }
                }
                _ => return Err(invalid(src, target, kind.as_str(), property)),
            }
        }
    }
    for relationship in &mut relationships {
        if relationship.animation.is_some() {
            relationship.animate = true;
        }
    }

    for state in &elements.values {
        let id = state.named.id.as_str();
        let icon = state.icon.as_ref().is_some_and(|icon| !icon.is_empty());
        if state.actor {
            if let Some(actor_type) = state.actor_type
                && icon
                && actor_type != ActorType::Normal
            {
                return Err(fail(
                    src,
                    &text(&[
                        "Actor '",
                        id,
                        "' cannot combine icon with type '",
                        actor_type.as_str(),
                        "'",
                    ]),
                    state.named.span,
                ));
            }
            if state.business == Some(true)
                && (icon || state.actor_type == Some(ActorType::Awesome))
            {
                return Err(fail(
                    src,
                    &text(&[
                        "Business actor '",
                        id,
                        "' must use normal or hollow geometry",
                    ]),
                    state.named.span,
                ));
            }
        } else if state.shape == Some(Shape::Rect) && state.business == Some(true) {
            return Err(fail(
                src,
                &text(&[
                    "Rectangular use case '",
                    id,
                    "' cannot be a business use case",
                ]),
                state.named.span,
            ));
        }
    }

    let mut by_offset: Vec<&DraftElement> = drafts.elements.iter().collect();
    by_offset.sort_by_key(|draft| draft.named.span.0);
    for draft in by_offset {
        let Some(parent) = boundary_named(draft.parent).filter(|parent| !parent.id.is_empty())
        else {
            continue;
        };
        if let Some(boundary) = boundaries.get_mut(&parent.id)
            && !boundary.members.contains(&draft.named.id)
        {
            boundary.members.push(draft.named.id.clone());
        }
    }

    let order = |id: &str| first.get(id).copied().unwrap_or(0);
    let mut model = UsecaseModel {
        acc_descr: drop_indentation(&drafts.acc_description),
        direction: drafts.direction.unwrap_or("LR"),
        title: title.map(str::to_owned),
        ..UsecaseModel::default()
    };

    let mut states: Vec<&ElementState> = elements.values.iter().collect();
    states.sort_by_key(|state| order(&state.named.id));
    for state in states {
        let named = &state.named;
        let parent_id = boundary_named(state.parent)
            .filter(|parent| !parent.id.is_empty())
            .map(|parent| parent.id.clone());
        let stereotype = state
            .stereotype
            .clone()
            .filter(|stereotype| !stereotype.is_empty());
        if state.actor {
            let icon = state.icon.clone().filter(|icon| !icon.is_empty());
            model.actors.push(Actor {
                business: state.business.unwrap_or(false),
                classes: state.classes.clone(),
                r#type: if icon.is_some() {
                    ActorType::Icon
                } else {
                    state.actor_type.unwrap_or(ActorType::Normal)
                },
                icon,
                id: named.id.clone(),
                label: named.label.text.clone(),
                markdown: named.label.markdown,
                parent_id,
                stereotype,
                styles: Vec::new(),
            });
        } else {
            model.use_cases.push(UseCase {
                business: state.business.unwrap_or(false),
                classes: state.classes.clone(),
                id: named.id.clone(),
                label: named.label.text.clone(),
                markdown: named.label.markdown,
                parent_id,
                shape: state.shape.unwrap_or(Shape::Ellipse),
                stereotype,
                styles: Vec::new(),
            });
        }
    }

    let mut ordered: Vec<&BoundaryState> = boundaries.values.iter().collect();
    ordered.sort_by_key(|state| order(&state.named.id));
    for state in ordered {
        model.system_boundaries.push(SystemBoundary {
            classes: state.classes.clone(),
            id: state.named.id.clone(),
            label: state.named.label.text.clone(),
            markdown: state.named.label.markdown,
            members: state.members.clone(),
            package: state.package == Some(true),
            styles: Vec::new(),
        });
    }

    let mut jsons: Vec<DraftJson> = drafts.jsons;
    jsons.sort_by_key(|draft| order(&draft.id));
    for draft in jsons {
        model.json_nodes.push(JsonNode {
            classes: draft.classes,
            id: draft.id,
            styles: Vec::new(),
            value: draft.value,
        });
    }

    for (count, draft) in drafts.notes.iter().enumerate() {
        let Some(origin) = symbols.get(&draft.target) else {
            return Err(fail(
                src,
                &text(&["Note target '", &draft.target, "' is unresolved"]),
                draft.span,
            ));
        };
        if !matches!(origin.kind, SymbolKind::Actor | SymbolKind::Usecase) {
            return Err(conflict(
                src,
                &text(&[
                    "Note target '",
                    &draft.target,
                    "' must be an actor or use case, not ",
                    origin.kind.as_str(),
                ]),
                draft.span,
                origin.span,
                None,
                None,
            ));
        }
        let mut id = String::from("note-");
        super::lexer::push_usize(&mut id, count);
        model.notes.push(Note {
            id,
            label: draft.label.text.clone(),
            markdown: draft.label.markdown,
            target: draft.target.clone(),
        });
    }

    for definition in drafts.class_defs {
        for id in definition.ids {
            let styles = definition.styles.clone();
            match model.class_defs.iter_mut().find(|own| own.id == id) {
                Some(own) => own.styles = styles,
                None => model.class_defs.push(ClassDef { id, styles }),
            }
        }
    }

    model.relationships = relationships;
    for assignment in &drafts.classes {
        for (id, span) in &assignment.targets {
            let (classes, _) = target(&mut model, &symbols, &edges, src, id, *span)?;
            push_unique(classes, &assignment.classes);
        }
    }
    for assignment in &drafts.styles {
        let (_, styles) = target(
            &mut model,
            &symbols,
            &edges,
            src,
            &assignment.target,
            assignment.span,
        )?;
        styles.extend(assignment.styles.iter().cloned());
    }

    model.acc_title = drafts.acc_title;
    model.acc_description = drafts.acc_description;
    Ok(model)
}

/// The classes and styles of `id`, wherever it lives in the model. An id no symbol names may
/// still be an anonymous edge.
fn target<'m>(
    model: &'m mut UsecaseModel,
    symbols: &BTreeMap<String, Origin>,
    edges: &BTreeMap<String, usize>,
    src: &str,
    id: &str,
    span: Span,
) -> Result<(&'m mut Vec<String>, &'m mut Vec<String>), Error> {
    let found = match symbols.get(id).map(|origin| origin.kind) {
        Some(SymbolKind::Actor) => model
            .actors
            .iter_mut()
            .find(|actor| actor.id == id)
            .map(|actor| (&mut actor.classes, &mut actor.styles)),
        Some(SymbolKind::Usecase) => model
            .use_cases
            .iter_mut()
            .find(|use_case| use_case.id == id)
            .map(|use_case| (&mut use_case.classes, &mut use_case.styles)),
        Some(SymbolKind::Boundary) => model
            .system_boundaries
            .iter_mut()
            .find(|boundary| boundary.id == id)
            .map(|boundary| (&mut boundary.classes, &mut boundary.styles)),
        Some(SymbolKind::Json) => model
            .json_nodes
            .iter_mut()
            .find(|json| json.id == id)
            .map(|json| (&mut json.classes, &mut json.styles)),
        Some(SymbolKind::Edge) | None => edges
            .get(id)
            .and_then(|&at| model.relationships.get_mut(at))
            .map(|edge| (&mut edge.classes, &mut edge.styles)),
    };
    found.ok_or_else(|| {
        fail(
            src,
            &text(&["Class/style target '", id, "' is unresolved or anonymous"]),
            span,
        )
    })
}
