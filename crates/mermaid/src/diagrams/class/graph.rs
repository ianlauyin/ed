//! Resolves a parsed class diagram into the nodes and edges that are drawn. Port of Pele's
//! `src/diagrams/class/graph.ts`.

use std::collections::BTreeMap;

use super::db::ClassDb;
use super::types::{ClassNode, LOLLIPOP, RelationEnd};

/// What a node is drawn as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NodeShape {
    /// A class.
    ClassBox,
    /// A lollipop interface.
    Interface,
    /// A note.
    Note,
    /// A namespace.
    Rect,
}

/// A drawn node.
#[derive(Clone, Debug)]
pub(crate) struct GraphNode<'m> {
    /// The class it draws.
    pub(crate) class_node: Option<&'m ClassNode>,
    /// The id.
    pub(crate) id: String,
    /// Whether the node is a namespace.
    pub(crate) is_group: bool,
    /// The shown text.
    pub(crate) label: String,
    /// Index of the namespace node this one is drawn in, or -1.
    pub(crate) parent: i32,
    /// The id of the namespace this one is drawn in.
    pub(crate) parent_id: Option<String>,
    /// What it is drawn as.
    pub(crate) shape: NodeShape,
}

/// A drawn edge.
#[derive(Clone, Debug)]
pub(crate) struct GraphEdge {
    /// The marker at the start.
    pub(crate) arrow_type_start: RelationEnd,
    /// The marker at the end.
    pub(crate) arrow_type_end: RelationEnd,
    /// Whether the edge joins a note to its class.
    pub(crate) dotted: bool,
    /// Whether the line is dashed.
    pub(crate) dashed: bool,
    /// The text beside the end.
    pub(crate) end_label_left: String,
    /// The id.
    pub(crate) id: String,
    /// The label.
    pub(crate) label: Option<String>,
    /// The index of the start node, or `None` when it names none.
    pub(crate) source: Option<usize>,
    /// The text beside the start.
    pub(crate) start_label_right: String,
    /// The index of the end node, or `None` when it names none.
    pub(crate) target: Option<usize>,
}

/// The drawn nodes and edges.
#[derive(Debug)]
pub(crate) struct ClassGraph<'m> {
    /// Edges in drawing order.
    pub(crate) edges: Vec<GraphEdge>,
    /// Namespaces, then classes, then notes, then interfaces.
    pub(crate) nodes: Vec<GraphNode<'m>>,
}

/// Builds the drawn graph. With `hierarchical` off, only namespaces the source declares are kept
/// and each is labeled with its full name.
pub(crate) fn build(model: &ClassDb, hierarchical: bool) -> ClassGraph<'_> {
    let mut nodes: Vec<GraphNode<'_>> = Vec::new();
    let mut edges: Vec<GraphEdge> = Vec::new();
    let mut namespace_at: BTreeMap<&str, usize> = BTreeMap::new();
    let mut class_at: BTreeMap<&str, usize> = BTreeMap::new();
    let mut interface_at: BTreeMap<&str, usize> = BTreeMap::new();

    let mut declared: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut declared_ancestor = |id: Option<&str>| -> Option<String> {
        let id = id?;
        if hierarchical {
            return Some(id.to_owned());
        }
        let mut chain: Vec<String> = Vec::new();
        let mut current = Some(id.to_owned());
        let mut found: Option<String> = None;
        while let Some(at) = current.take().filter(|at| !at.is_empty()) {
            if let Some(known) = declared.get(&at) {
                found.clone_from(known);
                break;
            }
            let Some(ns) = model.namespaces.get(&at) else {
                break;
            };
            if ns.explicit {
                found = Some(at);
                break;
            }
            current.clone_from(&ns.parent);
            chain.push(at);
        }
        for visited in chain {
            declared.insert(visited, found.clone());
        }
        found
    };

    for ns in &model.namespaces.values {
        if !hierarchical && !ns.explicit {
            continue;
        }
        namespace_at.insert(&ns.id, nodes.len());
        nodes.push(GraphNode {
            class_node: None,
            id: ns.id.clone(),
            is_group: true,
            label: if hierarchical {
                ns.label.clone()
            } else {
                ns.id.clone()
            },
            parent: -1,
            parent_id: if hierarchical {
                ns.parent.clone()
            } else {
                None
            },
            shape: NodeShape::Rect,
        });
    }
    let group = |namespace_at: &BTreeMap<&str, usize>, id: Option<&str>| -> i32 {
        id.and_then(|id| namespace_at.get(id))
            .map_or(-1, |&at| i32::try_from(at).unwrap_or(-1))
    };
    for node in &mut nodes {
        node.parent = group(&namespace_at, node.parent_id.as_deref());
    }

    for class_node in &model.classes.values {
        let parent_id = declared_ancestor(class_node.parent.as_deref());
        class_at.insert(&class_node.id, nodes.len());
        nodes.push(GraphNode {
            class_node: Some(class_node),
            id: class_node.id.clone(),
            is_group: false,
            label: class_node.label.clone(),
            parent: group(&namespace_at, parent_id.as_deref()),
            parent_id,
            shape: NodeShape::ClassBox,
        });
    }

    for note in &model.notes.values {
        let parent_id = declared_ancestor(note.parent.as_deref());
        let target = note.class.as_deref().and_then(|class| class_at.get(class));
        if let Some(&target) = target {
            let mut id = String::from("edgeNote");
            crate::error::push_u32(&mut id, u32::try_from(note.index).unwrap_or(u32::MAX));
            edges.push(GraphEdge {
                arrow_type_end: RelationEnd::None,
                arrow_type_start: RelationEnd::None,
                dashed: false,
                dotted: true,
                end_label_left: String::new(),
                id,
                label: None,
                source: Some(nodes.len()),
                start_label_right: String::new(),
                target: Some(target),
            });
        }
        nodes.push(GraphNode {
            class_node: None,
            id: note.id.clone(),
            is_group: false,
            label: note.text.clone(),
            parent: group(&namespace_at, parent_id.as_deref()),
            parent_id,
            shape: NodeShape::Note,
        });
    }

    // Mermaid leaves every lollipop interface outside the namespaces. Here it stays beside its
    // class.
    for item in &model.interfaces {
        let parent = class_at
            .get(item.class_id.as_str())
            .and_then(|&owner| nodes.get(owner))
            .map_or(-1, |owner| owner.parent);
        interface_at.insert(&item.id, nodes.len());
        nodes.push(GraphNode {
            class_node: None,
            id: item.id.clone(),
            is_group: false,
            label: item.label.clone(),
            parent,
            parent_id: None,
            shape: NodeShape::Interface,
        });
    }

    for (count, relation) in model.relations.iter().enumerate() {
        let (type1, type2) = (relation.type1, relation.type2);
        // `addRelation` swaps in an interface for the lollipop end of a relation with no other
        // marker.
        let start_interface = type1 == LOLLIPOP && type2 == RelationEnd::None;
        let end_interface = !start_interface && type2 == LOLLIPOP && type1 == RelationEnd::None;
        let start_at = if start_interface {
            &interface_at
        } else {
            &class_at
        };
        let end_at = if end_interface {
            &interface_at
        } else {
            &class_at
        };
        let mut id = String::from("id_");
        id.push_str(&relation.id1);
        id.push('_');
        id.push_str(&relation.id2);
        id.push('_');
        crate::error::push_u32(&mut id, u32::try_from(count + 1).unwrap_or(u32::MAX));
        edges.push(GraphEdge {
            arrow_type_end: type2,
            arrow_type_start: type1,
            dashed: relation.line_type == 1,
            dotted: false,
            end_label_left: if relation.relation_title2 == "none" {
                String::new()
            } else {
                relation.relation_title2.clone()
            },
            id,
            label: relation.title.clone(),
            source: start_at.get(relation.id1.as_str()).copied(),
            start_label_right: if relation.relation_title1 == "none" {
                String::new()
            } else {
                relation.relation_title1.clone()
            },
            target: end_at.get(relation.id2.as_str()).copied(),
        });
    }

    ClassGraph { edges, nodes }
}
