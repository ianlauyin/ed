//! Mindmaps: an indented outline of nodes around one root, drawn as branches to both sides or as
//! an outline running down. Port of Pele's `src/diagrams/mindmap/index.ts`. Pele's
//! `parser.ts` only forwards to the shared outline grammar, so [`parse`] calls it directly.

pub(crate) mod db;
pub(crate) mod render;

/// The JSON reader of the parity fixtures, loaded once per build.
#[cfg(all(test, not(feature = "sankey")))]
#[path = "../../tests/fixtures/json.rs"]
mod fixture;
#[cfg(all(test, feature = "sankey"))]
use super::sankey::fixture;

use super::common::outline::parse_outline;
use super::{Diagram, Input, Rendered};
use crate::error::Error;
use crate::svg::root::RenderOptions;
use crate::util::url::UrlOptions;

/// The `mindmap` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram {
    keep_comments: false,
    render: render_input,
    section: None,
};

fn render_input(
    input: &Input<'_>,
    options: &RenderOptions,
    _url: &UrlOptions<'_>,
) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    // Icons need a host resolver, which WebAssembly hosts do not pass yet.
    Ok(render::render(&model, input.config, options, None))
}

/// The parsed mindmap model: Pele's `MindmapDb`.
pub(crate) use db::MindmapDb as Model;

/// Parses preprocessed mindmap `source`, as Pele's `mindmap.parse` does. `title` comes from front
/// matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for a second root.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    db.title = title.map(str::to_owned);
    let parsed = parse_outline(source, &mut db, false);
    // An action's error came first: Pele throws it before the parser reads further.
    if let Some(error) = db.error.take() {
        return Err(error);
    }
    parsed?;
    Ok(db)
}

/// Serializes `model` as `JSON.stringify` serializes this view of Pele's parsed `MindmapDb`,
/// which the parity tests build on Pele's side. Children become their ids.
///
/// ```ts
/// JSON.stringify({
///   type: db.type, title: db.title,
///   nodes: db.nodes.map((n) => ({
///     id: n.id, nodeId: n.nodeId, level: n.level, descr: n.descr, type: n.type,
///     children: n.children.map((c) => c.id), isRoot: n.isRoot, section: n.section,
///     class: n.class, icon: n.icon,
///   })),
/// });
/// ```
#[cfg(feature = "model")]
pub(crate) fn to_json(model: &Model) -> String {
    use crate::yaml::push_number;

    #[expect(clippy::cast_precision_loss, reason = "counts are far below 2^52")]
    fn index(out: &mut String, value: usize) {
        push_number(out, value as f64);
    }

    let mut out = String::from("{\"type\":\"mindmap\"");
    if let Some(title) = &model.title {
        out.push_str(",\"title\":");
        push_json_string(&mut out, title);
    }
    out.push_str(",\"nodes\":[");
    for (i, node) in model.nodes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"id\":");
        index(&mut out, node.id);
        out.push_str(",\"nodeId\":");
        push_json_string(&mut out, &node.node_id);
        out.push_str(",\"level\":");
        #[expect(clippy::cast_precision_loss, reason = "levels are far below 2^52")]
        push_number(&mut out, node.level as f64);
        out.push_str(",\"descr\":");
        push_json_string(&mut out, &node.descr);
        out.push_str(",\"type\":");
        push_number(&mut out, f64::from(node.kind));
        out.push_str(",\"children\":[");
        for (j, &child) in node.children.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            index(&mut out, child);
        }
        out.push_str("],\"isRoot\":");
        out.push_str(if node.is_root { "true" } else { "false" });
        if let Some(section) = node.section {
            out.push_str(",\"section\":");
            index(&mut out, section);
        }
        if let Some(class) = &node.class {
            out.push_str(",\"class\":");
            push_json_string(&mut out, class);
        }
        if let Some(icon) = &node.icon {
            out.push_str(",\"icon\":");
            push_json_string(&mut out, icon);
        }
        out.push('}');
    }
    out.push_str("]}");
    out
}

/// Appends `text` as `JSON.stringify` quotes it.
#[cfg(feature = "model")]
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

#[cfg(test)]
mod tests {
    //! Parity with Pele's `mindmap.parse` over the Ondine vectors, every string literal of Pele's
    //! mindmap compat specs, and the documents those specs build at runtime, as recorded by
    //! running Pele's TypeScript.

    use super::fixture::cases;
    use super::parse;

    /// The parity fixture: Pele's parser input and its model JSON or error.
    const FIXTURE: &str = include_str!("../../tests/fixtures/mindmap-parse.json");

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
