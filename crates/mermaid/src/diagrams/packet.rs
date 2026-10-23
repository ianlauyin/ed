//! Packet diagrams: protocol fields drawn as rows of bits. Port of Pele's
//! `src/diagrams/packet/index.ts`.

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
use crate::yaml::Value;

/// The `packet` diagram type.
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
    let model = parse(input.source, input.config, input.title)?;
    Ok(render::render(&model, input.config, options))
}

/// The parsed packet diagram: Pele's `PacketModel`.
pub(crate) use model::PacketModel as Model;

/// Parses preprocessed packet `source`, as Pele's `packet.parse` does, with the `packet` section
/// of `config`. `title` comes from front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for fields that do not
/// follow each other or have no bits.
pub(crate) fn parse(source: &str, config: &Value, title: Option<&str>) -> Result<Model, Error> {
    model::build_packet(parser::parse_packet(source)?, config, title)
}

/// Serializes `model` as `JSON.stringify` serializes Pele's `PacketModel`.
#[cfg(feature = "model")]
pub(crate) fn to_json(model: &Model) -> String {
    use crate::yaml::push_number;

    let number = |out: &mut String, value: f64| {
        if value.is_finite() {
            push_number(out, value);
        } else {
            out.push_str("null");
        }
    };
    let mut out = String::from("{\"type\":\"packet\"");
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
    out.push_str(",\"bitsPerRow\":");
    number(&mut out, model.bits_per_row);
    out.push_str(",\"rows\":[");
    for (i, row) in model.rows.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('[');
        for (j, block) in row.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            out.push_str("{\"start\":");
            number(&mut out, block.start);
            out.push_str(",\"end\":");
            number(&mut out, block.end);
            out.push_str(",\"bits\":");
            number(&mut out, block.bits);
            out.push_str(",\"label\":");
            push_json_string(&mut out, &block.label);
            out.push('}');
        }
        out.push(']');
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
    //! Parity with Pele's `packet.parse` over every string literal of Pele's packet compat specs
    //! and the documents the specs build at runtime, as recorded by running Pele's TypeScript.

    use super::fixture::cases;
    use super::parse;
    use crate::yaml::Value;

    /// The parity fixture: Pele's parser input and its model JSON or error.
    const FIXTURE: &str = include_str!("../../tests/fixtures/packet-parse.json");

    #[test]
    fn matches_pele() {
        let mut differing = Vec::new();
        let cases = cases(FIXTURE);
        let config = Value::Object(Vec::new());
        for case in &cases {
            let input = case.str("input").unwrap_or_default();
            let actual = parse(input, &config, case.str("title"));
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
