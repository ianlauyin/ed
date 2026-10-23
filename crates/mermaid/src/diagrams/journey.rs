//! User journeys: scored tasks with their actors, grouped in sections. Port of Pele's
//! `src/diagrams/journey/index.ts`.

pub(crate) mod db;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod render;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::error::Error;
use crate::svg::root::RenderOptions;
use crate::util::url::UrlOptions;

/// The `journey` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input, section: None };

fn render_input(input: &Input<'_>, options: &RenderOptions, _url: &UrlOptions<'_>) -> Result<Rendered, Error> {
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
    for (key, value) in [("title", &model.title), ("accTitle", &model.acc_title), ("accDescr", &model.acc_descr)] {
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
