//! Requirement diagrams: requirements and elements with their fields, joined by relationships.
//! Port of Pele's `src/diagrams/requirement/index.ts`.

pub(crate) mod db;
pub(crate) mod graph;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod render;
pub(crate) mod tokens;
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

/// The `requirement` diagram type.
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
    Ok(render::render(&db, options))
}

/// The parsed requirement model: Pele's `RequirementDb`.
pub(crate) use db::RequirementDb as Model;

/// Parses preprocessed requirement `source`, as Pele's `requirement.parse` does. `title` comes
/// from front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a limit error when classes copy too
/// many style declarations.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        db.title = Some(title.to_owned());
    }
    parser::parse_requirement(source, &mut db)?;
    Ok(db)
}

/// Serializes `model` as `JSON.stringify` serializes this view of Pele's parsed
/// `RequirementDb`, which the parity tests build on Pele's side. Maps become arrays of their
/// values, whose keys repeat the records' names.
///
/// ```ts
/// const pick = (o, keys) => Object.fromEntries(keys.map((k) => [k, o[k]]));
/// const REQ = ['name', 'type', 'requirementId', 'text', 'risk', 'verifyMethod', 'cssStyles', 'classes'];
/// const EL = ['name', 'type', 'docRef', 'cssStyles', 'classes'];
/// JSON.stringify({
///   type: db.type, direction: db.direction,
///   requirements: [...db.requirements.values()].map((r) => pick(r, REQ)),
///   elements: [...db.elements.values()].map((e) => pick(e, EL)),
///   relations: db.relations.map((r) => pick(r, ['type', 'src', 'dst'])),
///   classes: [...db.classes.values()].map((c) => pick(c, ['id', 'styles', 'textStyles'])),
///   title: db.title, accTitle: db.accTitle, accDescr: db.accDescr,
/// });
/// ```
#[cfg(feature = "model")]
pub(crate) fn to_json(model: &Model) -> String {
    let mut out = String::from("{\"type\":\"requirement\",\"direction\":");
    push_json_string(&mut out, model.direction);

    out.push_str(",\"requirements\":[");
    for (i, r) in model.requirements.values.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_fields(
            &mut out,
            &[
                ("name", &r.name),
                ("type", &r.r#type),
                ("requirementId", &r.requirement_id),
                ("text", &r.text),
                ("risk", &r.risk),
                ("verifyMethod", &r.verify_method),
            ],
        );
        push_lists(
            &mut out,
            &[("cssStyles", &r.css_styles), ("classes", &r.classes)],
        );
    }

    out.push_str("],\"elements\":[");
    for (i, e) in model.elements.values.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_fields(
            &mut out,
            &[
                ("name", &e.name),
                ("type", &e.r#type),
                ("docRef", &e.doc_ref),
            ],
        );
        push_lists(
            &mut out,
            &[("cssStyles", &e.css_styles), ("classes", &e.classes)],
        );
    }

    out.push_str("],\"relations\":[");
    for (i, r) in model.relations.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_fields(
            &mut out,
            &[("type", &r.r#type), ("src", &r.src), ("dst", &r.dst)],
        );
        out.push('}');
    }

    out.push_str("],\"classes\":[");
    for (i, c) in model.classes.values.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_fields(&mut out, &[("id", &c.id)]);
        push_lists(
            &mut out,
            &[("styles", &c.styles), ("textStyles", &c.text_styles)],
        );
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

/// Opens an object and writes string members.
#[cfg(feature = "model")]
fn push_fields(out: &mut String, fields: &[(&str, &String)]) {
    out.push('{');
    for (i, (key, value)) in fields.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_json_string(out, key);
        out.push(':');
        push_json_string(out, value);
    }
}

/// Writes string-array members and closes the object.
#[cfg(feature = "model")]
fn push_lists(out: &mut String, lists: &[(&str, &Vec<String>)]) {
    for (key, items) in lists {
        out.push(',');
        push_json_string(out, key);
        out.push_str(":[");
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            push_json_string(out, item);
        }
        out.push(']');
    }
    out.push('}');
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
    //! Parity with Pele's `requirement.parse` over the Ondine vectors, every string literal of
    //! Pele's requirement compat specs, and edge cases, as recorded by running Pele's TypeScript.

    use super::fixture::cases;
    use super::parse;

    /// The parity fixture: Pele's parser input and its model JSON or error.
    const FIXTURE: &str = include_str!("../../tests/fixtures/requirement-parse.json");

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
