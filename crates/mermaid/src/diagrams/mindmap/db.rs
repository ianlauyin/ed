//! The mindmap database. Port of Pele's `src/diagrams/mindmap/db.ts`.

use crate::diagrams::common::outline::{Decoration, OutlineDb};
use crate::error::{Code, Error};

/// A mindmap node.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct MindmapNode {
    /// The indexes of the node's children, in source order.
    pub(crate) children: Vec<usize>,
    /// The class names from `:::`.
    pub(crate) class: Option<String>,
    /// The text.
    pub(crate) descr: String,
    /// The icon name from `::icon(...)`.
    pub(crate) icon: Option<String>,
    /// The node's index in [`MindmapDb::nodes`].
    pub(crate) id: usize,
    /// Whether the node is the root.
    pub(crate) is_root: bool,
    /// Mermaid's node type, which selects the shape.
    pub(crate) kind: u8,
    /// The indentation relative to the root's, in UTF-16 units. Negative when a node is indented
    /// less than the root.
    pub(crate) level: i64,
    /// The authored id.
    pub(crate) node_id: String,
    /// Index of the first-level branch the node belongs to, wrapping after eleven as in Mermaid.
    pub(crate) section: Option<usize>,
}

/// The parsed mindmap: Pele's `MindmapDb`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct MindmapDb {
    base_level: i64,
    /// The first error an action raised. Pele throws it, and the parser stops there.
    pub(crate) error: Option<Error>,
    /// In source order, which is depth-first. The first node is the root.
    pub(crate) nodes: Vec<MindmapNode>,
    /// The chain of nodes that can still take children, with strictly increasing levels.
    open: Vec<usize>,
    /// The front matter title.
    pub(crate) title: Option<String>,
}

const SECTIONS: usize = 11;

/// The error for a node that has no parent.
#[cold]
fn no_parent(descr: &str) -> Error {
    let mut error = Error::new(
        Code::Semantic,
        [
            "There can be only one root. No parent could be found for (\"",
            descr,
            "\")",
        ]
        .concat(),
    );
    error.r#type = Some("mindmap");
    error
}

impl OutlineDb for MindmapDb {
    fn add_node(&mut self, level: usize, id: &str, descr: &str, kind: u8, _: Option<&str>) {
        if self.error.is_some() {
            return;
        }

        let level = i64::try_from(level).unwrap_or(i64::MAX);
        let is_root = self.nodes.is_empty();
        if is_root {
            self.base_level = level;
        }
        let level = level - self.base_level;

        // Mermaid's parent is the last node with a smaller indentation, wherever it is in the tree.
        let mut depth = self.open.len();
        while depth > 0
            && self
                .open
                .get(depth - 1)
                .and_then(|&at| self.nodes.get(at))
                .is_some_and(|node| node.level >= level)
        {
            depth -= 1;
        }
        let parent = depth
            .checked_sub(1)
            .and_then(|at| self.open.get(at))
            .copied();
        if parent.is_none() && !is_root {
            self.error = Some(no_parent(descr));
            return;
        }

        let id_at = self.nodes.len();
        let mut node = MindmapNode {
            descr: descr.to_owned(),
            id: id_at,
            is_root,
            kind,
            level,
            node_id: id.to_owned(),
            ..MindmapNode::default()
        };
        if let Some(parent) = parent.and_then(|at| self.nodes.get_mut(at)) {
            node.section = if parent.is_root {
                Some(parent.children.len() % SECTIONS)
            } else {
                parent.section
            };
            parent.children.push(id_at);
        }
        self.open.truncate(depth);
        self.open.push(id_at);
        self.nodes.push(node);
    }

    // Mermaid throws a TypeError when a decoration comes before any node. Here it is ignored.
    fn decorate_node(&mut self, decoration: Decoration<'_>) {
        if self.error.is_some() {
            return;
        }
        let Some(node) = self.nodes.last_mut() else {
            return;
        };
        if let Some(icon) = decoration.icon.filter(|icon| !icon.is_empty()) {
            node.icon = Some(icon.to_owned());
        }
        if let Some(class) = decoration.class.filter(|class| !class.is_empty()) {
            node.class = Some(class.to_owned());
        }
    }
}
