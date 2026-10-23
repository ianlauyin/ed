//! Sequence diagrams: participants on lifelines, the messages between them, notes, and blocks.
//! Port of Pele's `src/diagrams/sequence/index.ts`.

pub(crate) mod actors;
pub(crate) mod colors;
pub(crate) mod db;
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
use crate::yaml::Value;

/// The `sequence` diagram type.
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
    let db = parse(input.source, input.config, input.title)?;
    Ok(render::render(&db, input.config, options))
}

/// The parsed sequence diagram model: Pele's `SeqDb`.
pub(crate) use db::SeqDb as Model;

/// Parses preprocessed sequence diagram `source`, as Pele's `sequence.parse` does. `config` is
/// the diagram configuration, of which `wrap` and `sequence.wrap` are read. `title` comes from
/// front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects or a participant config that is not valid
/// YAML, and a semantic error for a participant in two boxes, a misplaced `create` or `destroy`,
/// and a deactivation of an inactive participant.
pub(crate) fn parse(source: &str, config: &Value, title: Option<&str>) -> Result<Model, Error> {
    let wrap = match member(config, "wrap") {
        Some(Value::Bool(wrap)) => Some(*wrap),
        _ => None,
    };
    let sequence_wrap = member(config, "sequence")
        .and_then(|section| member(section, "wrap"))
        .is_some_and(|wrap| *wrap == Value::Bool(true));
    let mut db = Model::new(wrap, sequence_wrap);
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        db.set_diagram_title(title);
    }
    parser::parse_sequence(source, &mut db)?;
    Ok(db)
}

/// The value under `key` of a YAML mapping.
fn member<'v>(value: &'v Value, key: &str) -> Option<&'v Value> {
    match value {
        Value::Object(entries) => entries.iter().find(|(own, _)| own == key).map(|(_, v)| v),
        _ => None,
    }
}

/// Serializes `model` as `JSON.stringify` serializes this view of Pele's parsed `SeqDb`, which
/// the parity tests build on Pele's side. Maps become arrays of entries, a participant's box
/// becomes its index, and `undefined` becomes `null`.
///
/// ```ts
/// const v = (x) => (x === undefined ? null : x);
/// JSON.stringify({
///   type: db.type,
///   actors: [...db.actors].map(([id, a]) => ({ id, box: a.box ? db.boxes.indexOf(a.box) : null,
///     name: a.name, description: a.description, wrap: a.wrap, prevActor: v(a.prevActor),
///     nextActor: v(a.nextActor), links: [...a.links], properties: [...a.properties],
///     details: v(a.details), type: a.type })),
///   createdActors: [...db.createdActors], destroyedActors: [...db.destroyedActors],
///   boxes: db.boxes.map((b) => ({ name: v(b.name), wrap: b.wrap, fill: b.fill, actorKeys: b.actorKeys })),
///   messages: db.messages.map((m) => ({ id: m.id, from: v(m.from), to: v(m.to),
///     message: typeof m.message === 'string' ? m.message
///       : { start: v(m.message.start), step: v(m.message.step), visible: m.message.visible },
///     wrap: m.wrap, type: v(m.type), activate: v(m.activate), placement: v(m.placement),
///     centralConnection: v(m.centralConnection) })),
///   notes: db.notes.map((n) => ({ actor: n.actor, placement: n.placement, message: n.message, wrap: n.wrap })),
///   title: v(db.title), accTitle: v(db.accTitle), accDescr: v(db.accDescr),
/// });
/// ```
#[cfg(feature = "model")]
pub(crate) fn to_json(model: &Model) -> String {
    use types::{MessageText, NoteActor};

    let mut out = String::from("{\"type\":\"sequence\",\"actors\":[");
    for (i, a) in model.actors.iter().enumerate() {
        comma(&mut out, i);
        out.push_str("{\"id\":");
        string(&mut out, &a.name);
        out.push_str(",\"box\":");
        option_number(&mut out, a.r#box.map(usize_f64));
        out.push_str(",\"name\":");
        string(&mut out, &a.name);
        out.push_str(",\"description\":");
        string(&mut out, &a.description);
        out.push_str(",\"wrap\":");
        boolean(&mut out, a.wrap);
        out.push_str(",\"prevActor\":");
        option_string(&mut out, a.prev_actor.as_deref());
        out.push_str(",\"nextActor\":");
        option_string(&mut out, a.next_actor.as_deref());
        out.push_str(",\"links\":");
        entries(&mut out, &a.links);
        out.push_str(",\"properties\":");
        entries(&mut out, &a.properties);
        out.push_str(",\"details\":");
        option_string(&mut out, a.details.as_deref());
        out.push_str(",\"type\":");
        string(&mut out, &a.r#type);
        out.push('}');
    }
    for (key, list) in [
        ("createdActors", &model.created_actors),
        ("destroyedActors", &model.destroyed_actors),
    ] {
        out.push_str("],\"");
        out.push_str(key);
        out.push_str("\":[");
        for (i, (id, at)) in list.iter().enumerate() {
            comma(&mut out, i);
            out.push('[');
            string(&mut out, id);
            out.push(',');
            crate::yaml::push_number(&mut out, usize_f64(*at));
            out.push(']');
        }
    }
    out.push_str("],\"boxes\":[");
    for (i, b) in model.boxes.iter().enumerate() {
        comma(&mut out, i);
        out.push_str("{\"name\":");
        option_string(&mut out, b.name.as_deref());
        out.push_str(",\"wrap\":");
        boolean(&mut out, b.wrap);
        out.push_str(",\"fill\":");
        string(&mut out, &b.fill);
        out.push_str(",\"actorKeys\":[");
        for (k, key) in b.actor_keys.iter().enumerate() {
            comma(&mut out, k);
            string(&mut out, key);
        }
        out.push_str("]}");
    }
    out.push_str("],\"messages\":[");
    for (i, m) in model.messages.iter().enumerate() {
        comma(&mut out, i);
        out.push_str("{\"id\":");
        string(&mut out, &m.id);
        out.push_str(",\"from\":");
        option_string(&mut out, m.from.as_deref());
        out.push_str(",\"to\":");
        option_string(&mut out, m.to.as_deref());
        out.push_str(",\"message\":");
        match &m.message {
            MessageText::Text(text) => string(&mut out, text),
            MessageText::Numbering(n) => {
                out.push_str("{\"start\":");
                option_number(&mut out, n.start);
                out.push_str(",\"step\":");
                option_number(&mut out, n.step);
                out.push_str(",\"visible\":");
                boolean(&mut out, n.visible);
                out.push('}');
            }
        }
        out.push_str(",\"wrap\":");
        boolean(&mut out, m.wrap);
        out.push_str(",\"type\":");
        option_number(&mut out, m.r#type.map(f64::from));
        out.push_str(",\"activate\":");
        match m.activate {
            Some(activate) => boolean(&mut out, activate),
            None => out.push_str("null"),
        }
        out.push_str(",\"placement\":");
        option_number(&mut out, m.placement.map(f64::from));
        out.push_str(",\"centralConnection\":");
        option_number(&mut out, m.central_connection.map(f64::from));
        out.push('}');
    }
    out.push_str("],\"notes\":[");
    for (i, n) in model.notes.iter().enumerate() {
        comma(&mut out, i);
        out.push_str("{\"actor\":");
        match &n.actor {
            NoteActor::Single(id) => string(&mut out, id),
            NoteActor::Over(a, b) => {
                out.push('[');
                string(&mut out, a);
                out.push(',');
                string(&mut out, b);
                out.push(']');
            }
        }
        out.push_str(",\"placement\":");
        crate::yaml::push_number(&mut out, f64::from(n.placement));
        out.push_str(",\"message\":");
        string(&mut out, &n.message);
        out.push_str(",\"wrap\":");
        boolean(&mut out, n.wrap);
        out.push('}');
    }
    out.push(']');
    for (key, value) in [
        ("title", &model.title),
        ("accTitle", &model.acc_title),
        ("accDescr", &model.acc_descr),
    ] {
        out.push_str(",\"");
        out.push_str(key);
        out.push_str("\":");
        option_string(&mut out, value.as_deref());
    }
    out.push('}');
    out
}

#[cfg(feature = "model")]
fn comma(out: &mut String, i: usize) {
    if i > 0 {
        out.push(',');
    }
}

#[cfg(feature = "model")]
#[expect(clippy::cast_precision_loss, reason = "counts of a parsed document")]
fn usize_f64(value: usize) -> f64 {
    value as f64
}

#[cfg(feature = "model")]
fn boolean(out: &mut String, value: bool) {
    out.push_str(if value { "true" } else { "false" });
}

#[cfg(feature = "model")]
fn option_number(out: &mut String, value: Option<f64>) {
    match value.filter(|value| value.is_finite()) {
        Some(value) => crate::yaml::push_number(out, value),
        None => out.push_str("null"),
    }
}

#[cfg(feature = "model")]
fn option_string(out: &mut String, value: Option<&str>) {
    match value {
        Some(value) => string(out, value),
        None => out.push_str("null"),
    }
}

/// Appends map entries as `[key, value]` pairs.
#[cfg(feature = "model")]
fn entries(out: &mut String, entries: &[(String, Value)]) {
    out.push('[');
    for (i, (key, value)) in entries.iter().enumerate() {
        comma(out, i);
        out.push('[');
        string(out, key);
        out.push(',');
        json_value(out, value);
        out.push(']');
    }
    out.push(']');
}

/// Appends `value` as `JSON.stringify` writes it.
#[cfg(feature = "model")]
fn json_value(out: &mut String, value: &Value) {
    match value {
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                comma(out, i);
                json_value(out, item);
            }
            out.push(']');
        }
        Value::Bool(value) => boolean(out, *value),
        Value::Null => out.push_str("null"),
        Value::Number(value) => option_number(out, Some(*value)),
        Value::Object(members) => {
            out.push('{');
            for (i, (key, value)) in members.iter().enumerate() {
                comma(out, i);
                string(out, key);
                out.push(':');
                json_value(out, value);
            }
            out.push('}');
        }
        Value::String(text) => string(out, text),
    }
}

/// Appends `text` as `JSON.stringify` quotes it.
#[cfg(feature = "model")]
fn string(out: &mut String, text: &str) {
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
    //! Parity with Pele's `sequence.parse` over every string literal of Pele's sequence compat
    //! specs and every document those specs build at run time, as recorded by running Pele's
    //! TypeScript. Each is parsed once as the public `parse()` sees it (preprocessed, with its
    //! config) and once as written.

    use super::fixture::{Json, cases};
    use super::parse;
    use crate::yaml::Value;

    /// The parity fixture: Pele's parser input, config, and its model JSON or error.
    const FIXTURE: &str = include_str!("../../tests/fixtures/sequence-parse.json");

    #[test]
    fn matches_pele() {
        let mut differing = Vec::new();
        let cases = cases(FIXTURE);
        for case in &cases {
            let input = case.str("input").unwrap_or_default();
            let mut config = Vec::new();
            if let Some(Json::Bool(wrap)) = case.get("wrap") {
                config.push(("wrap".to_owned(), Value::Bool(*wrap)));
            }
            if let Some(Json::Bool(true)) = case.get("sequenceWrap") {
                config.push((
                    "sequence".to_owned(),
                    Value::Object(vec![("wrap".to_owned(), Value::Bool(true))]),
                ));
            }
            let actual = parse(input, &Value::Object(config), case.str("title"));
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
