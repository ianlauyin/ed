//! Wardley maps: Mermaid's `wardley-beta`, components placed by visibility and evolution. Port of
//! Pele's `src/diagrams/wardley/index.ts`.

pub(crate) mod builder;
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

/// The `wardley` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram {
    keep_comments: false,
    render: render_input,
    section: Some("wardley-beta"),
};

fn render_input(
    input: &Input<'_>,
    options: &RenderOptions,
    _url: &UrlOptions<'_>,
) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render(&model, input.config, options))
}

/// The parsed map: Pele's `WardleyModel`.
pub(crate) use model::WardleyModel as Model;

/// Parses preprocessed Wardley `source`, as Pele's `wardley.parse` does. `title` comes from front
/// matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for a coordinate out of
/// range or a pipeline without its component.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    model::build_wardley(parser::parse_wardley(source)?, title)
}

/// Serializes `model` as `JSON.stringify` serializes Pele's `WardleyModel` after the parity
/// tests' normalization on Pele's side, which sorts every object's keys and drops `undefined`:
///
/// ```ts
/// const norm = (v) =>
///   Array.isArray(v) ? v.map(norm)
///   : v && typeof v === 'object'
///     ? Object.fromEntries(Object.keys(v).sort().filter((k) => v[k] !== undefined).map((k) => [k, norm(v[k])]))
///     : v;
/// JSON.stringify(norm(model));
/// ```
#[cfg(feature = "model")]
pub(crate) fn to_json(model: &Model) -> String {
    use json::{array, number, object, point, string};

    let build = &model.build;
    let mut fields = Vec::new();
    if let Some(text) = &model.acc_descr {
        fields.push(("accDescr", string(text)));
    }
    if let Some(text) = &model.acc_title {
        fields.push(("accTitle", string(text)));
    }
    let forces = |forces: &[builder::WardleyForce]| {
        array(forces.iter().map(|force| {
            object(vec![
                ("name", string(&force.name)),
                ("x", number(force.x)),
                ("y", number(force.y)),
            ])
        }))
    };
    fields.push(("accelerators", forces(&build.accelerators)));
    fields.push((
        "annotations",
        array(build.annotations.iter().map(|annotation| {
            let mut fields = vec![
                (
                    "coordinates",
                    array(annotation.coordinates.iter().map(|&(x, y)| point(x, y))),
                ),
                ("number", number(annotation.number)),
            ];
            if let Some(text) = &annotation.text {
                fields.push(("text", string(text)));
            }
            object(fields)
        })),
    ));
    if let Some((x, y)) = build.annotations_box {
        fields.push(("annotationsBox", point(x, y)));
    }
    let mut axes = Vec::new();
    if let Some(boundaries) = &build.axes.stage_boundaries {
        axes.push((
            "stageBoundaries",
            array(boundaries.iter().map(|&b| number(b))),
        ));
    }
    if let Some(stages) = &build.axes.stages {
        axes.push(("stages", array(stages.iter().map(|stage| string(stage)))));
    }
    fields.push(("axes", object(axes)));
    fields.push(("deaccelerators", forces(&build.deaccelerators)));
    fields.push((
        "links",
        array(build.links.iter().map(|link| {
            let mut fields = vec![(
                "dashed",
                String::from(if link.dashed { "true" } else { "false" }),
            )];
            if let Some(flow) = link.flow {
                fields.push(("flow", string(flow.as_str())));
            }
            if let Some(label) = &link.label {
                fields.push(("label", string(label)));
            }
            fields.push(("source", string(&link.source)));
            fields.push(("target", string(&link.target)));
            object(fields)
        })),
    ));
    fields.push((
        "nodes",
        array(build.nodes.iter().map(|node| {
            let mut fields = Vec::new();
            if let Some(class_name) = node.class_name {
                fields.push(("className", string(class_name)));
            }
            fields.push(("id", string(&node.id)));
            if node.in_pipeline {
                fields.push(("inPipeline", String::from("true")));
            }
            if let Some(inertia) = node.inertia {
                fields.push((
                    "inertia",
                    String::from(if inertia { "true" } else { "false" }),
                ));
            }
            if node.is_pipeline_parent {
                fields.push(("isPipelineParent", String::from("true")));
            }
            fields.push(("label", string(&node.label)));
            if let Some(offset) = node.label_offset_x {
                fields.push(("labelOffsetX", number(offset)));
            }
            if let Some(offset) = node.label_offset_y {
                fields.push(("labelOffsetY", number(offset)));
            }
            if let Some(strategy) = &node.source_strategy {
                fields.push(("sourceStrategy", string(strategy)));
            }
            fields.push(("x", number(node.x)));
            fields.push(("y", number(node.y)));
            object(fields)
        })),
    ));
    fields.push((
        "notes",
        array(build.notes.iter().map(|note| {
            object(vec![
                ("text", string(&note.text)),
                ("x", number(note.x)),
                ("y", number(note.y)),
            ])
        })),
    ));
    fields.push((
        "pipelines",
        array(build.pipelines.iter().map(|pipeline| {
            object(vec![
                (
                    "componentIds",
                    array(pipeline.component_ids.iter().map(|id| string(id))),
                ),
                ("nodeId", string(&pipeline.node_id)),
            ])
        })),
    ));
    if let Some((width, height)) = build.size {
        fields.push((
            "size",
            object(vec![("height", number(height)), ("width", number(width))]),
        ));
    }
    if let Some(title) = &model.title {
        fields.push(("title", string(title)));
    }
    fields.push((
        "trends",
        array(build.trends.iter().map(|trend| {
            object(vec![
                ("nodeId", string(&trend.node_id)),
                ("targetX", number(trend.target_x)),
                ("targetY", number(trend.target_y)),
            ])
        })),
    ));
    fields.push(("type", string("wardley")));
    object(fields)
}

/// `JSON.stringify` pieces for [`to_json`].
#[cfg(feature = "model")]
mod json {
    use crate::yaml::push_number;

    /// A number, or `null` when it is not finite.
    pub(super) fn number(value: f64) -> String {
        if !value.is_finite() {
            return String::from("null");
        }
        let mut out = String::new();
        push_number(&mut out, value);
        out
    }

    /// `{"x":…,"y":…}`.
    pub(super) fn point(x: f64, y: f64) -> String {
        object(vec![("x", number(x)), ("y", number(y))])
    }

    /// An array of serialized items.
    pub(super) fn array(items: impl Iterator<Item = String>) -> String {
        let mut out = String::from("[");
        for (i, item) in items.enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&item);
        }
        out.push(']');
        out
    }

    /// An object of serialized fields, in the given order.
    pub(super) fn object(fields: Vec<(&str, String)>) -> String {
        let mut out = String::from("{");
        for (i, (key, value)) in fields.into_iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&string(key));
            out.push(':');
            out.push_str(&value);
        }
        out.push('}');
        out
    }

    /// `text` as `JSON.stringify` quotes it.
    pub(super) fn string(text: &str) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::from("\"");
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
        out
    }
}

#[cfg(test)]
mod tests {
    //! Parity with Pele's `wardley.parse` over every string literal of Pele's wardley compat
    //! specs, the documents those specs parse at run time, the vectors, and edge cases, as
    //! recorded by running Pele's TypeScript.

    use super::fixture::cases;
    use super::parse;

    /// The parity fixture: Pele's parser input and its model JSON or error.
    const FIXTURE: &str = include_str!("../../tests/fixtures/wardley-parse.json");

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
