//! The ER model and the operations the parser applies to it. Port of Pele's
//! `src/diagrams/er/db.ts`.

use std::collections::{BTreeMap, BTreeSet};

use super::types::{
    DocItem, ErAttribute, ErClassDef, ErEntity, ErRelationship, ErSubgraph, RelSpec,
};
use crate::text::label::{trim, trim_start};
use crate::util::chars::is_space;

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

    /// The record for `key`, to change.
    fn get_mut(&mut self, key: &str) -> Option<&mut V> {
        self.index.get(key).and_then(|&at| self.values.get_mut(at))
    }

    /// Appends a record under `key` and returns its position.
    fn insert(&mut self, key: String, value: V) -> usize {
        let at = self.values.len();
        self.index.insert(key, at);
        self.values.push(value);
        at
    }
}

/// The parsed ER diagram: Pele's `ErDb`.
#[derive(Debug)]
pub(crate) struct ErDb {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    /// Names already placed in a subgraph.
    claimed: BTreeSet<String>,
    /// Class definitions by name.
    pub(crate) classes: Ordered<ErClassDef>,
    /// The flow direction: `TB`, `BT`, `LR`, or `RL`.
    pub(crate) direction: &'static str,
    /// Entities by name.
    pub(crate) entities: Ordered<ErEntity>,
    /// Relationships in authored order.
    pub(crate) relationships: Vec<ErRelationship>,
    /// The latest subgraph of each id.
    subgraph_lookup: BTreeMap<String, usize>,
    /// Subgraphs in the order they close.
    pub(crate) subgraphs: Vec<ErSubgraph>,
    /// The front matter title.
    pub(crate) title: Option<String>,
}

impl Default for ErDb {
    fn default() -> Self {
        Self {
            acc_descr: None,
            acc_title: None,
            claimed: BTreeSet::new(),
            classes: Ordered::default(),
            direction: "TB",
            entities: Ordered::default(),
            relationships: Vec::new(),
            subgraph_lookup: BTreeMap::new(),
            subgraphs: Vec::new(),
            title: None,
        }
    }
}

impl ErDb {
    /// Adds the entity `name`, unless it exists, and gives it `alias` if it has none. Returns its
    /// position.
    pub(crate) fn add_entity(&mut self, name: &str, alias: &str) -> usize {
        if let Some(&at) = self.entities.index.get(name) {
            if let Some(entity) = self.entities.values.get_mut(at)
                && entity.alias.is_empty()
                && !alias.is_empty()
            {
                alias.clone_into(&mut entity.alias);
            }
            return at;
        }
        let mut id = String::from("entity-");
        id.push_str(name);
        id.push('-');
        crate::error::push_u32(
            &mut id,
            u32::try_from(self.entities.values.len()).unwrap_or(u32::MAX),
        );
        self.entities.insert(
            name.to_owned(),
            ErEntity {
                alias: alias.to_owned(),
                attributes: Vec::new(),
                css_classes: String::from("default"),
                css_styles: Vec::new(),
                id,
                label: name.to_owned(),
            },
        )
    }

    /// Adds attributes to an entity, in order.
    pub(crate) fn add_attributes(&mut self, name: &str, attributes: Vec<ErAttribute>) {
        let at = self.add_entity(name, "");
        if let Some(entity) = self.entities.values.get_mut(at) {
            entity.attributes.extend(attributes);
        }
    }

    /// The id of an end of a relationship: the subgraph's when the name is one, else the
    /// entity's.
    fn end_id(&mut self, name: &str) -> String {
        if self.subgraph_lookup.contains_key(name) {
            return name.to_owned();
        }
        let at = self.add_entity(name, "");
        self.entities
            .values
            .get(at)
            .map(|entity| entity.id.clone())
            .unwrap_or_default()
    }

    /// Adds a relationship.
    pub(crate) fn add_relationship(
        &mut self,
        ent_a: &str,
        role_a: &str,
        ent_b: &str,
        rel_spec: RelSpec,
    ) {
        let entity_a = self.end_id(ent_a);
        let entity_b = self.end_id(ent_b);
        self.relationships.push(ErRelationship {
            entity_a,
            entity_b,
            rel_spec,
            role_a: role_a.to_owned(),
        });
    }

    /// Sets the flow direction.
    pub(crate) fn set_direction(&mut self, dir: &'static str) {
        self.direction = dir;
    }

    /// Adds style declarations to the entities and subgraphs `ids` names.
    pub(crate) fn add_css_styles(&mut self, ids: &[&str], styles: &[String]) {
        for &id in ids {
            if let Some(entity) = self.entities.get_mut(id) {
                entity.css_styles.extend(styles.iter().cloned());
            }
            if let Some(subgraph) = self
                .subgraph_lookup
                .get(id)
                .and_then(|&at| self.subgraphs.get_mut(at))
            {
                subgraph.css_styles.extend(styles.iter().cloned());
            }
        }
    }

    /// Defines classes.
    pub(crate) fn add_class(&mut self, ids: &[&str], styles: &[String]) {
        for &id in ids {
            let at = match self.classes.index.get(id) {
                Some(&at) => at,
                None => self.classes.insert(
                    id.to_owned(),
                    ErClassDef {
                        id: id.to_owned(),
                        ..ErClassDef::default()
                    },
                ),
            };
            let Some(def) = self.classes.values.get_mut(at) else {
                continue;
            };
            for style in styles {
                if style.contains("color") {
                    def.text_styles.push(replace_first(style, "fill", "bgFill"));
                }
                def.styles.push(style.clone());
            }
        }
    }

    /// Adds class names to the entities and subgraphs `ids` names.
    pub(crate) fn set_class(&mut self, ids: &[&str], class_names: &[&str]) {
        for &id in ids {
            if let Some(entity) = self.entities.get_mut(id) {
                for name in class_names {
                    entity.css_classes.push(' ');
                    entity.css_classes.push_str(name);
                }
            }
            if let Some(subgraph) = self
                .subgraph_lookup
                .get(id)
                .and_then(|&at| self.subgraphs.get_mut(at))
            {
                subgraph
                    .classes
                    .extend(class_names.iter().map(|&name| name.to_owned()));
            }
        }
    }

    /// Closes a subgraph holding the statements `list`, and returns its id. A name belongs to
    /// the first subgraph that lists it.
    pub(crate) fn add_sub_graph(&mut self, id: &str, list: &[DocItem], title: &str) -> String {
        let id = trim(id).to_owned();
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut nodes: Vec<String> = Vec::new();
        let mut dir = None;
        for item in list {
            match item {
                DocItem::Dir(value) => dir = Some(*value),
                DocItem::Name(item) => {
                    let trimmed = trim(item);
                    if trimmed.is_empty() || !seen.insert(trimmed) {
                        continue;
                    }
                    if !self.claimed.contains(item) {
                        nodes.push(item.clone());
                    }
                }
            }
        }
        for node in &nodes {
            self.claimed.insert(node.clone());
        }
        self.subgraph_lookup
            .insert(id.clone(), self.subgraphs.len());
        self.subgraphs.push(ErSubgraph {
            classes: Vec::new(),
            css_styles: Vec::new(),
            dir,
            id: id.clone(),
            nodes,
            title: trim(title).to_owned(),
        });
        id
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
