//! Tree views: files and folders nested by indentation or box-drawing prefixes. Port of Pele's
//! `src/diagrams/treeview/index.ts`.

pub(crate) mod box_drawing;
pub(crate) mod icons;
pub(crate) mod model;
pub(crate) mod parser;
pub(crate) mod render;

/// The JSON reader of the parity fixtures, loaded once per build.
#[cfg(all(test, not(feature = "sankey")))]
#[path = "../../tests/fixtures/json.rs"]
mod fixture;
#[cfg(all(test, feature = "sankey"))]
use super::sankey::fixture;

use super::{Diagram, Input, Rendered};
use crate::error::Error;
use crate::svg::root::RenderOptions;
use crate::util::url::UrlOptions;

/// The `treeView` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram {
    keep_comments: false,
    render: render_input,
    section: None,
};

/// Parses and draws the diagram.
fn render_input(
    input: &Input<'_>,
    options: &RenderOptions,
    _url: &UrlOptions<'_>,
) -> Result<Rendered, Error> {
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
    Ok(model::build_tree_view(
        model::read_tree_view(source)?,
        title,
    ))
}

/// Serializes `model` as the parity tests serialize Pele's `TreeViewModel`: object keys sorted,
/// absent fields left out.
#[cfg(feature = "model")]
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
#[cfg(feature = "model")]
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

#[cfg(test)]
mod tests {
    //! Parity with Pele's `treeView.parse` over every string literal of Pele's tree view compat
    //! specs and the documents the specs build at runtime, as recorded by running Pele's
    //! TypeScript.

    use super::fixture::cases;
    use super::parse;

    /// The parity fixture: Pele's parser input and its model JSON or error.
    const FIXTURE: &str = include_str!("../../tests/fixtures/treeview-parse.json");

    #[test]
    fn matches_pele() {
        let mut differing = Vec::new();
        let cases = cases(FIXTURE);
        for case in &cases {
            let input = case.str("input").unwrap_or_default();
            let actual = parse(input, case.str("title"));
            let same = match (&actual, case.get("error")) {
                (Ok(_), None) => {
                    #[cfg(feature = "model")]
                    {
                        actual.as_ref().map(super::to_json).ok().as_deref() == case.str("model")
                    }
                    #[cfg(not(feature = "model"))]
                    {
                        true
                    }
                }
                (Err(error), Some(expected)) => {
                    Some(error.code.as_str()) == expected.str("code")
                        && Some(error.message.as_str()) == expected.str("message")
                        && Some(f64::from(error.line)) == expected.num("line")
                        && Some(f64::from(error.column)) == expected.num("column")
                }
                _ => false,
            };
            if !same {
                differing.push((case.str("origin").unwrap_or("?"), input, actual));
            }
        }
        assert!(
            differing.is_empty(),
            "{} of {} cases differ from Pele: {:#?}",
            differing.len(),
            cases.len(),
            differing.iter().take(10).collect::<Vec<_>>()
        );
    }
}
