//! The sankey model: nodes in order of first mention and the links between them. Port of Pele's
//! `src/diagrams/sankey/db.ts`.

use std::collections::BTreeMap;

/// A node, named by the rows that mention it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SankeyNode {
    /// The node's name.
    pub(crate) id: String,
    /// Its position in [`SankeyDb::nodes`].
    pub(crate) index: u32,
}

/// A flow from one node to another.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SankeyLink {
    /// Index of the source node.
    pub(crate) source: u32,
    /// Index of the target node.
    pub(crate) target: u32,
    /// The amount, as `parseFloat` read it. It may be `NaN`.
    pub(crate) value: f64,
}

/// The parsed sankey diagram: Pele's `SankeyDb`.
#[derive(Debug, Default)]
pub(crate) struct SankeyDb {
    /// The accessible description. Mermaid's sankey grammar has no statement that sets it.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title. Mermaid's sankey grammar has no statement that sets it.
    pub(crate) acc_title: Option<String>,
    /// Links in authored order.
    pub(crate) links: Vec<SankeyLink>,
    /// Node indexes by name. Only looked up, never iterated.
    lookup: BTreeMap<String, u32>,
    /// Nodes in order of first mention.
    pub(crate) nodes: Vec<SankeyNode>,
    /// The front matter title.
    pub(crate) title: Option<String>,
}

impl SankeyDb {
    /// The index of the node named `id`, created at the end when it is new.
    pub(crate) fn find_or_create_node(&mut self, id: String) -> u32 {
        if let Some(&index) = self.lookup.get(&id) {
            return index;
        }
        let index = u32::try_from(self.nodes.len()).unwrap_or(u32::MAX);
        self.lookup.insert(id.clone(), index);
        self.nodes.push(SankeyNode { id, index });
        index
    }

    /// Adds a link between two node indexes.
    pub(crate) fn add_link(&mut self, source: u32, target: u32, value: f64) {
        self.links.push(SankeyLink {
            source,
            target,
            value,
        });
    }
}
