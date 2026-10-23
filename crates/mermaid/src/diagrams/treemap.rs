//! Treemaps: nested sections and valued leaves drawn as nested rectangles. Port of Pele's
//! `src/diagrams/treemap/index.ts`.

pub(crate) mod format;
pub(crate) mod layout;
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

/// The `treemap` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram {
    keep_comments: false,
    render: render_input,
    section: None,
};

fn render_input(
    input: &Input<'_>,
    options: &RenderOptions,
    _url: &UrlOptions<'_>,
) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render(&model, input.config, options))
}

/// The parsed treemap: Pele's `TreemapModel`.
pub(crate) use model::TreemapModel as Model;

/// Parses preprocessed treemap `source`, as Pele's `treemap.parse` does. `title` comes from
/// front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    Ok(model::build_treemap(parser::parse_treemap(source)?, title))
}

/// Serializes `model` as the parity tests serialize Pele's `TreemapModel`, with each `Map`
/// written as an array of its entries:
///
/// ```ts
/// JSON.stringify(model, (_, v) => (v instanceof Map ? [...v] : v));
/// ```
#[cfg(feature = "model")]
pub(crate) fn to_json(model: &Model) -> String {
    let mut out = String::from("{\"type\":\"treemap\"");
    for (key, value) in [
        ("title", &model.title),
        ("accTitle", &model.acc_title),
        ("accDescr", &model.acc_descr),
    ] {
        if let Some(value) = value {
            out.push_str(",\"");
            out.push_str(key);
            out.push_str("\":");
            push_json_string(&mut out, value);
        }
    }
    out.push_str(",\"roots\":");
    push_nodes(&mut out, &model.roots);
    out.push_str(",\"classes\":[");
    for (i, (name, styles)) in model.classes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('[');
        push_json_string(&mut out, name);
        out.push(',');
        push_strings(&mut out, styles);
        out.push(']');
    }
    out.push_str("]}");
    out
}

/// Appends `nodes` as `JSON.stringify` writes them.
#[cfg(feature = "model")]
fn push_nodes(out: &mut String, nodes: &[model::TreemapNode]) {
    use crate::yaml::push_number;

    out.push('[');
    for (i, node) in nodes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        push_json_string(out, &node.name);
        if let Some(children) = &node.children {
            out.push_str(",\"children\":");
            push_nodes(out, children);
        }
        if let Some(selector) = &node.class_selector {
            out.push_str(",\"classSelector\":");
            push_json_string(out, selector);
        }
        if let Some(styles) = &node.css_compiled_styles {
            out.push_str(",\"cssCompiledStyles\":");
            push_strings(out, styles);
        }
        if let Some(value) = node.value {
            out.push_str(",\"value\":");
            if value.is_finite() {
                push_number(out, value);
            } else {
                out.push_str("null");
            }
        }
        out.push('}');
    }
    out.push(']');
}

/// Appends `items` as a JSON array of strings.
#[cfg(feature = "model")]
fn push_strings(out: &mut String, items: &[String]) {
    out.push('[');
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_json_string(out, item);
    }
    out.push(']');
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
    //! Parity with Pele's `treemap.parse` over every string literal of Pele's treemap compat
    //! specs and the documents the specs build at runtime, as recorded by running Pele's
    //! TypeScript.

    use super::fixture::cases;
    use super::parse;

    /// The parity fixture: Pele's parser input and its model JSON or error.
    const FIXTURE: &str = include_str!("../../tests/fixtures/treemap-parse.json");

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
