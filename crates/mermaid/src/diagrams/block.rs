//! Block diagrams: Mermaid's `block-beta` grid of blocks, composites, and links. Port of Pele's
//! `src/diagrams/block/index.ts`.

pub(crate) mod arrows;
pub(crate) mod db;
pub(crate) mod layout;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod render;
pub(crate) mod tokens;
pub(crate) mod types;

use super::{Diagram, Input, Rendered};
use crate::error::Error;
use crate::svg::root::RenderOptions;
use crate::util::url::UrlOptions;

/// The `block` diagram type.
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
    render::render(&model, input.config, options)
}

/// The parsed block diagram model: Pele's `BlockDb`.
pub(crate) use db::BlockDb as Model;

/// Parses preprocessed block diagram `source`, as Pele's `block.parse` does. `title` comes from
/// front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a limit error past 100,000 space blocks.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    db.title = title.map(str::to_owned);
    parser::parse_block(source, &mut db)?;
    Ok(db)
}

/// Serializes `model` as `JSON.stringify` serializes Pele's parsed `BlockDb` after this
/// normalization, which the parity tests apply on Pele's side. Maps become arrays of entries, so
/// their order is insertion order, and each block lists its fields in the order of Pele's `Block`
/// interface, without the `undefined` ones.
///
/// ```ts
/// const BLOCK = ['id', 'type', 'label', 'children', 'columns', 'widthInColumns', 'width',
///   'directions', 'classes', 'styles', 'colorIndex', 'start', 'end', 'arrowTypeStart',
///   'arrowTypeEnd', 'thickness', 'pattern', 'css', 'styleClass', 'stylesStr'];
/// const pick = (o, keys, map = (k, v) => v) => Object.fromEntries(
///   keys.filter((k) => o[k] !== undefined).map((k) => [k, map(k, o[k])]));
/// const block = (b) => pick(b, BLOCK, (k, v) => (k === 'children' ? v.map(block) : v));
/// JSON.stringify({
///   type: model.type, root: block(model.root), edges: model.edges.map(block),
///   classes: [...model.classes.values()].map((c) => pick(c, ['id', 'styles', 'textStyles'])),
///   byId: [...model.byId].map(([k, v]) => [k, block(v)]), title: model.title,
/// });
/// ```
#[cfg(feature = "model")]
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

    fn strings(out: &mut String, items: &[String]) {
        out.push('[');
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            string(out, item);
        }
        out.push(']');
    }

    fn key(out: &mut String, first: &mut bool, name: &str) {
        if !*first {
            out.push(',');
        }
        *first = false;
        string(out, name);
        out.push(':');
    }

    fn block(out: &mut String, model: &Model, index: usize) {
        let Some(b) = model.arena.get(index) else {
            out.push_str("null");
            return;
        };
        let mut first = true;
        out.push('{');
        let text = |out: &mut String, first: &mut bool, name: &str, value: Option<&str>| {
            if let Some(value) = value {
                key(out, first, name);
                string(out, value);
            }
        };
        let number = |out: &mut String, first: &mut bool, name: &str, value: Option<f64>| {
            if let Some(value) = value {
                key(out, first, name);
                if value.is_finite() {
                    push_number(out, value);
                } else {
                    out.push_str("null");
                }
            }
        };
        let list = |out: &mut String, first: &mut bool, name: &str, value: Option<&[String]>| {
            if let Some(value) = value {
                key(out, first, name);
                strings(out, value);
            }
        };
        text(out, &mut first, "id", b.id.as_deref());
        text(out, &mut first, "type", Some(b.r#type));
        text(out, &mut first, "label", b.label.as_deref());
        if let Some(children) = &b.children {
            key(out, &mut first, "children");
            out.push('[');
            for (i, &child) in children.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                block(out, model, child);
            }
            out.push(']');
        }
        number(out, &mut first, "columns", b.columns);
        number(out, &mut first, "widthInColumns", b.width_in_columns);
        number(out, &mut first, "width", b.width);
        list(out, &mut first, "directions", b.directions.as_deref());
        list(out, &mut first, "classes", b.classes.as_deref());
        list(out, &mut first, "styles", b.styles.as_deref());
        number(out, &mut first, "colorIndex", b.color_index.map(f64::from));
        text(out, &mut first, "start", b.start.as_deref());
        text(out, &mut first, "end", b.end.as_deref());
        text(out, &mut first, "arrowTypeStart", b.arrow_type_start);
        text(out, &mut first, "arrowTypeEnd", b.arrow_type_end);
        text(out, &mut first, "thickness", b.thickness);
        text(out, &mut first, "pattern", b.pattern);
        text(out, &mut first, "css", b.css.as_deref());
        text(out, &mut first, "styleClass", b.style_class.as_deref());
        text(out, &mut first, "stylesStr", b.styles_str.as_deref());
        out.push('}');
    }

    let mut out = String::from("{\"type\":\"block\",\"root\":");
    block(&mut out, model, db::ROOT);
    out.push_str(",\"edges\":[");
    for (i, &edge) in model.edges.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        block(&mut out, model, edge);
    }
    out.push_str("],\"classes\":[");
    for (i, class) in model.classes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"id\":");
        string(&mut out, &class.id);
        out.push_str(",\"styles\":");
        strings(&mut out, &class.styles);
        out.push_str(",\"textStyles\":");
        strings(&mut out, &class.text_styles);
        out.push('}');
    }
    out.push_str("],\"byId\":[");
    for (i, (id, index)) in model.by_id.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('[');
        string(&mut out, id);
        out.push(',');
        block(&mut out, model, *index);
        out.push(']');
    }
    out.push(']');
    if let Some(title) = &model.title {
        out.push_str(",\"title\":");
        string(&mut out, title);
    }
    out.push('}');
    out
}

#[cfg(test)]
mod tests {
    //! Parity with Pele's `parse()` over every string literal of Pele's block compat specs. The
    //! fixture was generated by running Pele's TypeScript on each literal, once as the public
    //! `parse()` sees it (preprocessed, when it is detected as this type) and once as written.

    use std::fmt::Write;

    use super::parse;

    const FIXTURE: &str = include_str!("../../tests/fixtures/block-parse.json");

    /// A JSON value of the fixture.
    #[derive(Debug, PartialEq)]
    enum Json {
        Arr(Vec<Json>),
        Null,
        Num(f64),
        Obj(Vec<(String, Json)>),
        Str(String),
    }

    impl Json {
        fn get(&self, key: &str) -> Option<&Json> {
            let Json::Obj(entries) = self else {
                return None;
            };
            entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
        }

        fn str(&self, key: &str) -> Option<&str> {
            match self.get(key) {
                Some(Json::Str(s)) => Some(s),
                _ => None,
            }
        }
    }

    /// Reads the fixture: arrays, objects, strings, numbers, and `null`.
    fn read(bytes: &[u8], at: &mut usize) -> Json {
        while bytes[*at].is_ascii_whitespace() {
            *at += 1;
        }
        match bytes[*at] {
            open @ (b'[' | b'{') => {
                *at += 1;
                let mut items = Vec::new();
                let mut entries = Vec::new();
                loop {
                    while bytes[*at].is_ascii_whitespace() || bytes[*at] == b',' {
                        *at += 1;
                    }
                    if bytes[*at] == b']' || bytes[*at] == b'}' {
                        *at += 1;
                        break;
                    }
                    if open == b'[' {
                        items.push(read(bytes, at));
                    } else {
                        let Json::Str(key) = read(bytes, at) else {
                            panic!("key");
                        };
                        while bytes[*at] != b':' {
                            *at += 1;
                        }
                        *at += 1;
                        entries.push((key, read(bytes, at)));
                    }
                }
                if open == b'[' {
                    Json::Arr(items)
                } else {
                    Json::Obj(entries)
                }
            }
            b'"' => {
                *at += 1;
                let mut units: Vec<u16> = Vec::new();
                loop {
                    let c = bytes[*at];
                    *at += 1;
                    match c {
                        b'"' => break,
                        b'\\' => {
                            let e = bytes[*at];
                            *at += 1;
                            units.push(match e {
                                b'n' => 10,
                                b'r' => 13,
                                b't' => 9,
                                b'b' => 8,
                                b'f' => 12,
                                b'u' => {
                                    let hex = std::str::from_utf8(&bytes[*at..*at + 4]).unwrap();
                                    *at += 4;
                                    u16::from_str_radix(hex, 16).unwrap()
                                }
                                other => u16::from(other),
                            });
                        }
                        _ => {
                            let start = *at - 1;
                            let len = match c {
                                0..0x80 => 1,
                                0xc0..0xe0 => 2,
                                0xe0..0xf0 => 3,
                                _ => 4,
                            };
                            let s = std::str::from_utf8(&bytes[start..start + len]).unwrap();
                            units.extend(s.encode_utf16());
                            *at = start + len;
                        }
                    }
                }
                Json::Str(String::from_utf16(&units).unwrap())
            }
            b'n' => {
                *at += 4;
                Json::Null
            }
            _ => {
                let start = *at;
                while b"+-.eE0123456789".contains(&bytes[*at]) {
                    *at += 1;
                }
                Json::Num(
                    std::str::from_utf8(&bytes[start..*at])
                        .unwrap()
                        .parse()
                        .unwrap(),
                )
            }
        }
    }

    /// A model's JSON when the `model` feature is on, or an error's code, message, line, and
    /// column.
    type Outcome = Result<Option<String>, (String, String, u32, u32)>;

    fn actual(case: &Json) -> Outcome {
        let input = case.str("input").unwrap();
        match parse(input, case.str("title")) {
            #[cfg(feature = "model")]
            Ok(model) => Ok(Some(super::to_json(&model))),
            #[cfg(not(feature = "model"))]
            Ok(_) => Ok(None),
            Err(error) => Err((
                error.code.as_str().to_owned(),
                error.message,
                error.line,
                error.column,
            )),
        }
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "test data"
    )]
    fn expected(case: &Json) -> Outcome {
        if let Some(model) = case.str("model") {
            return Ok(cfg!(feature = "model").then(|| model.to_owned()));
        }
        let error = case.get("error").unwrap();
        let num = |key| match error.get(key) {
            Some(Json::Num(n)) => *n as u32,
            _ => 0,
        };
        Err((
            error.str("code").unwrap().to_owned(),
            error.str("message").unwrap().to_owned(),
            num("line"),
            num("column"),
        ))
    }

    #[test]
    fn matches_pele() {
        let Json::Arr(cases) = read(FIXTURE.as_bytes(), &mut 0) else {
            panic!("fixture is not an array");
        };

        let mut report = String::new();
        let mut differing = 0;
        for case in &cases {
            let (actual, expected) = (actual(case), expected(case));
            if actual != expected {
                differing += 1;
                if differing <= 20 {
                    write!(
                        report,
                        "\n{}\n  input: {:?}\n  pele:  {expected:?}\n  ondine: {actual:?}",
                        case.str("origin").unwrap_or("?"),
                        case.str("input").unwrap(),
                    )
                    .unwrap();
                }
            }
        }
        assert!(
            differing == 0,
            "{differing} of {} cases differ from Pele:{report}",
            cases.len()
        );
    }
}
