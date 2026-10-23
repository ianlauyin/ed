//! Resolves a parsed agentflow into the nodes and edges that are drawn: flow membership, collapsed
//! flows, shapes reduced to the ones agentflow has, class styles, and palette slots. Shape and
//! containment problems are recorded on the model as diagnostics. Port of Pele's
//! `src/diagrams/agentflow/graph.ts`.

use std::collections::{BTreeMap, BTreeSet};

use super::color_slots::assign_color_slots;
use super::db::{AgentflowDb, js_trim};
use super::shapes::{normalise_node_shapes, resolve_shape_alias};
use super::types::{LabelType, Metadata};
use crate::yaml::{self, Value};

/// A node or flow to draw.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GraphNode {
    /// The palette slot of a flow.
    pub(crate) color_index: Option<u32>,
    /// Space-separated class names.
    pub(crate) css_classes: String,
    /// Declarations from the node's classes.
    pub(crate) css_compiled_styles: Vec<String>,
    /// Declarations written on the node itself.
    pub(crate) css_styles: Vec<String>,
    /// The direction inside a flow.
    pub(crate) dir: Option<String>,
    /// Node id.
    pub(crate) id: String,
    /// Whether the node is drawn as a flow around others.
    pub(crate) is_group: bool,
    /// What the node is, which picks its palette slot.
    pub(crate) kind: Option<&'static str>,
    /// Label text.
    pub(crate) label: Option<String>,
    /// How the label is written.
    pub(crate) label_type: LabelType,
    /// Link address.
    pub(crate) link: Option<String>,
    /// Link target.
    pub(crate) link_target: Option<String>,
    /// The node's metadata.
    pub(crate) metadata: Option<Metadata>,
    /// The enclosing flow's id.
    pub(crate) parent_id: Option<String>,
    /// Shape name.
    pub(crate) shape: String,
    /// Tooltip text.
    pub(crate) tooltip: Option<String>,
}

/// An edge to draw.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GraphEdge {
    /// `animate` from metadata.
    pub(crate) animate: Option<Value>,
    /// `animation` from metadata.
    pub(crate) animation: Option<Value>,
    /// Marker at the head.
    pub(crate) arrow_type_end: &'static str,
    /// Declarations from the edge's classes.
    pub(crate) css_compiled_styles: Vec<String>,
    /// The curve: the edge's own, the default, or the configured one.
    pub(crate) curve: Option<Value>,
    /// Target node id.
    pub(crate) end: String,
    /// Edge id.
    pub(crate) id: String,
    /// Whether the id was written in the source.
    pub(crate) is_user_defined_id: bool,
    /// Label text.
    pub(crate) label: String,
    /// How the label is written.
    pub(crate) label_type: LabelType,
    /// The edge's metadata.
    pub(crate) metadata: Option<Metadata>,
    /// The extra rank span.
    pub(crate) minlen: Option<i32>,
    /// Source node id.
    pub(crate) start: String,
    /// Declarations from `linkStyle default` and the edge's own `linkStyle`.
    pub(crate) style: Vec<String>,
    /// The stroke, which Pele also stores as the edge's pattern.
    pub(crate) thickness: &'static str,
    /// The arrowhead kind.
    pub(crate) r#type: &'static str,
}

impl GraphEdge {
    /// Marker at the tail, which agentflow never draws.
    pub(crate) const ARROW_TYPE_START: &'static str = "none";
    /// The class names Mermaid gives every agentflow edge.
    pub(crate) const CLASSES: &'static str =
        "edge-thickness-normal edge-pattern-solid flowchart-link";
}

/// The drawn graph.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct AgentGraph {
    /// Edges in declaration order.
    pub(crate) edges: Vec<GraphEdge>,
    /// Flows (last closed first), then nodes in declaration order.
    pub(crate) nodes: Vec<GraphNode>,
}

fn is_collapsed(metadata: Option<&Metadata>) -> bool {
    metadata.is_some_and(|entries| {
        entries.iter().any(|(key, value)| {
            key == "view" && matches!(value, Value::String(view) if view == "collapsed")
        })
    })
}

/// Builds the drawn graph with a palette of `palette_length` colors. `default_curve` comes from
/// the `flowchart.curve` config.
#[expect(
    clippy::too_many_lines,
    reason = "mirrors Pele's buildAgentGraph, one pass per stage"
)]
pub(crate) fn build_agent_graph(
    db: &mut AgentflowDb,
    palette_length: u32,
    default_curve: Option<&str>,
) -> AgentGraph {
    let compiled = |db: &AgentflowDb, names: &[&str]| -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for name in names {
            if let Some(def) = db.class(name) {
                out.extend(def.styles.iter().cloned());
                out.extend(def.text_styles.iter().cloned());
                for entry in &mut out {
                    let trimmed = js_trim(entry);
                    if trimmed.len() != entry.len() {
                        *entry = String::from(trimmed);
                    }
                }
            }
        }
        out
    };

    // Everything inside a collapsed flow is hidden, and stands for its outermost collapsed flow.
    // Mermaid walks the whole inside of each collapsed flow, in the order the flows closed, and
    // the last walk to reach a thing wins. A thing has one container, so a later walk can only
    // enter an earlier one through its root: it stops there, and the root's owner is followed
    // afterwards.
    let mut hidden: Vec<String> = Vec::new();
    let mut hidden_set: BTreeSet<String> = BTreeSet::new();
    let mut owner: BTreeMap<String, String> = BTreeMap::new();
    let mut walked: BTreeSet<String> = BTreeSet::new();
    for sg in &db.subgraphs {
        if !is_collapsed(sg.metadata.as_ref()) || hidden_set.contains(&sg.id) {
            continue;
        }
        let mut pending = vec![sg.id.clone()];
        while let Some(id) = pending.pop() {
            let Some(members) = db.subgraph(&id).map(|member| &member.nodes) else {
                continue;
            };
            for child in members {
                if *child == sg.id || hidden_set.contains(child) {
                    continue;
                }
                hidden_set.insert(child.clone());
                hidden.push(child.clone());
                owner.insert(child.clone(), sg.id.clone());
                if !walked.contains(child) {
                    pending.push(child.clone());
                }
            }
        }
        walked.insert(sg.id.clone());
    }

    // Owners are always shown when they take a member, so each chain ends; the hop count only
    // guards that invariant.
    let mut collapsed_into: BTreeMap<String, String> = BTreeMap::new();
    for id in &hidden {
        let mut path = vec![id.clone()];
        let Some(mut top) = owner.get(id).cloned() else {
            continue;
        };
        for _ in 0..=hidden.len() {
            let known = collapsed_into.get(&top).cloned();
            let Some(up) = known.clone().or_else(|| owner.get(&top).cloned()) else {
                break;
            };
            if known.is_none() {
                path.push(top.clone());
            }
            top = up;
            if known.is_some() {
                break;
            }
        }
        for p in path {
            collapsed_into.insert(p, top.clone());
        }
    }

    let count = db.subgraphs.len();
    let position: BTreeMap<&str, usize> = db
        .subgraphs
        .iter()
        .enumerate()
        .map(|(i, sg)| (sg.id.as_str(), i))
        .collect();

    // Two flows can each name the other. The nesting that would close the loop is dropped.
    let mut parent_of: BTreeMap<String, String> = BTreeMap::new();
    let mut loops: Vec<(String, String)> = Vec::new();
    for i in (0..count).rev() {
        let Some(sg) = db.subgraphs.get(i) else {
            continue;
        };
        if hidden_set.contains(&sg.id) {
            continue;
        }
        for id in &sg.nodes {
            // Only a flow handled earlier in this pass can already be above the parent.
            let closes = position.get(id.as_str()).is_some_and(|&at| at >= i) && {
                let mut current = Some(sg.id.as_str());
                let mut found = false;
                for _ in 0..=count {
                    let Some(key) = current else { break };
                    if key == id {
                        found = true;
                        break;
                    }
                    current = parent_of.get(key).map(String::as_str);
                }
                found
            };
            if closes {
                loops.push((sg.id.clone(), id.clone()));
                continue;
            }
            parent_of.insert(id.clone(), sg.id.clone());
        }
    }
    for (container, id) in loops {
        let mut message = String::from("Container \"");
        message.push_str(&container);
        message.push_str("\" cannot contain \"");
        message.push_str(&id);
        message.push_str("\" because \"");
        message.push_str(&id);
        message
            .push_str("\" already contains it. The nesting that would close the loop is dropped.");
        db.emit("CONTAINMENT_VIOLATION", "warning", message, &id);
    }

    let mut nodes: Vec<GraphNode> = Vec::new();
    let mut by_id: BTreeMap<String, usize> = BTreeMap::new();
    for sg in db.subgraphs.iter().rev() {
        if hidden_set.contains(&sg.id) {
            continue;
        }
        let collapsed = is_collapsed(sg.metadata.as_ref());
        let classes: Vec<&str> = sg.classes.iter().map(String::as_str).collect();
        let metadata = if collapsed {
            let mut metadata = sg.metadata.clone().unwrap_or_default();
            yaml::set(
                &mut metadata,
                "containerType".to_owned(),
                Value::String("flow".to_owned()),
            );
            Some(metadata)
        } else {
            sg.metadata.clone()
        };
        by_id.insert(sg.id.clone(), nodes.len());
        nodes.push(GraphNode {
            color_index: None,
            css_classes: sg.classes.join(" "),
            css_compiled_styles: compiled(db, &classes),
            css_styles: Vec::new(),
            dir: sg.dir.clone(),
            id: sg.id.clone(),
            is_group: !collapsed,
            kind: None,
            label: Some(sg.title.clone()),
            label_type: sg.label_type,
            link: None,
            link_target: None,
            metadata,
            parent_id: parent_of.get(&sg.id).cloned(),
            shape: String::from(if collapsed {
                "collapsedGroup"
            } else {
                "flowGroup"
            }),
            tooltip: None,
        });
    }

    for vertex in &db.nodes {
        if hidden_set.contains(&vertex.id) {
            continue;
        }
        let classes: Vec<&str> = vertex.classes.iter().map(String::as_str).collect();
        if let Some(existing) = by_id.get(&vertex.id).and_then(|&at| nodes.get_mut(at)) {
            existing.css_styles.clone_from(&vertex.styles);
            existing.css_compiled_styles = compiled(db, &classes);
            existing.css_classes = vertex.classes.join(" ");
            continue;
        }

        let mut names = vec!["default", "node"];
        names.extend(classes.iter().copied());
        let mut css_classes = String::from("default ");
        css_classes.push_str(&vertex.classes.join(" "));
        let shape = match vertex.r#type.as_deref().map(resolve_shape_alias) {
            None | Some("square") => String::from("squareRect"),
            Some(other) => String::from(other),
        };
        by_id.insert(vertex.id.clone(), nodes.len());
        nodes.push(GraphNode {
            color_index: None,
            css_classes,
            css_compiled_styles: compiled(db, &names),
            css_styles: vertex.styles.clone(),
            dir: vertex.dir.clone(),
            id: vertex.id.clone(),
            is_group: false,
            kind: None,
            label: vertex.text.clone(),
            label_type: vertex.label_type,
            link: vertex.link.clone(),
            link_target: vertex.link_target.clone(),
            metadata: vertex.metadata.clone(),
            parent_id: parent_of.get(&vertex.id).cloned(),
            shape,
            tooltip: db.tooltip(&vertex.id).map(String::from),
        });
    }

    let mut edges = Vec::with_capacity(db.edges.len());
    for (index, raw) in db.edges.iter().enumerate() {
        let start = collapsed_into.get(&raw.start).unwrap_or(&raw.start);
        let end = collapsed_into.get(&raw.end).unwrap_or(&raw.end);
        // An edge between two things inside one collapsed flow has nothing left to connect.
        if start == end && raw.start != raw.end {
            continue;
        }

        let mut style = db.default_edge_style.clone().unwrap_or_default();
        if let Some(own) = &raw.style {
            style.extend(own.iter().cloned());
        }
        let arrow = match raw.r#type {
            "arrow_open" => "none",
            "arrow_cross" => "arrow_cross",
            _ => "arrow_point",
        };
        let id = if raw.id.is_empty() {
            let mut id = String::from("L_");
            id.push_str(start);
            id.push('_');
            id.push_str(end);
            id.push('_');
            crate::error::push_u32(&mut id, u32::try_from(index).unwrap_or(u32::MAX));
            id
        } else {
            raw.id.clone()
        };
        // `raw.interpolate || db.defaultInterpolate || defaultCurve`, as JavaScript reads it.
        let curve = match raw.interpolate.as_ref().filter(|value| value.truthy()) {
            Some(value) => Some(value.clone()),
            None => db
                .default_interpolate
                .clone()
                .filter(|text| !text.is_empty())
                .or_else(|| default_curve.map(String::from))
                .map(Value::String),
        };
        let classes: Vec<&str> = raw.classes.iter().map(String::as_str).collect();

        edges.push(GraphEdge {
            animate: raw.animate.clone(),
            animation: raw.animation.clone(),
            arrow_type_end: arrow,
            css_compiled_styles: compiled(db, &classes),
            curve,
            end: end.clone(),
            id,
            is_user_defined_id: raw.is_user_defined_id,
            label: raw.text.clone(),
            label_type: raw.label_type,
            metadata: raw.metadata.clone(),
            minlen: raw.length,
            start: start.clone(),
            style,
            thickness: raw.stroke,
            r#type: raw.r#type,
        });
    }

    normalise_node_shapes(&mut nodes, db);

    // Containers are numbered in the order they were written: a walk of the nesting, parents
    // first. `subgraphs` itself is in the order the blocks closed, with a nested flow before its
    // parent.
    let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for sg in &db.subgraphs {
        if let Some(parent) = parent_of.get(&sg.id) {
            children.entry(parent).or_default().push(&sg.id);
        }
    }
    let mut order: BTreeMap<String, u32> = BTreeMap::new();
    for sg in &db.subgraphs {
        if parent_of.contains_key(&sg.id) {
            continue;
        }
        let mut pending = vec![sg.id.as_str()];
        while let Some(id) = pending.pop() {
            if order.contains_key(id) {
                continue;
            }
            let n = u32::try_from(order.len()).unwrap_or(u32::MAX);
            order.insert(id.to_owned(), n);
            if let Some(kids) = children.get(id) {
                pending.extend(kids.iter().rev());
            }
        }
    }

    let db = &*db;
    assign_color_slots(
        &mut nodes,
        |id| {
            if db.is_connector(id) {
                return Some("connector");
            }
            db.node(id).map(AgentflowDb::vertex_kind)
        },
        &order,
        palette_length,
    );

    AgentGraph { edges, nodes }
}
