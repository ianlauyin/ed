//! Kanban boards: columns of cards written as an indented outline, with `@{ ... }` metadata.
//! Port of Pele's `src/diagrams/kanban/index.ts`. Pele's `parser.ts` only forwards to the shared
//! outline grammar, so [`parse`] calls it directly.

pub(crate) mod db;
pub(crate) mod render;

use super::Diagram;
use super::Input;
use super::Rendered;
use super::common::outline::parse_outline;
use crate::error::Error;
use crate::svg::root::RenderOptions;
use crate::util::url::UrlOptions;

/// The `kanban` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input, section: None };

fn render_input(input: &Input<'_>, options: &RenderOptions, url: &UrlOptions<'_>) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    // Icons need a host resolver, which WebAssembly hosts do not pass yet.
    Ok(render::render(&model, input.config, options, url, None))
}

/// The parsed kanban model: Pele's `KanbanDb`.
pub(crate) use db::KanbanDb as Model;

/// Parses preprocessed kanban `source`, as Pele's `kanban.parse` does. `title` comes from front
/// matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects or metadata that is not YAML, and a semantic
/// error for a shape name that is not lowercase or a card before any column.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    db.title = title.map(str::to_owned);
    let parsed = parse_outline(source, &mut db, true);
    // An action's error came first: Pele throws it before the parser reads further.
    if let Some(error) = db.error.take() {
        return Err(error);
    }
    parsed?;
    Ok(db)
}

/// Serializes `model` as `JSON.stringify` serializes this view of Pele's parsed `KanbanDb`,
/// which the parity tests build on Pele's side. Fields are picked in a fixed order, since Pele
/// adds them in the order statements set them.
///
/// ```ts
/// const node = (n) => ({
///   id: n.id, level: n.level, label: n.label, icon: n.icon, assigned: n.assigned,
///   ticket: n.ticket, priority: n.priority, cssClasses: n.cssClasses,
/// });
/// JSON.stringify({
///   type: db.type, title: db.title,
///   sections: db.sections.map((s) => ({ ...node(s), items: s.items.map(node) })),
/// });
/// ```
pub(crate) fn to_json(model: &Model) -> String {
    use db::KanbanNode;

    use crate::yaml::push_number;

    /// Appends a node's fields, without the closing brace.
    #[expect(clippy::cast_precision_loss, reason = "levels are far below 2^52")]
    fn node(out: &mut String, node: &KanbanNode) {
        out.push_str("{\"id\":");
        push_json_string(out, &node.id);
        out.push_str(",\"level\":");
        push_number(out, node.level as f64);
        out.push_str(",\"label\":");
        push_json_string(out, &node.label);
        for (key, value) in [
            ("icon", &node.icon),
            ("assigned", &node.assigned),
            ("ticket", &node.ticket),
            ("priority", &node.priority),
            ("cssClasses", &node.css_classes),
        ] {
            if let Some(value) = value {
                out.push_str(",\"");
                out.push_str(key);
                out.push_str("\":");
                push_json_string(out, value);
            }
        }
    }

    let mut out = String::from("{\"type\":\"kanban\"");
    if let Some(title) = &model.title {
        out.push_str(",\"title\":");
        push_json_string(&mut out, title);
    }
    out.push_str(",\"sections\":[");
    for (i, section) in model.sections.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        node(&mut out, &section.node);
        out.push_str(",\"items\":[");
        for (j, item) in section.items.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            node(&mut out, item);
            out.push('}');
        }
        out.push_str("]}");
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
