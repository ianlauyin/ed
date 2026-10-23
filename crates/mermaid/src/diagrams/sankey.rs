//! Sankey diagrams: flows between named nodes, drawn as bands between bars. Port of Pele's
//! `src/diagrams/sankey/index.ts`.

pub(crate) mod db;
pub(crate) mod layout;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod render;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::error::Error;
use crate::svg::root::RenderOptions;
use crate::util::url::UrlOptions;

/// The `sankey` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input, section: None };

fn render_input(input: &Input<'_>, options: &RenderOptions, _url: &UrlOptions<'_>) -> Result<Rendered, Error> {
    let db = parse(input.source, input.title)?;
    render::render(&db, input.config, options)
}

/// The parsed sankey model: Pele's `SankeyDb`.
pub(crate) use db::SankeyDb as Model;

/// Parses preprocessed sankey `source`, as Pele's `sankey.parse` does. `title` comes from front
/// matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        db.title = Some(title.to_owned());
    }
    parser::parse_sankey(&parser::prepare_text_for_parsing(source), &mut db)?;
    Ok(db)
}

/// Serializes `model` as `JSON.stringify` serializes this view of Pele's parsed `SankeyDb`, which
/// the parity tests build on Pele's side:
///
/// ```ts
/// JSON.stringify({
///   type: db.type,
///   nodes: db.nodes.map((n) => n.id),
///   links: db.links.map((l) => ({ source: l.source.index, target: l.target.index, value: l.value })),
///   title: db.title, accTitle: db.accTitle, accDescr: db.accDescr,
/// });
/// ```
pub(crate) fn to_json(model: &Model) -> String {
    use crate::yaml::push_number;

    let mut out = String::from("{\"type\":\"sankey\",\"nodes\":[");
    for (i, node) in model.nodes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_json_string(&mut out, &node.id);
    }
    out.push_str("],\"links\":[");
    for (i, link) in model.links.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"source\":");
        push_number(&mut out, f64::from(link.source));
        out.push_str(",\"target\":");
        push_number(&mut out, f64::from(link.target));
        out.push_str(",\"value\":");
        if link.value.is_finite() {
            push_number(&mut out, link.value);
        } else {
            out.push_str("null");
        }
        out.push('}');
    }
    out.push(']');
    for (key, value) in [("title", &model.title), ("accTitle", &model.acc_title), ("accDescr", &model.acc_descr)] {
        if let Some(value) = value {
            out.push_str(",\"");
            out.push_str(key);
            out.push_str("\":");
            push_json_string(&mut out, value);
        }
    }
    out.push('}');
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
