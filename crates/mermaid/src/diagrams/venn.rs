//! Venn diagrams: Mermaid's `venn-beta` sets, overlaps, and texts. Port of Pele's
//! `src/diagrams/venn/index.ts`.

pub(crate) mod db;
pub(crate) mod layout;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod render;
pub(crate) mod types;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::error::Error;
use crate::svg::root::RenderOptions;
use crate::util::url::UrlOptions;

/// The `venn` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input, section: None };

fn render_input(input: &Input<'_>, options: &RenderOptions, _url: &UrlOptions<'_>) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render(&model, options))
}

/// The parsed venn diagram model: Pele's `VennDb`.
pub(crate) use db::VennDb as Model;

/// Parses preprocessed venn diagram `source`, as Pele's `venn.parse` does. `title` comes from
/// front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for a union of one set
/// or of unknown sets, and for an indented text before any set.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        db.title = Some(title.to_owned());
    }
    parser::parse_venn(source, &mut db)?;
    Ok(db)
}

/// Serializes `model` as `JSON.stringify` serializes Pele's parsed `VennDb` after this
/// normalization, which the parity tests apply on Pele's side. Each style's map becomes an array
/// of entries in insertion order, and `undefined` fields are dropped.
///
/// ```ts
/// const pick = (o, keys) => Object.fromEntries(
///   keys.filter((k) => o[k] !== undefined).map((k) => [k, o[k]]));
/// JSON.stringify({
///   type: model.type, title: model.title,
///   subsets: model.subsets.map((s) => pick(s, ['sets', 'size', 'label'])),
///   textNodes: model.textNodes.map((t) => pick(t, ['sets', 'id', 'label'])),
///   styles: model.styles.map((s) => ({ targets: s.targets, styles: [...s.styles] })),
/// });
/// ```
pub(crate) fn to_json(model: &Model) -> String {
    use crate::yaml::push_number;

    /// Appends `text` as `JSON.stringify` quotes it.
    fn string(out: &mut String, text: &str) {
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
                    let code = u32::from(c);
                    out.push_str("\\u00");
                    for nibble in [code >> 4, code & 15] {
                        out.push(char::from_digit(nibble, 16).unwrap_or('0'));
                    }
                }
                c => out.push(c),
            }
        }
        out.push('"');
    }

    fn strings<S: AsRef<str>>(out: &mut String, items: &[S]) {
        out.push('[');
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            string(out, item.as_ref());
        }
        out.push(']');
    }

    let mut out = String::from("{\"type\":\"venn\"");
    if let Some(title) = &model.title {
        out.push_str(",\"title\":");
        string(&mut out, title);
    }
    out.push_str(",\"subsets\":[");
    for (i, subset) in model.subsets.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"sets\":");
        strings(&mut out, &subset.sets);
        out.push_str(",\"size\":");
        if subset.size.is_finite() {
            push_number(&mut out, subset.size);
        } else {
            out.push_str("null");
        }
        if let Some(label) = &subset.label {
            out.push_str(",\"label\":");
            string(&mut out, label);
        }
        out.push('}');
    }
    out.push_str("],\"textNodes\":[");
    for (i, node) in model.text_nodes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"sets\":");
        strings(&mut out, &node.sets);
        out.push_str(",\"id\":");
        string(&mut out, &node.id);
        if let Some(label) = &node.label {
            out.push_str(",\"label\":");
            string(&mut out, label);
        }
        out.push('}');
    }
    out.push_str("],\"styles\":[");
    for (i, style) in model.styles.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"targets\":");
        strings(&mut out, &style.targets);
        out.push_str(",\"styles\":[");
        for (j, (key, value)) in style.styles.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            strings(&mut out, &[key, value]);
        }
        out.push_str("]}");
    }
    out.push_str("]}");
    out
}
