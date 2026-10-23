//! Tree views: files and folders nested by indentation or box-drawing prefixes. Port of Pele's
//! `src/diagrams/treeview/index.ts`.

pub(crate) mod box_drawing;
pub(crate) mod icons;
pub(crate) mod model;
pub(crate) mod parser;
pub(crate) mod render;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::error::Error;
use crate::svg::root::RenderOptions;
use crate::util::url::UrlOptions;

/// The `treeView` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input, section: None };

/// Parses and draws the diagram.
fn render_input(input: &Input<'_>, options: &RenderOptions, _url: &UrlOptions<'_>) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render(&model, input.config, options))
}

/// The parsed tree view: Pele's `TreeViewModel`.
pub(crate) use model::TreeViewModel as Model;

/// Parses preprocessed tree view `source`, as Pele's `treeView.parse` does. `title` comes from
/// front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    Ok(model::build_tree_view(model::read_tree_view(source)?, title))
}

/// Serializes `model` as the parity tests serialize Pele's `TreeViewModel`: object keys sorted,
/// absent fields left out.
pub(crate) fn to_json(model: &Model) -> String {
    use crate::yaml::push_number;

    let mut out = String::from("{");
    if let Some(text) = &model.acc_descr {
        out.push_str("\"accDescr\":");
        push_json_string(&mut out, text);
        out.push(',');
    }
    if let Some(text) = &model.acc_title {
        out.push_str("\"accTitle\":");
        push_json_string(&mut out, text);
        out.push(',');
    }
    out.push_str("\"count\":");
    push_number(&mut out, f64::from(model.count));
    out.push_str(",\"root\":");

    // Each entry opens with its children and closes with its other fields, which sort after
    // `children`. The walk keeps its own stack, so deep trees do not overflow.
    let mut stack: Vec<(usize, usize)> = vec![(0, 0)];
    out.push_str("{\"children\":[");
    while let Some((index, next)) = stack.pop() {
        let Some(node) = model.node(index) else {
            continue;
        };
        if let Some(&child) = node.children.get(next) {
            if next > 0 {
                out.push(',');
            }
            stack.push((index, next + 1));
            stack.push((child, 0));
            out.push_str("{\"children\":[");
            continue;
        }
        out.push(']');
        if let Some(class) = &node.css_class {
            out.push_str(",\"cssClass\":");
            push_json_string(&mut out, class);
        }
        if let Some(text) = &node.description {
            out.push_str(",\"description\":");
            push_json_string(&mut out, text);
        }
        if let Some(icon) = &node.icon {
            out.push_str(",\"icon\":");
            push_json_string(&mut out, icon);
        }
        out.push_str(",\"id\":");
        push_number(&mut out, f64::from(node.id));
        out.push_str(",\"level\":");
        #[expect(clippy::cast_precision_loss, reason = "levels stay far below 2^52")]
        push_number(&mut out, node.level as f64);
        out.push_str(",\"name\":");
        push_json_string(&mut out, &node.name);
        out.push_str(",\"nodeType\":\"");
        out.push_str(node.node_type.as_str());
        out.push_str("\"}");
    }
    if let Some(title) = &model.title {
        out.push_str(",\"title\":");
        push_json_string(&mut out, title);
    }
    out.push_str(",\"type\":\"treeView\"}");
    out
}

/// Appends `text` as `JSON.stringify` quotes it.
fn push_json_string(out: &mut String, text: &str) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let code = u32::from(c) as usize;
                out.push_str("\\u00");
                out.push(char::from(HEX.get(code >> 4).copied().unwrap_or(b'0')));
                out.push(char::from(HEX.get(code & 15).copied().unwrap_or(b'0')));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
