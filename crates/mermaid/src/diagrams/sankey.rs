//! Sankey diagrams: flows between named nodes, drawn as bands between bars. Port of Pele's
//! `src/diagrams/sankey/index.ts`.

pub(crate) mod db;
pub(crate) mod layout;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod render;

/// The JSON reader of the parity fixtures.
#[cfg(test)]
#[path = "../../tests/fixtures/json.rs"]
pub(super) mod fixture;

use super::{Diagram, Input, Rendered};
use crate::error::Error;
use crate::svg::root::RenderOptions;
use crate::util::url::UrlOptions;

/// The `sankey` diagram type.
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
#[cfg(feature = "model")]
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
    //! Parity with Pele's `sankey.parse` over every string literal of Pele's sankey compat specs
    //! plus edge cases, as recorded by running Pele's TypeScript.

    use super::fixture::cases;
    use super::parse;

    /// The parity fixture: Pele's parser input and its model JSON or error.
    const FIXTURE: &str = include_str!("../../tests/fixtures/sankey-parse.json");

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
