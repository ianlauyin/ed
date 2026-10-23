//! XY charts: lines and bars over a category or numeric x axis. Port of Pele's
//! `src/diagrams/xychart/index.ts`.

pub(crate) mod db;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod render;
pub(crate) mod ticks;
pub(crate) mod tokens;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::error::Error;
use crate::svg::root::RenderOptions;
use crate::util::url::UrlOptions;

/// The `xychart` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input, section: Some("xyChart") };

fn render_input(input: &Input<'_>, options: &RenderOptions, _url: &UrlOptions<'_>) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    render::render(&model, input.config, options)
}

/// The parsed XY chart model: Pele's `XyChartDb`.
pub(crate) use db::XyChartDb as Model;

/// Parses preprocessed XY chart `source`, as Pele's `xychart.parse` does. `title` comes from
/// front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    db.title = title.map(str::to_owned);
    parser::parse_xychart(source, &mut db)?;
    Ok(db)
}

/// Serializes `model` as `JSON.stringify` serializes this view of Pele's parsed `XyChartDb`,
/// which the parity tests build on Pele's side.
///
/// ```ts
/// JSON.stringify({
///   type: db.type, title: db.title, accTitle: db.accTitle, accDescr: db.accDescr,
///   orientation: db.orientation, xAxis: db.xAxis, yAxis: db.yAxis,
///   hasSetYAxis: db.hasSetYAxis, plots: db.plots,
/// });
/// ```
pub(crate) fn to_json(model: &Model) -> String {
    use db::LinearAxis;
    use db::XAxis;

    use crate::yaml::push_number;

    fn number(out: &mut String, value: Option<f64>) {
        match value.filter(|value| value.is_finite()) {
            Some(value) => push_number(out, value),
            None => out.push_str("null"),
        }
    }

    fn linear(out: &mut String, axis: &LinearAxis) {
        out.push_str("{\"type\":\"linear\",\"title\":");
        push_json_string(out, &axis.title);
        out.push_str(",\"min\":");
        number(out, Some(axis.min));
        out.push_str(",\"max\":");
        number(out, Some(axis.max));
        out.push('}');
    }

    fn strings(out: &mut String, items: &[String]) {
        out.push('[');
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            push_json_string(out, item);
        }
        out.push(']');
    }

    let mut out = String::from("{\"type\":\"xychart\"");
    for (key, value) in [("title", &model.title), ("accTitle", &model.acc_title), ("accDescr", &model.acc_descr)] {
        if let Some(value) = value {
            out.push_str(",\"");
            out.push_str(key);
            out.push_str("\":");
            push_json_string(&mut out, value);
        }
    }
    if let Some(horizontal) = model.horizontal {
        out.push_str(if horizontal { ",\"orientation\":\"horizontal\"" } else { ",\"orientation\":\"vertical\"" });
    }
    out.push_str(",\"xAxis\":");
    match &model.x_axis {
        XAxis::Band { categories, title } => {
            out.push_str("{\"type\":\"band\",\"title\":");
            push_json_string(&mut out, title);
            out.push_str(",\"categories\":");
            strings(&mut out, categories);
            out.push('}');
        }
        XAxis::Linear(axis) => linear(&mut out, axis),
    }
    out.push_str(",\"yAxis\":");
    linear(&mut out, &model.y_axis);
    out.push_str(if model.has_set_y_axis {
        ",\"hasSetYAxis\":true,\"plots\":["
    } else {
        ",\"hasSetYAxis\":false,\"plots\":["
    });
    for (i, plot) in model.plots.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(if plot.bar { "{\"type\":\"bar\",\"title\":" } else { "{\"type\":\"line\",\"title\":" });
        push_json_string(&mut out, &plot.title);
        out.push_str(",\"data\":[");
        for (k, (at, value)) in plot.data.iter().enumerate() {
            if k > 0 {
                out.push(',');
            }
            out.push('[');
            push_json_string(&mut out, at);
            out.push(',');
            number(&mut out, *value);
            out.push(']');
        }
        out.push(']');
        if let Some(labels) = &plot.point_labels {
            out.push_str(",\"pointLabels\":");
            strings(&mut out, labels);
        }
        out.push('}');
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
