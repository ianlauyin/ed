//! Picks the notation from the keyword and loads the parsed rules into the model. Port of Pele's
//! `src/diagrams/railroad/db.ts`.

use super::abnf::{abnf_rules, parse_abnf};
use super::common::Grammar;
use super::ebnf::{ebnf_rules, parse_ebnf};
use super::ir::parse_ir;
use super::peg::{parse_peg, peg_rules};
use super::types::{Notation, RailroadModel, RailroadRule};
use crate::error::{Code, Error};
use crate::text::label::trim_start;
use crate::util::chars::is_space;

/// The notation the source's keyword names: `^\s*railroad-(ebnf-|abnf-|peg-)?beta`, without
/// regard to case. Mermaid has a detector per keyword.
pub(crate) fn notation_of(source: &str) -> Option<Notation> {
    let rest = trim_start(source);
    let starts = |text: &str, word: &str| {
        text.as_bytes()
            .get(..word.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(word.as_bytes()))
    };
    if !starts(rest, "railroad-") {
        return None;
    }
    let rest = rest.get(9..)?;
    for (infix, notation) in [
        ("ebnf-", Notation::Ebnf),
        ("abnf-", Notation::Abnf),
        ("peg-", Notation::Peg),
    ] {
        if starts(rest, infix)
            && rest
                .get(infix.len()..)
                .is_some_and(|tail| starts(tail, "beta"))
        {
            return Some(notation);
        }
    }
    starts(rest, "beta").then_some(Notation::Railroad)
}

/// `text.replace(/\n\s+/g, '\n')`.
fn dedent_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\n' {
            while chars.next_if(|&c| is_space(u32::from(c))).is_some() {}
        }
    }
    out
}

/// Parses `source` in `notation`, returning its statements and rules with their nodes.
fn grammar(
    source: &str,
    notation: Notation,
    model: &mut RailroadModel,
) -> Result<(Grammar<()>, Vec<RailroadRule>), Error> {
    let nodes = &mut model.nodes;
    let (grammar, rules) = match notation {
        Notation::Ebnf => {
            let mut ast = Vec::new();
            let grammar = parse_ebnf(source, &mut ast)?;
            let rules = ebnf_rules(&ast, &grammar.rules, nodes);
            (strip(grammar), rules)
        }
        Notation::Abnf => {
            let mut ast = Vec::new();
            let grammar = parse_abnf(source, &mut ast)?;
            let rules = abnf_rules(&ast, &grammar.rules, nodes);
            (strip(grammar), rules)
        }
        Notation::Peg => {
            let mut ast = Vec::new();
            let grammar = parse_peg(source, &mut ast)?;
            let rules = peg_rules(&ast, &grammar.rules, nodes);
            (strip(grammar), rules)
        }
        Notation::Railroad => {
            let mut grammar = parse_ir(source, nodes)?;
            let rules = std::mem::take(&mut grammar.rules);
            (strip(grammar), rules)
        }
    };
    Ok((grammar, rules))
}

/// The statements of `grammar`, without its rules.
fn strip<R>(grammar: Grammar<R>) -> Grammar<()> {
    Grammar {
        acc_descr: grammar.acc_descr,
        acc_title: grammar.acc_title,
        rules: Vec::new(),
        title: grammar.title,
    }
}

/// Parses `source`, as Pele's `parseRailroad` does. `title` comes from front matter.
///
/// # Errors
///
/// Returns a syntax error when the source has no railroad keyword or leaves its grammar.
pub(crate) fn parse_railroad(source: &str, title: Option<&str>) -> Result<RailroadModel, Error> {
    let Some(notation) = notation_of(source) else {
        let mut error = Error::new(Code::Syntax, "No railroad diagram keyword found.");
        error.r#type = Some("railroad");
        return Err(error);
    };
    let mut model = RailroadModel {
        acc_descr: None,
        acc_title: None,
        nodes: Vec::new(),
        notation: Notation::Railroad,
        rules: Vec::new(),
        title: title.filter(|title| !title.is_empty()).map(str::to_owned),
    };

    // Parses the text in one notation and loads the result into the model, as Mermaid's four
    // parsers do.
    let (ast, rules) = grammar(source, notation, &mut model)?;
    model.notation = notation;
    if let Some(text) = ast.acc_descr.filter(|text| !text.is_empty()) {
        model.acc_descr = Some(dedent_lines(&text));
    }
    if let Some(text) = ast.acc_title.filter(|text| !text.is_empty()) {
        model.acc_title = Some(trim_start(&text).to_owned());
    }
    if let Some(text) = ast.title.filter(|text| !text.is_empty()) {
        model.title = Some(text);
    }
    model.rules = rules;
    Ok(model)
}
