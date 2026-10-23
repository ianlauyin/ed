//! Entity relationship diagrams: entities with attributes, relationships, and subgraphs. Port of
//! Pele's `src/diagrams/er/index.ts`.

pub(crate) mod db;
pub(crate) mod graph;
pub(crate) mod lexer;
pub(crate) mod markers;
pub(crate) mod parser;
pub(crate) mod render;
pub(crate) mod tokens;
pub(crate) mod types;

/// The JSON reader of the parity fixtures, loaded once per build.
#[cfg(all(test, not(feature = "sankey")))]
#[path = "../../tests/fixtures/json.rs"]
mod fixture;
#[cfg(all(test, feature = "sankey"))]
use super::sankey::fixture;

use super::{Diagram, Input, Rendered};
use crate::error::Error;
use crate::svg::root::RenderOptions;
use crate::util::url::UrlOptions;

/// The `er` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram {
    keep_comments: false,
    render: render_input,
    section: None,
};

/// Parses and draws an ER diagram.
fn render_input(
    input: &Input<'_>,
    options: &RenderOptions,
    url: &UrlOptions<'_>,
) -> Result<Rendered, Error> {
    let db = parse(input.source, input.title)?;
    Ok(render::render(&db, input.config, options, url))
}

/// The parsed ER model: Pele's `ErDb`.
pub(crate) use db::ErDb as Model;

/// Parses preprocessed ER diagram `source`, as Pele's `er.parse` does. `title` comes from front
/// matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        db.title = Some(title.to_owned());
    }
    parser::parse_er(source, &mut db)?;
    Ok(db)
}

/// Serializes `model` as `JSON.stringify` serializes this view of Pele's parsed `ErDb`, which the
/// parity tests build on Pele's side. Maps become arrays of their values, and `undefined` fields
/// are dropped.
///
/// ```ts
/// JSON.stringify({
///   type: db.type, direction: db.direction,
///   entities: [...db.entities.values()].map((e) => ({ id: e.id, label: e.label, alias: e.alias,
///     attributes: e.attributes.map((a) => ({ type: a.type, name: a.name, keys: a.keys,
///       comment: a.comment })),
///     cssClasses: e.cssClasses, cssStyles: e.cssStyles })),
///   relationships: db.relationships.map((r) => ({ entityA: r.entityA, roleA: r.roleA,
///     entityB: r.entityB,
///     relSpec: { cardA: r.relSpec.cardA, relType: r.relSpec.relType, cardB: r.relSpec.cardB } })),
///   classes: [...db.classes.values()].map((c) => ({ id: c.id, styles: c.styles,
///     textStyles: c.textStyles })),
///   subgraphs: db.subgraphs.map((s) => ({ id: s.id, nodes: s.nodes, title: s.title,
///     classes: s.classes, cssStyles: s.cssStyles, dir: s.dir, labelType: s.labelType })),
///   title: db.title, accTitle: db.accTitle, accDescr: db.accDescr,
/// });
/// ```
#[cfg(feature = "model")]
pub(crate) fn to_json(model: &Model) -> String {
    use json::Obj;

    let mut out = String::new();
    let mut root = Obj::new(&mut out);
    root.str("type", "er");
    root.str("direction", model.direction);

    root.key("entities");
    root.out.push('[');
    for (i, e) in model.entities.values.iter().enumerate() {
        if i > 0 {
            root.out.push(',');
        }
        let mut o = Obj::new(root.out);
        o.str("id", &e.id);
        o.str("label", &e.label);
        o.str("alias", &e.alias);
        o.key("attributes");
        o.out.push('[');
        for (k, a) in e.attributes.iter().enumerate() {
            if k > 0 {
                o.out.push(',');
            }
            let mut attribute = Obj::new(o.out);
            attribute.str("type", &a.r#type);
            attribute.str("name", &a.name);
            attribute.list("keys", &a.keys);
            attribute.str("comment", &a.comment);
            attribute.end();
        }
        o.out.push(']');
        o.str("cssClasses", &e.css_classes);
        o.list("cssStyles", &e.css_styles);
        o.end();
    }
    root.out.push(']');

    root.key("relationships");
    root.out.push('[');
    for (i, r) in model.relationships.iter().enumerate() {
        if i > 0 {
            root.out.push(',');
        }
        let mut o = Obj::new(root.out);
        o.str("entityA", &r.entity_a);
        o.str("roleA", &r.role_a);
        o.str("entityB", &r.entity_b);
        o.key("relSpec");
        let mut spec = Obj::new(o.out);
        spec.str("cardA", r.rel_spec.card_a.as_str());
        spec.str(
            "relType",
            if r.rel_spec.identifying {
                "IDENTIFYING"
            } else {
                "NON_IDENTIFYING"
            },
        );
        spec.str("cardB", r.rel_spec.card_b.as_str());
        spec.end();
        o.end();
    }
    root.out.push(']');

    root.key("classes");
    root.out.push('[');
    for (i, c) in model.classes.values.iter().enumerate() {
        if i > 0 {
            root.out.push(',');
        }
        let mut o = Obj::new(root.out);
        o.str("id", &c.id);
        o.list("styles", &c.styles);
        o.list("textStyles", &c.text_styles);
        o.end();
    }
    root.out.push(']');

    root.key("subgraphs");
    root.out.push('[');
    for (i, s) in model.subgraphs.iter().enumerate() {
        if i > 0 {
            root.out.push(',');
        }
        let mut o = Obj::new(root.out);
        o.str("id", &s.id);
        o.list("nodes", &s.nodes);
        o.str("title", &s.title);
        o.list("classes", &s.classes);
        o.list("cssStyles", &s.css_styles);
        o.opt("dir", s.dir);
        o.str("labelType", "markdown");
        o.end();
    }
    root.out.push(']');

    root.opt("title", model.title.as_deref());
    root.opt("accTitle", model.acc_title.as_deref());
    root.opt("accDescr", model.acc_descr.as_deref());
    root.end();
    out
}

/// A minimal writer of the JSON that `JSON.stringify` produces, for [`to_json`].
#[cfg(feature = "model")]
mod json {
    /// An object being written: its output and whether a member was written yet.
    pub(super) struct Obj<'a> {
        first: bool,
        /// The output.
        pub(super) out: &'a mut String,
    }

    impl<'a> Obj<'a> {
        /// Opens an object.
        pub(super) fn new(out: &'a mut String) -> Self {
            out.push('{');
            Self { first: true, out }
        }

        /// Writes a member's key.
        pub(super) fn key(&mut self, key: &str) {
            if !self.first {
                self.out.push(',');
            }
            self.first = false;
            push_json_string(self.out, key);
            self.out.push(':');
        }

        /// Writes a string member.
        pub(super) fn str(&mut self, key: &str, value: &str) {
            self.key(key);
            push_json_string(self.out, value);
        }

        /// Writes a string member, or nothing for `None`, as `undefined` is dropped.
        pub(super) fn opt(&mut self, key: &str, value: Option<&str>) {
            if let Some(value) = value {
                self.str(key, value);
            }
        }

        /// Writes a string-array member.
        pub(super) fn list(&mut self, key: &str, items: &[String]) {
            self.key(key);
            push_list(self.out, items);
        }

        /// Closes the object.
        pub(super) fn end(self) {
            self.out.push('}');
        }
    }

    /// Appends a JSON array of strings.
    pub(super) fn push_list(out: &mut String, items: &[String]) {
        out.push('[');
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            push_json_string(out, item);
        }
        out.push(']');
    }

    /// Appends `text` as `JSON.stringify` quotes it.
    pub(super) fn push_json_string(out: &mut String, text: &str) {
        /// Hexadecimal digits for `\u00XX` escapes.
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
}

#[cfg(test)]
mod tests {
    //! Parity with Pele's `er.parse` over the Ondine vectors, every string literal of
    //! Pele's er compat specs, the documents those specs build at runtime, and edge cases, as
    //! recorded by running Pele's TypeScript.

    use super::fixture::cases;
    use super::parse;

    /// The parity fixture: Pele's parser input and its model JSON or error.
    const FIXTURE: &str = include_str!("../../tests/fixtures/er-parse.json");

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
