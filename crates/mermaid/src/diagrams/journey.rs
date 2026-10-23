//! User journeys: scored tasks with their actors, grouped in sections. Port of Pele's
//! `src/diagrams/journey/index.ts`.

pub(crate) mod db;
pub(crate) mod lexer;
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

/// The `journey` diagram type.
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

/// The parsed user journey model: Pele's `JourneyDb`.
pub(crate) use db::JourneyDb as Model;

/// Parses preprocessed journey `source`, as Pele's `journey.parse` does. `title` comes from front
/// matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    db.title = title.map(str::to_owned);
    parser::parse_journey(source, &mut db)?;
    Ok(db)
}

/// Serializes `model` as `JSON.stringify` serializes this view of Pele's parsed `JourneyDb`,
/// which the parity tests build on Pele's side. A score that is not finite becomes `null`.
///
/// ```ts
/// JSON.stringify({
///   type: db.type, title: db.title, accTitle: db.accTitle, accDescr: db.accDescr,
///   sections: db.sections,
///   tasks: db.tasks.map((t) => ({ task: t.task, score: t.score, people: t.people, section: t.section })),
///   actors: db.getActors(),
/// });
/// ```
#[cfg(feature = "model")]
pub(crate) fn to_json(model: &Model) -> String {
    use crate::yaml::push_number;

    fn strings<S: AsRef<str>>(out: &mut String, items: &[S]) {
        out.push('[');
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            push_json_string(out, item.as_ref());
        }
        out.push(']');
    }

    let mut out = String::from("{\"type\":\"journey\"");
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
    out.push_str(",\"sections\":");
    strings(&mut out, &model.sections);
    out.push_str(",\"tasks\":[");
    for (i, task) in model.tasks.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"task\":");
        push_json_string(&mut out, &task.task);
        out.push_str(",\"score\":");
        if task.score.is_finite() {
            push_number(&mut out, task.score);
        } else {
            out.push_str("null");
        }
        out.push_str(",\"people\":");
        strings(&mut out, &task.people);
        out.push_str(",\"section\":");
        push_json_string(&mut out, &task.section);
        out.push('}');
    }
    out.push_str("],\"actors\":");
    strings(&mut out, &db::journey_actors(&model.tasks));
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
    //! Parity with Pele's `journey.parse` over the Ondine vectors, every string literal of Pele's
    //! journey compat specs, and the documents those specs build at runtime, as recorded by
    //! running Pele's TypeScript.

    use super::fixture::cases;
    use super::parse;

    /// The parity fixture: Pele's parser input and its model JSON or error.
    const FIXTURE: &str = include_str!("../../tests/fixtures/journey-parse.json");

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
