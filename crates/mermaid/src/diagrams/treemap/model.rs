//! The treemap model: rows nested into sections by indentation, with class styles. Port of
//! Pele's `src/diagrams/treemap/model.ts`.

use super::parser::{RowAst, TreemapAst};
use crate::text::label::trim_start;
use crate::util::chars::is_space;

/// A section or a leaf: Pele's `TreemapNode`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TreemapNode {
    /// Sections have a list, which may be empty. Leaves have none.
    pub(crate) children: Option<Vec<TreemapNode>>,
    /// The class after `:::`.
    pub(crate) class_selector: Option<String>,
    /// The declarations of that class, when it has any.
    pub(crate) css_compiled_styles: Option<Vec<String>>,
    /// The name.
    pub(crate) name: String,
    /// A leaf's value.
    pub(crate) value: Option<f64>,
}

/// A row ready to nest: Pele's `TreemapItem`.
struct TreemapItem {
    class_selector: Option<String>,
    css_compiled_styles: Option<Vec<String>>,
    leaf: bool,
    level: usize,
    name: String,
    value: Option<f64>,
}

/// A treemap ready to draw: Pele's `TreemapModel`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TreemapModel {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    /// Declarations by class name, in first-seen order.
    pub(crate) classes: Vec<(String, Vec<String>)>,
    /// The top-level nodes.
    pub(crate) roots: Vec<TreemapNode>,
    /// The title.
    pub(crate) title: Option<String>,
}

/// `text.replace(/\n\s+/g, '\n')`.
fn dedent_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\n' {
            while chars.next_if(|&c| is_space(u32::from(c))).is_some() {}
        }
    }
    out
}

/// `text` with each `from` replaced by `to`.
fn replace_all(text: &str, from: &str, to: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(from) {
        out.push_str(rest.get(..at).unwrap_or(""));
        out.push_str(to);
        rest = rest.get(at + from.len()..).unwrap_or("");
    }
    out.push_str(rest);
    out
}

/// Nests `items`: a row belongs to the nearest section above it that is indented less.
fn build_hierarchy(items: Vec<TreemapItem>) -> Vec<TreemapNode> {
    // Open sections, innermost last, each with its level. A section moves into its parent once
    // a row at its level or above closes it.
    let mut roots = Vec::new();
    let mut stack: Vec<(TreemapNode, usize)> = Vec::new();
    let close = |stack: &mut Vec<(TreemapNode, usize)>, roots: &mut Vec<TreemapNode>| {
        if let Some((node, _)) = stack.pop() {
            match stack.last_mut() {
                Some((parent, _)) => parent.children.get_or_insert_with(Vec::new).push(node),
                None => roots.push(node),
            }
        }
    };

    for item in items {
        let node = TreemapNode {
            children: (!item.leaf).then(Vec::new),
            class_selector: item.class_selector,
            css_compiled_styles: item.css_compiled_styles,
            name: item.name,
            value: if item.leaf { item.value } else { None },
        };
        while stack.last().is_some_and(|(_, level)| *level >= item.level) {
            close(&mut stack, &mut roots);
        }
        if item.leaf {
            match stack.last_mut() {
                Some((parent, _)) => parent.children.get_or_insert_with(Vec::new).push(node),
                None => roots.push(node),
            }
        } else {
            stack.push((node, item.level));
        }
    }
    while !stack.is_empty() {
        close(&mut stack, &mut roots);
    }
    roots
}

/// Builds the model from `ast`, as Pele's `buildTreemap` does. `title` comes from front matter.
pub(crate) fn build_treemap(ast: TreemapAst, title: Option<&str>) -> TreemapModel {
    let mut classes: Vec<(String, Vec<String>)> = Vec::new();
    for row in &ast.rows {
        let RowAst::ClassDef {
            class_name,
            style_text,
            ..
        } = row
        else {
            continue;
        };
        let at = if let Some(at) = classes.iter().position(|(name, _)| name == class_name) {
            at
        } else {
            classes.push((class_name.clone(), Vec::new()));
            classes.len() - 1
        };
        // An escaped comma stays in the value; the others separate declarations.
        let text = replace_all(style_text.as_deref().unwrap_or(""), "\\,", "§§§");
        let text = replace_all(&text, ",", ";");
        let text = replace_all(&text, "§§§", ",");
        if let Some((_, styles)) = classes.get_mut(at) {
            styles.extend(text.split(';').map(str::to_owned));
        }
    }

    let mut items = Vec::new();
    for row in ast.rows {
        let RowAst::Item { indent, item } = row else {
            continue;
        };
        let styles = item.class_selector.as_ref().and_then(|selector| {
            classes
                .iter()
                .find(|(name, _)| name == selector)
                .map(|(_, styles)| styles)
        });
        items.push(TreemapItem {
            class_selector: item.class_selector.clone(),
            css_compiled_styles: styles.filter(|styles| !styles.is_empty()).cloned(),
            leaf: item.value.is_some(),
            level: indent.unwrap_or(0),
            name: item.name,
            value: item.value,
        });
    }

    TreemapModel {
        acc_descr: ast
            .acc_descr
            .filter(|text| !text.is_empty())
            .map(|text| dedent_lines(&text)),
        acc_title: ast
            .acc_title
            .filter(|text| !text.is_empty())
            .map(|text| trim_start(&text).to_owned()),
        classes,
        roots: build_hierarchy(items),
        title: ast
            .title
            .filter(|text| !text.is_empty())
            .or_else(|| title.map(str::to_owned)),
    }
}
