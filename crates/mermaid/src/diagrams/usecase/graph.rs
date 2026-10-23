//! What a use case diagram draws: every actor, use case, note, JSON table, and boundary as a node
//! with its class styles, and every relationship and note connector as an edge with its markers.
//! Port of Pele's `src/diagrams/usecase/graph.ts`, which ports `getData` from Mermaid's use case
//! DB.

use super::json::{JsonDoc, JsonValue};
use super::types::{ActorType, Relationship, RelationshipType, Shape, UsecaseModel, arrow};
use crate::text::label::trim;
use crate::yaml::push_number;

/// One row of a JSON table.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct JsonRow {
    /// The key path. Blank on the second and later values of an array.
    pub(crate) key: String,
    /// The value.
    pub(crate) value: String,
}

/// What kind of node is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NodeShape {
    /// An actor's figure.
    Actor(ActorType),
    /// A system boundary.
    Boundary,
    /// A business use case's oval.
    Business,
    /// A use case's oval.
    Ellipse,
    /// A JSON table.
    Json,
    /// A note.
    Note,
    /// A rectangular use case.
    Rect,
}

/// A node to draw.
#[derive(Clone, Debug)]
pub(crate) struct GraphNode<'m> {
    /// Whether it is a business actor or use case.
    pub(crate) business: bool,
    /// Its classes, starting with `default`.
    pub(crate) css_classes: String,
    /// The declarations its classes give it.
    pub(crate) css_compiled_styles: Vec<String>,
    /// Its inline styles.
    pub(crate) css_styles: &'m [String],
    /// The icon of an `icon` actor.
    pub(crate) icon: Option<&'m str>,
    /// Its id.
    pub(crate) id: &'m str,
    /// The rows of a JSON table.
    pub(crate) json_rows: Option<Vec<JsonRow>>,
    /// Its label.
    pub(crate) label: &'m str,
    /// Whether the label is markdown.
    pub(crate) markdown: bool,
    /// Whether a boundary is drawn as a package.
    pub(crate) package: bool,
    /// Room around its text.
    pub(crate) padding: f64,
    /// The boundary it is in.
    pub(crate) parent_id: Option<&'m str>,
    /// What is drawn.
    pub(crate) shape: NodeShape,
    /// Its stereotype.
    pub(crate) stereotype: Option<&'m str>,
}

/// An edge to draw.
#[derive(Clone, Debug)]
pub(crate) struct GraphEdge<'m> {
    /// Whether it is drawn animated.
    pub(crate) animate: bool,
    /// The marker at its end.
    pub(crate) arrow_end: &'static str,
    /// The marker at its start.
    pub(crate) arrow_start: &'static str,
    /// Its classes, starting with `default relationship`.
    pub(crate) classes: String,
    /// The declarations its classes give it.
    pub(crate) css_compiled_styles: Vec<String>,
    /// Whether it is dotted.
    pub(crate) dotted: bool,
    /// The target id.
    pub(crate) end: &'m str,
    /// Its id.
    pub(crate) id: String,
    /// The dotted line from a note to what it is about.
    pub(crate) internal: bool,
    /// Its label and whether the label is markdown.
    pub(crate) label: Option<(&'m str, bool)>,
    /// The least number of ranks between its ends.
    pub(crate) minlen: u32,
    /// Its kind, or `note`.
    pub(crate) relationship_type: &'static str,
    /// The source id.
    pub(crate) start: &'m str,
    /// Its inline styles.
    pub(crate) style: &'m [String],
}

/// The nodes and edges to draw.
pub(crate) struct UsecaseGraph<'m> {
    /// Edges: relationships, then note connectors.
    pub(crate) edges: Vec<GraphEdge<'m>>,
    /// Nodes: actors, use cases, notes, JSON tables, then boundaries.
    pub(crate) nodes: Vec<GraphNode<'m>>,
}

/// Appends a JSON scalar as `String(value)` writes it.
fn scalar(out: &mut String, value: &JsonValue) {
    match value {
        JsonValue::String(text) => out.push_str(text),
        JsonValue::Null => out.push_str("null"),
        JsonValue::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        JsonValue::Number(value) => push_number(out, *value),
        JsonValue::Array(_) | JsonValue::Object(_) => {}
    }
}

/// One row for each leaf of a JSON object, depth first in the order the keys were written.
/// Nesting is kept on a stack of its own, so depth is limited by memory and not by the call
/// stack.
pub(crate) fn flatten_json_rows(json: &JsonDoc) -> Vec<JsonRow> {
    let mut rows = Vec::new();
    let mut stack: Vec<(usize, String)> = vec![(0, String::new())];
    let mut children: Vec<(usize, String)> = Vec::new();
    while let Some((node, path)) = stack.pop() {
        children.clear();
        match json.get(node) {
            Some(JsonValue::Array(items)) => {
                let leaf = |&item: &usize| {
                    !matches!(
                        json.get(item),
                        Some(JsonValue::Array(_) | JsonValue::Object(_))
                    )
                };
                if items.is_empty() {
                    rows.push(JsonRow {
                        key: path,
                        value: String::from("[]"),
                    });
                } else if items.iter().all(leaf) {
                    for (i, &item) in items.iter().enumerate() {
                        let mut value = String::new();
                        if let Some(item) = json.get(item) {
                            scalar(&mut value, item);
                        }
                        rows.push(JsonRow {
                            key: if i == 0 { path.clone() } else { String::new() },
                            value,
                        });
                    }
                } else {
                    for (i, &item) in items.iter().enumerate() {
                        let mut child = path.clone();
                        child.push('[');
                        crate::diagrams::usecase::lexer::push_usize(&mut child, i);
                        child.push(']');
                        children.push((item, child));
                    }
                }
            }
            Some(JsonValue::Object(entries)) => {
                if entries.is_empty() {
                    rows.push(JsonRow {
                        key: path.clone(),
                        value: String::from("{}"),
                    });
                }
                for (key, item) in entries {
                    let mut child = path.clone();
                    if !child.is_empty() {
                        child.push('.');
                    }
                    child.push_str(key);
                    children.push((*item, child));
                }
            }
            Some(value) => {
                let mut text = String::new();
                scalar(&mut text, value);
                rows.push(JsonRow {
                    key: path,
                    value: text,
                });
            }
            None => {}
        }
        stack.extend(children.drain(..).rev());
    }
    rows
}

/// Classes joined by spaces, skipping empty ones.
fn class_list<'a>(names: impl IntoIterator<Item = &'a str>) -> String {
    let mut out = String::new();
    for name in names {
        if name.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(name);
    }
    out
}

/// The start and end markers of a relationship.
fn markers(relationship: &Relationship) -> (&'static str, &'static str) {
    match relationship.r#type {
        RelationshipType::Generalization => ("none", "extension"),
        RelationshipType::Include | RelationshipType::Extend => ("none", "arrow_point"),
        RelationshipType::Association => match relationship.arrow_type {
            arrow::SOLID_ARROW => ("none", "arrow_point"),
            arrow::BACK_ARROW => ("arrow_point", "none"),
            arrow::CIRCLE_ARROW => ("none", "arrow_circle"),
            arrow::CROSS_ARROW => ("none", "arrow_cross"),
            arrow::CIRCLE_ARROW_REVERSED => ("arrow_circle", "none"),
            arrow::CROSS_ARROW_REVERSED => ("arrow_cross", "none"),
            _ => ("none", "none"),
        },
    }
}

/// Resolves a parsed use case diagram into what is drawn.
pub(crate) fn build_usecase_graph(model: &UsecaseModel) -> UsecaseGraph<'_> {
    // A later class wins a property but keeps the place the property first took.
    let compiled = |class_names: &[String]| -> Vec<String> {
        let mut out: Vec<(&str, &str)> = Vec::new();
        for name in std::iter::once("default").chain(class_names.iter().map(String::as_str)) {
            let Some(definition) = model.class_defs.iter().find(|own| own.id == name) else {
                continue;
            };
            for raw in &definition.styles {
                let style = trim(raw);
                let property = trim(
                    style
                        .find(':')
                        .map_or(style, |at| style.get(..at).unwrap_or("")),
                );
                if property.is_empty() {
                    continue;
                }
                match out.iter_mut().find(|(own, _)| *own == property) {
                    Some(entry) => entry.1 = style,
                    None => out.push((property, style)),
                }
            }
        }
        out.into_iter().map(|(_, style)| style.to_owned()).collect()
    };

    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    for actor in &model.actors {
        let kind = {
            let mut kind = String::from("usecase-actor-");
            kind.push_str(actor.r#type.as_str());
            kind
        };
        nodes.push(GraphNode {
            business: actor.business,
            css_classes: class_list(
                ["default", "usecase-actor", &kind]
                    .into_iter()
                    .chain(actor.business.then_some("usecase-business"))
                    .chain(actor.classes.iter().map(String::as_str)),
            ),
            css_compiled_styles: compiled(&actor.classes),
            css_styles: &actor.styles,
            icon: actor.icon.as_deref().filter(|icon| !icon.is_empty()),
            id: &actor.id,
            json_rows: None,
            label: &actor.label,
            markdown: actor.markdown,
            package: false,
            padding: 10.0,
            parent_id: actor.parent_id.as_deref().filter(|id| !id.is_empty()),
            shape: NodeShape::Actor(actor.r#type),
            stereotype: actor.stereotype.as_deref().filter(|s| !s.is_empty()),
        });
    }
    for use_case in &model.use_cases {
        let ellipse = use_case.shape == Shape::Ellipse;
        let kind = {
            let mut kind = String::from("usecase-");
            kind.push_str(use_case.shape.as_str());
            kind
        };
        nodes.push(GraphNode {
            business: use_case.business,
            css_classes: class_list(
                ["default", "usecase-element", &kind]
                    .into_iter()
                    .chain(use_case.business.then_some("usecase-business"))
                    .chain(use_case.classes.iter().map(String::as_str)),
            ),
            css_compiled_styles: compiled(&use_case.classes),
            css_styles: &use_case.styles,
            icon: None,
            id: &use_case.id,
            json_rows: None,
            label: &use_case.label,
            markdown: use_case.markdown,
            package: false,
            padding: if ellipse { 20.0 } else { 10.0 },
            parent_id: use_case.parent_id.as_deref().filter(|id| !id.is_empty()),
            shape: if !ellipse {
                NodeShape::Rect
            } else if use_case.business {
                NodeShape::Business
            } else {
                NodeShape::Ellipse
            },
            stereotype: use_case.stereotype.as_deref().filter(|s| !s.is_empty()),
        });
    }
    for note in &model.notes {
        nodes.push(GraphNode {
            business: false,
            css_classes: String::from("default usecase-note"),
            css_compiled_styles: compiled(&[]),
            css_styles: &[],
            icon: None,
            id: &note.id,
            json_rows: None,
            label: &note.label,
            markdown: note.markdown,
            package: false,
            padding: 10.0,
            parent_id: None,
            shape: NodeShape::Note,
            stereotype: None,
        });
    }
    for json in &model.json_nodes {
        nodes.push(GraphNode {
            business: false,
            css_classes: class_list(
                ["default", "usecase-json-table"]
                    .into_iter()
                    .chain(json.classes.iter().map(String::as_str)),
            ),
            css_compiled_styles: compiled(&json.classes),
            css_styles: &json.styles,
            icon: None,
            id: &json.id,
            json_rows: Some(flatten_json_rows(&json.value)),
            label: &json.id,
            markdown: false,
            package: false,
            padding: 10.0,
            parent_id: None,
            shape: NodeShape::Json,
            stereotype: None,
        });
    }
    for boundary in &model.system_boundaries {
        nodes.push(GraphNode {
            business: false,
            css_classes: class_list(
                [
                    "default",
                    "system-boundary",
                    if boundary.package {
                        "system-boundary-package"
                    } else {
                        "system-boundary-rect"
                    },
                ]
                .into_iter()
                .chain(boundary.classes.iter().map(String::as_str)),
            ),
            css_compiled_styles: compiled(&boundary.classes),
            css_styles: &boundary.styles,
            icon: None,
            id: &boundary.id,
            json_rows: None,
            label: &boundary.label,
            markdown: boundary.markdown,
            package: boundary.package,
            padding: 20.0,
            parent_id: None,
            shape: NodeShape::Boundary,
            stereotype: None,
        });
    }

    for relationship in &model.relationships {
        let semantic = matches!(
            relationship.r#type,
            RelationshipType::Include | RelationshipType::Extend
        );
        let (arrow_start, arrow_end) = markers(relationship);
        let animated = relationship.animate || relationship.animation.is_some();
        let mut kind = String::from("relationship-");
        kind.push_str(relationship.r#type.as_str());
        let mut animation = String::new();
        if animated {
            animation.push_str("edge-animation-");
            animation.push_str(relationship.animation.unwrap_or("fast"));
        }
        // Include and extend are labelled with their kind, whatever label the relationship
        // carries.
        let label = if semantic {
            Some((relationship.r#type.as_str(), false))
        } else if relationship.r#type == RelationshipType::Association {
            relationship
                .label
                .as_ref()
                .filter(|(text, _)| !text.is_empty())
                .map(|(text, markdown)| (text.as_str(), *markdown))
        } else {
            None
        };
        edges.push(GraphEdge {
            animate: relationship.animate,
            arrow_end,
            arrow_start,
            classes: class_list(
                ["default", "relationship", &kind]
                    .into_iter()
                    .chain(relationship.classes.iter().map(String::as_str))
                    .chain(std::iter::once(animation.as_str())),
            ),
            css_compiled_styles: compiled(&relationship.classes),
            dotted: semantic,
            end: &relationship.target,
            id: relationship.id.clone(),
            internal: false,
            label,
            minlen: relationship.minlen,
            relationship_type: relationship.r#type.as_str(),
            start: &relationship.source,
            style: &relationship.styles,
        });
    }
    for note in &model.notes {
        let mut id = note.id.clone();
        id.push_str("-edge");
        edges.push(GraphEdge {
            animate: false,
            arrow_end: "none",
            arrow_start: "none",
            classes: String::from("default relationship relationship-note"),
            css_compiled_styles: compiled(&[]),
            dotted: true,
            end: &note.target,
            id,
            internal: true,
            label: None,
            minlen: 1,
            relationship_type: "note",
            start: &note.id,
            style: &[],
        });
    }
    UsecaseGraph { edges, nodes }
}
