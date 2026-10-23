//! Cynefin diagrams: items sorted into the five Cynefin domains, with transitions between them.
//! Port of Pele's `src/diagrams/cynefin/index.ts`.

pub(crate) mod boundaries;
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

/// The `cynefin` diagram type.
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
#[cfg(feature = "model")]
pub(crate) fn to_json(model: &Model) -> String {
    let mut out = String::from("{\"type\":\"cynefin\"");
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
    //! Parity with Pele's `cynefin.parse` over every string literal of Pele's Cynefin compat
    //! specs and the documents the specs build at runtime, as recorded by running Pele's
    //! TypeScript.

    use super::fixture::cases;
    use super::parse;

    /// The parity fixture: Pele's parser input and its model JSON or error.
    const FIXTURE: &str = include_str!("../../tests/fixtures/cynefin-parse.json");

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
