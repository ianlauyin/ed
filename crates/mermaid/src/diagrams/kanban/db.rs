//! The kanban database. Port of Pele's `src/diagrams/kanban/db.ts`.

use crate::diagrams::common::outline::{Decoration, OutlineDb};
use crate::error::{Code, Error};
use crate::yaml::{self, Value};

/// A card, or the head of a column.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct KanbanNode {
    /// The assignee from metadata.
    pub(crate) assigned: Option<String>,
    /// The class names from `:::`.
    pub(crate) css_classes: Option<String>,
    /// The icon name from metadata or `::icon(...)`.
    pub(crate) icon: Option<String>,
    /// The authored id.
    pub(crate) id: String,
    /// The text, or the metadata's label.
    pub(crate) label: String,
    /// The indentation, in UTF-16 units.
    pub(crate) level: usize,
    /// The priority from metadata.
    pub(crate) priority: Option<String>,
    /// The ticket from metadata.
    pub(crate) ticket: Option<String>,
}

/// A column: its head and its cards.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct KanbanSection {
    /// The cards, in source order.
    pub(crate) items: Vec<KanbanNode>,
    /// The column's own fields.
    pub(crate) node: KanbanNode,
}

/// Which node the last statement added, for decorations.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Last {
    /// The head of the last column.
    Section,
    /// The last card of the last column.
    Item,
}

/// The parsed kanban board: Pele's `KanbanDb`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct KanbanDb {
    /// The first error an action raised. Pele throws it, and the parser stops there.
    pub(crate) error: Option<Error>,
    last: Option<Last>,
    /// Whether a card was indented less than the first column.
    misplaced: Option<String>,
    section_level: usize,
    /// The columns, in source order.
    pub(crate) sections: Vec<KanbanSection>,
    /// The front matter title.
    pub(crate) title: Option<String>,
}

/// An error of a kanban board.
#[cold]
fn failure(code: Code, message: String) -> Error {
    let mut error = Error::new(code, message);
    error.r#type = Some("kanban");
    error
}

/// A property of a YAML mapping, as JavaScript's `value.key` reads it.
fn property<'v>(value: &'v Value, key: &str) -> Option<&'v Value> {
    let Value::Object(entries) = value else {
        return None;
    };
    entries
        .iter()
        .find(|(own, _)| own == key)
        .map(|(_, value)| value)
}

/// `String(value)` of a truthy property, as `if (doc.key) node.key = String(doc.key)` sets it.
fn truthy_string(doc: &Value, key: &str) -> Option<String> {
    let value = property(doc, key).filter(|value| value.truthy())?;
    let mut out = String::new();
    value.push_js_string(&mut out);
    Some(out)
}

/// Whether JavaScript's `toLowerCase` would change `text`.
fn has_upper(text: &str) -> bool {
    text.chars().any(|c| {
        let mut lower = c.to_lowercase();
        lower.next() != Some(c) || lower.next().is_some()
    })
}

impl KanbanDb {
    fn add(
        &mut self,
        level: usize,
        id: &str,
        descr: &str,
        shape_data: Option<&str>,
    ) -> Result<(), Error> {
        let mut node = KanbanNode {
            id: id.to_owned(),
            label: descr.to_owned(),
            level,
            ..KanbanNode::default()
        };

        if let Some(data) = shape_data {
            let yaml = if data.contains('\n') {
                [data, "\n"].concat()
            } else {
                ["{\n", data, "\n}"].concat()
            };
            let doc = yaml::parse(&yaml).map_err(|message| {
                failure(
                    Code::Syntax,
                    [
                        "Metadata of \"",
                        &node.id,
                        "\" is not valid YAML. ",
                        &message,
                    ]
                    .concat(),
                )
            })?;
            // Mermaid throws a TypeError on metadata that is empty or not a mapping. Here it is
            // ignored.
            if matches!(doc, Value::Object(_)) {
                let shape = truthy_string(&doc, "shape").unwrap_or_default();
                if has_upper(&shape) || shape.contains('_') {
                    return Err(failure(
                        Code::Semantic,
                        [
                            "No such shape: ",
                            &shape,
                            ". Shape names should be lowercase.",
                        ]
                        .concat(),
                    ));
                }
                if let Some(label) = truthy_string(&doc, "label") {
                    node.label = label;
                }
                for (key, slot) in [
                    ("icon", &mut node.icon),
                    ("assigned", &mut node.assigned),
                    ("ticket", &mut node.ticket),
                    ("priority", &mut node.priority),
                ] {
                    if let Some(value) = truthy_string(&doc, key) {
                        *slot = Some(value);
                    }
                }
            }
        }

        // A node indented less than the first column becomes a card, and Mermaid rejects whatever
        // follows it.
        if let Some(misplaced) = &self.misplaced {
            return Err(failure(
                Code::Semantic,
                [
                    "Items without section detected, found section (\"",
                    misplaced,
                    "\")",
                ]
                .concat(),
            ));
        }
        if self.sections.is_empty() {
            self.section_level = level;
        }
        if level == self.section_level {
            self.last = Some(Last::Section);
            self.sections.push(KanbanSection {
                items: Vec::new(),
                node,
            });
        } else if let Some(section) = self.sections.last_mut() {
            self.last = Some(Last::Item);
            if level < self.section_level {
                self.misplaced = Some(node.label.clone());
            }
            section.items.push(node);
        }
        Ok(())
    }
}

impl OutlineDb for KanbanDb {
    fn add_node(&mut self, level: usize, id: &str, descr: &str, _: u8, shape_data: Option<&str>) {
        if self.error.is_some() {
            return;
        }
        if let Err(error) = self.add(level, id, descr, shape_data) {
            self.error = Some(error);
        }
    }

    // Mermaid throws a TypeError when a decoration comes before any node. Here it is ignored.
    fn decorate_node(&mut self, decoration: Decoration<'_>) {
        if self.error.is_some() {
            return;
        }
        let section = self.sections.last_mut();
        let node = match (self.last, section) {
            (Some(Last::Section), Some(section)) => &mut section.node,
            (Some(Last::Item), Some(section)) => match section.items.last_mut() {
                Some(item) => item,
                None => return,
            },
            _ => return,
        };
        if let Some(icon) = decoration.icon.filter(|icon| !icon.is_empty()) {
            node.icon = Some(icon.to_owned());
        }
        if let Some(class) = decoration.class.filter(|class| !class.is_empty()) {
            node.css_classes = Some(class.to_owned());
        }
    }
}
