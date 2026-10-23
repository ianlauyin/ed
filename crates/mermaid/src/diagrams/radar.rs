//! Radar charts: series of values drawn as closed curves over radial axes. Port of Pele's
//! `src/diagrams/radar/index.ts`.

pub(crate) mod model;
pub(crate) mod parser;
pub(crate) mod render;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::error::Error;
use crate::svg::root::RenderOptions;
use crate::util::url::UrlOptions;

/// The `radar` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input, section: None };

fn render_input(input: &Input<'_>, options: &RenderOptions, _url: &UrlOptions<'_>) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render(&model, input.config, options))
}

/// The parsed radar chart: Pele's `RadarModel`.
pub(crate) use model::RadarModel as Model;

/// Parses preprocessed radar `source`, as Pele's `radar.parse` does. `title` comes from front
/// matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for a curve whose axis
/// references cannot be resolved.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    model::build_radar(parser::parse_radar(source)?, title)
}

/// Serializes `model` as `JSON.stringify` serializes Pele's `RadarModel`.
pub(crate) fn to_json(model: &Model) -> String {
    use crate::yaml::push_number;

    let number = |out: &mut String, value: f64| {
        if value.is_finite() {
            push_number(out, value);
        } else {
            out.push_str("null");
        }
    };
    let mut out = String::from("{\"type\":\"radar\"");
    for (key, value) in [("title", &model.title), ("accTitle", &model.acc_title), ("accDescr", &model.acc_descr)] {
        if let Some(value) = value {
            out.push_str(",\"");
            out.push_str(key);
            out.push_str("\":");
            push_json_string(&mut out, value);
        }
    }
    out.push_str(",\"axes\":[");
    for (i, axis) in model.axes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        push_json_string(&mut out, &axis.name);
        out.push_str(",\"label\":");
        push_json_string(&mut out, &axis.label);
        out.push('}');
    }
    out.push_str("],\"curves\":[");
    for (i, curve) in model.curves.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        push_json_string(&mut out, &curve.name);
        out.push_str(",\"label\":");
        push_json_string(&mut out, &curve.label);
        out.push_str(",\"entries\":[");
        for (j, value) in curve.entries.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            number(&mut out, *value);
        }
        out.push_str("]}");
    }
    let options = &model.options;
    out.push_str("],\"options\":{\"showLegend\":");
    out.push_str(if options.show_legend { "true" } else { "false" });
    out.push_str(",\"ticks\":");
    number(&mut out, options.ticks);
    out.push_str(",\"max\":");
    number(&mut out, options.max.unwrap_or(f64::NAN));
    out.push_str(",\"min\":");
    number(&mut out, options.min);
    out.push_str(",\"graticule\":");
    push_json_string(&mut out, &options.graticule);
    out.push_str("}}");
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
