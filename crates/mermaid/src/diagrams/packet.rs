//! Packet diagrams: protocol fields drawn as rows of bits. Port of Pele's
//! `src/diagrams/packet/index.ts`.

pub(crate) mod model;
pub(crate) mod parser;
pub(crate) mod render;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::error::Error;
use crate::svg::root::RenderOptions;
use crate::util::url::UrlOptions;
use crate::yaml::Value;

/// The `packet` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input, section: None };

fn render_input(input: &Input<'_>, options: &RenderOptions, _url: &UrlOptions<'_>) -> Result<Rendered, Error> {
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
    for (key, value) in [("title", &model.title), ("accTitle", &model.acc_title), ("accDescr", &model.acc_descr)] {
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
