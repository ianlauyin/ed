//! Timelines: time periods with their events, grouped in sections. Port of Pele's
//! `src/diagrams/timeline/index.ts`.

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

/// The `timeline` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input, section: None };

fn render_input(input: &Input<'_>, options: &RenderOptions, _url: &UrlOptions<'_>) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render(&model, input.config, options))
}

/// The parsed timeline model: Pele's `TimelineDb`.
pub(crate) use db::TimelineDb as Model;

/// Parses preprocessed timeline `source`, as Pele's `timeline.parse` does. `title` comes from
/// front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for an event before any
/// period.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    db.title = title.map(str::to_owned);
    parser::parse_timeline(source, &mut db)?;
    Ok(db)
}

/// Serializes `model` as `JSON.stringify` serializes this view of Pele's parsed `TimelineDb`,
/// which the parity tests build on Pele's side:
///
/// ```ts
/// JSON.stringify({
///   type: db.type, title: db.title, accTitle: db.accTitle, accDescr: db.accDescr,
///   direction: db.direction, sections: db.sections,
///   periods: db.periods.map((p) => ({
///     text: p.text, events: p.events, section: p.section, sectionIndex: p.sectionIndex,
///   })),
/// });
/// ```
pub(crate) fn to_json(model: &Model) -> String {
    use crate::yaml::push_number;

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

    let mut out = String::from("{\"type\":\"timeline\"");
    for (key, value) in [("title", &model.title), ("accTitle", &model.acc_title), ("accDescr", &model.acc_descr)] {
        if let Some(value) = value {
            out.push_str(",\"");
            out.push_str(key);
            out.push_str("\":");
            push_json_string(&mut out, value);
        }
    }
    out.push_str(",\"direction\":");
    push_json_string(&mut out, model.direction);
    out.push_str(",\"sections\":");
    strings(&mut out, &model.sections);
    out.push_str(",\"periods\":[");
    for (i, period) in model.periods.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"text\":");
        push_json_string(&mut out, &period.text);
        out.push_str(",\"events\":");
        strings(&mut out, &period.events);
        out.push_str(",\"section\":");
        push_json_string(&mut out, &period.section);
        out.push_str(",\"sectionIndex\":");
        #[expect(clippy::cast_precision_loss, reason = "indexes are far below 2^52")]
        push_number(&mut out, period.section_index as f64);
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
