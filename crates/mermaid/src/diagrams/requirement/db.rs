//! The requirement model and the operations the parser applies to it. Port of Pele's
//! `src/diagrams/requirement/db.ts`.

use std::collections::BTreeMap;

use super::types::{Relation, Requirement, RequirementClass, RequirementElement};
use crate::error::{Code, Error};
use crate::text::label::trim_start;
use crate::util::chars::is_space;

/// What `contains` relationships are called.
pub(crate) const CONTAINS: &str = "contains";

/// Applying a class copies its declarations onto each node, so a short text can ask for hundreds
/// of millions of copies. Past this many the diagram is refused.
const MAX_STYLE_COPIES: usize = 1_000_000;

/// Records in insertion order with an index by key, as Pele's `Map` keeps them.
#[derive(Debug)]
pub(crate) struct Ordered<V> {
    /// Positions by key. Only looked up, never iterated.
    index: BTreeMap<String, usize>,
    /// The records in insertion order.
    pub(crate) values: Vec<V>,
}

impl<V> Default for Ordered<V> {
    fn default() -> Self {
        Self {
            index: BTreeMap::new(),
            values: Vec::new(),
        }
    }
}

impl<V> Ordered<V> {
    /// The record for `key`.
    pub(crate) fn get(&self, key: &str) -> Option<&V> {
        self.index.get(key).and_then(|&at| self.values.get(at))
    }

    /// Whether a record has `key`.
    pub(crate) fn has(&self, key: &str) -> bool {
        self.index.contains_key(key)
    }

    fn position(&self, key: &str) -> Option<usize> {
        self.index.get(key).copied()
    }

    fn insert(&mut self, key: String, value: V) -> usize {
        let at = self.values.len();
        self.index.insert(key, at);
        self.values.push(value);
        at
    }
}

/// Which node a name resolves to: requirements take precedence over elements.
#[derive(Clone, Copy)]
enum NodeRef {
    Element(usize),
    Requirement(usize),
}

/// The parsed requirement diagram: Pele's `RequirementDb`.
#[derive(Debug)]
pub(crate) struct RequirementDb {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    /// Classes by name.
    pub(crate) classes: Ordered<RequirementClass>,
    /// The flow direction: `TB`, `BT`, `LR`, or `RL`.
    pub(crate) direction: &'static str,
    /// Elements by name.
    pub(crate) elements: Ordered<RequirementElement>,
    // The grammar reports a body's fields before the requirement or element they belong to.
    new_doc_ref: String,
    new_element_type: String,
    new_id: String,
    new_risk: String,
    new_text: String,
    new_verify_method: String,
    /// Relationships in authored order.
    pub(crate) relations: Vec<Relation>,
    /// Requirements by name.
    pub(crate) requirements: Ordered<Requirement>,
    style_copies: usize,
    /// The front matter title.
    pub(crate) title: Option<String>,
}

impl Default for RequirementDb {
    fn default() -> Self {
        Self {
            acc_descr: None,
            acc_title: None,
            classes: Ordered::default(),
            direction: "TB",
            elements: Ordered::default(),
            new_doc_ref: String::new(),
            new_element_type: String::new(),
            new_id: String::new(),
            new_risk: String::new(),
            new_text: String::new(),
            new_verify_method: String::new(),
            relations: Vec::new(),
            requirements: Ordered::default(),
            style_copies: 0,
            title: None,
        }
    }
}

#[cold]
fn too_many_copies() -> Error {
    let mut error = Error::new(
        Code::Limit,
        "Requirement diagram applies too many style declarations.",
    );
    error.r#type = Some("requirement");
    error
}

impl RequirementDb {
    /// Counts `count` more copied declarations against the limit.
    fn count(&mut self, count: usize) -> Result<(), Error> {
        self.style_copies += count;
        if self.style_copies > MAX_STYLE_COPIES {
            return Err(too_many_copies());
        }
        Ok(())
    }

    /// Sets the flow direction.
    pub(crate) fn set_direction(&mut self, dir: &'static str) {
        self.direction = dir;
    }

    /// Adds a requirement with the pending fields, unless one has this name.
    pub(crate) fn add_requirement(&mut self, name: &str, r#type: &str) {
        if !self.requirements.has(name) {
            let requirement = Requirement {
                name: name.to_owned(),
                r#type: r#type.to_owned(),
                requirement_id: std::mem::take(&mut self.new_id),
                text: std::mem::take(&mut self.new_text),
                risk: std::mem::take(&mut self.new_risk),
                verify_method: std::mem::take(&mut self.new_verify_method),
                css_styles: Vec::new(),
                classes: vec!["default".to_owned()],
            };
            self.requirements.insert(name.to_owned(), requirement);
        }
        self.new_id.clear();
        self.new_text.clear();
        self.new_risk.clear();
        self.new_verify_method.clear();
    }

    /// Sets the pending `id` field.
    pub(crate) fn set_new_req_id(&mut self, id: &str) {
        id.clone_into(&mut self.new_id);
    }

    /// Sets the pending `text` field.
    pub(crate) fn set_new_req_text(&mut self, text: &str) {
        text.clone_into(&mut self.new_text);
    }

    /// Sets the pending `risk` field.
    pub(crate) fn set_new_req_risk(&mut self, risk: &str) {
        risk.clone_into(&mut self.new_risk);
    }

    /// Sets the pending `verifymethod` field.
    pub(crate) fn set_new_req_verify_method(&mut self, method: &str) {
        method.clone_into(&mut self.new_verify_method);
    }

    /// Adds an element with the pending fields, unless one has this name.
    pub(crate) fn add_element(&mut self, name: &str) {
        if !self.elements.has(name) {
            let element = RequirementElement {
                name: name.to_owned(),
                r#type: std::mem::take(&mut self.new_element_type),
                doc_ref: std::mem::take(&mut self.new_doc_ref),
                css_styles: Vec::new(),
                classes: vec!["default".to_owned()],
            };
            self.elements.insert(name.to_owned(), element);
        }
        self.new_element_type.clear();
        self.new_doc_ref.clear();
    }

    /// Sets the pending `type` field.
    pub(crate) fn set_new_element_type(&mut self, r#type: &str) {
        r#type.clone_into(&mut self.new_element_type);
    }

    /// Sets the pending `docref` field.
    pub(crate) fn set_new_element_doc_ref(&mut self, doc_ref: &str) {
        doc_ref.clone_into(&mut self.new_doc_ref);
    }

    /// Adds a relationship.
    pub(crate) fn add_relationship(&mut self, r#type: &str, src: &str, dst: &str) {
        self.relations.push(Relation {
            r#type: r#type.to_owned(),
            src: src.to_owned(),
            dst: dst.to_owned(),
        });
    }

    fn node(&self, id: &str) -> Option<NodeRef> {
        self.requirements
            .position(id)
            .map(NodeRef::Requirement)
            .or_else(|| self.elements.position(id).map(NodeRef::Element))
    }

    fn node_mut(&mut self, node: NodeRef) -> Option<(&mut Vec<String>, &mut Vec<String>)> {
        match node {
            NodeRef::Requirement(at) => self
                .requirements
                .values
                .get_mut(at)
                .map(|r| (&mut r.css_styles, &mut r.classes)),
            NodeRef::Element(at) => self
                .elements
                .values
                .get_mut(at)
                .map(|e| (&mut e.css_styles, &mut e.classes)),
        }
    }

    /// Adds style declarations to the named nodes.
    ///
    /// # Errors
    ///
    /// Returns a limit error past the most copied declarations.
    pub(crate) fn set_css_style(&mut self, ids: &[&str], styles: &[String]) -> Result<(), Error> {
        for id in ids {
            // Mermaid stops at the first unknown id, leaving the rest of the list unstyled.
            let Some(node) = self.node(id) else {
                return Ok(());
            };
            for style in styles {
                let parts: Vec<&str> = style.split(',').collect();
                self.count(parts.len())?;
                if let Some((css, _)) = self.node_mut(node) {
                    css.extend(parts.into_iter().map(str::to_owned));
                }
            }
        }
        Ok(())
    }

    /// Adds classes to the named nodes, with the declarations they define so far.
    ///
    /// # Errors
    ///
    /// Returns a limit error past the most copied declarations.
    pub(crate) fn set_class(&mut self, ids: &[&str], class_names: &[&str]) -> Result<(), Error> {
        for id in ids {
            let Some(node) = self.node(id) else {
                continue;
            };
            for name in class_names {
                let styles = self
                    .classes
                    .get(name)
                    .map(|class| class.styles.clone())
                    .unwrap_or_default();
                if let Some((_, classes)) = self.node_mut(node) {
                    classes.push((*name).to_owned());
                }
                self.count(styles.len())?;
                if let Some((css, _)) = self.node_mut(node) {
                    css.extend(styles);
                }
            }
        }
        Ok(())
    }

    /// Defines classes, and adds their declarations to the nodes that already have them.
    ///
    /// # Errors
    ///
    /// Returns a limit error past the most copied declarations.
    pub(crate) fn define_class(&mut self, ids: &[&str], style: &[String]) -> Result<(), Error> {
        let split: Vec<String> = style
            .iter()
            .flat_map(|s| s.split(','))
            .map(str::to_owned)
            .collect();
        for &id in ids {
            let at = match self.classes.position(id) {
                Some(at) => at,
                None => self.classes.insert(
                    id.to_owned(),
                    RequirementClass {
                        id: id.to_owned(),
                        ..RequirementClass::default()
                    },
                ),
            };
            if let Some(class) = self.classes.values.get_mut(at) {
                for s in style {
                    if s.contains("color") {
                        class.text_styles.push(replace_first(s, "fill", "bgFill"));
                    }
                    class.styles.push(s.clone());
                }
            }

            let mut targets = Vec::new();
            for (at, requirement) in self.requirements.values.iter().enumerate() {
                if requirement.classes.iter().any(|class| class == id) {
                    targets.push(NodeRef::Requirement(at));
                }
            }
            for (at, element) in self.elements.values.iter().enumerate() {
                if element.classes.iter().any(|class| class == id) {
                    targets.push(NodeRef::Element(at));
                }
            }
            for node in targets {
                self.count(split.len())?;
                if let Some((css, _)) = self.node_mut(node) {
                    css.extend(split.iter().cloned());
                }
            }
        }
        Ok(())
    }

    /// Sets the accessible title without its leading whitespace.
    pub(crate) fn set_acc_title(&mut self, text: &str) {
        self.acc_title = Some(trim_start(text).to_owned());
    }

    /// Sets the accessible description, without the indentation of its continuation lines.
    pub(crate) fn set_acc_description(&mut self, text: &str) {
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            out.push(c);
            if c == '\n' {
                while chars.next_if(|&c| is_space(u32::from(c))).is_some() {}
            }
        }
        self.acc_descr = Some(out);
    }
}

/// `text.replace(from, to)` with a string pattern: the first occurrence only.
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
