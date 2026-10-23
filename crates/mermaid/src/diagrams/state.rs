//! State diagrams: Mermaid's `stateDiagram` and `stateDiagram-v2`. Port of Pele's
//! `src/diagrams/state/index.ts`.

pub(crate) mod db;
pub(crate) mod graph;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod render;
pub(crate) mod tokens;
pub(crate) mod types;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::error::Error;
use crate::svg::root::RenderOptions;
use crate::util::url::UrlOptions;

/// The `state` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input, section: None };

fn render_input(input: &Input<'_>, options: &RenderOptions, url: &UrlOptions<'_>) -> Result<Rendered, Error> {
    let db = parse(input.source, input.title)?;
    Ok(render::render(&db, input.config, options, url))
}

/// The parsed state diagram model: Pele's `StateDb`.
pub(crate) use db::StateDb as Model;

/// Parses preprocessed state diagram `source`, as Pele's `state.parse` does. `title` comes from
/// front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for a composite state
/// with a description.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        db.title = Some(title.to_owned());
    }
    parser::parse_state(source, &mut db)?;
    Ok(db)
}

/// Serializes `model` as `JSON.stringify` serializes Pele's parsed `StateDb` after this
/// normalization, which the parity tests apply on Pele's side:
///
/// - The top level holds `accDescr`, `accTitle`, `classes`, `direction`, `graph`, `links`,
///   `relations`, `rootDoc`, `states`, `title`, and `type`.
/// - Every `Map` becomes an array of `[key, value]` pairs in insertion order.
/// - Every object's keys are sorted, and `undefined` fields are dropped.
///
/// On Pele's side:
///
/// ```ts
/// const norm = (v) =>
///   v instanceof Map ? [...v].map(([k, x]) => [k, norm(x)])
///   : Array.isArray(v) ? v.map(norm)
///   : v && typeof v === 'object'
///     ? Object.fromEntries(Object.keys(v).sort().filter((k) => v[k] !== undefined).map((k) => [k, norm(v[k])]))
///     : v;
/// JSON.stringify(norm({ accDescr: db.accDescr, accTitle: db.accTitle, classes: db.classes,
///   direction: db.direction, graph: db.graph, links: db.links, relations: db.relations,
///   rootDoc: db.rootDoc, states: db.states, title: db.title, type: db.type }));
/// ```
pub(crate) fn to_json(model: &Model) -> String {
    use json::bool;
    use json::obj;
    use json::opt;
    use json::string;
    use json::strings;

    let classes = model.classes.iter().map(|class| {
        pair(
            &class.id,
            obj(vec![
                ("id", string(&class.id)),
                ("styles", strings(&class.styles)),
                ("textStyles", strings(&class.text_styles)),
            ]),
        )
    });
    let nodes = model.graph.nodes.iter().map(|node| {
        let mut fields = vec![
            ("cssClasses", string(&node.css_classes)),
            ("cssCompiledStyles", strings(&node.css_compiled_styles)),
            ("cssStyles", strings(&node.css_styles)),
            ("description", strings(&node.description)),
            ("id", string(&node.id)),
            ("isGroup", bool(node.is_group)),
        ];
        if let Some(index) = node.color_index {
            let mut number = String::new();
            crate::error::push_u32(&mut number, index);
            fields.push(("colorIndex", number));
        }
        opt(&mut fields, "dir", node.dir);
        opt(&mut fields, "label", node.label.as_deref());
        opt(&mut fields, "noteFor", node.note_for.as_deref());
        opt(&mut fields, "parentId", node.parent_id.as_deref());
        opt(&mut fields, "position", node.position.as_deref());
        opt(&mut fields, "shape", node.shape);
        opt(&mut fields, "type", node.r#type);
        obj(fields)
    });
    let edges = model.graph.edges.iter().map(|edge| {
        let mut fields = vec![
            ("classes", string(edge.classes)),
            ("end", string(&edge.end)),
            ("id", string(&edge.id)),
            ("label", string(&edge.label)),
            ("start", string(&edge.start)),
        ];
        opt(&mut fields, "pattern", edge.pattern);
        obj(fields)
    });
    let links = model
        .links
        .iter()
        .map(|(id, link)| pair(id, obj(vec![("tooltip", string(&link.tooltip)), ("url", string(&link.url))])));
    let relations = model.relations.iter().map(|relation| {
        obj(vec![
            ("id1", string(&relation.id1)),
            ("id2", string(&relation.id2)),
            ("relationTitle", string(&relation.relation_title)),
        ])
    });
    let states = model.states.iter().enumerate().map(|(at, state)| {
        let body = model.state_docs.get(at).copied().flatten().and_then(|at| match model.root_doc.get(at) {
            Some(types::Stmt::State(owner)) => owner.doc.as_ref(),
            _ => None,
        });
        pair(&state.id, json::state(state, body))
    });

    let mut fields = vec![
        ("classes", json::array(classes)),
        ("direction", string(model.direction())),
        ("graph", obj(vec![("edges", json::array(edges)), ("nodes", json::array(nodes))])),
        ("links", json::array(links)),
        ("relations", json::array(relations)),
        ("rootDoc", json::doc(&model.root_doc)),
        ("states", json::array(states)),
        ("type", string("state")),
    ];
    opt(&mut fields, "accDescr", model.acc_descr.as_deref());
    opt(&mut fields, "accTitle", model.acc_title.as_deref());
    opt(&mut fields, "title", model.title.as_deref());
    obj(fields)
}

/// A `[key, value]` pair, as a `Map` entry serializes.
fn pair(key: &str, value: String) -> String {
    json::array([json::string(key), value].into_iter())
}

/// A JSON writer that matches `JSON.stringify` output, for sorted-key objects.
mod json {
    use super::types::Description;
    use super::types::Doc;
    use super::types::StateStmt;
    use super::types::Stmt;

    pub(super) fn string(text: &str) -> String {
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
        out
    }

    pub(super) fn bool(value: bool) -> String {
        String::from(if value { "true" } else { "false" })
    }

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

    pub(super) fn strings(items: &[String]) -> String {
        array(items.iter().map(|item| string(item)))
    }

    /// An object with its keys sorted.
    pub(super) fn obj(mut fields: Vec<(&str, String)>) -> String {
        fields.sort_by(|a, b| a.0.cmp(b.0));
        let mut out = String::from("{");
        for (i, (key, value)) in fields.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&string(key));
            out.push(':');
            out.push_str(value);
        }
        out.push('}');
        out
    }

    pub(super) fn opt<'a>(fields: &mut Vec<(&'a str, String)>, key: &'a str, value: Option<&str>) {
        if let Some(value) = value {
            fields.push((key, string(value)));
        }
    }

    /// A state with its body, which is `state.doc` except for a state the model records.
    pub(super) fn state(state: &StateStmt, body: Option<&Doc>) -> String {
        let mut fields = vec![("id", string(&state.id)), ("stmt", string("state"))];
        if let Some(classes) = &state.classes {
            fields.push(("classes", strings(classes)));
        }
        match &state.description {
            Some(Description::Many(texts)) => fields.push(("description", strings(texts))),
            Some(Description::One(text)) => fields.push(("description", string(text))),
            None => {}
        }
        if let Some(descriptions) = &state.descriptions {
            fields.push(("descriptions", strings(descriptions)));
        }
        if let Some(body) = body {
            fields.push(("doc", doc(&body.0)));
        }
        if let Some(note) = &state.note {
            let mut note_fields = vec![("text", string(&note.text))];
            opt(&mut note_fields, "position", note.position.as_deref());
            fields.push(("note", obj(note_fields)));
        }
        if let Some(start) = state.start {
            fields.push(("start", bool(start)));
        }
        if let Some(styles) = &state.styles {
            fields.push(("styles", strings(styles)));
        }
        if let Some(text_styles) = &state.text_styles {
            fields.push(("textStyles", strings(text_styles)));
        }
        opt(&mut fields, "type", state.r#type);
        obj(fields)
    }

    pub(super) fn doc(items: &[Stmt]) -> String {
        array(items.iter().map(stmt))
    }

    fn stmt(item: &Stmt) -> String {
        match item {
            Stmt::ApplyClass { id, style_class } => {
                obj(vec![("id", string(id)), ("stmt", string("applyClass")), ("styleClass", string(style_class))])
            }
            Stmt::ClassDef { classes, id } => {
                obj(vec![("classes", string(classes)), ("id", string(id)), ("stmt", string("classDef"))])
            }
            Stmt::Click { id, tooltip, url } => obj(vec![
                ("id", state(id, id.doc.as_ref())),
                ("stmt", string("click")),
                ("tooltip", string(tooltip)),
                ("url", string(url)),
            ]),
            Stmt::Dir(value) => obj(vec![("stmt", string("dir")), ("value", string(value))]),
            Stmt::Relation(relation) => {
                let mut fields = vec![
                    ("state1", state(&relation.state1, relation.state1.doc.as_ref())),
                    ("state2", state(&relation.state2, relation.state2.doc.as_ref())),
                    ("stmt", string("relation")),
                ];
                opt(&mut fields, "description", relation.description.as_deref());
                obj(fields)
            }
            Stmt::State(item) => state(item, item.doc.as_ref()),
            Stmt::Style { id, style_class } => {
                obj(vec![("id", string(id)), ("stmt", string("style")), ("styleClass", string(style_class))])
            }
            Stmt::Text(text) => string(text),
        }
    }
}
