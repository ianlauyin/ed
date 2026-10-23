//! C4 diagrams: people, systems, containers and components in nested boundaries, joined by
//! relations. Port of Pele's `src/diagrams/c4/index.ts`.

pub(crate) mod db;
pub(crate) mod layout;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod render;
pub(crate) mod route;
pub(crate) mod tokens;
pub(crate) mod types;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::error::Error;
use crate::svg::root::RenderOptions;
use crate::util::url::UrlOptions;

/// The `c4` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input, section: None };

fn render_input(input: &Input<'_>, options: &RenderOptions, url: &UrlOptions<'_>) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    render::render(&model, options, url)
}

/// The parsed C4 model: Pele's `C4Db`.
pub(crate) use db::C4Db as Model;

/// Parses preprocessed C4 `source`, as Pele's `c4.parse` does. `title` comes from front matter
/// and is kept when it is not empty.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        db.title = Some(title.to_owned());
    }
    parser::parse_c4(source, &mut db)?;
    Ok(db)
}

/// Serializes `model` as `JSON.stringify` serializes this view of Pele's parsed `C4Db`, which the
/// parity tests build on Pele's side. `pick` keeps the keys that are not `undefined`, and a
/// parent is the index of its boundary, or -1.
///
/// ```ts
/// const parent = (el) => db.boundaries.indexOf(el.parent);
/// const extra = (el) => (el.extra ? { extra: [...el.extra] } : {});
/// JSON.stringify({
///   type: db.type, c4Type: db.c4Type, title: db.title, accTitle: db.accTitle,
///   accDescr: db.accDescr,
///   shapes: db.shapes.map((s) => ({ ...pick(s, ELEMENT), parent: parent(s), kind: s.kind, ...extra(s) })),
///   boundaries: db.boundaries.map((b) => ({ ...pick(b, ELEMENT), parent: parent(b),
///     ...pick(b, ['nodeType']), ...extra(b) })),
///   rels: db.rels.map((r) => ({ ...pick(r, REL), ...extra(r) })),
///   shapeInRow: db.shapeInRow, boundaryInRow: db.boundaryInRow,
/// });
/// ```
///
/// `ELEMENT` lists alias, label, aliasAttr, labelAttr, type, descr, techn, sprite, tags, link,
/// bgColor, fontColor, borderColor, shadowing, shape, legendText and legendSprite. `REL` lists
/// type, from, to, label, fromAttr, toAttr, labelAttr, techn, descr, sprite, tags, link,
/// textColor, lineColor, offsetX and offsetY.
pub(crate) fn to_json(model: &Model) -> String {
    use types::C4Element;
    use types::C4Named;

    use crate::yaml::push_number;

    fn key(out: &mut String, name: &str) {
        if !out.ends_with('{') {
            out.push(',');
        }
        out.push('"');
        out.push_str(name);
        out.push_str("\":");
    }

    fn text(out: &mut String, name: &str, value: Option<&String>) {
        if let Some(value) = value {
            key(out, name);
            push_json_string(out, value);
        }
    }

    fn named(out: &mut String, name: &str, value: Option<&C4Named>) {
        if let Some(value) = value {
            key(out, name);
            out.push_str("{\"key\":");
            push_json_string(out, &value.key);
            out.push_str(",\"value\":");
            push_json_string(out, &value.value);
            out.push('}');
        }
    }

    fn number(out: &mut String, name: &str, value: Option<f64>) {
        if let Some(value) = value {
            key(out, name);
            if value.is_finite() {
                push_number(out, value);
            } else {
                out.push_str("null");
            }
        }
    }

    fn extra(out: &mut String, extra: Option<&Vec<(String, String)>>) {
        if let Some(entries) = extra {
            key(out, "extra");
            out.push('[');
            for (i, (name, value)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('[');
                push_json_string(out, name);
                out.push(',');
                push_json_string(out, value);
                out.push(']');
            }
            out.push(']');
        }
    }

    fn element(out: &mut String, el: &C4Element) {
        out.push('{');
        text(out, "alias", Some(&el.alias));
        text(out, "label", Some(&el.label));
        named(out, "aliasAttr", el.alias_attr.as_ref());
        named(out, "labelAttr", el.label_attr.as_ref());
        for (name, value) in [
            ("type", &el.r#type),
            ("descr", &el.descr),
            ("techn", &el.techn),
            ("sprite", &el.sprite),
            ("tags", &el.tags),
            ("link", &el.link),
            ("bgColor", &el.bg_color),
            ("fontColor", &el.font_color),
            ("borderColor", &el.border_color),
            ("shadowing", &el.shadowing),
            ("shape", &el.shape),
            ("legendText", &el.legend_text),
            ("legendSprite", &el.legend_sprite),
        ] {
            text(out, name, value.as_ref());
        }
        key(out, "parent");
        match el.parent {
            Some(parent) => {
                #[expect(clippy::cast_precision_loss, reason = "indexes stay far below 2^52")]
                let parent = parent as f64;
                push_number(out, parent);
            }
            None => out.push_str("-1"),
        }
    }

    let mut out = String::from("{\"type\":\"c4\"");
    text(&mut out, "c4Type", model.c4_type.as_ref());
    text(&mut out, "title", model.title.as_ref());
    text(&mut out, "accTitle", model.acc_title.as_ref());
    text(&mut out, "accDescr", model.acc_descr.as_ref());
    out.push_str(",\"shapes\":[");
    for (i, shape) in model.shapes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        element(&mut out, shape);
        text(&mut out, "kind", Some(&shape.kind));
        extra(&mut out, shape.extra.as_ref());
        out.push('}');
    }
    out.push_str("],\"boundaries\":[");
    for (i, boundary) in model.boundaries.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        element(&mut out, boundary);
        text(&mut out, "nodeType", boundary.node_type.as_ref());
        extra(&mut out, boundary.extra.as_ref());
        out.push('}');
    }
    out.push_str("],\"rels\":[");
    for (i, rel) in model.rels.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('{');
        text(&mut out, "type", Some(&rel.r#type));
        text(&mut out, "from", Some(&rel.from));
        text(&mut out, "to", Some(&rel.to));
        text(&mut out, "label", Some(&rel.label));
        named(&mut out, "fromAttr", rel.from_attr.as_ref());
        named(&mut out, "toAttr", rel.to_attr.as_ref());
        named(&mut out, "labelAttr", rel.label_attr.as_ref());
        for (name, value) in [
            ("techn", &rel.techn),
            ("descr", &rel.descr),
            ("sprite", &rel.sprite),
            ("tags", &rel.tags),
            ("link", &rel.link),
            ("textColor", &rel.text_color),
            ("lineColor", &rel.line_color),
        ] {
            text(&mut out, name, value.as_ref());
        }
        number(&mut out, "offsetX", rel.offset_x);
        number(&mut out, "offsetY", rel.offset_y);
        extra(&mut out, rel.extra.as_ref());
        out.push('}');
    }
    out.push_str("],\"shapeInRow\":");
    push_number(&mut out, model.shape_in_row);
    out.push_str(",\"boundaryInRow\":");
    push_number(&mut out, model.boundary_in_row);
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
