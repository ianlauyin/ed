//! Railroad diagrams: grammar rules drawn as tracks, written in Mermaid's own notation, EBNF,
//! ABNF, or PEG. Port of Pele's `src/diagrams/railroad/index.ts`.

pub(crate) mod abnf;
pub(crate) mod common;
pub(crate) mod db;
pub(crate) mod ebnf;
pub(crate) mod ir;
pub(crate) mod peg;
pub(crate) mod render;
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

/// The `railroad` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram {
    keep_comments: false,
    render: render_input,
    section: None,
};

/// Parses and draws the diagram.
fn render_input(
    input: &Input<'_>,
    options: &RenderOptions,
    _url: &UrlOptions<'_>,
) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render(&model, input.config, options))
}

/// The parsed railroad diagram: Pele's `RailroadDb`.
pub(crate) use types::RailroadModel as Model;

/// Parses preprocessed railroad `source`, as Pele's `railroad.parse` does. `title` comes from
/// front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    db::parse_railroad(source, title)
}

/// Serializes `model` as the parity tests serialize Pele's `RailroadDb`: its public fields, keys
/// sorted, absent fields left out, and an unbounded count as the string `Infinity`.
#[cfg(feature = "model")]
pub(crate) fn to_json(model: &Model) -> String {
    use types::RailroadNode;

    /// What is left to write: text, a count, or a node.
    enum Work<'a> {
        /// A repetition count.
        Count(f64),
        /// A node of the definition.
        Node(usize),
        /// Literal JSON.
        Text(&'a str),
    }

    let mut out = String::from("{");
    if let Some(text) = &model.acc_descr {
        out.push_str("\"accDescr\":");
        push_json_string(&mut out, text);
        out.push(',');
    }
    if let Some(text) = &model.acc_title {
        out.push_str("\"accTitle\":");
        push_json_string(&mut out, text);
        out.push(',');
    }
    out.push_str("\"notation\":\"");
    out.push_str(model.notation.as_str());
    out.push_str("\",\"rules\":[");

    // The walk keeps its own stack, so deep definitions do not overflow.
    for (i, rule) in model.rules.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"definition\":");
        let mut stack = vec![Work::Node(rule.definition)];
        while let Some(work) = stack.pop() {
            match work {
                Work::Text(text) => out.push_str(text),
                Work::Count(value) if value.is_finite() => {
                    crate::yaml::push_number(&mut out, value);
                }
                Work::Count(_) => out.push_str("\"Infinity\""),
                Work::Node(index) => match model.nodes.get(index) {
                    Some(RailroadNode::Terminal(value)) => {
                        out.push_str("{\"type\":\"terminal\",\"value\":");
                        push_json_string(&mut out, value);
                        out.push('}');
                    }
                    Some(RailroadNode::NonTerminal(name)) => {
                        out.push_str("{\"name\":");
                        push_json_string(&mut out, name);
                        out.push_str(",\"type\":\"nonterminal\"}");
                    }
                    Some(RailroadNode::Special(text)) => {
                        out.push_str("{\"text\":");
                        push_json_string(&mut out, text);
                        out.push_str(",\"type\":\"special\"}");
                    }
                    Some(RailroadNode::Sequence(list) | RailroadNode::Choice(list)) => {
                        let sequence =
                            matches!(model.nodes.get(index), Some(RailroadNode::Sequence(_)));
                        out.push_str(if sequence {
                            "{\"elements\":["
                        } else {
                            "{\"alternatives\":["
                        });
                        stack.push(Work::Text(if sequence {
                            "],\"type\":\"sequence\"}"
                        } else {
                            "],\"type\":\"choice\"}"
                        }));
                        for (j, &kid) in list.iter().enumerate().rev() {
                            stack.push(Work::Node(kid));
                            if j > 0 {
                                stack.push(Work::Text(","));
                            }
                        }
                    }
                    Some(RailroadNode::Optional(element)) => {
                        out.push_str("{\"element\":");
                        stack.push(Work::Text(",\"type\":\"optional\"}"));
                        stack.push(Work::Node(*element));
                    }
                    Some(&RailroadNode::Repetition { element, max, min }) => {
                        out.push_str("{\"element\":");
                        stack.push(Work::Text(",\"type\":\"repetition\"}"));
                        stack.push(Work::Count(min));
                        stack.push(Work::Text(",\"min\":"));
                        stack.push(Work::Count(max));
                        stack.push(Work::Text(",\"max\":"));
                        stack.push(Work::Node(element));
                    }
                    None => out.push_str("null"),
                },
            }
        }
        out.push_str(",\"name\":");
        push_json_string(&mut out, &rule.name);
        out.push('}');
    }
    out.push(']');
    if let Some(title) = &model.title {
        out.push_str(",\"title\":");
        push_json_string(&mut out, title);
    }
    out.push_str(",\"type\":\"railroad\"}");
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
    //! Parity with Pele's `railroad.parse` over every string literal of Pele's railroad compat
    //! specs and the documents the specs build at runtime, as recorded by running Pele's
    //! TypeScript.

    use super::fixture::cases;
    use super::parse;

    /// The parity fixture: Pele's parser input and its model JSON or error.
    const FIXTURE: &str = include_str!("../../tests/fixtures/railroad-parse.json");

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
