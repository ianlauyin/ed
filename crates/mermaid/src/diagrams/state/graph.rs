//! Resolves a state document into the nodes and edges that are drawn: Mermaid's `dataFetcher`.
//! Port of Pele's `src/diagrams/state/graph.ts`.

use std::collections::BTreeMap;

use super::db::StateDb;
use super::types::{Description, StateStmt, Stmt};
use crate::error::{Code, Error, push_u32};

/// Pele's `StateNode`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct StateNode {
    /// The palette slot of a container.
    pub(crate) color_index: Option<u32>,
    /// Class names, space-separated.
    pub(crate) css_classes: String,
    /// Styles from the node's classes.
    pub(crate) css_compiled_styles: Vec<String>,
    /// Styles from `style` statements.
    pub(crate) css_styles: Vec<String>,
    /// The lines below the label.
    pub(crate) description: Vec<String>,
    /// The direction inside a composite: its last `direction` statement, or TB.
    pub(crate) dir: Option<&'static str>,
    /// The node id.
    pub(crate) id: String,
    /// Whether the node is a container.
    pub(crate) is_group: bool,
    /// The label.
    pub(crate) label: Option<String>,
    /// The state a note belongs to.
    pub(crate) note_for: Option<String>,
    /// The id of the enclosing container.
    pub(crate) parent_id: Option<String>,
    /// A note's `left of` or `right of`.
    pub(crate) position: Option<String>,
    /// `rect`, `rectWithTitle`, `stateStart`, `stateEnd`, `choice`, `fork`, `join`, `divider`,
    /// `roundedWithTitle`, `note`, or `noteGroup`.
    pub(crate) shape: Option<&'static str>,
    /// `group` or `node`.
    pub(crate) r#type: Option<&'static str>,
}

/// Pele's `StateEdge`. Labels are markdown throughout, as in Mermaid.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct StateEdge {
    /// Class names, space-separated.
    pub(crate) classes: &'static str,
    /// The node the edge reaches.
    pub(crate) end: String,
    /// The edge id.
    pub(crate) id: String,
    /// The label.
    pub(crate) label: String,
    /// Whether the edge is dashed: a note's connector.
    pub(crate) pattern: Option<&'static str>,
    /// The node the edge leaves.
    pub(crate) start: String,
}

/// Pele's `StateGraph`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct StateGraph {
    /// The edges, in document order.
    pub(crate) edges: Vec<StateEdge>,
    /// The nodes, in document order.
    pub(crate) nodes: Vec<StateNode>,
}

/// A node's settings, decided when its state is first met and refined by later mentions.
struct Draft {
    color_index: Option<u32>,
    css_classes: String,
    css_styles: Vec<String>,
    description: Description,
    dir: Option<&'static str>,
    group: bool,
    shape: Option<&'static str>,
}

/// A body being walked.
struct Frame<'a> {
    alt: bool,
    at: usize,
    doc: &'a [Stmt],
    item: Option<&'a StateStmt>,
}

/// The direction of a body: its last `direction` statement, or TB.
fn direction(doc: &[Stmt]) -> &'static str {
    let mut dir = "TB";
    for item in doc {
        if let Stmt::Dir(value) = item {
            dir = value;
        }
    }
    dir
}

/// The length of a description as JavaScript reads it: UTF-16 units of a string, or items.
fn description_length(description: &Description) -> usize {
    match description {
        Description::Many(texts) => texts.len(),
        Description::One(text) => text.encode_utf16().count(),
    }
}

/// The styles of the classes a node names, in order, as Pele's `insert` compiles them. Pele
/// compiles on every insert, and the last insert decides, so compiling the final classes once
/// gives the same result.
fn compiled_styles(db: &StateDb, css_classes: &str) -> Vec<String> {
    if css_classes.is_empty() {
        return Vec::new();
    }
    let mut compiled = Vec::new();
    for name in css_classes.split(' ') {
        if let Some(def) = db.class(name) {
            compiled.extend(def.styles.iter().cloned());
        }
    }
    compiled
}

/// The builder's state. Pele keeps these as closure variables of `buildStateGraph`.
struct Builder<'a> {
    by_id: BTreeMap<String, usize>,
    counter: u32,
    db: &'a StateDb,
    /// For each node, whether a state's visit wrote it last, so it copies its draft at the end.
    deferred: Vec<bool>,
    drafts: BTreeMap<String, Draft>,
    edges: Vec<StateEdge>,
    next_slot: u32,
    nodes: Vec<StateNode>,
    slots: BTreeMap<String, Option<u32>>,
}

impl Builder<'_> {
    fn next_counter(&mut self) -> u32 {
        let value = self.counter;
        self.counter += 1;
        value
    }

    /// Adds a node, or merges it into the one already added under its id, as `Object.assign`
    /// does. A note's node has no `colorIndex`, and a state's node has no `position` or `noteFor`,
    /// so those are kept from the earlier node.
    ///
    /// A state's node arrives without its label, description, and classes. They are copied from
    /// its draft once the walk ends: the draft only changes while its state is visited, and every
    /// visit ends by inserting the node again, so the last copy Pele makes is of the final draft.
    /// Copying once keeps a state mentioned many times linear.
    fn insert(&mut self, node: StateNode, aside: bool) {
        // The two literal ids are what DOMPurify leaves of `<<fork>>` and `<<choice>>` in Mermaid,
        // kept for parity.
        if node.id.is_empty() || node.id == "</join></fork>" || node.id == "</choice>" {
            return;
        }

        let Some(&at) = self.by_id.get(&node.id) else {
            self.by_id.insert(node.id.clone(), self.nodes.len());
            self.nodes.push(node);
            self.deferred.push(!aside);
            return;
        };
        let (Some(existing), Some(deferred)) = (self.nodes.get_mut(at), self.deferred.get_mut(at))
        else {
            return;
        };
        *deferred = !aside;
        let mut node = node;
        let parent_id = node.parent_id.take().or_else(|| existing.parent_id.take());
        let color_index = existing.color_index;
        let note_for = existing.note_for.take();
        let position = existing.position.take();
        let aside_note_for = node.note_for.take();
        *existing = node;
        existing.parent_id = parent_id;
        if aside {
            existing.color_index = color_index;
            existing.note_for = aside_note_for.or(note_for);
        } else {
            existing.note_for = note_for;
            existing.position = position;
        }
    }

    fn visit(&mut self, parent: Option<&StateStmt>, item: &StateStmt, alt: bool) {
        let id = item.id.as_str();
        if id == "root" {
            return;
        }
        let known = self.db.state(id);
        let class_str = || {
            known
                .and_then(|known| known.classes.as_ref())
                .map(|classes| classes.join(" "))
                .unwrap_or_default()
        };
        let styles = known
            .and_then(|known| known.styles.as_deref())
            .unwrap_or_default();

        if !self.drafts.contains_key(id) {
            let mut shape = Some("rect");
            match item.start {
                Some(true) => shape = Some("stateStart"),
                Some(false) => shape = Some("stateEnd"),
                None => {}
            }
            if item.r#type != Some("default") {
                shape = item.r#type;
            }
            let mut css_classes = class_str();
            css_classes.push_str(" statediagram-state");
            self.drafts.insert(
                id.to_owned(),
                Draft {
                    color_index: None,
                    css_classes,
                    css_styles: styles.to_vec(),
                    description: Description::One(id.to_owned()),
                    dir: None,
                    group: false,
                    shape,
                },
            );
        }
        let Some(draft) = self.drafts.get_mut(id) else {
            return;
        };

        if let Some(description) = item.description.as_ref().filter(|d| d.truthy()) {
            match &mut draft.description {
                Description::Many(texts) => {
                    draft.shape = Some("rectWithTitle");
                    texts.extend(description.texts().iter().cloned());
                }
                Description::One(text) if !text.is_empty() => {
                    draft.shape = Some("rectWithTitle");
                    let mut texts = if text == id {
                        Vec::new()
                    } else {
                        vec![std::mem::take(text)]
                    };
                    texts.extend(description.texts().iter().cloned());
                    draft.description = Description::Many(texts);
                }
                Description::One(_) => {
                    draft.shape = Some("rect");
                    draft.description = description.clone();
                }
            }
        }
        if draft.shape == Some("rectWithTitle") && description_length(&draft.description) == 1 {
            draft.shape = Some(if draft.group {
                "roundedWithTitle"
            } else {
                "rect"
            });
        }

        if !draft.group
            && let Some(doc) = &item.doc
        {
            draft.group = true;
            draft.dir = Some(direction(&doc.0));
            draft.shape = Some(if item.r#type == Some("divider") {
                "divider"
            } else {
                "roundedWithTitle"
            });
            // A container takes the next palette slot unless it is a region, which shares its
            // composite's.
            let shared = parent
                .filter(|_| draft.shape == Some("divider"))
                .and_then(|parent| self.slots.get(&parent.id));
            if let Some(&slot) = shared {
                draft.color_index = slot;
            } else {
                let slot = self.next_slot;
                self.next_slot += 1;
                draft.color_index =
                    if crate::text::label::trim(&class_str()).is_empty() && styles.is_empty() {
                        Some(slot)
                    } else {
                        None
                    };
            }
            self.slots.insert(id.to_owned(), draft.color_index);
            draft.css_classes.push_str(" statediagram-cluster ");
            if alt {
                draft.css_classes.push_str("statediagram-cluster-alt");
            }
        }

        let mut node = StateNode {
            color_index: draft.color_index,
            dir: draft.dir,
            id: id.to_owned(),
            is_group: draft.group,
            shape: draft.shape,
            r#type: draft.group.then_some("group"),
            ..StateNode::default()
        };
        let group_classes = if item.note.is_some() {
            draft.css_classes.clone()
        } else {
            String::new()
        };
        if let Some(parent) = parent.filter(|parent| parent.id != "root") {
            node.parent_id = Some(parent.id.clone());
        }

        let Some(note) = &item.note else {
            self.insert(node, false);
            return;
        };
        // Mermaid puts each note in a group of its own, named after the state. Pele keeps that
        // group in the model and lays the note out beside its state instead.
        let aside =
            |aside_id: String, shape: &'static str, css_classes: String, group: bool| StateNode {
                color_index: None,
                css_classes,
                css_compiled_styles: Vec::new(),
                css_styles: Vec::new(),
                description: Vec::new(),
                dir: None,
                id: aside_id,
                is_group: group,
                label: Some(note.text.clone()),
                note_for: None,
                parent_id: None,
                position: note.position.clone(),
                shape: Some(shape),
                r#type: Some(if group { "group" } else { "node" }),
            };
        let mut note_id = String::from(id);
        note_id.push_str("----note-");
        push_u32(&mut note_id, self.next_counter());
        let mut group_id = String::from(id);
        group_id.push_str("----parent");
        let mut note_node = aside(note_id, "note", String::from("statediagram-note"), false);
        note_node.parent_id = Some(group_id.clone());
        note_node.note_for = Some(id.to_owned());
        self.insert(aside(group_id, "noteGroup", group_classes, true), true);
        let note_node_id = note_node.id.clone();
        self.insert(note_node, true);
        self.insert(node, false);

        let left = note.position.as_deref() == Some("left of");
        let (from, to) = if left {
            (note_node_id, id.to_owned())
        } else {
            (id.to_owned(), note_node_id)
        };
        let mut edge_id = from.clone();
        edge_id.push('-');
        edge_id.push_str(&to);
        self.edges.push(StateEdge {
            classes: "transition note-edge",
            end: to,
            id: edge_id,
            label: String::new(),
            pattern: Some("dashed"),
            start: from,
        });
    }
}

/// Walks the documents and resolves them into the nodes and edges that are drawn, reading the
/// root document's states and classes from `db`.
///
/// # Errors
///
/// Returns a semantic error for a composite state with a description.
pub(crate) fn build_state_graph(root_doc: &[Stmt], db: &StateDb) -> Result<StateGraph, Error> {
    let mut builder = Builder {
        by_id: BTreeMap::new(),
        counter: 0,
        db,
        deferred: Vec::new(),
        drafts: BTreeMap::new(),
        edges: Vec::new(),
        next_slot: 0,
        nodes: Vec::new(),
        slots: BTreeMap::new(),
    };

    let mut stack = vec![Frame {
        alt: true,
        at: 0,
        doc: root_doc,
        item: None,
    }];
    while let Some(frame) = stack.last_mut() {
        let Some(item) = frame.doc.get(frame.at) else {
            stack.pop();
            continue;
        };
        frame.at += 1;
        let (parent, alt) = (frame.item, frame.alt);
        match item {
            Stmt::State(state) => {
                builder.visit(parent, state, alt);
                if let Some(doc) = &state.doc {
                    stack.push(Frame {
                        alt: !alt,
                        at: 0,
                        doc: &doc.0,
                        item: Some(state),
                    });
                }
            }
            Stmt::Relation(relation) => {
                builder.visit(parent, &relation.state1, alt);
                builder.visit(parent, &relation.state2, alt);
                let mut id = String::from("edge");
                push_u32(&mut id, builder.next_counter());
                builder.edges.push(StateEdge {
                    classes: "transition",
                    end: relation.state2.id.clone(),
                    id,
                    label: relation.description.clone().unwrap_or_default(),
                    pattern: None,
                    start: relation.state1.id.clone(),
                });
            }
            _ => {}
        }
    }

    for (node, &deferred) in builder.nodes.iter_mut().zip(&builder.deferred) {
        if deferred && let Some(draft) = builder.drafts.get(&node.id) {
            match &draft.description {
                Description::Many(texts) => {
                    node.label = texts.first().cloned();
                    node.description = texts.get(1..).map(<[String]>::to_vec).unwrap_or_default();
                }
                Description::One(text) => {
                    node.label = Some(text.clone());
                    node.description = Vec::new();
                }
            }
            if node.shape == Some("divider") {
                node.label = Some(String::new());
            }
            node.css_classes.clone_from(&draft.css_classes);
            node.css_styles.clone_from(&draft.css_styles);
        }
        node.css_compiled_styles = compiled_styles(db, &node.css_classes);
    }

    if let Some(node) = builder
        .nodes
        .iter()
        .find(|node| node.is_group && !node.description.is_empty())
    {
        let mut message = String::from(
            "Group nodes can only have label. Remove the additional description for node [",
        );
        message.push_str(&node.id);
        message.push(']');
        let mut error = Error::new(Code::Semantic, message);
        error.r#type = Some("state");
        return Err(error);
    }
    Ok(StateGraph {
        edges: builder.edges,
        nodes: builder.nodes,
    })
}
