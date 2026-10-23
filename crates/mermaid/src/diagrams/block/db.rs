//! The block diagram database: builds the block tree from parsed statements. Port of Pele's
//! `src/diagrams/block/db.ts`.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use super::types::{Block, BlockClassDef};
use crate::error::{Code, Error};
use crate::text::label::trim;

/// The most space blocks a diagram may hold.
const MAX_SPACES: f64 = 100_000.0;

/// The arena index of the root block.
pub(crate) const ROOT: usize = 0;

/// The block type a shape's brackets stand for.
pub(crate) fn type_str_to_type(type_str: Option<&str>) -> &'static str {
    match type_str {
        Some("[]") => "square",
        Some("()") => "round",
        Some("(())") => "circle",
        Some(">]") => "rect_left_inv_arrow",
        Some("{}") => "diamond",
        Some("{{}}") => "hexagon",
        Some("([])") => "stadium",
        Some("[[]]") => "subroutine",
        Some("[()]") => "cylinder",
        Some("((()))") => "doublecircle",
        Some("[//]") => "lean_right",
        Some("[\\\\]") => "lean_left",
        Some("[/\\]") => "trapezoid",
        Some("[\\/]") => "inv_trapezoid",
        Some("<[]>") => "block_arrow",
        _ => "na",
    }
}

/// The marker at the end of a link.
pub(crate) fn edge_str_to_edge_data(type_str: &str) -> &'static str {
    match trim(type_str).chars().next_back() {
        Some('x') => "arrow_cross",
        Some('o') => "arrow_circle",
        Some('>') => "arrow_point",
        _ => "",
    }
}

/// The marker at the start of a link.
pub(crate) fn edge_str_to_edge_start_data(type_str: &str) -> &'static str {
    match trim(type_str).chars().next() {
        Some('x') => "arrow_cross",
        Some('o') => "arrow_circle",
        Some('<') => "arrow_point",
        _ => "arrow_open",
    }
}

/// The line weight of a link.
pub(crate) fn edge_str_to_thickness(type_str: &str) -> &'static str {
    if type_str.contains("==") {
        "thick"
    } else {
        "normal"
    }
}

/// The line pattern of a link.
pub(crate) fn edge_str_to_pattern(type_str: &str) -> &'static str {
    if type_str.contains(".-") {
        "dotted"
    } else {
        "solid"
    }
}

/// A statement list being placed under its parent.
struct Frame {
    at: usize,
    children: Vec<usize>,
    list: Vec<usize>,
    parent: usize,
}

/// The parsed block diagram: Pele's `BlockDb`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BlockDb {
    /// Every block, the root at [`ROOT`]. Statements and dropped blocks stay here unreferenced.
    pub(crate) arena: Vec<Block>,
    /// Blocks by id, in insertion order. The root stays out.
    pub(crate) by_id: Vec<(String, usize)>,
    /// Positions in `by_id` by id.
    by_id_index: BTreeMap<String, usize>,
    /// Class definitions, in insertion order.
    pub(crate) classes: Vec<BlockClassDef>,
    colors: u32,
    edge_count: BTreeMap<String, u32>,
    /// Edges, by arena index, in order.
    pub(crate) edges: Vec<usize>,
    ids: u32,
    /// Ids that a class or style statement named before any block had them.
    pending: BTreeSet<String>,
    spaces: f64,
    /// The front matter title.
    pub(crate) title: Option<String>,
}

impl Default for BlockDb {
    fn default() -> Self {
        Self {
            arena: vec![Block {
                children: Some(Vec::new()),
                columns: Some(-1.0),
                id: Some("root".to_owned()),
                r#type: "composite",
                ..Block::default()
            }],
            by_id: Vec::new(),
            by_id_index: BTreeMap::new(),
            classes: Vec::new(),
            colors: 0,
            edge_count: BTreeMap::new(),
            edges: Vec::new(),
            ids: 0,
            pending: BTreeSet::new(),
            spaces: 0.0,
            title: None,
        }
    }
}

impl BlockDb {
    /// Adds a block to the arena and returns its index.
    pub(crate) fn add(&mut self, block: Block) -> usize {
        self.arena.push(block);
        self.arena.len() - 1
    }

    /// The block with `id`, by arena index.
    pub(crate) fn get(&self, id: &str) -> Option<usize> {
        self.by_id_index
            .get(id)
            .and_then(|&at| self.by_id.get(at))
            .map(|&(_, index)| index)
    }

    /// The children of a block, by arena index.
    pub(crate) fn children(&self, index: usize) -> &[usize] {
        self.arena
            .get(index)
            .and_then(|block| block.children.as_deref())
            .unwrap_or(&[])
    }

    /// The class definition named `id`.
    pub(crate) fn class(&self, id: &str) -> Option<&BlockClassDef> {
        self.classes.iter().find(|class| class.id == id)
    }

    /// `Map.set`: replaces in place, or appends.
    fn set(&mut self, id: String, index: usize) {
        if let Some(&at) = self.by_id_index.get(&id) {
            if let Some(entry) = self.by_id.get_mut(at) {
                entry.1 = index;
            }
            return;
        }
        self.by_id_index.insert(id.clone(), self.by_id.len());
        self.by_id.push((id, index));
    }

    /// Mermaid's ids are random. These are counted, so the same text always gives the same output.
    pub(crate) fn generate_id(&mut self) -> String {
        self.ids += 1;
        let mut id = String::from("id-");
        crate::error::push_u32(&mut id, self.ids);
        id
    }

    fn add_style_class(&mut self, id: &str, style_attributes: &str) {
        let at = if let Some(at) = self.classes.iter().position(|class| class.id == id) {
            at
        } else {
            self.classes.push(BlockClassDef {
                id: id.to_owned(),
                ..BlockClassDef::default()
            });
            self.classes.len() - 1
        };
        let Some(found) = self.classes.get_mut(at) else {
            return;
        };
        for attribute in style_attributes.split(',') {
            let fixed = match attribute.find(';') {
                Some(semi) => {
                    let mut joined = attribute.get(..semi).unwrap_or("").to_owned();
                    joined.push_str(attribute.get(semi + 1..).unwrap_or(""));
                    trim(&joined).to_owned()
                }
                None => trim(attribute).to_owned(),
            };
            if attribute.contains("color") {
                found.text_styles.push(
                    fixed
                        .replacen("fill", "bgFill", 1)
                        .replacen("color", "fill", 1),
                );
            }
            found.styles.push(fixed);
        }
    }

    fn add_style_to_node(&mut self, id: &str, styles: &str) {
        let index = self.named(id);
        if let Some(block) = self.arena.get_mut(index) {
            block.styles = Some(styles.split(',').map(str::to_owned).collect());
        }
    }

    /// Mermaid looks each id up before trimming it, so `class a, b hot` misses b. Here it is
    /// trimmed first.
    fn set_css_class(&mut self, item_ids: &str, css_class_name: &str) {
        for id in item_ids.split(',') {
            let index = self.named(trim(id));
            if let Some(block) = self.arena.get_mut(index) {
                block
                    .classes
                    .get_or_insert_with(Vec::new)
                    .push(css_class_name.to_owned());
            }
        }
    }

    /// Mermaid throws on a style for an unknown id, and a class for one makes the block of that id
    /// disappear when it is declared later. Here both wait for the block.
    fn named(&mut self, id: &str) -> usize {
        if let Some(index) = self.get(id) {
            return index;
        }
        let index = self.add(Block {
            children: Some(Vec::new()),
            id: Some(id.to_owned()),
            r#type: "na",
            ..Block::default()
        });
        self.set(id.to_owned(), index);
        self.pending.insert(id.to_owned());
        index
    }

    /// Places parsed statements, by arena index, under the root.
    ///
    /// # Errors
    ///
    /// Returns a limit error past [`MAX_SPACES`] space blocks.
    pub(crate) fn set_hierarchy(&mut self, statements: Vec<usize>) -> Result<(), Error> {
        let mut stack = vec![Frame {
            at: 0,
            children: Vec::new(),
            list: statements,
            parent: ROOT,
        }];
        while let Some(f) = stack.last_mut() {
            let Some(&b) = f.list.get(f.at) else {
                let children = std::mem::take(&mut f.children);
                let parent = f.parent;
                if let Some(block) = self.arena.get_mut(parent) {
                    block.children = Some(children);
                }
                stack.pop();
                continue;
            };
            f.at += 1;
            let parent = f.parent;
            let Some(mut block) = self.arena.get(b).cloned() else {
                continue;
            };

            match block.r#type {
                "classDef" => {
                    let id = block.id.unwrap_or_default();
                    self.add_style_class(&id, block.css.as_deref().unwrap_or(""));
                    continue;
                }
                "applyClass" => {
                    let id = block.id.unwrap_or_default();
                    self.set_css_class(&id, block.style_class.as_deref().unwrap_or(""));
                    continue;
                }
                "applyStyles" => {
                    if let Some(styles) = block.styles_str.as_deref().filter(|s| !s.is_empty()) {
                        let id = block.id.unwrap_or_default();
                        self.add_style_to_node(&id, styles);
                    }
                    continue;
                }
                "column-setting" => {
                    if let Some(parent) = self.arena.get_mut(parent) {
                        parent.columns = Some(block.columns.unwrap_or(-1.0));
                    }
                    continue;
                }
                "edge" => {
                    let id = block.id.unwrap_or_default();
                    let count = self.edge_count.get(&id).copied().unwrap_or(0) + 1;
                    self.edge_count.insert(id.clone(), count);
                    let mut counted = String::new();
                    crate::error::push_u32(&mut counted, count);
                    counted.push('-');
                    counted.push_str(&id);
                    if let Some(edge) = self.arena.get_mut(b) {
                        edge.id = Some(counted);
                    }
                    self.edges.push(b);
                    continue;
                }
                _ => {}
            }

            // A composite whose header was a link statement has no id of its own.
            let id = match block.id.take() {
                Some(id) => id,
                None => self.generate_id(),
            };
            if block.label.as_deref().is_none_or(str::is_empty) {
                block.label = Some(if block.r#type == "composite" {
                    String::new()
                } else {
                    id.clone()
                });
            }
            block.id = Some(id.clone());
            let known = self.get(&id);
            let fresh = known.is_none() || self.pending.remove(&id);
            if let (false, Some(k)) = (fresh, known) {
                if let Some(known) = self.arena.get_mut(k) {
                    if block.r#type != "na" {
                        known.r#type = block.r#type;
                    }
                    if block.label.as_deref() != Some(id.as_str()) {
                        known.label.clone_from(&block.label);
                    }
                }
            } else {
                if let Some(known) = known.and_then(|k| self.arena.get(k)) {
                    block.classes.clone_from(&known.classes);
                    block.styles.clone_from(&known.styles);
                }
                if block.r#type == "composite" {
                    block.color_index = Some(self.colors);
                    self.colors += 1;
                }
                self.set(id.clone(), b);
            }

            let inner = block.children.replace(Vec::new()).unwrap_or_default();
            let is_space = block.r#type == "space";
            let width = block.width.unwrap_or(1.0);
            if let Some(slot) = self.arena.get_mut(b) {
                *slot = block;
            }

            let mut placed = Vec::new();
            if is_space {
                if !matches!(
                    width.partial_cmp(&(MAX_SPACES - self.spaces)),
                    Some(Ordering::Less | Ordering::Equal)
                ) {
                    let mut error = Error::new(
                        Code::Limit,
                        "A block diagram can hold at most 100000 space blocks.",
                    );
                    error.r#type = Some("block");
                    return Err(error);
                }
                self.spaces += width.max(0.0);
                let mut j = 0_u32;
                while f64::from(j) < width {
                    let mut copy = self.arena.get(b).cloned().unwrap_or_default();
                    let mut copy_id = id.clone();
                    copy_id.push('-');
                    crate::error::push_u32(&mut copy_id, j);
                    copy.id = Some(copy_id.clone());
                    copy.children = Some(Vec::new());
                    let index = self.add(copy);
                    self.set(copy_id, index);
                    placed.push(index);
                    j += 1;
                }
            } else if fresh {
                placed.push(b);
            }
            if let Some(f) = stack.last_mut() {
                f.children.extend(placed);
            }
            if !inner.is_empty() {
                stack.push(Frame {
                    at: 0,
                    children: Vec::new(),
                    list: inner,
                    parent: b,
                });
            }
        }
        Ok(())
    }
}
