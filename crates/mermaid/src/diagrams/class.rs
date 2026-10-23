//! Class diagrams: classes with members, namespaces, notes, and relations. Port of Pele's
//! `src/diagrams/class/index.ts`.

pub(crate) mod db;
pub(crate) mod graph;
pub(crate) mod lexer;
pub(crate) mod markers;
pub(crate) mod members;
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

/// The `class` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram {
    keep_comments: false,
    render: render_input,
    section: None,
};

/// Parses and draws a class diagram.
fn render_input(
    input: &Input<'_>,
    options: &RenderOptions,
    url: &UrlOptions<'_>,
) -> Result<Rendered, Error> {
    let db = parse(input.source, input.title)?;
    Ok(render::render(&db, input.config, options, url))
}

/// The parsed class model: Pele's `ClassDb`.
pub(crate) use db::ClassDb as Model;

/// Parses preprocessed class diagram `source`, as Pele's `classDiagram.parse` does. `title`
/// comes from front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        db.title = Some(title.to_owned());
    }
    parser::parse_class_diagram(source, &mut db)?;
    Ok(db)
}

/// Serializes `model` as `JSON.stringify` serializes this view of Pele's parsed `ClassDb`, which
/// the parity tests build on Pele's side. Maps become arrays of their values, a namespace's maps
/// become arrays of their keys, and `undefined` fields are dropped.
///
/// ```ts
/// const member = (m) => ({ id: m.id, memberType: m.memberType, visibility: m.visibility,
///   classifier: m.classifier, parameters: m.parameters, returnType: m.returnType });
/// JSON.stringify({
///   type: db.type, direction: db.direction,
///   classes: [...db.classes.values()].map((c) => ({ id: c.id, type: c.type, label: c.label,
///     cssClasses: c.cssClasses, methods: c.methods.map(member), members: c.members.map(member),
///     annotations: c.annotations, styles: c.styles, parent: c.parent, link: c.link,
///     linkTarget: c.linkTarget, haveCallback: c.haveCallback, tooltip: c.tooltip })),
///   relations: db.relations.map((r) => ({ id1: r.id1, id2: r.id2,
///     relationTitle1: r.relationTitle1, relationTitle2: r.relationTitle2, title: r.title,
///     relation: { type1: r.relation.type1, type2: r.relation.type2, lineType: r.relation.lineType } })),
///   notes: [...db.notes.values()].map((n) => ({ id: n.id, class: n.class, text: n.text,
///     index: n.index, parent: n.parent })),
///   namespaces: [...db.namespaces.values()].map((ns) => ({ id: ns.id, label: ns.label,
///     classes: [...ns.classes.keys()], notes: [...ns.notes.keys()],
///     children: [...ns.children.keys()], parent: ns.parent, explicit: ns.explicit })),
///   interfaces: db.interfaces.map((i) => ({ id: i.id, label: i.label, classId: i.classId })),
///   styleClasses: [...db.styleClasses.values()].map((s) => ({ id: s.id, styles: s.styles,
///     textStyles: s.textStyles })),
///   title: db.title, accTitle: db.accTitle, accDescr: db.accDescr,
/// });
/// ```
#[cfg(feature = "model")]
pub(crate) fn to_json(model: &Model) -> String {
    use json::{Obj, push_json_string};
    use members::ClassMember;
    use types::RelationEnd;

    /// Writes one member.
    fn member(out: &mut String, m: &ClassMember) {
        let mut o = Obj::new(out);
        o.str("id", &m.id);
        o.str("memberType", m.member_type.as_str());
        o.str("visibility", &m.visibility);
        o.str("classifier", &m.classifier);
        o.str("parameters", &m.parameters);
        o.str("returnType", &m.return_type);
        o.end();
    }
    /// Writes a list of members under `key`.
    fn members(o: &mut Obj<'_>, key: &str, list: &[ClassMember]) {
        o.key(key);
        o.out.push('[');
        for (i, m) in list.iter().enumerate() {
            if i > 0 {
                o.out.push(',');
            }
            member(o.out, m);
        }
        o.out.push(']');
    }
    /// Writes a relation end: its type number, or `none`.
    fn end(o: &mut Obj<'_>, key: &str, end: RelationEnd) {
        o.key(key);
        match end {
            RelationEnd::None => push_json_string(o.out, "none"),
            RelationEnd::Type(n) => crate::error::push_u32(o.out, u32::from(n)),
        }
    }
    /// A position as a JSON integer.
    fn index(n: usize) -> u32 {
        u32::try_from(n).unwrap_or(u32::MAX)
    }

    let mut out = String::new();
    let mut root = Obj::new(&mut out);
    root.str("type", "class");
    root.str("direction", model.direction);

    root.key("classes");
    root.out.push('[');
    for (i, c) in model.classes.values.iter().enumerate() {
        if i > 0 {
            root.out.push(',');
        }
        let mut o = Obj::new(root.out);
        o.str("id", &c.id);
        o.str("type", &c.r#type);
        o.str("label", &c.label);
        o.str("cssClasses", &c.css_classes);
        members(&mut o, "methods", &c.methods);
        members(&mut o, "members", &c.members);
        o.list("annotations", &c.annotations);
        o.list("styles", &c.styles);
        o.opt("parent", c.parent.as_deref());
        o.opt("link", c.link.as_deref());
        o.opt("linkTarget", c.link_target.as_deref());
        if c.have_callback {
            o.key("haveCallback");
            o.out.push_str("true");
        }
        o.opt("tooltip", c.tooltip.as_deref());
        o.end();
    }
    root.out.push(']');

    root.key("relations");
    root.out.push('[');
    for (i, r) in model.relations.iter().enumerate() {
        if i > 0 {
            root.out.push(',');
        }
        let mut o = Obj::new(root.out);
        o.str("id1", &r.id1);
        o.str("id2", &r.id2);
        o.str("relationTitle1", &r.relation_title1);
        o.str("relationTitle2", &r.relation_title2);
        o.opt("title", r.title.as_deref());
        o.key("relation");
        let mut rel = Obj::new(o.out);
        end(&mut rel, "type1", r.type1);
        end(&mut rel, "type2", r.type2);
        rel.key("lineType");
        crate::error::push_u32(rel.out, u32::from(r.line_type));
        rel.end();
        o.end();
    }
    root.out.push(']');

    root.key("notes");
    root.out.push('[');
    for (i, n) in model.notes.values.iter().enumerate() {
        if i > 0 {
            root.out.push(',');
        }
        let mut o = Obj::new(root.out);
        o.str("id", &n.id);
        o.opt("class", n.class.as_deref());
        o.str("text", &n.text);
        o.key("index");
        crate::error::push_u32(o.out, index(n.index));
        o.opt("parent", n.parent.as_deref());
        o.end();
    }
    root.out.push(']');

    root.key("namespaces");
    root.out.push('[');
    for (i, ns) in model.namespaces.values.iter().enumerate() {
        if i > 0 {
            root.out.push(',');
        }
        let mut o = Obj::new(root.out);
        o.str("id", &ns.id);
        o.str("label", &ns.label);
        o.list("classes", &ns.classes);
        o.list("notes", &ns.notes);
        o.list("children", &ns.children);
        o.opt("parent", ns.parent.as_deref());
        o.key("explicit");
        o.out.push_str(if ns.explicit { "true" } else { "false" });
        o.end();
    }
    root.out.push(']');

    root.key("interfaces");
    root.out.push('[');
    for (i, item) in model.interfaces.iter().enumerate() {
        if i > 0 {
            root.out.push(',');
        }
        let mut o = Obj::new(root.out);
        o.str("id", &item.id);
        o.str("label", &item.label);
        o.str("classId", &item.class_id);
        o.end();
    }
    root.out.push(']');

    root.key("styleClasses");
    root.out.push('[');
    for (i, s) in model.style_classes.values.iter().enumerate() {
        if i > 0 {
            root.out.push(',');
        }
        let mut o = Obj::new(root.out);
        o.str("id", &s.id);
        o.list("styles", &s.styles);
        o.list("textStyles", &s.text_styles);
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
    //! Parity with Pele's `classDiagram.parse` over the Ondine vectors, every string literal of
    //! Pele's class compat specs, the documents those specs build at runtime, and edge cases, as
    //! recorded by running Pele's TypeScript.

    use super::fixture::cases;
    use super::parse;

    /// The parity fixture: Pele's parser input and its model JSON or error.
    const FIXTURE: &str = include_str!("../../tests/fixtures/class-parse.json");

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
