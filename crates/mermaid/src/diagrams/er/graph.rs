//! Resolves a parsed ER diagram into what is drawn: subgraphs as groups, entities as nodes with
//! their class styles, and relationships as edges with a marker type at each end. Port of Pele's
//! `src/diagrams/er/graph.ts`.

use std::collections::{BTreeMap, BTreeSet};

use super::db::ErDb;
use super::types::ErAttribute;

/// A drawn node.
#[derive(Clone, Debug)]
pub(crate) struct ErGraphNode<'m> {
    /// The shown name in brackets, or empty.
    pub(crate) alias: &'m str,
    /// Attributes in authored order.
    pub(crate) attributes: &'m [ErAttribute],
    /// Class names, separated by spaces.
    pub(crate) css_classes: String,
    /// The declarations of its classes.
    pub(crate) css_compiled_styles: Vec<String>,
    /// Its own style declarations.
    pub(crate) css_styles: &'m [String],
    /// The direction inside a subgraph, when set.
    pub(crate) dir: Option<&'static str>,
    /// The id.
    pub(crate) id: &'m str,
    /// Whether the node is a subgraph.
    pub(crate) is_group: bool,
    /// The name or title.
    pub(crate) label: &'m str,
    /// The subgraph it is drawn in.
    pub(crate) parent_id: Option<&'m str>,
}

/// A drawn edge.
#[derive(Clone, Debug)]
pub(crate) struct ErGraphEdge<'m> {
    /// The marker at the end.
    pub(crate) arrow_type_end: &'static str,
    /// The marker at the start.
    pub(crate) arrow_type_start: &'static str,
    /// Whether the line is dashed: the relationship is not identifying.
    pub(crate) dashed: bool,
    /// The end node's id.
    pub(crate) end: &'m str,
    /// The id.
    pub(crate) id: String,
    /// The label.
    pub(crate) label: &'m str,
    /// The start node's id.
    pub(crate) start: &'m str,
}

/// The drawn nodes and edges.
#[derive(Debug)]
pub(crate) struct ErGraph<'m> {
    /// Edges in authored order.
    pub(crate) edges: Vec<ErGraphEdge<'m>>,
    /// Subgraphs, last first, then entities.
    pub(crate) nodes: Vec<ErGraphNode<'m>>,
}

/// Builds the drawn graph.
pub(crate) fn build(model: &ErDb) -> ErGraph<'_> {
    let mut nodes: Vec<ErGraphNode<'_>> = Vec::new();
    let mut edges: Vec<ErGraphEdge<'_>> = Vec::new();
    let subgraphs = &model.subgraphs;

    let compiled = |class_names: &mut dyn Iterator<Item = &str>| -> Vec<String> {
        let mut out = Vec::new();
        for name in class_names {
            let Some(def) = model.classes.get(name) else {
                continue;
            };
            out.extend(
                def.styles
                    .iter()
                    .map(|style| crate::text::label::trim(style).to_owned()),
            );
            out.extend(
                def.text_styles
                    .iter()
                    .map(|style| crate::text::label::trim(style).to_owned()),
            );
        }
        out
    };

    let mut parent_of: BTreeMap<&str, &str> = BTreeMap::new();
    let mut subgraph_ids: BTreeSet<&str> = BTreeSet::new();
    for subgraph in subgraphs.iter().rev() {
        subgraph_ids.insert(&subgraph.id);
        for id in &subgraph.nodes {
            parent_of.insert(id, &subgraph.id);
        }
    }

    for subgraph in subgraphs.iter().rev() {
        nodes.push(ErGraphNode {
            alias: "",
            attributes: &[],
            css_classes: subgraph.classes.join(" "),
            css_compiled_styles: compiled(&mut subgraph.classes.iter().map(String::as_str)),
            css_styles: &subgraph.css_styles,
            dir: subgraph.dir,
            id: &subgraph.id,
            is_group: true,
            label: &subgraph.title,
            parent_id: parent_of.get(subgraph.id.as_str()).copied(),
        });
    }

    // A name that is also a subgraph id stands for the subgraph. Mermaid drops such an entity and
    // leaves relationships written before the subgraph pointing at nothing. Here they follow it.
    let mut moved: BTreeMap<&str, &str> = BTreeMap::new();
    for entity in &model.entities.values {
        let name = entity.label.as_str();
        if subgraph_ids.contains(name) {
            moved.insert(&entity.id, name);
            continue;
        }
        nodes.push(ErGraphNode {
            alias: &entity.alias,
            attributes: &entity.attributes,
            css_classes: entity.css_classes.clone(),
            css_compiled_styles: compiled(&mut entity.css_classes.split(' ')),
            css_styles: &entity.css_styles,
            dir: None,
            id: &entity.id,
            is_group: false,
            label: name,
            parent_id: parent_of.get(name).copied(),
        });
    }

    for (index, relationship) in model.relationships.iter().enumerate() {
        let mut id = String::from("id_");
        id.push_str(&relationship.entity_a);
        id.push('_');
        id.push_str(&relationship.entity_b);
        id.push('_');
        crate::error::push_u32(&mut id, u32::try_from(index).unwrap_or(u32::MAX));
        let a = relationship.entity_a.as_str();
        let b = relationship.entity_b.as_str();
        edges.push(ErGraphEdge {
            arrow_type_end: relationship.rel_spec.card_a.marker(),
            arrow_type_start: relationship.rel_spec.card_b.marker(),
            dashed: !relationship.rel_spec.identifying,
            end: moved.get(b).copied().unwrap_or(b),
            id,
            label: &relationship.role_a,
            start: moved.get(a).copied().unwrap_or(a),
        });
    }

    ErGraph { edges, nodes }
}
