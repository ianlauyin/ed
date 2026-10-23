//! Quadrant charts: points placed on two axes split into four labelled quadrants. Port of Pele's
//! `src/diagrams/quadrant/index.ts`.

pub(crate) mod db;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod render;
pub(crate) mod tokens;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::error::Error;
use crate::svg::root::RenderOptions;
use crate::util::url::UrlOptions;

/// The `quadrantChart` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input, section: None };

fn render_input(input: &Input<'_>, options: &RenderOptions, _url: &UrlOptions<'_>) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render(&model, input.config, options))
}

/// The parsed quadrant chart model: Pele's `QuadrantDb`.
pub(crate) use db::QuadrantDb as Model;

/// Parses preprocessed quadrant chart `source`, as Pele's `quadrantChart.parse` does. `title`
/// comes from front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for an invalid point
/// style.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model { title: title.map(str::to_owned), ..Model::default() };
    parser::parse_quadrant(source, &mut db)?;
    Ok(db)
}

/// Serializes `model` as `JSON.stringify` serializes this view of Pele's parsed `QuadrantDb`,
/// which the parity tests build on Pele's side. `pick` keeps the keys that are not `undefined`.
///
/// ```ts
/// const STYLE = ['radius', 'color', 'strokeColor', 'strokeWidth'];
/// JSON.stringify({
///   type: db.type, title: db.title, accTitle: db.accTitle, accDescr: db.accDescr,
///   quadrants: db.quadrants, xAxisLeft: db.xAxisLeft, xAxisRight: db.xAxisRight,
///   yAxisBottom: db.yAxisBottom, yAxisTop: db.yAxisTop,
///   points: db.points.map((p) => pick(p, ['x', 'y', 'text', 'className', ...STYLE])),
///   classes: [...db.classes].map(([k, v]) => [k, pick(v, STYLE)]),
/// });
/// ```
pub(crate) fn to_json(model: &Model) -> String {
    use db::PointStyle;
    use db::QuadrantText;

    use crate::yaml::push_number;

    fn number(out: &mut String, value: f64) {
        if value.is_finite() {
            push_number(out, value);
        } else {
            out.push_str("null");
        }
    }

    fn text(out: &mut String, label: Option<&QuadrantText>) {
        let Some(label) = label else {
            out.push_str("null");
            return;
        };
        out.push_str("{\"text\":");
        push_json_string(out, &label.text);
        out.push_str(if label.markdown { ",\"type\":\"markdown\"}" } else { ",\"type\":\"text\"}" });
    }

    fn style(out: &mut String, style: &PointStyle) {
        if let Some(radius) = style.radius {
            out.push_str(",\"radius\":");
            number(out, radius);
        }
        for (key, value) in
            [("color", &style.color), ("strokeColor", &style.stroke_color), ("strokeWidth", &style.stroke_width)]
        {
            if let Some(value) = value {
                out.push_str(",\"");
                out.push_str(key);
                out.push_str("\":");
                push_json_string(out, value);
            }
        }
    }

    let mut out = String::from("{\"type\":\"quadrantChart\"");
    for (key, value) in [("title", &model.title), ("accTitle", &model.acc_title), ("accDescr", &model.acc_descr)] {
        if let Some(value) = value {
            out.push_str(",\"");
            out.push_str(key);
            out.push_str("\":");
            push_json_string(&mut out, value);
        }
    }
    out.push_str(",\"quadrants\":[");
    for (i, quadrant) in model.quadrants.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        text(&mut out, quadrant.as_ref());
    }
    out.push(']');
    for (key, value) in [
        ("xAxisLeft", &model.x_axis_left),
        ("xAxisRight", &model.x_axis_right),
        ("yAxisBottom", &model.y_axis_bottom),
        ("yAxisTop", &model.y_axis_top),
    ] {
        if value.is_some() {
            out.push_str(",\"");
            out.push_str(key);
            out.push_str("\":");
            text(&mut out, value.as_ref());
        }
    }
    out.push_str(",\"points\":[");
    for (i, point) in model.points.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"x\":");
        number(&mut out, point.x);
        out.push_str(",\"y\":");
        number(&mut out, point.y);
        out.push_str(",\"text\":");
        text(&mut out, Some(&point.text));
        out.push_str(",\"className\":");
        push_json_string(&mut out, &point.class_name);
        style(&mut out, &point.style);
        out.push('}');
    }
    out.push_str("],\"classes\":[");
    for (i, (name, own)) in model.classes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('[');
        push_json_string(&mut out, name);
        out.push_str(",{");
        let at = out.len();
        style(&mut out, own);
        // The style writer puts a comma before every key, and the first one has none.
        if out.get(at..=at) == Some(",") {
            out.remove(at);
        }
        out.push_str("}]");
    }
    out.push_str("]}");
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
