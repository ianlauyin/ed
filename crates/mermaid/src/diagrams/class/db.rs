//! The class model and the operations the parser applies to it. Port of Pele's
//! `src/diagrams/class/db.ts`.

use std::collections::{BTreeMap, BTreeSet};

use super::members::{ClassMember, MemberType};
use super::types::{
    ClassInterface, ClassNode, ClassNote, ClassRelation, LOLLIPOP, NamespaceNode, RelationEnd,
    StyleClass,
};
use crate::text::entities;
use crate::text::label::{trim, trim_start};
use crate::util::chars::is_space;
use crate::util::url::sanitize_url;

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
    pub(crate) fn get_mut(&mut self, key: &str) -> Option<&mut V> {
        self.index.get(key).and_then(|&at| self.values.get_mut(at))
    }

    /// The position of the record for `key`.
    pub(crate) fn position(&self, key: &str) -> Option<usize> {
        self.index.get(key).copied()
    }

    /// The number of records.
    pub(crate) fn len(&self) -> usize {
        self.values.len()
    }

    /// Appends a record under `key` and returns its position.
    fn insert(&mut self, key: String, value: V) -> usize {
        let at = self.values.len();
        self.index.insert(key, at);
        self.values.push(value);
        at
    }
}

/// Mermaid looks up ids that start with a digit under this prefix, so they never match a class.
const DOM_ID_PREFIX: &str = "classId-";

/// Splits `Name~T~` into the class name and its generic type.
pub(crate) fn split_name(id: &str) -> (&str, &str) {
    match id.find('~') {
        Some(at) if at > 0 => {
            let rest = id.get(at + 1..).unwrap_or("");
            let r#type = rest
                .find('~')
                .map_or(rest, |end| rest.get(..end).unwrap_or(""));
            (id.get(..at).unwrap_or(""), r#type)
        }
        _ => (id, ""),
    }
}

/// The class name Mermaid looks an id up under.
fn lookup_id(id: &str) -> String {
    if id.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        let mut prefixed = String::from(DOM_ID_PREFIX);
        prefixed.push_str(id);
        split_name(&prefixed).0.to_owned()
    } else {
        split_name(id).0.to_owned()
    }
}

/// The parsed class diagram: Pele's `ClassDb`.
#[derive(Debug)]
pub(crate) struct ClassDb {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    /// Classes by name.
    pub(crate) classes: Ordered<ClassNode>,
    /// The flow direction: `TB`, `BT`, `LR`, or `RL`.
    pub(crate) direction: &'static str,
    /// Lollipop interfaces in order.
    pub(crate) interfaces: Vec<ClassInterface>,
    /// The namespaces open while parsing, innermost last.
    namespace_stack: Vec<String>,
    /// Namespaces by full name.
    pub(crate) namespaces: Ordered<NamespaceNode>,
    /// Notes by id.
    pub(crate) notes: Ordered<ClassNote>,
    /// Relations in authored order.
    pub(crate) relations: Vec<ClassRelation>,
    /// Class definitions by name.
    pub(crate) style_classes: Ordered<StyleClass>,
    /// The front matter title.
    pub(crate) title: Option<String>,
    /// The classes tagged with each class name, in the order they were tagged.
    with_css_class: BTreeMap<String, Vec<usize>>,
    /// Which class name and class pairs are in `with_css_class`.
    tagged: BTreeSet<(String, usize)>,
}

impl Default for ClassDb {
    fn default() -> Self {
        Self {
            acc_descr: None,
            acc_title: None,
            classes: Ordered::default(),
            direction: "TB",
            interfaces: Vec::new(),
            namespace_stack: Vec::new(),
            namespaces: Ordered::default(),
            notes: Ordered::default(),
            relations: Vec::new(),
            style_classes: Ordered::default(),
            title: None,
            with_css_class: BTreeMap::new(),
            tagged: BTreeSet::new(),
        }
    }
}

impl ClassDb {
    /// Adds the class `id` names, unless it exists.
    pub(crate) fn add_class(&mut self, id: &str) {
        let (class_name, r#type) = split_name(id);
        if self.classes.position(class_name).is_some() {
            return;
        }
        let at = self.classes.insert(
            class_name.to_owned(),
            ClassNode {
                annotations: Vec::new(),
                css_classes: String::from("default"),
                have_callback: false,
                id: class_name.to_owned(),
                label: class_name.to_owned(),
                link: None,
                link_target: None,
                members: Vec::new(),
                methods: Vec::new(),
                parent: None,
                styles: Vec::new(),
                tooltip: None,
                r#type: r#type.to_owned(),
            },
        );
        self.tag(at, "default");
    }

    /// Records that a class carries `css_class`, once.
    fn tag(&mut self, node: usize, css_class: &str) {
        if self.tagged.insert((css_class.to_owned(), node)) {
            self.with_css_class
                .entry(css_class.to_owned())
                .or_default()
                .push(node);
        }
    }

    /// The class for a name that may carry a generic type, created if needed. Mermaid throws a
    /// `TypeError` where this creates, as for an annotation on a class not yet declared.
    fn ensure(&mut self, id: &str) -> usize {
        self.add_class(id);
        self.classes.position(split_name(id).0).unwrap_or(0)
    }

    /// Sets the shown name of a class.
    pub(crate) fn set_class_label(&mut self, id: &str, label: &str) {
        let at = self.ensure(id);
        if let Some(node) = self.classes.values.get_mut(at) {
            label.clone_into(&mut node.label);
        }
    }

    /// Adds a relation, creating its classes, and an interface for a lone lollipop end.
    pub(crate) fn add_relation(&mut self, mut relation: ClassRelation) {
        if relation.type1 == LOLLIPOP && relation.type2 == RelationEnd::None {
            self.add_class(&relation.id2);
            relation.id1 = self.add_interface(&relation.id1, &relation.id2);
        } else if relation.type2 == LOLLIPOP && relation.type1 == RelationEnd::None {
            self.add_class(&relation.id1);
            relation.id2 = self.add_interface(&relation.id2, &relation.id1);
        } else {
            self.add_class(&relation.id1);
            self.add_class(&relation.id2);
        }
        relation.id1 = split_name(&relation.id1).0.into();
        relation.id2 = split_name(&relation.id2).0.into();
        relation.relation_title1 = trim(&relation.relation_title1).into();
        relation.relation_title2 = trim(&relation.relation_title2).into();
        self.relations.push(relation);
    }

    /// Adds the interface a lone lollipop end stands for and returns its id.
    fn add_interface(&mut self, label: &str, class_id: &str) -> String {
        let mut id = String::from("interface");
        crate::error::push_u32(
            &mut id,
            u32::try_from(self.interfaces.len()).unwrap_or(u32::MAX),
        );
        self.interfaces.push(ClassInterface {
            class_id: class_id.to_owned(),
            id: id.clone(),
            label: label.to_owned(),
        });
        id
    }

    /// Adds an annotation to a class.
    pub(crate) fn add_annotation(&mut self, class_name: &str, annotation: &str) {
        let at = self.ensure(class_name);
        if let Some(node) = self.classes.values.get_mut(at) {
            node.annotations.push(annotation.to_owned());
        }
    }

    /// Adds a member line to a class: an annotation, a method, or an attribute.
    pub(crate) fn add_member(&mut self, class_name: &str, member: &str) {
        let at = self.ensure(class_name);
        let Some(node) = self.classes.values.get_mut(at) else {
            return;
        };
        let text = trim(member);
        if text.starts_with("<<") && text.ends_with(">>") {
            node.annotations
                .push(text.get(2..text.len() - 2).unwrap_or("").to_owned());
        } else if text.find(')').is_some_and(|at| at > 0) {
            let parsed = ClassMember::new(text, MemberType::Method);
            if parsed.member_type == MemberType::Method {
                node.methods.push(parsed);
            } else {
                node.members.push(parsed);
            }
        } else if !text.is_empty() {
            node.members
                .push(ClassMember::new(text, MemberType::Attribute));
        }
    }

    /// Adds member lines to a class, in order.
    pub(crate) fn add_members(&mut self, class_name: &str, members: &[&str]) {
        for member in members {
            self.add_member(class_name, member);
        }
    }

    /// Adds a note, for a class when `class_name` is given, and returns its id.
    pub(crate) fn add_note(&mut self, text: &str, class_name: Option<&str>) -> String {
        let index = self.notes.len();
        let mut id = String::from("note");
        crate::error::push_u32(&mut id, u32::try_from(index).unwrap_or(u32::MAX));
        self.notes.insert(
            id.clone(),
            ClassNote {
                class: class_name.map(str::to_owned),
                id: id.clone(),
                index,
                parent: None,
                text: text.to_owned(),
            },
        );
        id
    }

    /// A label without a leading colon or surrounding whitespace.
    pub(crate) fn cleanup_label(label: &str) -> &str {
        trim(label.strip_prefix(':').unwrap_or(label))
    }

    /// Adds `class_name` to the classes `ids` names, separated by commas.
    pub(crate) fn set_css_class(&mut self, ids: &str, class_name: &str) {
        for id in ids.split(',') {
            let Some(at) = self.classes.position(&lookup_id(id)) else {
                continue;
            };
            if let Some(node) = self.classes.values.get_mut(at) {
                node.css_classes.push(' ');
                node.css_classes.push_str(class_name);
            }
            for word in class_name.split(' ') {
                self.tag(at, word);
            }
        }
    }

    /// Defines classes, and copies their declarations to the classes already tagged with them.
    pub(crate) fn define_class(&mut self, ids: &[&str], style: &[String]) {
        for &id in ids {
            let at = match self.style_classes.position(id) {
                Some(at) => at,
                None => self.style_classes.insert(
                    id.to_owned(),
                    StyleClass {
                        id: id.to_owned(),
                        ..StyleClass::default()
                    },
                ),
            };
            if let Some(def) = self.style_classes.values.get_mut(at) {
                for s in style {
                    if s.contains("color") {
                        def.text_styles.push(replace_first(s, "fill", "bgFill"));
                    }
                    def.styles.push(s.clone());
                }
            }
            // Mermaid tests `cssClasses.includes(id)`, which also matches part of a longer name
            // (a classDef named `def` styles every class, through `default`). Whole names are
            // compared here.
            let Some(tagged) = self.with_css_class.get(id) else {
                continue;
            };
            for &node in tagged {
                let Some(node) = self.classes.values.get_mut(node) else {
                    continue;
                };
                for s in style {
                    for part in s.split(',') {
                        node.styles.push(part.to_owned());
                    }
                }
            }
        }
    }

    /// Sets the tooltip of the classes `ids` names.
    pub(crate) fn set_tooltip(&mut self, ids: &str, tooltip: Option<&str>) {
        let Some(tooltip) = tooltip else {
            return;
        };
        for id in ids.split(',') {
            if let Some(node) = self.classes.get_mut(split_name(id).0) {
                node.tooltip = Some(tooltip.to_owned());
            }
        }
    }

    /// Links the classes `ids` names, and marks them clickable.
    pub(crate) fn set_link(&mut self, ids: &str, link: &str, target: Option<&str>) {
        // Entity codes are resolved first, so that what is checked is what a browser would follow.
        let decoded = entities::decode(link);
        let url = trim(&decoded);
        for id in ids.split(',') {
            if let Some(node) = self.classes.get_mut(&lookup_id(id)) {
                node.link = (!url.is_empty()).then(|| sanitize_url(url));
                node.link_target = Some(target.unwrap_or("_blank").to_owned());
            }
        }
        self.set_css_class(ids, "clickable");
    }

    /// Marks the classes `ids` names as having a callback, and clickable. Callbacks are never
    /// executed, as in Mermaid's strict mode.
    pub(crate) fn set_click_event(&mut self, ids: &str) {
        for id in ids.split(',') {
            if let Some(node) = self.classes.get_mut(split_name(id).0) {
                node.have_callback = true;
            }
        }
        self.set_css_class(ids, "clickable");
    }

    /// Sets the flow direction.
    pub(crate) fn set_direction(&mut self, dir: &'static str) {
        self.direction = dir;
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

    /// Opens a namespace inside the open one and returns its full name. A dotted name creates
    /// its ancestors.
    pub(crate) fn add_namespace(&mut self, id: &str, label: Option<&str>) -> String {
        let prefix = self.namespace_stack.last().cloned().unwrap_or_default();
        let qualified = if prefix.is_empty() {
            id.to_owned()
        } else {
            let mut qualified = prefix.clone();
            qualified.push('.');
            qualified.push_str(id);
            qualified
        };
        self.namespace_stack.push(qualified.clone());
        let label = label.filter(|label| !label.is_empty());

        if let Some(existing) = self.namespaces.get_mut(&qualified) {
            existing.explicit = true;
            if let Some(label) = label {
                label.clone_into(&mut existing.label);
            }
            return qualified;
        }

        // The enclosing namespace and its ancestors exist already, so only the parts of `id` are
        // walked.
        let mut parent_id = (!prefix.is_empty()).then(|| prefix.clone());
        let mut from = if prefix.is_empty() {
            0
        } else {
            prefix.len() + 1
        };
        loop {
            let dot = qualified
                .get(from..)
                .and_then(|rest| rest.find('.'))
                .map(|at| from + at);
            let leaf = dot.is_none();
            let end = dot.unwrap_or(qualified.len());
            let current = qualified.get(..end).unwrap_or("").to_owned();
            match self.namespaces.get_mut(&current) {
                None => {
                    let part = qualified.get(from..end).unwrap_or("");
                    let node_label = match label {
                        Some(label) if leaf => label,
                        _ => part,
                    };
                    self.namespaces.insert(
                        current.clone(),
                        NamespaceNode {
                            children: Vec::new(),
                            classes: Vec::new(),
                            explicit: leaf,
                            id: current.clone(),
                            label: node_label.to_owned(),
                            notes: Vec::new(),
                            parent: parent_id.clone(),
                        },
                    );
                }
                Some(node) if leaf => node.explicit = true,
                Some(_) => {}
            }
            if let Some(parent_key) = parent_id.as_deref()
                && let Some(parent) = self.namespaces.get_mut(parent_key)
            {
                if !parent.children.contains(&current) {
                    parent.children.push(current.clone());
                }
                if let Some(node) = self.namespaces.get_mut(&current)
                    && node.parent.is_none()
                {
                    node.parent.clone_from(&parent_id);
                }
            }
            if leaf {
                return qualified;
            }
            parent_id = Some(current);
            from = end + 1;
        }
    }

    /// Closes the innermost open namespace.
    pub(crate) fn pop_namespace(&mut self) {
        self.namespace_stack.pop();
    }

    /// Puts classes and notes in the namespace `id`.
    pub(crate) fn add_classes_to_namespace(
        &mut self,
        id: &str,
        class_names: &[String],
        note_names: &[String],
    ) {
        let Some(at) = self.namespaces.position(id) else {
            return;
        };
        for name in class_names {
            let class_name = split_name(name).0;
            let Some(node) = self.classes.get_mut(class_name) else {
                continue;
            };
            node.parent = Some(id.to_owned());
            if let Some(namespace) = self.namespaces.values.get_mut(at)
                && !namespace.classes.iter().any(|c| c == class_name)
            {
                namespace.classes.push(class_name.to_owned());
            }
        }
        for name in note_names {
            let Some(note) = self.notes.get_mut(name) else {
                continue;
            };
            note.parent = Some(id.to_owned());
            if let Some(namespace) = self.namespaces.values.get_mut(at)
                && !namespace.notes.contains(name)
            {
                namespace.notes.push(name.clone());
            }
        }
    }

    /// Adds style declarations to the class `id` names exactly.
    pub(crate) fn set_css_style(&mut self, id: &str, styles: &[String]) {
        let Some(node) = self.classes.get_mut(id) else {
            return;
        };
        for s in styles {
            for part in s.split(',') {
                node.styles.push(part.to_owned());
            }
        }
    }
}

/// `text.replace(from, to)` with a string pattern: the first occurrence only.
pub(crate) fn replace_first(text: &str, from: &str, to: &str) -> String {
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
