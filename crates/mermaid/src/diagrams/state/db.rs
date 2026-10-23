//! The state model builder: Mermaid's `StateDB` in its version 2 form, which Mermaid uses for both
//! `stateDiagram` and `stateDiagram-v2`. Port of Pele's `src/diagrams/state/db.ts`.
//!
//! Pele keeps states, classes, and links in `Map`s. Here each is a `Vec` in insertion order with a
//! `BTreeMap` index for lookups, so iteration order never depends on hashing.

use std::collections::BTreeMap;

use super::graph::{StateGraph, build_state_graph};
use super::types::{
    Description, Doc, StateClassDef, StateLink, StateNote, StateRelation, StateStmt, Stmt,
};
use crate::error::{Error, push_u32};
use crate::text::label::trim;

/// The parsed state diagram: Pele's `StateDb`.
#[derive(Clone, Debug, Default)]
pub(crate) struct StateDb {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    /// Class definitions, in insertion order.
    pub(crate) classes: Vec<StateClassDef>,
    /// The nodes and edges that are drawn.
    pub(crate) graph: StateGraph,
    /// Links by state id, in insertion order.
    pub(crate) links: Vec<(String, StateLink)>,
    /// Transitions of the root document.
    pub(crate) relations: Vec<StateRelation>,
    /// The parsed document.
    pub(crate) root_doc: Vec<Stmt>,
    /// For each state, the root statement whose body Pele's model shares with it. Pele records the
    /// body itself, which nothing reads after the graph is built.
    pub(crate) state_docs: Vec<Option<usize>>,
    /// States of the root document, in insertion order.
    pub(crate) states: Vec<StateStmt>,
    /// The front matter title.
    pub(crate) title: Option<String>,

    class_index: BTreeMap<String, usize>,
    divider_count: u32,
    link_index: BTreeMap<String, usize>,
    state_index: BTreeMap<String, usize>,
}

/// A frame of the document walk in [`StateDb::translate`].
struct Frame {
    /// The body being walked.
    doc: Vec<Stmt>,
    /// The next statement to visit.
    at: usize,
    /// The id that names `[*]` states in the body.
    parent_id: String,
    /// The index of the body's owner in the enclosing frame, or `None` for the root document.
    slot: Option<usize>,
}

/// `text` with the first `from` replaced by `to`, as `String.prototype.replace` with a string does.
fn replace_first(text: &str, from: &str, to: &str) -> String {
    match text.find(from) {
        Some(at) => {
            let mut out = String::with_capacity(text.len() + to.len());
            out.push_str(text.get(..at).unwrap_or(""));
            out.push_str(to);
            out.push_str(text.get(at + from.len()..).unwrap_or(""));
            out
        }
        None => text.to_owned(),
    }
}

/// Renames a `[*]` state after its document, or trims another state's id.
fn name(node: &mut StateStmt, parent_id: &str, first: bool) {
    if node.id == "[*]" {
        let mut id = String::from(parent_id);
        id.push_str(if first { "_start" } else { "_end" });
        node.id = id;
        node.start = Some(first);
    } else {
        node.id = trim(&node.id).to_owned();
    }
}

impl StateDb {
    /// The direction the root document is laid out in: its last `direction` statement.
    pub(crate) fn direction(&self) -> &'static str {
        let mut dir = "TB";
        for item in &self.root_doc {
            if let Stmt::Dir(value) = item {
                dir = value;
            }
        }
        dir
    }

    /// Stores the parsed document and builds the model from it.
    ///
    /// # Errors
    ///
    /// Returns a semantic error for a composite state with a description.
    pub(crate) fn set_root_doc(&mut self, doc: Vec<Stmt>) -> Result<(), Error> {
        self.root_doc = doc;
        self.translate();
        self.extract();
        let graph = build_state_graph(&self.root_doc, self)?;
        self.graph = graph;
        Ok(())
    }

    /// The root document's state named `id`.
    pub(crate) fn state(&self, id: &str) -> Option<&StateStmt> {
        self.state_index.get(id).and_then(|&at| self.states.get(at))
    }

    /// The class definition named `id`.
    pub(crate) fn class(&self, id: &str) -> Option<&StateClassDef> {
        self.class_index
            .get(id)
            .and_then(|&at| self.classes.get(at))
    }

    /// The link on the state named `id`.
    pub(crate) fn link(&self, id: &str) -> Option<&StateLink> {
        self.link_index
            .get(id)
            .and_then(|&at| self.links.get(at))
            .map(|(_, link)| link)
    }

    /// Only the root document's statements become states, relations, and classes here. The
    /// statements inside composites are read when the graph is built.
    fn extract(&mut self) {
        self.states.clear();
        self.state_index.clear();
        self.relations.clear();
        self.classes.clear();
        self.class_index.clear();

        self.state_docs.clear();

        let doc = std::mem::take(&mut self.root_doc);
        for (at, item) in doc.iter().enumerate() {
            match item {
                Stmt::State(state) => {
                    self.add_state(
                        &state.id,
                        state.r#type,
                        state.doc.as_ref().map(|_| at),
                        state.description.as_ref(),
                        state.note.as_ref(),
                        None,
                    );
                }
                Stmt::Relation(relation) => {
                    self.add_relation(
                        &relation.state1,
                        &relation.state2,
                        relation.description.as_deref().unwrap_or(""),
                    );
                }
                Stmt::ClassDef { classes, id } => self.add_style_class(trim(id), classes),
                Stmt::Style { id, style_class } => {
                    let styles: Vec<String> = style_class
                        .split(',')
                        .map(|s| {
                            trim(&s.chars().filter(|&c| c != ';').collect::<String>()).to_owned()
                        })
                        .collect();
                    for at in self.named(trim(id)) {
                        if let Some(state) = self.states.get_mut(at) {
                            state.styles = Some(styles.clone());
                        }
                    }
                }
                Stmt::ApplyClass { id, style_class } => self.set_css_class(trim(id), style_class),
                Stmt::Click { id, tooltip, url } => self.add_link(&id.id, url, tooltip),
                Stmt::Dir(_) | Stmt::Text(_) => {}
            }
        }
        self.root_doc = doc;
    }

    /// Mermaid's `docTranslator`: gives each `[*]` an id scoped to its parent, and turns a body
    /// split by `--` into one region per part. Walks the tree with an explicit stack, in Pele's
    /// order, so deep nesting cannot overflow the call stack.
    fn translate(&mut self) {
        // Pele also regroups the root document, but into a copy it then drops. Dividers only lex
        // inside a body, so the root document never has one.
        let mut stack = vec![Frame {
            at: 0,
            doc: std::mem::take(&mut self.root_doc),
            parent_id: String::from("root"),
            slot: None,
        }];
        while let Some(top) = stack.last_mut() {
            let at = top.at;
            if at >= top.doc.len() {
                let Some(frame) = stack.pop() else { break };
                match (frame.slot, stack.last_mut()) {
                    (Some(slot), Some(parent)) => {
                        if let Some(Stmt::State(state)) = parent.doc.get_mut(slot) {
                            state.doc = Some(Doc(frame.doc));
                        }
                    }
                    _ => self.root_doc = frame.doc,
                }
                continue;
            }
            top.at += 1;

            let parent_id = top.parent_id.clone();
            let body = match top.doc.get_mut(at) {
                Some(Stmt::Relation(relation)) => {
                    name(&mut relation.state1, &parent_id, true);
                    name(&mut relation.state2, &parent_id, false);
                    None
                }
                Some(Stmt::State(state)) => {
                    name(state, &parent_id, true);
                    state
                        .doc
                        .take()
                        .map(|mut doc| (state.id.clone(), std::mem::take(&mut doc.0)))
                }
                _ => None,
            };
            if let Some((id, doc)) = body {
                let doc = self.regions(doc);
                stack.push(Frame {
                    at: 0,
                    doc,
                    parent_id: id,
                    slot: Some(at),
                });
            }
        }
    }

    /// A body split into one region per part between dividers, or the body as it is when it has
    /// none.
    fn regions(&mut self, doc: Vec<Stmt>) -> Vec<Stmt> {
        let mut regions: Vec<Stmt> = Vec::new();
        let mut current: Vec<Stmt> = Vec::new();
        for item in doc {
            match item {
                Stmt::State(mut state)
                    if state.r#type == Some("divider") && state.doc.is_none() =>
                {
                    state.doc = Some(Doc(std::mem::take(&mut current)));
                    regions.push(Stmt::State(state));
                }
                item => current.push(item),
            }
        }
        if regions.is_empty() {
            return current;
        }
        // Mermaid names the region after the last divider with a random id. Here it continues the
        // divider numbering. Mermaid also leaves a body that ends with a divider ungrouped and
        // then fails to draw it. Here the regions before that divider are kept.
        if !current.is_empty() {
            regions.push(Stmt::State(StateStmt {
                doc: Some(Doc(current)),
                id: self.divider_id(),
                r#type: Some("divider"),
                ..StateStmt::default()
            }));
        }
        regions
    }

    /// Records a state, or merges into the one already recorded under the trimmed id.
    fn add_state(
        &mut self,
        id: &str,
        r#type: Option<&'static str>,
        doc: Option<usize>,
        descr: Option<&Description>,
        note: Option<&StateNote>,
        classes: Option<&[String]>,
    ) {
        let r#type = r#type.unwrap_or("default");
        let trimmed = trim(id);
        let at = if let Some(&at) = self.state_index.get(trimmed) {
            if let Some(slot) = self.state_docs.get_mut(at)
                && slot.is_none()
            {
                *slot = doc;
            }
            if let Some(state) = self.states.get_mut(at)
                && state.r#type.is_none_or(str::is_empty)
            {
                state.r#type = Some(r#type);
            }
            at
        } else {
            self.state_index
                .insert(trimmed.to_owned(), self.states.len());
            self.state_docs.push(doc);
            self.states.push(StateStmt {
                classes: Some(Vec::new()),
                descriptions: Some(Vec::new()),
                id: trimmed.to_owned(),
                note: note.cloned(),
                styles: Some(Vec::new()),
                text_styles: Some(Vec::new()),
                r#type: Some(r#type),
                ..StateStmt::default()
            });
            self.states.len() - 1
        };

        if let Some(descr) = descr.filter(|descr| descr.truthy()) {
            for text in descr.texts() {
                self.add_description(at, trim(text));
            }
        }
        if let (Some(note), Some(state)) = (note, self.states.get_mut(at)) {
            state.note = Some(note.clone());
        }
        if let Some(classes) = classes {
            let trimmed = trimmed.to_owned();
            for name in classes {
                self.set_css_class(&trimmed, trim(name));
            }
        }
    }

    fn add_link(&mut self, state_id: &str, url: &str, tooltip: &str) {
        let link = StateLink {
            tooltip: tooltip.to_owned(),
            url: url.to_owned(),
        };
        if let Some(&at) = self.link_index.get(state_id)
            && let Some(entry) = self.links.get_mut(at)
        {
            entry.1 = link;
            return;
        }
        self.link_index
            .insert(state_id.to_owned(), self.links.len());
        self.links.push((state_id.to_owned(), link));
    }

    fn add_relation(&mut self, item1: &StateStmt, item2: &StateStmt, title: &str) {
        // A transition's states have no body.
        for item in [item1, item2] {
            self.add_state(
                &item.id,
                item.r#type,
                None,
                item.description.as_ref(),
                item.note.as_ref(),
                item.classes.as_deref(),
            );
        }
        self.relations.push(StateRelation {
            id1: trim(&item1.id).to_owned(),
            id2: trim(&item2.id).to_owned(),
            relation_title: title.to_owned(),
        });
    }

    fn add_description(&mut self, at: usize, descr: &str) {
        let text = match descr.strip_prefix(':') {
            Some(rest) => trim(rest).to_owned(),
            None => descr.to_owned(),
        };
        if let Some(descriptions) = self
            .states
            .get_mut(at)
            .and_then(|state| state.descriptions.as_mut())
        {
            descriptions.push(text);
        }
    }

    /// The next divider id, as Pele's `getDividerId` numbers them.
    pub(crate) fn divider_id(&mut self) -> String {
        self.divider_count += 1;
        let mut id = String::from("divider-id-");
        push_u32(&mut id, self.divider_count);
        id
    }

    fn add_style_class(&mut self, id: &str, style_attributes: &str) {
        let at = if let Some(&at) = self.class_index.get(id) {
            at
        } else {
            self.class_index.insert(id.to_owned(), self.classes.len());
            self.classes.push(StateClassDef {
                id: id.to_owned(),
                styles: Vec::new(),
                text_styles: Vec::new(),
            });
            self.classes.len() - 1
        };
        if style_attributes.is_empty() {
            return;
        }
        let Some(found) = self.classes.get_mut(at) else {
            return;
        };
        for attrib in style_attributes.split(',') {
            let fixed = trim(&replace_first(attrib, ";", "")).to_owned();
            if attrib.contains("color") {
                found.text_styles.push(replace_first(
                    &replace_first(&fixed, "fill", "bgFill"),
                    "color",
                    "fill",
                ));
            }
            found.styles.push(fixed);
        }
    }

    fn set_css_class(&mut self, item_ids: &str, css_class_name: &str) {
        for at in self.named(item_ids) {
            if let Some(classes) = self
                .states
                .get_mut(at)
                .and_then(|state| state.classes.as_mut())
            {
                classes.push(css_class_name.to_owned());
            }
        }
    }

    /// The states a comma-separated list of ids names, created where they do not exist yet. As in
    /// Mermaid, an id is looked up as written and trimmed only when it has to be created.
    fn named(&mut self, ids: &str) -> Vec<usize> {
        let mut out = Vec::new();
        for id in ids.split(',') {
            if let Some(&at) = self.state_index.get(id) {
                out.push(at);
                continue;
            }
            self.add_state(id, None, None, None, None, None);
            if let Some(&at) = self.state_index.get(trim(id)) {
                out.push(at);
            }
        }
        out
    }
}
