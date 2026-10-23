//! Ishikawa diagrams: an effect and its causes, drawn as a fishbone. Port of Pele's
//! `src/diagrams/ishikawa/index.ts`.

pub(crate) mod db;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod render;
pub(crate) mod types;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::error::Error;
use crate::svg::root::RenderOptions;
use crate::util::url::UrlOptions;

/// The `ishikawa` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input, section: None };

fn render_input(input: &Input<'_>, options: &RenderOptions, _url: &UrlOptions<'_>) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render(&model, options))
}

/// The parsed Ishikawa model: Pele's `IshikawaDb`.
pub(crate) use db::IshikawaDb as Model;

/// Parses preprocessed Ishikawa `source`, as Pele's `ishikawa.parse` does. `title` comes from
/// front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    db.title = title.map(str::to_owned);
    parser::parse_ishikawa(source, &mut db)?;
    Ok(db)
}

/// Serializes `model` as `JSON.stringify` serializes this view of Pele's parsed `IshikawaDb`,
/// which the parity tests build on Pele's side. Nodes nest as in Pele.
///
/// ```ts
/// JSON.stringify({ type: db.type, title: db.title, root: db.root });
/// ```
pub(crate) fn to_json(model: &Model) -> String {
    let mut out = String::from("{\"type\":\"ishikawa\"");
    if let Some(title) = &model.title {
        out.push_str(",\"title\":");
        push_json_string(&mut out, title);
    }
    if model.root().is_some() {
        out.push_str(",\"root\":");
        // Each entry is a node to open, or `None` to close the node opened last.
        let mut stack: Vec<Option<usize>> = vec![Some(0)];
        while let Some(entry) = stack.pop() {
            match entry {
                None => out.push_str("]}"),
                Some(node) => {
                    let Some(node) = model.nodes.get(node) else {
                        continue;
                    };
                    if out.ends_with('}') {
                        out.push(',');
                    }
                    out.push_str("{\"text\":");
                    push_json_string(&mut out, &node.text);
                    out.push_str(",\"children\":[");
                    stack.push(None);
                    for &child in node.children.iter().rev() {
                        stack.push(Some(child));
                    }
                }
            }
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
