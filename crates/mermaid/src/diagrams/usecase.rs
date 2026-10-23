//! Use case diagrams: actors, use cases, notes, JSON tables, and system boundaries, joined by
//! relationships. Port of Pele's `src/diagrams/usecase/index.ts`.

pub(crate) mod db;
pub(crate) mod graph;
pub(crate) mod json;
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

/// The `usecase` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram {
    keep_comments: false,
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
    Ok(render::render(&model, input.config, options))
}

/// The parsed use case model: Pele's `UsecaseModel`.
pub(crate) use types::UsecaseModel as Model;

/// Parses preprocessed use case `source`, as Pele's `usecase.parse` does. `title` comes from
/// front matter.
///
/// # Errors
///
/// Returns Mermaid's lexing, parsing, and JSON errors as syntax errors, and its model errors as
/// semantic errors.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    parser::parse_usecase(source, title)
}

/// Serializes `model` as `JSON.stringify` serializes this view of Pele's parsed `UsecaseModel`,
/// which the parity tests build on Pele's side. Maps become arrays of their values, and each JSON
/// node adds the rows Pele draws for it.
///
/// ```ts
/// const pick = (o, keys) => Object.fromEntries(keys.map((k) => [k, o[k]]));
/// const ACTOR = ['id', 'label', 'labelType', 'type', 'icon', 'business', 'stereotype', 'parentId', 'classes', 'styles'];
/// const USE_CASE = ['id', 'label', 'labelType', 'shape', 'business', 'stereotype', 'parentId', 'classes', 'styles'];
/// const BOUNDARY = ['id', 'label', 'labelType', 'type', 'members', 'classes', 'styles'];
/// const EDGE = ['id', 'explicitId', 'source', 'target', 'type', 'arrowType', 'label', 'labelType', 'minlen', 'classes', 'styles', 'animate', 'animation'];
/// JSON.stringify({
///   type: m.type, direction: m.direction,
///   actors: [...m.actors.values()].map((a) => pick(a, ACTOR)),
///   useCases: [...m.useCases.values()].map((u) => pick(u, USE_CASE)),
///   systemBoundaries: [...m.systemBoundaries.values()].map((b) => pick(b, BOUNDARY)),
///   relationships: m.relationships.map((r) => pick(r, EDGE)),
///   notes: [...m.notes.values()].map((n) => pick(n, ['id', 'target', 'label', 'labelType'])),
///   jsonNodes: [...m.jsonNodes.values()].map((j) => ({
///     id: j.id, value: j.value, rows: flattenJsonRows(j).map((r) => [r.key, r.value]),
///     classes: j.classes, styles: j.styles,
///   })),
///   classDefs: [...m.classDefs.values()].map((c) => pick(c, ['id', 'styles'])),
///   title: m.title, accTitle: m.accTitle, accDescr: m.accDescr, accDescription: m.accDescription,
/// });
/// ```
#[cfg(feature = "model")]
pub(crate) fn to_json(model: &Model) -> String {
    use json::{JsonDoc, JsonValue};

    /// Pele's name for a label's type.
    fn label_type(markdown: bool) -> &'static str {
        if markdown { "markdown" } else { "text" }
    }

    /// Writes `name` as the next member key.
    fn key(out: &mut String, name: &str) {
        if !out.ends_with('{') {
            out.push(',');
        }
        push_json_string(out, name);
        out.push(':');
    }

    /// Writes a string member.
    fn string(out: &mut String, name: &str, value: &str) {
        key(out, name);
        push_json_string(out, value);
    }

    /// Writes a string member when the value is present, as `JSON.stringify` drops `undefined`.
    fn optional(out: &mut String, name: &str, value: Option<&str>) {
        if let Some(value) = value {
            string(out, name, value);
        }
    }

    /// Writes a boolean member.
    fn boolean(out: &mut String, name: &str, value: bool) {
        key(out, name);
        out.push_str(if value { "true" } else { "false" });
    }

    /// Writes a member that is an array of strings.
    fn list(out: &mut String, name: &str, items: &[String]) {
        key(out, name);
        out.push('[');
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            push_json_string(out, item);
        }
        out.push(']');
    }

    /// Writes a member that is an array of objects, each written by `each`.
    fn array<T>(out: &mut String, name: &str, items: &[T], each: impl Fn(&mut String, &T)) {
        key(out, name);
        out.push('[');
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push('{');
            each(out, item);
            out.push('}');
        }
        out.push(']');
    }

    /// Whether `key` is an array index, which JavaScript objects list first, in numeric order.
    fn index(key: &str) -> Option<u32> {
        let bytes = key.as_bytes();
        if bytes.is_empty()
            || bytes.len() > 10
            || (bytes.len() > 1 && bytes.first() == Some(&b'0'))
            || !bytes.iter().all(u8::is_ascii_digit)
        {
            return None;
        }
        key.parse::<u64>()
            .ok()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|&value| value != u32::MAX)
    }

    /// Writes a JSON node as `JSON.stringify` writes the value `JSON.parse` built.
    fn value(out: &mut String, doc: &JsonDoc, node: usize) {
        match doc.get(node) {
            Some(JsonValue::Array(items)) => {
                out.push('[');
                for (i, &item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    value(out, doc, item);
                }
                out.push(']');
            }
            Some(JsonValue::Object(entries)) => {
                let mut ordered: Vec<&(String, usize)> = entries.iter().collect();
                ordered.sort_by_key(|(key, _)| index(key).map_or((1, 0), |i| (0, i)));
                out.push('{');
                for (i, (name, item)) in ordered.into_iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    push_json_string(out, name);
                    out.push(':');
                    value(out, doc, *item);
                }
                out.push('}');
            }
            Some(JsonValue::Bool(b)) => out.push_str(if *b { "true" } else { "false" }),
            Some(JsonValue::Number(n)) if n.is_finite() => crate::yaml::push_number(out, *n),
            Some(JsonValue::String(text)) => push_json_string(out, text),
            Some(JsonValue::Null | JsonValue::Number(_)) | None => out.push_str("null"),
        }
    }

    let mut out = String::from("{");
    string(&mut out, "type", "usecase");
    string(&mut out, "direction", model.direction);
    array(&mut out, "actors", &model.actors, |out, a| {
        string(out, "id", &a.id);
        string(out, "label", &a.label);
        string(out, "labelType", label_type(a.markdown));
        string(out, "type", a.r#type.as_str());
        optional(out, "icon", a.icon.as_deref());
        boolean(out, "business", a.business);
        optional(out, "stereotype", a.stereotype.as_deref());
        optional(out, "parentId", a.parent_id.as_deref());
        list(out, "classes", &a.classes);
        list(out, "styles", &a.styles);
    });
    array(&mut out, "useCases", &model.use_cases, |out, u| {
        string(out, "id", &u.id);
        string(out, "label", &u.label);
        string(out, "labelType", label_type(u.markdown));
        string(out, "shape", u.shape.as_str());
        boolean(out, "business", u.business);
        optional(out, "stereotype", u.stereotype.as_deref());
        optional(out, "parentId", u.parent_id.as_deref());
        list(out, "classes", &u.classes);
        list(out, "styles", &u.styles);
    });
    array(
        &mut out,
        "systemBoundaries",
        &model.system_boundaries,
        |out, b| {
            string(out, "id", &b.id);
            string(out, "label", &b.label);
            string(out, "labelType", label_type(b.markdown));
            string(out, "type", if b.package { "package" } else { "rect" });
            list(out, "members", &b.members);
            list(out, "classes", &b.classes);
            list(out, "styles", &b.styles);
        },
    );
    array(&mut out, "relationships", &model.relationships, |out, r| {
        string(out, "id", &r.id);
        boolean(out, "explicitId", r.explicit_id);
        string(out, "source", &r.source);
        string(out, "target", &r.target);
        string(out, "type", r.r#type.as_str());
        key(out, "arrowType");
        crate::error::push_u32(out, u32::from(r.arrow_type));
        if let Some((text, markdown)) = &r.label {
            string(out, "label", text);
            string(out, "labelType", label_type(*markdown));
        }
        key(out, "minlen");
        crate::error::push_u32(out, r.minlen);
        list(out, "classes", &r.classes);
        list(out, "styles", &r.styles);
        boolean(out, "animate", r.animate);
        optional(out, "animation", r.animation);
    });
    array(&mut out, "notes", &model.notes, |out, n| {
        string(out, "id", &n.id);
        string(out, "target", &n.target);
        string(out, "label", &n.label);
        string(out, "labelType", label_type(n.markdown));
    });
    array(&mut out, "jsonNodes", &model.json_nodes, |out, j| {
        string(out, "id", &j.id);
        key(out, "value");
        value(out, &j.value, 0);
        key(out, "rows");
        out.push('[');
        for (i, row) in graph::flatten_json_rows(&j.value).iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push('[');
            push_json_string(out, &row.key);
            out.push(',');
            push_json_string(out, &row.value);
            out.push(']');
        }
        out.push(']');
        list(out, "classes", &j.classes);
        list(out, "styles", &j.styles);
    });
    array(&mut out, "classDefs", &model.class_defs, |out, c| {
        string(out, "id", &c.id);
        list(out, "styles", &c.styles);
    });
    optional(&mut out, "title", model.title.as_deref());
    string(&mut out, "accTitle", &model.acc_title);
    string(&mut out, "accDescr", &model.acc_descr);
    string(&mut out, "accDescription", &model.acc_description);
    out.push('}');
    out
}

/// Appends `text` as `JSON.stringify` quotes it.
#[cfg(feature = "model")]
fn push_json_string(out: &mut String, text: &str) {
    /// Hexadecimal digits.
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
    //! Parity with Pele's `usecase.parse` over the Ondine vectors, every string literal and
    //! runtime-built document of Pele's use case compat specs, and edge cases, as recorded by
    //! running Pele's TypeScript.

    use super::fixture::cases;
    use super::parse;

    /// The parity fixture: Pele's parser input and its model JSON or error.
    const FIXTURE: &str = include_str!("../../tests/fixtures/usecase-parse.json");

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
                #[cfg(feature = "model")]
                let actual = actual.map(|model| super::to_json(&model));
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
