//! Architecture diagrams: Mermaid's `architecture-beta` services, junctions, groups, and edges
//! between their sides. Port of Pele's `src/diagrams/architecture/index.ts`.

pub(crate) mod db;
pub(crate) mod icons;
pub(crate) mod layout;
pub(crate) mod parser;
pub(crate) mod render;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::error::Error;
use crate::svg::root::RenderOptions;
use crate::util::url::UrlOptions;

/// The `architecture` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input, section: None };

fn render_input(input: &Input<'_>, options: &RenderOptions, _url: &UrlOptions<'_>) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render_architecture(&model, input.config, options))
}

/// The parsed architecture model: Pele's `ArchitectureDb`.
pub(crate) use db::ArchitectureDb as Model;

/// Parses preprocessed architecture `source`, as Pele's `architecture.parse` does. `title` comes
/// from front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for a model Mermaid's
/// database refuses.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        db.title = Some(title.to_owned());
    }
    db::populate(parser::parse_architecture(source)?, &mut db)?;
    Ok(db)
}

/// Serializes `model` as `JSON.stringify` serializes this view of Pele's parsed
/// `ArchitectureDb`, which the parity tests build on Pele's side (`undefined` fields drop out):
///
/// ```ts
/// JSON.stringify({
///   type: db.type, title: db.title, accTitle: db.accTitle, accDescr: db.accDescr,
///   nodes: [...db.nodes.values()].map((n) =>
///     ({ id: n.id, type: n.type, icon: n.icon, iconText: n.iconText, title: n.title, in: n.in })),
///   groups: [...db.groups.values()].map((g) => ({ id: g.id, icon: g.icon, title: g.title, in: g.in })),
///   edges: db.edges.map((e) => ({ lhsId: e.lhsId, lhsDir: e.lhsDir, lhsInto: e.lhsInto,
///     lhsGroup: e.lhsGroup, rhsId: e.rhsId, rhsDir: e.rhsDir, rhsInto: e.rhsInto,
///     rhsGroup: e.rhsGroup, title: e.title })),
///   layoutHints: db.layoutHints.map((h) => ({ direction: h.direction, members: h.members })),
/// });
/// ```
pub(crate) fn to_json(model: &Model) -> String {
    /// Appends `text` as `JSON.stringify` quotes it.
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

    fn field(out: &mut String, key: &str, value: Option<&str>) {
        if let Some(value) = value {
            out.push_str(",\"");
            out.push_str(key);
            out.push_str("\":");
            string(out, value);
        }
    }

    fn flag(out: &mut String, key: &str, value: bool) {
        out.push_str(",\"");
        out.push_str(key);
        out.push_str(if value { "\":true" } else { "\":false" });
    }

    let mut out = String::from("{\"type\":\"architecture\"");
    field(&mut out, "title", model.title.as_deref());
    field(&mut out, "accTitle", model.acc_title.as_deref());
    field(&mut out, "accDescr", model.acc_descr.as_deref());
    out.push_str(",\"nodes\":[");
    for (i, node) in model.nodes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"id\":");
        string(&mut out, &node.id);
        out.push_str(match node.kind {
            db::NodeKind::Junction => ",\"type\":\"junction\"",
            db::NodeKind::Service => ",\"type\":\"service\"",
        });
        field(&mut out, "icon", node.icon.as_deref());
        field(&mut out, "iconText", node.icon_text.as_deref());
        field(&mut out, "title", node.title.as_deref());
        field(&mut out, "in", node.r#in.as_deref());
        out.push('}');
    }
    out.push_str("],\"groups\":[");
    for (i, group) in model.groups.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"id\":");
        string(&mut out, &group.id);
        field(&mut out, "icon", group.icon.as_deref());
        field(&mut out, "title", group.title.as_deref());
        field(&mut out, "in", group.r#in.as_deref());
        out.push('}');
    }
    out.push_str("],\"edges\":[");
    for (i, edge) in model.edges.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"lhsId\":");
        string(&mut out, &edge.lhs_id);
        field(&mut out, "lhsDir", Some(edge.lhs_dir.as_str()));
        flag(&mut out, "lhsInto", edge.lhs_into);
        flag(&mut out, "lhsGroup", edge.lhs_group);
        field(&mut out, "rhsId", Some(&edge.rhs_id));
        field(&mut out, "rhsDir", Some(edge.rhs_dir.as_str()));
        flag(&mut out, "rhsInto", edge.rhs_into);
        flag(&mut out, "rhsGroup", edge.rhs_group);
        field(&mut out, "title", edge.title.as_deref());
        out.push('}');
    }
    out.push_str("],\"layoutHints\":[");
    for (i, hint) in model.layout_hints.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"direction\":\"");
        out.push_str(hint.direction.as_str());
        out.push_str("\",\"members\":[");
        for (j, member) in hint.members.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            string(&mut out, member);
        }
        out.push_str("]}");
    }
    out.push_str("]}");
    out
}
