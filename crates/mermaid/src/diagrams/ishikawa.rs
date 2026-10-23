//! Ishikawa diagrams: an effect and its causes, drawn as a fishbone. Port of Pele's
//! `src/diagrams/ishikawa/index.ts`.

pub(crate) mod db;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod render;
pub(crate) mod types;

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

/// The `ishikawa` diagram type.
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
#[cfg(feature = "model")]
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
    //! Parity with Pele's `ishikawa.parse` over every string literal of Pele's ishikawa compat
    //! specs, the documents the specs build at runtime, and the vectors, as recorded by running
    //! Pele's TypeScript.

    use super::fixture::cases;
    use super::parse;

    /// The parity fixture: Pele's parser input and its model JSON or error.
    const FIXTURE: &str = include_str!("../../tests/fixtures/ishikawa-parse.json");

    #[test]
    fn matches_pele() {
        let mut differing = Vec::new();
        let cases = cases(FIXTURE);
        for case in &cases {
            let input = case.str("input").unwrap_or_default();
            let actual = parse(input, case.str("title"));
            let expected_error = case.get("error");
            let same = match (&actual, expected_error) {
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
