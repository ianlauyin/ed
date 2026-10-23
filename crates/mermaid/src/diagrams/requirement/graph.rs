//! The nodes and edges of a requirement diagram to lay out. Port of Pele's
//! `src/diagrams/requirement/graph.ts`.

use super::db::{CONTAINS, RequirementDb};
use super::types::{Requirement, RequirementElement};

/// A node: a requirement or an element.
pub(crate) struct GraphNode<'a> {
    /// Class names joined by spaces.
    pub(crate) css_classes: String,
    /// Style declarations.
    pub(crate) css_styles: &'a [String],
    /// The element, when the node is one.
    pub(crate) element: Option<&'a RequirementElement>,
    /// The node's name.
    pub(crate) id: &'a str,
    /// The requirement, when the node is one.
    pub(crate) requirement: Option<&'a Requirement>,
}

/// An edge: a relationship whose ends may name no node.
pub(crate) struct GraphEdge<'a> {
    /// Whether the relationship is `contains`.
    pub(crate) contains: bool,
    /// The destination name, when a node has it.
    pub(crate) end: Option<&'a str>,
    /// `src-dst-index`.
    pub(crate) id: String,
    /// The source name, when a node has it.
    pub(crate) start: Option<&'a str>,
    /// The relationship.
    pub(crate) r#type: &'a str,
}

/// The graph to lay out.
pub(crate) struct RequirementGraph<'a> {
    /// The flow direction.
    pub(crate) direction: &'static str,
    /// Edges in authored order.
    pub(crate) edges: Vec<GraphEdge<'a>>,
    /// Requirements first, then elements, as Mermaid orders them.
    pub(crate) nodes: Vec<GraphNode<'a>>,
}

/// Builds the nodes and edges to lay out: requirements first, then elements.
pub(crate) fn build_requirement_graph(model: &RequirementDb) -> RequirementGraph<'_> {
    let mut nodes =
        Vec::with_capacity(model.requirements.values.len() + model.elements.values.len());
    for requirement in &model.requirements.values {
        nodes.push(GraphNode {
            css_classes: requirement.classes.join(" "),
            css_styles: &requirement.css_styles,
            element: None,
            id: &requirement.name,
            requirement: Some(requirement),
        });
    }
    for element in &model.elements.values {
        nodes.push(GraphNode {
            css_classes: element.classes.join(" "),
            css_styles: &element.css_styles,
            element: Some(element),
            id: &element.name,
            requirement: None,
        });
    }

    let has = |id: &str| model.requirements.has(id) || model.elements.has(id);
    let edges = model
        .relations
        .iter()
        .enumerate()
        .map(|(index, relation)| {
            let mut id = String::with_capacity(relation.src.len() + relation.dst.len() + 8);
            id.push_str(&relation.src);
            id.push('-');
            id.push_str(&relation.dst);
            id.push('-');
            crate::error::push_u32(&mut id, u32::try_from(index).unwrap_or(u32::MAX));
            GraphEdge {
                contains: relation.r#type == CONTAINS,
                end: has(&relation.dst).then_some(relation.dst.as_str()),
                id,
                start: has(&relation.src).then_some(relation.src.as_str()),
                r#type: &relation.r#type,
            }
        })
        .collect();

    RequirementGraph {
        direction: model.direction,
        edges,
        nodes,
    }
}
