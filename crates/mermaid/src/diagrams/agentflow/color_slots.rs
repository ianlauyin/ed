//! Palette slots for nodes and containers. Port of Pele's `src/diagrams/agentflow/colorSlots.ts`.
//!
//! Two rules, as in Mermaid: a node takes the color of its kind, from a fixed slot, so editing the
//! diagram around it never recolors it. Containers count through the slots above the kinds, in
//! the order they were declared, so a frame does not take the color of a node inside it.

use std::collections::BTreeMap;

use super::graph::GraphNode;

/// Each kind's slot. Slot order is observable, so the list keeps Pele's order.
pub(crate) const KIND_SLOT: [(&str, u32); 7] = [
    ("tool", 0),
    ("task", 1),
    ("decision", 2),
    ("input", 3),
    ("refdoc", 4),
    ("connector", 5),
    ("action", 6),
];

/// The number of kinds.
pub(crate) const KIND_COUNT: u32 = 7;

/// The slot of `kind`.
pub(crate) fn kind_slot(kind: &str) -> Option<u32> {
    KIND_SLOT
        .iter()
        .find(|(name, _)| *name == kind)
        .map(|&(_, slot)| slot)
}

/// The slot of the `n`th container. At least one slot is kept for containers, so that a palette
/// shorter than the kinds still gives them one.
fn container_slot(n: u32, palette_length: u32) -> u32 {
    let count = palette_length.saturating_sub(KIND_COUNT).max(1);
    let slot = KIND_COUNT + n % count;
    if palette_length > 0 {
        slot % palette_length
    } else {
        slot
    }
}

/// Gives every container its slot and every node its kind. `kind_of` names a node's kind, and
/// `container_order` numbers the containers in the order they were written.
pub(crate) fn assign_color_slots(
    nodes: &mut [GraphNode],
    kind_of: impl Fn(&str) -> Option<&'static str>,
    container_order: &BTreeMap<String, u32>,
    palette_length: u32,
) {
    let mut fallback = u32::try_from(container_order.len()).unwrap_or(u32::MAX);
    for node in nodes {
        // A collapsed container is drawn as a node but keeps its slot, so collapsing one does not
        // recolor the containers after it.
        if node.is_group || node.shape == "collapsedGroup" {
            let n = if let Some(&n) = container_order.get(&node.id) {
                n
            } else {
                let n = fallback;
                fallback = fallback.saturating_add(1);
                n
            };
            node.color_index = Some(container_slot(n, palette_length));
            continue;
        }
        if let Some(kind) = kind_of(&node.id).filter(|kind| kind_slot(kind).is_some()) {
            node.kind = Some(kind);
        }
    }
}
