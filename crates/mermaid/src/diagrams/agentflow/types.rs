//! Records of the agentflow model. Port of Pele's `src/diagrams/agentflow/types.ts`.
//!
//! Labels, class definitions, and subgraph body items have the flowchart's shape, so those types
//! come from the flowchart module.

use crate::yaml::Value;

pub(crate) use crate::diagrams::flowchart::types::{DocItem, FlowClassDef, FlowText, LabelType};

/// Metadata from `@{ ... }` blocks, merged in JavaScript property order.
pub(crate) type Metadata = Vec<(String, Value)>;

/// A source span as Jison reports it: lines start at 1, columns at 0, in UTF-16 units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Loc {
    /// The column the span starts at.
    pub(crate) first_column: u32,
    /// The line the span starts on.
    pub(crate) first_line: u32,
    /// The column the span ends at.
    pub(crate) last_column: u32,
    /// The line the span ends on.
    pub(crate) last_line: u32,
}

/// A node, or a connector, which is a node too.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AgentNode {
    /// Class names, in application order.
    pub(crate) classes: Vec<String>,
    /// A direction. The parser never sets it, but the model keeps its slot.
    pub(crate) dir: Option<String>,
    /// The node id.
    pub(crate) id: String,
    /// Whether a `connector` statement made the node.
    pub(crate) is_connector: bool,
    /// How the label is written.
    pub(crate) label_type: LabelType,
    /// The sanitized click target.
    pub(crate) link: Option<String>,
    /// The click target window.
    pub(crate) link_target: Option<String>,
    /// Metadata merged across statements.
    pub(crate) metadata: Option<Metadata>,
    /// `[|field:value|...]` properties, in insertion order. Set by the first `addVertex`.
    pub(crate) props: Option<Vec<(String, String)>>,
    /// Inline styles, in application order.
    pub(crate) styles: Vec<String>,
    /// The label.
    pub(crate) text: Option<String>,
    /// The shape: a bracket syntax name or a metadata shape name.
    pub(crate) r#type: Option<String>,
}

/// An edge.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AgentEdge {
    /// `animate` from metadata, as YAML read it.
    pub(crate) animate: Option<Value>,
    /// `animation` from metadata, as YAML read it.
    pub(crate) animation: Option<Value>,
    /// Class names, in application order.
    pub(crate) classes: Vec<String>,
    /// `sequence`, `reference`, or `failure`, for the arrows that mean one.
    pub(crate) edge_semantic: Option<&'static str>,
    /// The target node id.
    pub(crate) end: String,
    /// The user-defined or generated id.
    pub(crate) id: String,
    /// The curve: from `linkStyle ... interpolate`, or any YAML value from metadata `curve`.
    pub(crate) interpolate: Option<Value>,
    /// Whether `id` was written in the source.
    pub(crate) is_user_defined_id: bool,
    /// How the label is written.
    pub(crate) label_type: LabelType,
    /// The extra rank span, capped at 10.
    pub(crate) length: Option<i32>,
    /// Metadata merged across statements.
    pub(crate) metadata: Option<Metadata>,
    /// The source node id.
    pub(crate) start: String,
    /// `normal`, `dotted`, or `INVALID`.
    pub(crate) stroke: &'static str,
    /// Styles from `linkStyle`.
    pub(crate) style: Option<Vec<String>>,
    /// The label.
    pub(crate) text: String,
    /// The arrowhead kind, such as `arrow_point`.
    pub(crate) r#type: &'static str,
}

/// A `flow` block.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AgentSubgraph {
    /// Class names, in application order.
    pub(crate) classes: Vec<String>,
    /// The layout direction.
    pub(crate) dir: Option<String>,
    /// The flow id.
    pub(crate) id: String,
    /// How the title is written.
    pub(crate) label_type: LabelType,
    /// Metadata merged across statements.
    pub(crate) metadata: Option<Metadata>,
    /// Member node and flow ids.
    pub(crate) nodes: Vec<String>,
    /// The title.
    pub(crate) title: String,
}

/// An arrow as `destructLink` reads it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LinkInfo {
    /// What the arrow means, when it means something.
    pub(crate) edge_semantic: Option<&'static str>,
    /// The user-defined id, with its `@`.
    pub(crate) id: Option<String>,
    /// The extra rank span.
    pub(crate) length: Option<i32>,
    /// The stroke.
    pub(crate) stroke: &'static str,
    /// The label.
    pub(crate) text: Option<FlowText>,
    /// The arrowhead kind.
    pub(crate) r#type: &'static str,
}

/// Where a statement sits in the source. Lines include the front matter offset. Mermaid builds
/// its parser without ranges, so the start and end indexes are always zero and are left out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ElementPosition {
    /// The column the statement ends at.
    pub(crate) end_column: u32,
    /// The line the statement ends on.
    pub(crate) end_line: u32,
    /// The column the statement starts at.
    pub(crate) start_column: u32,
    /// The line the statement starts on.
    pub(crate) start_line: u32,
}

/// A statement's id, kind, and position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ElementMapping {
    /// The element's id.
    pub(crate) id: String,
    /// Where the statement sits.
    pub(crate) position: ElementPosition,
    /// `vertex`, `edge`, `subgraph`, `connector`, or `attachment`.
    pub(crate) r#type: &'static str,
}

/// A finding about the diagram's meaning, recorded on the model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Diagnostic {
    /// The finding's id, such as `SHAPE_REMOVED`.
    pub(crate) id: &'static str,
    /// The finding, in words.
    pub(crate) message: String,
    /// The node the finding is about. Pele's `edgeId` is never set by a parser, so it is left out.
    pub(crate) node_id: Option<String>,
    /// Where the element it is about was first written.
    pub(crate) position: Option<ElementPosition>,
    /// `warning` or `error`.
    pub(crate) severity: &'static str,
}
