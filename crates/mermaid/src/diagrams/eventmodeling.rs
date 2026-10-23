//! Event models: frames in swimlanes along a timeline, with notes and given/when/then
//! specifications. Port of Pele's `src/diagrams/eventmodeling/index.ts`.

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

/// The `eventmodeling` diagram type. It parses source with its `%%` comment lines kept.
pub(crate) const DIAGRAM: Diagram = Diagram {
    keep_comments: true,
    render: render_input,
    section: None,
};

/// Parses and draws the diagram.
fn render_input(
    input: &Input<'_>,
    options: &RenderOptions,
    _url: &UrlOptions<'_>,
) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render(&model, options))
}

/// The parsed event model: Pele's `EventModelingModel`.
pub(crate) use model::EventModelingModel as Model;

/// Parses preprocessed event modeling `source`, as Pele's `eventmodeling.parse` does. `title`
/// comes from front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for a repeated frame ID.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    model::build_event_model(&parser::parse_event_model(source)?, source, title)
}

/// Serializes `model` as the parity tests serialize Pele's `EventModelingModel`: object keys
/// sorted, absent fields left out.
#[cfg(feature = "model")]
pub(crate) fn to_json(model: &Model) -> String {
    let mut out = String::from("{");
    if let Some(text) = &model.acc_descr {
        out.push_str("\"accDescr\":");
        push_json_string(&mut out, text);
        out.push(',');
    }
    if let Some(text) = &model.acc_title {
        out.push_str("\"accTitle\":");
        push_json_string(&mut out, text);
        out.push(',');
    }
    out.push_str("\"boxes\":[");
    for (i, item) in model.boxes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_box(&mut out, model, item);
    }
    out.push_str("],\"entities\":");
    push_strings(&mut out, &model.entities);
    out.push_str(",\"lanes\":[");
    for (i, lane) in model.lanes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_lane(&mut out, lane);
    }
    out.push_str("],\"notes\":[");
    for (i, note) in model.notes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('{');
        if let Some(kind) = &note.data_type {
            out.push_str("\"dataType\":");
            push_json_string(&mut out, kind);
            out.push(',');
        }
        out.push_str("\"frame\":");
        push_json_string(&mut out, &note.frame);
        out.push_str(",\"text\":");
        push_json_string(&mut out, &note.text);
        out.push('}');
    }
    out.push_str("],\"relations\":[");
    for (i, &(source, target)) in model.relations.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"source\":");
        if let Some(item) = model.boxes.get(source) {
            push_box(&mut out, model, item);
        }
        out.push_str(",\"target\":");
        if let Some(item) = model.boxes.get(target) {
            push_box(&mut out, model, item);
        }
        out.push('}');
    }
    out.push_str("],\"specifications\":[");
    for (i, spec) in model.specifications.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"frame\":");
        push_json_string(&mut out, &spec.frame);
        out.push_str(",\"given\":");
        push_strings(&mut out, &spec.given);
        out.push_str(",\"then\":");
        push_strings(&mut out, &spec.then);
        out.push_str(",\"when\":");
        push_strings(&mut out, &spec.when);
        out.push('}');
    }
    out.push(']');
    if let Some(title) = &model.title {
        out.push_str(",\"title\":");
        push_json_string(&mut out, title);
    }
    out.push_str(",\"type\":\"eventmodeling\"}");
    out
}

/// Appends a box as JSON, its lane written out.
#[cfg(feature = "model")]
fn push_box(out: &mut String, model: &Model, item: &model::EmBox) {
    out.push('{');
    if let Some(data) = &item.data {
        out.push_str("\"data\":");
        push_json_string(out, data);
        out.push(',');
    }
    if let Some(kind) = &item.data_type {
        out.push_str("\"dataType\":");
        push_json_string(out, kind);
        out.push(',');
    }
    out.push_str("\"entity\":");
    push_json_string(out, &item.entity);
    out.push_str(",\"id\":");
    push_json_string(out, &item.id);
    out.push_str(",\"kind\":\"");
    out.push_str(item.kind.as_str());
    out.push_str("\",\"lane\":");
    if let Some(lane) = model.lanes.get(item.lane) {
        push_lane(out, lane);
    }
    out.push_str(",\"name\":");
    push_json_string(out, &item.name);
    if let Some(namespace) = &item.namespace {
        out.push_str(",\"namespace\":");
        push_json_string(out, namespace);
    }
    out.push_str(",\"reset\":");
    out.push_str(if item.reset { "true" } else { "false" });
    out.push('}');
}

/// Appends a lane as JSON.
#[cfg(feature = "model")]
fn push_lane(out: &mut String, lane: &model::EmLane) {
    out.push_str("{\"group\":\"");
    out.push_str(lane.group.as_str());
    out.push_str("\",\"index\":");
    crate::yaml::push_number(out, f64::from(lane.index));
    out.push_str(",\"label\":");
    push_json_string(out, &lane.label);
    if let Some(namespace) = &lane.namespace {
        out.push_str(",\"namespace\":");
        push_json_string(out, namespace);
    }
    out.push('}');
}

/// Appends a list of strings as JSON.
#[cfg(feature = "model")]
fn push_strings(out: &mut String, list: &[String]) {
    out.push('[');
    for (i, text) in list.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_json_string(out, text);
    }
    out.push(']');
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
    //! Parity with Pele's `eventmodeling.parse` over every string literal of Pele's event modeling compat
    //! specs and the documents the specs build at runtime, as recorded by running Pele's
    //! TypeScript.

    use super::fixture::cases;
    use super::parse;

    /// The parity fixture: Pele's parser input and its model JSON or error.
    const FIXTURE: &str = include_str!("../../tests/fixtures/eventmodeling-parse.json");

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
