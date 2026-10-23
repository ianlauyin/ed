//! The shapes an agentflow draws, their author-facing aliases, and the reduction of every other
//! shape to the default. Port of Pele's `src/diagrams/agentflow/shapes.ts`.

use super::db::AgentflowDb;
use super::graph::GraphNode;

/// Author-facing names for the shapes that carry meaning in an agentflow.
const SHAPE_ALIASES: [(&str, &str); 7] = [
    ("task", "roundedRect"),
    ("tool", "subroutine"),
    ("input", "lean-right"),
    ("decision", "diamond"),
    ("refdoc", "lin-doc"),
    ("action", "hexagon"),
    ("round", "rect"),
];

/// Shapes agentflow removed in v0.8.1.
const REMOVED_SHAPES: [&str; 27] = [
    "doc",
    "stadium",
    "terminal",
    "circle",
    "trapezoid",
    "inv_trapezoid",
    "inv-trapezoid",
    "doublecircle",
    "double-circle",
    "typeDeclaration",
    "procs",
    "lean_left",
    "lean-left",
    "in-out",
    "cylinder",
    "ellipse",
    "odd",
    "tag-rect",
    "tagged-rectangle",
    "delay",
    "half-rounded-rectangle",
    "lin-rect",
    "lined-rectangle",
    "win-pane",
    "window-pane",
    "curv-trap",
    "curved-trapezoid",
];

/// Shapes agentflow draws.
const ALLOWED_SHAPES: [&str; 13] = [
    "roundedRect",
    "subroutine",
    "subprocess",
    "subproc",
    "framed-rectangle",
    "lean-right",
    "diamond",
    "lin-doc",
    "lined-document",
    "hexagon",
    "hex",
    "connector",
    "collapsedGroup",
];

/// The shape of a node that names none, or one agentflow does not draw.
pub(crate) const DEFAULT_SHAPE: &str = "roundedRect";

/// The shape an alias stands for, or `shape` itself.
pub(crate) fn resolve_shape_alias(shape: &str) -> &str {
    SHAPE_ALIASES
        .iter()
        .find(|(alias, _)| *alias == shape)
        .map_or(shape, |(_, resolved)| resolved)
}

/// Whether `name` is a shape. Mermaid looks a shape name up with `in` on a plain object, so
/// `constructor` passes there too.
pub(crate) fn is_valid_shape(name: &str) -> bool {
    crate::diagrams::flowchart::shapes::is_valid_shape(name) || name == "constructor"
}

/// Reduces every node's shape to one agentflow draws, reporting the removed and unsupported ones.
pub(crate) fn normalise_node_shapes(nodes: &mut [GraphNode], db: &mut AgentflowDb) {
    for node in nodes {
        if node.is_group {
            continue;
        }
        if node.shape.is_empty() || node.shape == "squareRect" || node.shape == "rect" {
            DEFAULT_SHAPE.clone_into(&mut node.shape);
        }

        if REMOVED_SHAPES.contains(&node.shape.as_str()) {
            let mut message = String::from("shape \"");
            message.push_str(&node.shape);
            message.push_str("\" was removed in v0.8.1, using \"");
            message.push_str(DEFAULT_SHAPE);
            message.push('"');
            db.emit("SHAPE_REMOVED", "error", message, &node.id);
            DEFAULT_SHAPE.clone_into(&mut node.shape);
        } else if !ALLOWED_SHAPES.contains(&node.shape.as_str()) {
            let mut message = String::from("shape \"");
            message.push_str(&node.shape);
            message.push_str("\" is not supported, using \"");
            message.push_str(DEFAULT_SHAPE);
            message.push('"');
            db.emit("SHAPE_UNSUPPORTED", "warning", message, &node.id);
            DEFAULT_SHAPE.clone_into(&mut node.shape);
        }
    }
}
