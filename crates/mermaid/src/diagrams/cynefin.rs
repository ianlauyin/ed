//! Cynefin diagrams: items sorted into the five Cynefin domains, with transitions between them.
//! Port of Pele's `src/diagrams/cynefin/index.ts`.

pub(crate) mod boundaries;
pub(crate) mod model;
pub(crate) mod parser;
pub(crate) mod render;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::error::Error;
use crate::svg::root::RenderOptions;
use crate::util::url::UrlOptions;

/// The `cynefin` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input, section: None };

fn render_input(input: &Input<'_>, options: &RenderOptions, _url: &UrlOptions<'_>) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render(&model, input.config, options))
}

/// The parsed Cynefin diagram: Pele's `CynefinModel`.
pub(crate) use model::CynefinModel as Model;

/// Parses preprocessed Cynefin `source`, as Pele's `cynefin.parse` does. `title` comes from
/// front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    Ok(model::build_cynefin(parser::parse_cynefin(source)?, title))
}

/// Serializes `model` as the parity tests serialize Pele's `CynefinModel`, with each `Map`
/// written as an array of its entries:
///
/// ```ts
/// JSON.stringify(model, (_, v) => (v instanceof Map ? [...v] : v));
/// ```
pub(crate) fn to_json(model: &Model) -> String {
    let mut out = String::from("{\"type\":\"cynefin\"");
    for (key, value) in [("title", &model.title), ("accTitle", &model.acc_title), ("accDescr", &model.acc_descr)] {
        if let Some(value) = value {
            out.push_str(",\"");
            out.push_str(key);
            out.push_str("\":");
            push_json_string(&mut out, value);
        }
    }
    out.push_str(",\"domains\":[");
    for (i, domain) in model.domains.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('[');
        push_json_string(&mut out, domain.name);
        out.push_str(",{\"name\":");
        push_json_string(&mut out, domain.name);
        out.push_str(",\"items\":[");
        for (j, item) in domain.items.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            out.push_str("{\"label\":");
            push_json_string(&mut out, item);
            out.push('}');
        }
        out.push_str("]}]");
    }
    out.push_str("],\"transitions\":[");
    for (i, transition) in model.transitions.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"from\":");
        push_json_string(&mut out, transition.from);
        out.push_str(",\"to\":");
        push_json_string(&mut out, transition.to);
        if let Some(label) = &transition.label {
            out.push_str(",\"label\":");
            push_json_string(&mut out, label);
        }
        out.push('}');
    }
    out.push_str("]}");
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
