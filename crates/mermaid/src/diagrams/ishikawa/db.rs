//! The Ishikawa diagram database. Port of Pele's `src/diagrams/ishikawa/db.ts`.

use super::types::IshikawaNode;

/// The parsed Ishikawa diagram: Pele's `IshikawaDb`. The root is the effect, its children are the
/// categories of causes, theirs the causes, and so on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct IshikawaDb {
    base_level: Option<usize>,
    /// Every node. The first one, when there is one, is the root.
    pub(crate) nodes: Vec<IshikawaNode>,
    /// The line of ancestors of the last node, each with its level.
    stack: Vec<(usize, usize)>,
    /// The title from front matter.
    pub(crate) title: Option<String>,
}

impl IshikawaDb {
    /// The root, the effect, when there is one.
    pub(crate) fn root(&self) -> Option<&IshikawaNode> {
        self.nodes.first()
    }

    /// Adds a line. The first line is the effect. Every later line is a cause, nested by its
    /// indentation relative to the first cause, so the effect itself may be indented more than
    /// the causes.
    pub(crate) fn add_node(&mut self, raw_level: usize, text: String) {
        let index = self.nodes.len();
        self.nodes.push(IshikawaNode {
            children: Vec::new(),
            text,
        });
        if index == 0 {
            self.stack = vec![(0, 0)];
            return;
        }

        let base = *self.base_level.get_or_insert(raw_level);
        // Pele's `Math.max(rawLevel - baseLevel + 1, 1)`, in unsigned arithmetic.
        let level = (raw_level + 1).saturating_sub(base).max(1);
        while self.stack.len() > 1 && self.stack.last().is_some_and(|&(own, _)| own >= level) {
            self.stack.pop();
        }
        if let Some(&(_, parent)) = self.stack.last()
            && let Some(parent) = self.nodes.get_mut(parent)
        {
            parent.children.push(index);
        }
        self.stack.push((level, index));
    }
}
