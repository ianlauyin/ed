//! Tokens and a hand-written parser for Mermaid's Langium Wardley grammar. Port of Pele's
//! `src/diagrams/wardley/parser.ts`.

use crate::diagrams::common::acc_descr::acc_descr;
use crate::diagrams::common::tokens::{
    ACC_TITLE, DIRECTIVE, EOF, Memo, NEWLINE, Pattern, Reader, SINGLE_LINE_COMMENT, STRING, TITLE,
    TokenType, WHITESPACE, YAML, acc_descr_value, acc_title_value, literal, skip_blanks,
    string_value, title_value, tokenize,
};
use crate::error::Error;
use crate::text::label::trim;
use crate::util::chars::is_space;

// Token kinds: indices into `WARDLEY_TOKENS`, in Pele's `T` order.
const LBRACKET: u16 = 0;
const COMMA: u16 = 1;
const RBRACKET: u16 = 2;
const AT: u16 = 3;
const SLASH: u16 = 4;
const LPAREN: u16 = 5;
const RPAREN: u16 = 6;
const MINUS: u16 = 7;
const LBRACE: u16 = 8;
const RBRACE: u16 = 9;
const NUMBER: u16 = 10;
const ARROW: u16 = 11;
const LINK_PORT: u16 = 12;
const LINK_ARROW: u16 = 13;
const LINK_LABEL: u16 = 14;
const STRATEGY: u16 = 15;
const WARDLEY: u16 = 16;
const SIZE: u16 = 17;
const EVOLUTION: u16 = 18;
const ANCHOR: u16 = 19;
const COMPONENT: u16 = 20;
const LABEL: u16 = 21;
const INERTIA: u16 = 22;
const EVOLVE: u16 = 23;
const PIPELINE: u16 = 24;
const NOTE: u16 = 25;
const ANNOTATIONS: u16 = 26;
const ANNOTATION: u16 = 27;
const ACCELERATOR: u16 = 28;
const DEACCELERATOR: u16 = 29;
const NAME: u16 = 30;
const ACC_DESCR_KIND: u16 = 32;
const ACC_TITLE_KIND: u16 = 33;
const TITLE_KIND: u16 = 34;
const INT: u16 = 35;
const STRING_KIND: u16 = 36;
const ID: u16 = 37;
const NEWLINE_KIND: u16 = 38;

/// The index of `YAML` in [`WARDLEY_TOKENS`].
const YAML_KIND: u16 = 40;

/// Where a run of ASCII digits from `from` ends.
fn digits(src: &str, from: usize) -> usize {
    let bytes = src.as_bytes();
    let mut at = from;
    while bytes.get(at).is_some_and(u8::is_ascii_digit) {
        at += 1;
    }
    at
}

/// `[0-9]+\.[0-9]+`. The pattern reads a whole run of digits before it finds there is no
/// fraction, and would do so again from every digit of a long run, so `memo` keeps the run:
/// `memo[0]` is where it starts and `memo[1]` where it ends.
fn number(src: &str, p: usize, memo: &mut Memo) -> Option<usize> {
    if p < memo[0] || p >= memo[1] {
        memo[0] = p;
        memo[1] = digits(src, p);
    }
    let end = memo[1];
    if end == p || src.as_bytes().get(end) != Some(&b'.') {
        return None;
    }
    let stop = digits(src, end + 1);
    (stop > end + 1).then_some(stop)
}

/// `-->|-\.->|>|\+'[^']*'<>|\+'[^']*'<|\+'[^']*'>`.
fn link_arrow(src: &str, p: usize) -> Option<usize> {
    let rest = src.get(p..)?;
    if rest.starts_with("-->") {
        return Some(p + 3);
    }
    if rest.starts_with("-.->") {
        return Some(p + 4);
    }
    if rest.starts_with('>') {
        return Some(p + 1);
    }
    let body = rest.strip_prefix("+'")?;
    let close = p + 2 + body.find('\'')?;
    let after = src.get(close + 1..)?;
    if after.starts_with("<>") {
        return Some(close + 3);
    }
    (after.starts_with('<') || after.starts_with('>')).then_some(close + 2)
}

/// `\+<>|\+>|\+<`.
fn link_port(src: &str, p: usize) -> Option<usize> {
    let rest = src.get(p..)?;
    if rest.starts_with("+<>") {
        return Some(p + 3);
    }
    (rest.starts_with("+>") || rest.starts_with("+<")).then_some(p + 2)
}

/// `;[^\n\r]+`.
fn link_label(src: &str, p: usize) -> Option<usize> {
    let rest = src.get(p..)?.strip_prefix(';')?;
    let end = p + 1 + rest.find(['\n', '\r']).unwrap_or(rest.len());
    (end > p + 1).then_some(end)
}

/// `build|buy|outsource|market`.
fn strategy(src: &str, p: usize) -> Option<usize> {
    let rest = src.get(p..)?;
    ["build", "buy", "outsource", "market"]
        .iter()
        .find(|word| rest.starts_with(*word))
        .map(|word| p + word.len())
}

/// `(?:[A-Za-z0-9_()&]|-(?!>))*` from `at`.
fn name_chars(bytes: &[u8], mut at: usize) -> usize {
    while let Some(&c) = bytes.get(at) {
        let ok = c.is_ascii_alphanumeric()
            || matches!(c, b'_' | b'(' | b')' | b'&')
            || (c == b'-' && bytes.get(at + 1) != Some(&b'>'));
        if !ok {
            break;
        }
        at += 1;
    }
    at
}

/// `(?!title\s|accTitle|accDescr)[A-Za-z](?:[A-Za-z0-9_()&]|-(?!>))*(?:[ \t]+[A-Za-z(](?:[A-Za-z0-9_()&]|-(?!>))*)*`.
fn name_with_spaces(src: &str, p: usize) -> Option<usize> {
    let rest = src.get(p..)?;
    if rest.starts_with("accTitle") || rest.starts_with("accDescr") {
        return None;
    }
    if let Some(after) = rest.strip_prefix("title")
        && after.chars().next().is_some_and(|c| is_space(u32::from(c)))
    {
        return None;
    }

    let bytes = src.as_bytes();
    if !bytes.get(p).is_some_and(u8::is_ascii_alphabetic) {
        return None;
    }
    let mut end = name_chars(bytes, p + 1);
    loop {
        let q = skip_blanks(src, end);
        match bytes.get(q) {
            Some(&c) if q > end && (c.is_ascii_alphabetic() || c == b'(') => {
                end = name_chars(bytes, q + 1);
            }
            _ => return Some(end),
        }
    }
}

/// `[ \t]+`.
fn ws(src: &str, p: usize) -> Option<usize> {
    let end = skip_blanks(src, p);
    (end > p).then_some(end)
}

/// `0|[1-9][0-9]*(?!\.)`. The digits give back their last one when a `.` follows them.
fn int(src: &str, p: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    match bytes.get(p) {
        Some(b'0') => Some(p + 1),
        Some(b'1'..=b'9') => {
            let end = digits(src, p + 1);
            if bytes.get(end) == Some(&b'.') {
                (end >= p + 2).then(|| end - 1)
            } else {
                Some(end)
            }
        }
        _ => None,
    }
}

/// `[\w]([-\w]*\w)?`.
fn id(src: &str, p: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    let word = |c: &u8| c.is_ascii_alphanumeric() || *c == b'_';
    if !bytes.get(p).is_some_and(word) {
        return None;
    }
    let mut end = p + 1;
    while bytes.get(end).is_some_and(|c| word(c) || *c == b'-') {
        end += 1;
    }
    while end > p + 1 && bytes.get(end - 1) == Some(&b'-') {
        end -= 1;
    }
    Some(end)
}

/// A token type that matches `text`, named `name`. Pele writes these as sticky expressions of
/// the text, which match it as a prefix of a longer word too.
const fn word(name: &'static str, text: &'static str) -> TokenType {
    TokenType {
        name,
        ..literal(text)
    }
}

/// A token type with a hand-written matcher.
const fn matcher(
    name: &'static str,
    first: &'static str,
    pattern: fn(&str, usize) -> Option<usize>,
) -> TokenType {
    TokenType {
        first: Some(first),
        name,
        pattern: Pattern::Match(pattern),
        ..literal("")
    }
}

/// The token types, in matching order.
pub(crate) const WARDLEY_TOKENS: &[TokenType] = &[
    literal("["),
    literal(","),
    literal("]"),
    literal("@"),
    literal("/"),
    literal("("),
    literal(")"),
    TokenType {
        longer: &[ARROW, LINK_ARROW, YAML_KIND],
        ..literal("-")
    },
    literal("{"),
    literal("}"),
    TokenType {
        first: Some("0123456789"),
        name: "WARDLEY_NUMBER",
        pattern: Pattern::Scan(number),
        ..literal("")
    },
    word("ARROW", "->"),
    matcher("LINK_PORT", "+", link_port),
    matcher("LINK_ARROW", "->+", link_arrow),
    matcher("LINK_LABEL", ";", link_label),
    matcher("STRATEGY", "bom", strategy),
    word("KW_WARDLEY", "wardley-beta"),
    word("KW_SIZE", "size"),
    word("KW_EVOLUTION", "evolution"),
    word("KW_ANCHOR", "anchor"),
    word("KW_COMPONENT", "component"),
    word("KW_LABEL", "label"),
    word("KW_INERTIA", "inertia"),
    word("KW_EVOLVE", "evolve"),
    word("KW_PIPELINE", "pipeline"),
    word("KW_NOTE", "note"),
    word("KW_ANNOTATIONS", "annotations"),
    word("KW_ANNOTATION", "annotation"),
    word("KW_ACCELERATOR", "accelerator"),
    word("KW_DEACCELERATOR", "deaccelerator"),
    matcher(
        "NAME_WITH_SPACES",
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz",
        name_with_spaces,
    ),
    TokenType {
        hidden: true,
        ..matcher("WS", "\t ", ws)
    },
    acc_descr("ACC_DESCR"),
    ACC_TITLE,
    TITLE,
    matcher("INT", "0123456789", int),
    STRING,
    matcher(
        "ID",
        "_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz",
        id,
    ),
    NEWLINE,
    WHITESPACE,
    YAML,
    DIRECTIVE,
    SINGLE_LINE_COMMENT,
];

const _: () = assert!(WARDLEY_TOKENS.len() == 43);

/// A label offset: `label [x, y]`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LabelAst {
    /// Whether `x` is negated.
    pub(crate) neg_x: bool,
    /// Whether `y` is negated.
    pub(crate) neg_y: bool,
    /// The horizontal offset.
    pub(crate) offset_x: f64,
    /// The vertical offset.
    pub(crate) offset_y: f64,
}

/// A stage of `evolution`.
#[derive(Clone, Debug)]
pub(crate) struct StageAst {
    /// The right edge after `@`.
    pub(crate) boundary: Option<f64>,
    /// The name.
    pub(crate) name: String,
    /// The name after `/`.
    pub(crate) second_name: Option<String>,
}

/// A `component` statement.
#[derive(Clone, Debug)]
pub(crate) struct ComponentAst {
    /// The source strategy in parentheses.
    pub(crate) decorator: Option<String>,
    /// The evolution coordinate.
    pub(crate) evolution: f64,
    /// Whether `inertia` follows.
    pub(crate) inertia: bool,
    /// The label offset.
    pub(crate) label: Option<LabelAst>,
    /// The name.
    pub(crate) name: String,
    /// The visibility coordinate.
    pub(crate) visibility: f64,
}

/// A `component` inside a pipeline.
#[derive(Clone, Debug)]
pub(crate) struct PipelineComponentAst {
    /// The evolution coordinate.
    pub(crate) evolution: f64,
    /// The label offset.
    pub(crate) label: Option<LabelAst>,
    /// The name.
    pub(crate) name: String,
}

/// A link between two names.
#[derive(Clone, Debug)]
pub(crate) struct LinkAst {
    /// The arrow text.
    pub(crate) arrow: Option<String>,
    /// The source name.
    pub(crate) from: String,
    /// The port after the source.
    pub(crate) from_port: Option<String>,
    /// The `;` label, trimmed.
    pub(crate) link_label: Option<String>,
    /// The target name.
    pub(crate) to: String,
    /// The port after the target.
    pub(crate) to_port: Option<String>,
}

/// A named force with a position: an accelerator or deaccelerator.
#[derive(Clone, Debug)]
pub(crate) struct PlacedAst {
    /// The name.
    pub(crate) name: String,
    /// The first coordinate.
    pub(crate) x: f64,
    /// The second coordinate.
    pub(crate) y: f64,
}

/// A numbered annotation.
#[derive(Clone, Debug)]
pub(crate) struct AnnotationAst {
    /// The number.
    pub(crate) number: f64,
    /// The text.
    pub(crate) text: String,
    /// The first coordinate.
    pub(crate) x: f64,
    /// The second coordinate.
    pub(crate) y: f64,
}

/// An anchor.
#[derive(Clone, Debug)]
pub(crate) struct AnchorAst {
    /// The evolution coordinate.
    pub(crate) evolution: f64,
    /// The name.
    pub(crate) name: String,
    /// The visibility coordinate.
    pub(crate) visibility: f64,
}

/// A note.
#[derive(Clone, Debug)]
pub(crate) struct NoteAst {
    /// The evolution coordinate.
    pub(crate) evolution: f64,
    /// The text.
    pub(crate) text: String,
    /// The visibility coordinate.
    pub(crate) visibility: f64,
}

/// A pipeline.
#[derive(Clone, Debug)]
pub(crate) struct PipelineAst {
    /// The components.
    pub(crate) components: Vec<PipelineComponentAst>,
    /// The parent component's name.
    pub(crate) parent: String,
}

/// An `evolve` statement.
#[derive(Clone, Debug)]
pub(crate) struct EvolveAst {
    /// The component's name.
    pub(crate) component: String,
    /// The target evolution.
    pub(crate) target: f64,
}

/// The syntax tree: Pele's `WardleyAst`.
#[derive(Clone, Debug, Default)]
pub(crate) struct WardleyAst {
    /// The `accDescr` text.
    pub(crate) acc_descr: Option<String>,
    /// The `accTitle` text.
    pub(crate) acc_title: Option<String>,
    /// Accelerators.
    pub(crate) accelerators: Vec<PlacedAst>,
    /// Anchors.
    pub(crate) anchors: Vec<AnchorAst>,
    /// Numbered annotations.
    pub(crate) annotation: Vec<AnnotationAst>,
    /// Annotation box positions, as `(x, y)`.
    pub(crate) annotations: Vec<(f64, f64)>,
    /// Components.
    pub(crate) components: Vec<ComponentAst>,
    /// Deaccelerators.
    pub(crate) deaccelerators: Vec<PlacedAst>,
    /// The evolution stages.
    pub(crate) evolution: Option<Vec<StageAst>>,
    /// Evolve statements.
    pub(crate) evolves: Vec<EvolveAst>,
    /// Links.
    pub(crate) links: Vec<LinkAst>,
    /// Notes.
    pub(crate) notes: Vec<NoteAst>,
    /// Pipelines.
    pub(crate) pipelines: Vec<PipelineAst>,
    /// The size, as `(width, height)`.
    pub(crate) size: Option<(f64, f64)>,
    /// The `title` text.
    pub(crate) title: Option<String>,
}

/// JavaScript's `Number` of a `WARDLEY_NUMBER` or `parseInt` of an `INT` token: both are plain
/// decimal digits.
fn decimal(text: &str) -> f64 {
    text.parse().unwrap_or(f64::NAN)
}

/// Reads tokens of the grammar.
struct Parser<'a> {
    r: Reader<'a>,
}

impl Parser<'_> {
    fn at(&self, kind: u16) -> bool {
        self.r.kind() == kind
    }

    fn name(&mut self) -> Result<String, Error> {
        if self.at(STRING_KIND) {
            return Ok(string_value(self.r.take()).into_owned());
        }
        if self.at(ID) || self.at(NAME) {
            return Ok(self.r.take().to_owned());
        }
        Err(self.r.fail("a name"))
    }

    fn number(&mut self) -> Result<f64, Error> {
        Ok(decimal(self.r.expect(NUMBER)?))
    }

    fn int(&mut self) -> Result<f64, Error> {
        Ok(decimal(self.r.expect(INT)?))
    }

    fn coordinate(&mut self) -> Result<f64, Error> {
        if self.at(NUMBER) {
            return Ok(decimal(self.r.take()));
        }
        self.int()
    }

    fn end_of_line(&mut self) -> Result<(), Error> {
        if self.at(EOF) {
            return Ok(());
        }
        if !self.at(NEWLINE_KIND) {
            return Err(self.r.fail("a line break or the end of input"));
        }
        while self.at(NEWLINE_KIND) {
            self.r.i += 1;
        }
        Ok(())
    }

    fn pair(&mut self, read: fn(&mut Self) -> Result<f64, Error>) -> Result<(f64, f64), Error> {
        self.r.expect(LBRACKET)?;
        let first = read(self)?;
        self.r.expect(COMMA)?;
        let second = read(self)?;
        self.r.expect(RBRACKET)?;
        Ok((first, second))
    }

    fn label(&mut self) -> Result<LabelAst, Error> {
        self.r.expect(LABEL)?;
        self.r.expect(LBRACKET)?;
        let neg_x = self.r.accept(MINUS);
        let offset_x = self.int()?;
        self.r.expect(COMMA)?;
        let neg_y = self.r.accept(MINUS);
        let offset_y = self.int()?;
        self.r.expect(RBRACKET)?;
        Ok(LabelAst {
            neg_x,
            neg_y,
            offset_x,
            offset_y,
        })
    }

    fn stage(&mut self) -> Result<StageAst, Error> {
        let name = self.name()?;
        let boundary = if self.r.accept(AT) {
            Some(self.number()?)
        } else {
            None
        };
        let second_name = if self.r.accept(SLASH) {
            Some(self.name()?)
        } else {
            None
        };
        Ok(StageAst {
            boundary,
            name,
            second_name,
        })
    }

    fn label_if_any(&mut self) -> Result<Option<LabelAst>, Error> {
        if self.at(LABEL) {
            return Ok(Some(self.label()?));
        }
        Ok(None)
    }

    fn component(&mut self) -> Result<ComponentAst, Error> {
        self.r.i += 1;
        let name = self.name()?;
        let (visibility, evolution) = self.pair(Self::number)?;
        let label = self.label_if_any()?;
        let next = self
            .r
            .tokens
            .tokens
            .get(self.r.i + 1)
            .map(|token| token.kind);
        let decorator = if self.at(LPAREN) && next == Some(STRATEGY) {
            self.r.i += 1;
            let strategy = self.r.take().to_owned();
            self.r.expect(RPAREN)?;
            Some(strategy)
        } else {
            None
        };
        let inertia = if self.r.accept(INERTIA) {
            true
        } else if self.r.accept(LPAREN) {
            self.r.expect(INERTIA)?;
            self.r.expect(RPAREN)?;
            true
        } else {
            false
        };
        self.end_of_line()?;
        Ok(ComponentAst {
            decorator,
            evolution,
            inertia,
            label,
            name,
            visibility,
        })
    }

    fn pipeline(&mut self) -> Result<PipelineAst, Error> {
        self.r.i += 1;
        let parent = self.name()?;
        self.r.expect(LBRACE)?;
        self.r.expect(NEWLINE_KIND)?;
        while self.at(NEWLINE_KIND) {
            self.r.i += 1;
        }
        let mut components = Vec::new();
        loop {
            self.r.expect(COMPONENT)?;
            let name = self.name()?;
            self.r.expect(LBRACKET)?;
            let evolution = self.number()?;
            self.r.expect(RBRACKET)?;
            let label = self.label_if_any()?;
            self.end_of_line()?;
            components.push(PipelineComponentAst {
                evolution,
                label,
                name,
            });
            if !self.at(COMPONENT) {
                break;
            }
        }
        self.r.expect(RBRACE)?;
        self.end_of_line()?;
        Ok(PipelineAst { components, parent })
    }

    fn link(&mut self) -> Result<LinkAst, Error> {
        if !(self.at(STRING_KIND) || self.at(ID) || self.at(NAME)) {
            return Err(self.r.fail("a statement"));
        }
        let from = self.name()?;
        let from_port = self.at(LINK_PORT).then(|| self.r.take().to_owned());
        let arrow = (self.at(LINK_ARROW) || self.at(ARROW)).then(|| self.r.take().to_owned());
        let to = self.name()?;
        let to_port = self.at(LINK_PORT).then(|| self.r.take().to_owned());
        let link_label = self.at(LINK_LABEL).then(|| {
            let text = self.r.take();
            trim(text.get(1..).unwrap_or("")).to_owned()
        });
        self.end_of_line()?;
        Ok(LinkAst {
            arrow,
            from,
            from_port,
            link_label,
            to,
            to_port,
        })
    }

    fn statement(&mut self, ast: &mut WardleyAst) -> Result<(), Error> {
        match self.r.kind() {
            NEWLINE_KIND => self.r.i += 1,
            ACC_DESCR_KIND => {
                ast.acc_descr = Some(acc_descr_value(self.r.take()));
                self.end_of_line()?;
            }
            ACC_TITLE_KIND => {
                ast.acc_title = Some(acc_title_value(self.r.take()));
                self.end_of_line()?;
            }
            TITLE_KIND => {
                ast.title = Some(title_value(self.r.take()));
                self.end_of_line()?;
            }
            SIZE => {
                self.r.i += 1;
                let size = self.pair(Self::int)?;
                self.end_of_line()?;
                ast.size = Some(size);
            }
            EVOLUTION => {
                self.r.i += 1;
                let mut stages = vec![self.stage()?];
                self.r.expect(ARROW)?;
                stages.push(self.stage()?);
                while self.r.accept(ARROW) {
                    stages.push(self.stage()?);
                }
                self.end_of_line()?;
                ast.evolution = Some(stages);
            }
            ANCHOR => {
                self.r.i += 1;
                let name = self.name()?;
                let (visibility, evolution) = self.pair(Self::number)?;
                self.end_of_line()?;
                ast.anchors.push(AnchorAst {
                    evolution,
                    name,
                    visibility,
                });
            }
            COMPONENT => {
                let component = self.component()?;
                ast.components.push(component);
            }
            EVOLVE => {
                self.r.i += 1;
                let component = self.name()?;
                let target = self.number()?;
                self.end_of_line()?;
                ast.evolves.push(EvolveAst { component, target });
            }
            PIPELINE => {
                let pipeline = self.pipeline()?;
                ast.pipelines.push(pipeline);
            }
            NOTE => {
                self.r.i += 1;
                let text = string_value(self.r.expect(STRING_KIND)?).into_owned();
                let (visibility, evolution) = self.pair(Self::number)?;
                self.end_of_line()?;
                ast.notes.push(NoteAst {
                    evolution,
                    text,
                    visibility,
                });
            }
            ANNOTATIONS => {
                self.r.i += 1;
                let at = self.pair(Self::coordinate)?;
                self.end_of_line()?;
                ast.annotations.push(at);
            }
            ANNOTATION => {
                self.r.i += 1;
                let number = self.int()?;
                self.r.expect(COMMA)?;
                let (x, y) = self.pair(Self::coordinate)?;
                let text = string_value(self.r.expect(STRING_KIND)?).into_owned();
                self.end_of_line()?;
                ast.annotation.push(AnnotationAst { number, text, x, y });
            }
            kind @ (ACCELERATOR | DEACCELERATOR) => {
                self.r.i += 1;
                let name = self.name()?;
                let (x, y) = self.pair(Self::number)?;
                self.end_of_line()?;
                let force = PlacedAst { name, x, y };
                if kind == DEACCELERATOR {
                    ast.deaccelerators.push(force);
                } else {
                    ast.accelerators.push(force);
                }
            }
            _ => {
                let link = self.link()?;
                ast.links.push(link);
            }
        }
        Ok(())
    }
}

/// Parses Wardley `src` into its syntax tree, as Pele's `parseWardley` does.
///
/// # Errors
///
/// Returns a syntax error where the source leaves the grammar.
pub(crate) fn parse_wardley(src: &str) -> Result<WardleyAst, Error> {
    let tokens = tokenize(src, WARDLEY_TOKENS, "wardley")?;
    let mut p = Parser {
        r: Reader::new(tokens, WARDLEY_TOKENS, "wardley"),
    };
    let mut ast = WardleyAst::default();

    while p.at(NEWLINE_KIND) {
        p.r.i += 1;
    }
    p.r.expect(WARDLEY)?;
    while !p.at(EOF) {
        p.statement(&mut ast)?;
    }
    Ok(ast)
}
