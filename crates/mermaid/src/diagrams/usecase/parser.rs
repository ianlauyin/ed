//! The use case grammar. Port of Pele's `src/diagrams/usecase/parser.ts`.
//!
//! Mermaid parses use case diagrams with a Chevrotain parser, which decides each choice by
//! looking at most three tokens ahead and reports the first token that fits nowhere. This parser
//! makes the same decisions from the same lookahead, so it accepts the same text and names the
//! same token in its errors.

use super::db::{
    DraftBoundary, DraftClassDef, DraftClasses, DraftElement, DraftEndpoint, DraftJson, DraftLabel,
    DraftMetadata, DraftNote, DraftProperty, DraftRelationship, DraftStyle, Drafts, Named,
    PropValue, Span, resolve_usecase,
};
use super::json::{JsonDoc, parse_ordered_json};
use super::lexer::{Tok, error, line_column, push_usize, tokenize, utf16};
use super::tokens::{T, name};
use super::types::{RelationshipType, Shape, UsecaseModel, arrow};
use crate::error::{Code, Error, push_u32};
use crate::text::label::trim;
use crate::util::chars::is_word;

/// The arrow tokens, in Mermaid's order.
const ARROWS: [T; 9] = [
    T::DependencyArrow,
    T::Generalization,
    T::ForwardSolid,
    T::BackwardSolid,
    T::MarkerlessSolid,
    T::ForwardCircle,
    T::BackwardCircle,
    T::ForwardCross,
    T::BackwardCross,
];

/// The tokens that name an element.
const NAMES: [T; 3] = [T::Identifier, T::PlainString, T::MarkdownString];

/// The tokens that start a statement other than an element.
const STATEMENTS: [T; 11] = [
    T::AccTitleLine,
    T::AccDescrLine,
    T::AccDescrBlock,
    T::Direction,
    T::Actor,
    T::SystemBoundary,
    T::Note,
    T::JsonDeclarationStart,
    T::ClassDef,
    T::Class,
    T::Style,
];

/// The parts of a style value, as Mermaid lists them.
const STYLE_PARTS: &str = "WORD|PLAIN_STRING|NUMBER|HASH_COLOR|CSS_IDENTIFIER|CSS_ESCAPED_COMMA|DASH|DOT|PERCENT|CSS_PUNCTUATION|COLON|LPAREN|RPAREN|LBRACKET|RBRACKET|LBRACE|RBRACE|AT|MARKERLESS_SOLID";

/// The statements `PlantUML` users are most likely to try, which Mermaid turns away.
const FORBIDDEN: [&str; 5] = [
    "allowmixing",
    "newpage",
    "package",
    "rectangle",
    "skinparam",
];

/// Mermaid's WORD category: every keyword and identifier.
fn is_word_token(t: T) -> bool {
    (T::Usecase..=T::Identifier).contains(&t)
}

/// Mermaid's `LABEL_TEXT` category.
fn is_label_text(t: T) -> bool {
    (T::Usecase..=T::LabelSymbol).contains(&t)
}

/// Whether `t` names an element.
fn is_name(t: T) -> bool {
    NAMES.contains(&t)
}

/// Whether `t` starts a top-level line.
fn is_line(t: T) -> bool {
    matches!(t, T::Newline | T::Comment) || STATEMENTS.contains(&t) || is_name(t)
}

/// Whether `t` starts a relationship after its source.
fn is_relation(t: T) -> bool {
    t == T::Identifier || ARROWS.contains(&t)
}

/// Whether `t` may be part of a style value.
fn is_style_part(t: T) -> bool {
    is_word_token(t)
        || matches!(
            t,
            T::PlainString
                | T::Number
                | T::HashColor
                | T::CssIdentifier
                | T::CssEscapedComma
                | T::Dash
                | T::Dot
                | T::Percent
                | T::CssPunctuation
                | T::Colon
                | T::LParen
                | T::RParen
                | T::LBracket
                | T::RBracket
                | T::LBrace
                | T::RBrace
                | T::At
                | T::MarkerlessSolid
        )
}

/// Whether `t` may follow the name of an element statement.
fn follows_name(t: T) -> bool {
    matches!(
        t,
        T::StereotypeStart | T::ClassSeparator | T::Identifier | T::Newline | T::Eof
    ) || ARROWS.contains(&t)
}

/// Whether `t` starts a line inside a system boundary.
fn is_boundary_line(t: T) -> bool {
    matches!(t, T::Newline | T::Comment | T::Actor) || is_name(t)
}

/// Whether `t` may close a label that `--` opened.
fn closes_label(t: T) -> bool {
    matches!(
        t,
        T::ForwardSolid | T::MarkerlessSolid | T::ForwardCircle | T::ForwardCross
    )
}

/// Whether `t` may come first inside a metadata block.
fn is_metadata_first(t: T) -> bool {
    matches!(t, T::Newline | T::Identifier | T::PlainString | T::RBrace)
}

/// Whether `t` may be a metadata key.
fn is_key(t: T) -> bool {
    t == T::Identifier || t == T::PlainString
}

/// Pele's `label.replace(/\W/g, '_')`: every UTF-16 unit outside `\w` becomes `_`.
fn generated_id(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    for c in label.chars() {
        if is_word(u32::from(c)) {
            out.push(c);
        } else {
            for _ in 0..c.len_utf16() {
                out.push('_');
            }
        }
    }
    out
}

/// Every token sequence that may start a statement, as Chevrotain lists them.
fn statement_sequences() -> Vec<String> {
    let mut out: Vec<String> = STATEMENTS.iter().map(|&t| name(t).to_owned()).collect();
    let pair = |a: T, b: T| {
        let mut sequence = String::from(name(a));
        sequence.push_str(", ");
        sequence.push_str(name(b));
        sequence
    };
    let metadata: Vec<String> = NAMES
        .iter()
        .flat_map(|&n| {
            [T::Newline, T::Identifier, T::PlainString, T::RBrace].map(|next| {
                let mut sequence = String::from(name(n));
                sequence.push_str(", METADATA_START, ");
                sequence.push_str(name(next));
                sequence
            })
        })
        .collect();
    let mut follows = vec![T::StereotypeStart, T::ClassSeparator, T::Identifier];
    follows.extend(ARROWS);
    follows.extend([T::Newline, T::Eof]);
    out.extend(metadata.iter().cloned());
    for (i, &n) in NAMES.iter().enumerate() {
        if i == 0 {
            out.push(pair(n, T::LParen));
            out.push(pair(n, T::LBracket));
        }
        for &next in &follows {
            out.push(pair(n, next));
        }
    }
    out.extend(metadata);
    out
}

/// An element as one mention writes it.
struct Item {
    /// Its `:::` classes.
    classes: Vec<String>,
    /// Whether this mention says something about the element, and so declares it.
    explicit: bool,
    /// Its inline metadata.
    metadata: Option<Vec<DraftProperty>>,
    /// Its id and label.
    named: Named,
    /// Its shape, from parentheses or brackets.
    shape: Option<Shape>,
    /// Its stereotype.
    stereotype: Option<String>,
}

/// An arrow and the element it points at.
struct Tail {
    /// One of the arrow types.
    arrow_type: u8,
    /// The id the author gave the relationship, and where.
    explicit_id: Option<(String, Span)>,
    /// Its label.
    label: Option<DraftLabel>,
    /// The least number of ranks between the ends.
    minlen: u32,
    /// Where the arrow and target are.
    span: Span,
    /// The target.
    target: Item,
    /// The kind of relationship.
    r#type: RelationshipType,
}

/// The parser's state over one source.
struct Parser<'a> {
    /// What was read so far.
    drafts: Drafts,
    /// The current token.
    p: usize,
    /// The boundary being read, by index.
    parent: Option<usize>,
    /// JSON nodes and their literal tokens, read once the whole text is well formed.
    pending_json: Vec<(usize, usize)>,
    /// The source.
    src: &'a str,
    /// The tokens, ending in three `Eof`s.
    toks: Vec<Tok>,
}

impl Parser<'_> {
    /// The type of token `k`.
    fn kind(&self, k: usize) -> T {
        self.toks.get(k).map_or(T::Eof, |tok| tok.kind)
    }

    /// Where token `k` starts.
    fn start(&self, k: usize) -> usize {
        self.toks.get(k).map_or(self.src.len(), |tok| tok.start)
    }

    /// Where token `k` ends.
    fn end(&self, k: usize) -> usize {
        self.toks.get(k).map_or(self.src.len(), |tok| tok.end)
    }

    /// The source text of `span`.
    fn slice(&self, span: Span) -> &str {
        self.src.get(span.0..span.1).unwrap_or("")
    }

    /// The text of token `k`.
    fn image(&self, k: usize) -> &str {
        self.slice(self.span(k))
    }

    /// Where token `k` is.
    fn span(&self, k: usize) -> Span {
        (self.start(k), self.end(k))
    }

    /// A syntax error at the current token.
    #[cold]
    fn fail(&self, detail: &str) -> Error {
        let start = self.start(self.p);
        let (line, column) = line_column(self.src, start);
        let mut message = String::from("Error parsing usecase diagram: ");
        message.push_str(detail);
        message.push_str(" at line ");
        push_u32(&mut message, line);
        message.push_str(", column ");
        push_u32(&mut message, column);
        message.push_str(" [");
        push_usize(&mut message, utf16(self.src, start));
        message.push(',');
        push_usize(&mut message, utf16(self.src, self.end(self.p)));
        message.push(')');
        error(self.src, Code::Syntax, message, start)
    }

    #[cold]
    fn mismatch(&self, expected: &str) -> Error {
        let mut detail = String::from("Expecting token of type --> ");
        detail.push_str(expected);
        detail.push_str(" <-- but found --> '");
        detail.push_str(self.image(self.p));
        detail.push_str("' <--");
        self.fail(&detail)
    }

    /// Consumes a token of type `t` and returns its index.
    fn expect(&mut self, t: T) -> Result<usize, Error> {
        if self.kind(self.p) != t {
            return Err(self.mismatch(name(t)));
        }
        self.p += 1;
        Ok(self.p - 1)
    }

    /// Chevrotain lists every token sequence that would have been accepted.
    #[cold]
    fn no_alternative<S: AsRef<str>>(&self, expected: &[S]) -> Error {
        let mut detail = String::from("Expecting: one of these possible Token sequences:\n");
        for (i, sequence) in expected.iter().enumerate() {
            if i > 0 {
                detail.push('\n');
            }
            detail.push_str("  ");
            push_usize(&mut detail, i + 1);
            detail.push_str(". [");
            detail.push_str(sequence.as_ref());
            detail.push(']');
        }
        detail.push_str("\nbut found: '");
        detail.push_str(self.image(self.p));
        detail.push('\'');
        self.fail(&detail)
    }

    #[cold]
    fn no_token(&self, expected: &[T]) -> Error {
        let names: Vec<&str> = expected.iter().map(|&t| name(t)).collect();
        self.no_alternative(&names)
    }

    /// Consumes the end of a line.
    fn line_end(&mut self) -> Result<(), Error> {
        match self.kind(self.p) {
            T::Newline => {
                self.p += 1;
                Ok(())
            }
            T::Eof => Ok(()),
            _ => Err(self.no_token(&[T::Newline, T::Eof])),
        }
    }

    /// The text of a name or string token, without its quotes.
    fn content(&self, k: usize) -> Span {
        let trim = match self.kind(k) {
            T::MarkdownString => 2,
            T::PlainString => 1,
            _ => 0,
        };
        (self.start(k) + trim, self.end(k) - trim)
    }

    /// The label a name or string token gives.
    fn token_label(&self, k: usize) -> DraftLabel {
        let span = self.content(k);
        DraftLabel {
            markdown: self.kind(k) == T::MarkdownString,
            span,
            text: self.slice(span).to_owned(),
        }
    }

    /// An unquoted label runs to the next delimiter and is taken from the source as written.
    fn label(&mut self) -> Result<DraftLabel, Error> {
        let t = self.kind(self.p);
        if t == T::PlainString || t == T::MarkdownString {
            self.p += 1;
            return Ok(self.token_label(self.p - 1));
        }
        if !is_label_text(t) {
            return Err(self.no_alternative(&["PLAIN_STRING", "MARKDOWN_STRING", "LABEL_TEXT"]));
        }
        let first = self.p;
        while is_label_text(self.kind(self.p)) {
            self.p += 1;
        }
        let span = (self.start(first), self.end(self.p - 1));
        Ok(DraftLabel {
            markdown: false,
            span,
            text: self.slice(span).to_owned(),
        })
    }

    /// Reads one `key: value` metadata property.
    fn property(&mut self) -> Result<DraftProperty, Error> {
        let key = self.p;
        if !is_key(self.kind(key)) {
            return Err(self.no_token(&[T::Identifier, T::PlainString]));
        }
        self.p += 1;
        self.expect(T::Colon)?;
        let value = self.p;
        let t = self.kind(value);
        if !is_key(t) && t != T::True && t != T::False {
            return Err(self.no_token(&[T::Identifier, T::PlainString, T::True, T::False]));
        }
        self.p += 1;
        let key_span = self.content(key);
        Ok(DraftProperty {
            key: self.slice(key_span).to_owned(),
            key_span,
            value: match t {
                T::True => PropValue::Bool(true),
                T::False => PropValue::Bool(false),
                _ => PropValue::Str(self.slice(self.content(value)).to_owned()),
            },
        })
    }

    /// Whether another property follows. A comma or line breaks before the closing brace do not
    /// count, but Chevrotain looks only three tokens ahead, so a comma and two line breaks, or
    /// three line breaks, are taken for a separator and the brace after them is then an error.
    fn separator_ahead(&self) -> bool {
        let a = self.kind(self.p);
        let b = self.kind(self.p + 1);
        let c = self.kind(self.p + 2);
        if a == T::Comma {
            return is_key(b) || (b == T::Newline && (c == T::Newline || is_key(c)));
        }
        if a != T::Newline {
            return false;
        }
        b == T::Comma
            || is_key(b)
            || (b == T::Newline && (c == T::Newline || c == T::Comma || is_key(c)))
    }

    /// Skips line breaks.
    fn skip_newlines(&mut self) {
        while self.kind(self.p) == T::Newline {
            self.p += 1;
        }
    }

    /// Reads a metadata block from `@{` to `}`.
    fn metadata(&mut self) -> Result<Vec<DraftProperty>, Error> {
        self.expect(T::MetadataStart)?;
        let mut properties = Vec::new();
        self.skip_newlines();
        if is_key(self.kind(self.p)) {
            properties.push(self.property()?);
            while self.separator_ahead() {
                if self.kind(self.p) == T::Comma {
                    self.p += 1;
                } else {
                    self.skip_newlines();
                    if self.kind(self.p) == T::Comma {
                        self.p += 1;
                    }
                }
                self.skip_newlines();
                properties.push(self.property()?);
            }
            if self.kind(self.p) == T::Comma {
                self.p += 1;
            }
            self.skip_newlines();
        }
        self.expect(T::RBrace)?;
        Ok(properties)
    }

    /// Reads a `:::` suffix and its class names into `classes`.
    fn class_suffix(&mut self, classes: &mut Vec<String>) -> Result<(), Error> {
        loop {
            self.p += 1;
            let id = self.expect(T::Identifier)?;
            classes.push(self.image(id).to_owned());
            if self.kind(self.p) != T::Comma {
                return Ok(());
            }
        }
    }

    /// An id with an optional label in parentheses or brackets, or a string that stands for
    /// both.
    fn name(&mut self, brackets: bool) -> Result<(Named, Option<Shape>), Error> {
        let k = self.p;
        let t = self.kind(k);
        let span = self.content(k);
        if t == T::Identifier {
            self.p += 1;
            let id = self.image(k).to_owned();
            let open = self.kind(self.p);
            if open == T::LParen || (brackets && open == T::LBracket) {
                self.p += 1;
                let label = self.label()?;
                let ellipse = open == T::LParen;
                self.expect(if ellipse { T::RParen } else { T::RBracket })?;
                let named = Named {
                    generated: false,
                    id,
                    label,
                    span,
                };
                return Ok((
                    named,
                    Some(if ellipse { Shape::Ellipse } else { Shape::Rect }),
                ));
            }
            let named = Named {
                generated: false,
                id,
                label: self.token_label(k),
                span,
            };
            return Ok((named, None));
        }
        if t == T::PlainString || t == T::MarkdownString {
            self.p += 1;
            let label = self.token_label(k);
            let named = Named {
                generated: true,
                id: generated_id(&label.text),
                label,
                span,
            };
            return Ok((named, None));
        }
        Err(self.no_token(&NAMES))
    }

    /// Reads an actor or use case with its metadata, stereotype, and classes.
    fn element(&mut self, actor: bool) -> Result<Item, Error> {
        let (named, shape) = self.name(!actor)?;
        let shape = if actor { None } else { shape };
        let metadata = if self.kind(self.p) == T::MetadataStart {
            Some(self.metadata()?)
        } else {
            None
        };
        let mut stereotype = None;
        if self.kind(self.p) == T::StereotypeStart {
            self.p += 1;
            let text = self.expect(T::StereotypeText)?;
            self.expect(T::StereotypeEnd)?;
            stereotype = Some(trim(self.image(text)).to_owned());
        }
        let mut classes = Vec::new();
        if self.kind(self.p) == T::ClassSeparator {
            self.class_suffix(&mut classes)?;
        }
        Ok(Item {
            classes,
            explicit: shape.is_some()
                || metadata.is_some()
                || stereotype.as_ref().is_some_and(|s| !s.is_empty()),
            metadata,
            named,
            shape,
            stereotype,
        })
    }

    /// The declaration a mention makes.
    fn draft(&self, item: &Item, actor: bool) -> DraftElement {
        DraftElement {
            actor,
            classes: item.classes.clone(),
            metadata: item.metadata.clone(),
            named: item.named.clone(),
            parent: self.parent,
            shape: item.shape,
            stereotype: item.stereotype.clone(),
        }
    }

    /// Whether a label and then one of the operators that can close it come next.
    fn labeled_right(&self, closers: bool) -> bool {
        let in_label = |t: T| (T::Usecase..=T::MarkdownString).contains(&t);
        if !in_label(self.kind(self.p)) {
            return false;
        }
        let mut k = self.p + 1;
        loop {
            let t = self.kind(k);
            if t == T::MarkerlessSolid || (closers && closes_label(t)) {
                return true;
            }
            if !in_label(t) {
                return false;
            }
            k += 1;
        }
    }

    /// Each dash beyond the second asks for one more rank between the two ends.
    fn minimum_length(&self, k: usize) -> u32 {
        let length = self.end(k) - self.start(k);
        let dashes = if self.kind(k) == T::MarkerlessSolid {
            length
        } else {
            length.saturating_sub(1)
        };
        u32::try_from(dashes.saturating_sub(1).max(1)).unwrap_or(u32::MAX)
    }

    /// Reads an arrow, its optional id and label, and its target.
    fn relation_tail(&mut self) -> Result<Tail, Error> {
        let first = self.p;
        let mut r#type = RelationshipType::Association;
        let mut arrow_type = arrow::SOLID_ARROW;
        let mut minlen = 1;
        let mut label = None;
        let mut explicit_id = None;
        if self.kind(self.p) == T::Identifier && self.kind(self.p + 1) == T::At {
            explicit_id = Some((self.image(self.p).to_owned(), self.span(self.p)));
            self.p += 2;
        }
        let k = self.p;
        let t = self.kind(k);
        self.p += 1;
        match t {
            T::DependencyArrow => {
                self.expect(T::Colon)?;
                let kind = self.kind(self.p);
                if kind != T::Include && kind != T::Extend {
                    return Err(self.no_token(&[T::Include, T::Extend]));
                }
                r#type = if kind == T::Include {
                    RelationshipType::Include
                } else {
                    RelationshipType::Extend
                };
                label = Some(DraftLabel {
                    markdown: false,
                    span: self.span(self.p),
                    text: r#type.as_str().to_owned(),
                });
                self.p += 1;
            }
            T::Generalization => r#type = RelationshipType::Generalization,
            T::ForwardSolid => minlen = self.minimum_length(k),
            T::BackwardSolid => {
                arrow_type = arrow::BACK_ARROW;
                // Only the shortest operator can open a label: `A <-- label -- B`.
                if self.end(k) - self.start(k) == 3 && self.labeled_right(false) {
                    label = Some(self.label()?);
                    let close = self.expect(T::MarkerlessSolid)?;
                    minlen = self.minimum_length(close);
                } else {
                    minlen = self.minimum_length(k);
                }
            }
            T::MarkerlessSolid => {
                arrow_type = arrow::LINE_SOLID;
                if self.end(k) - self.start(k) == 2 && self.labeled_right(true) {
                    label = Some(self.label()?);
                    match self.kind(self.p) {
                        T::ForwardSolid => arrow_type = arrow::SOLID_ARROW,
                        T::ForwardCircle => arrow_type = arrow::CIRCLE_ARROW,
                        T::ForwardCross => arrow_type = arrow::CROSS_ARROW,
                        T::MarkerlessSolid => {}
                        _ => {
                            return Err(self.no_token(&[
                                T::ForwardSolid,
                                T::MarkerlessSolid,
                                T::ForwardCircle,
                                T::ForwardCross,
                            ]));
                        }
                    }
                    if arrow_type == arrow::SOLID_ARROW || arrow_type == arrow::LINE_SOLID {
                        minlen = self.minimum_length(self.p);
                    }
                    self.p += 1;
                } else {
                    minlen = self.minimum_length(k);
                }
            }
            T::ForwardCircle => arrow_type = arrow::CIRCLE_ARROW,
            T::ForwardCross => arrow_type = arrow::CROSS_ARROW,
            T::BackwardCircle | T::BackwardCross => {
                arrow_type = if t == T::BackwardCircle {
                    arrow::CIRCLE_ARROW_REVERSED
                } else {
                    arrow::CROSS_ARROW_REVERSED
                };
                if self.labeled_right(false) {
                    label = Some(self.label()?);
                    self.expect(T::MarkerlessSolid)?;
                }
            }
            _ => {
                self.p -= 1;
                return Err(self.no_token(&ARROWS));
            }
        }
        let target = self.element(false)?;
        Ok(Tail {
            arrow_type,
            explicit_id,
            label,
            minlen,
            span: (self.start(first), self.end(self.p - 1)),
            target,
            r#type,
        })
    }

    /// Records a relationship from `source` and the target it declares.
    fn relationship(&mut self, source: &Item, declares: bool, tail: Tail) {
        let target = tail.target;
        if target.explicit {
            let draft = self.draft(&target, false);
            self.drafts.elements.push(draft);
        }
        self.drafts.relationships.push(DraftRelationship {
            arrow_type: tail.arrow_type,
            id: tail.explicit_id,
            label: tail.label,
            minlen: tail.minlen,
            source: DraftEndpoint {
                classed: !source.classes.is_empty(),
                declares,
                named: source.named.clone(),
            },
            span: tail.span,
            target: DraftEndpoint {
                classed: !target.classes.is_empty(),
                declares: target.explicit,
                named: target.named,
            },
            r#type: tail.r#type,
        });
    }

    /// `actor A, B` declares several actors; at the top level `actor A --> B` declares one and
    /// relates it.
    fn actors(&mut self, top: bool) -> Result<(), Error> {
        self.p += 1;
        let mut items = vec![self.element(true)?];
        let mut tail = None;
        if self.kind(self.p) == T::Comma {
            while self.kind(self.p) == T::Comma {
                self.p += 1;
                items.push(self.element(true)?);
            }
        } else if top && is_relation(self.kind(self.p)) {
            tail = Some(self.relation_tail()?);
        }
        self.line_end()?;
        for item in &items {
            let draft = self.draft(item, true);
            self.drafts.elements.push(draft);
        }
        if let (Some(tail), Some(first)) = (tail, items.first()) {
            self.relationship(first, true, tail);
        }
        Ok(())
    }

    /// Reads a use case statement, with a relationship at the top level.
    fn entity(&mut self, top: bool) -> Result<(), Error> {
        let mut source = self.element(false)?;
        let tail = if top && is_relation(self.kind(self.p)) {
            Some(self.relation_tail()?)
        } else {
            None
        };
        self.line_end()?;
        if tail.is_none() {
            source.explicit = true;
            source.shape = source.shape.or(Some(Shape::Ellipse));
        }
        if source.explicit {
            let draft = self.draft(&source, false);
            self.drafts.elements.push(draft);
        }
        if let Some(tail) = tail {
            let declares = source.explicit;
            self.relationship(&source, declares, tail);
        }
        Ok(())
    }

    /// Reads a system boundary through its `end`.
    fn boundary(&mut self) -> Result<(), Error> {
        self.p += 1;
        let (named, _) = self.name(true)?;
        let inline = if self.kind(self.p) == T::MetadataStart {
            Some(self.metadata()?)
        } else {
            None
        };
        let mut classes = Vec::new();
        if self.kind(self.p) == T::ClassSeparator {
            self.class_suffix(&mut classes)?;
        }
        self.line_end()?;
        let id = named.id.clone();
        let span = named.span;
        self.drafts
            .boundaries
            .push(DraftBoundary { classes, named });
        self.parent = Some(self.drafts.boundaries.len() - 1);
        while is_boundary_line(self.kind(self.p)) {
            self.line(false)?;
        }
        self.parent = None;
        self.expect(T::End)?;
        self.line_end()?;
        if let Some(properties) = inline {
            self.drafts.metadata.push(DraftMetadata {
                properties,
                span,
                target: id,
            });
        }
        Ok(())
    }

    /// Reads one style declaration, as written.
    fn style(&mut self) -> Result<String, Error> {
        let first = self.p;
        let t = self.kind(self.p);
        if is_word_token(t) || t == T::CssIdentifier {
            self.p += 1;
        } else if t == T::MarkerlessSolid {
            self.p += 1;
            if !is_word_token(self.kind(self.p)) {
                return Err(self.mismatch("WORD"));
            }
            self.p += 1;
        } else {
            return Err(self.no_alternative(&["WORD", "CSS_IDENTIFIER", "MARKERLESS_SOLID"]));
        }
        self.expect(T::Colon)?;
        if !is_style_part(self.kind(self.p)) {
            let mut detail = String::from(
                "Expecting: expecting at least one iteration which starts with one of these possible Token sequences::\n  <[",
            );
            for (i, part) in STYLE_PARTS.split('|').enumerate() {
                if i > 0 {
                    detail.push_str("] ,[");
                }
                detail.push_str(part);
            }
            detail.push_str("]>\nbut found: '");
            detail.push_str(self.image(self.p));
            detail.push('\'');
            return Err(self.fail(&detail));
        }
        while is_style_part(self.kind(self.p)) {
            self.p += 1;
        }
        // The value is taken from the source, so the spaces inside `1px solid red` survive.
        let raw = self.slice((self.start(first), self.end(self.p - 1)));
        let mut out = String::with_capacity(raw.len());
        let mut rest = raw;
        while let Some(at) = rest.find("\\,") {
            out.push_str(rest.get(..at).unwrap_or(""));
            out.push(',');
            rest = rest.get(at + 2..).unwrap_or("");
        }
        out.push_str(rest);
        Ok(out)
    }

    /// Reads comma-separated style declarations.
    fn styles(&mut self) -> Result<Vec<String>, Error> {
        let mut out = vec![self.style()?];
        while self.kind(self.p) == T::Comma {
            self.p += 1;
            out.push(self.style()?);
        }
        Ok(out)
    }

    /// Reads comma-separated identifiers.
    fn identifiers(&mut self) -> Result<Vec<usize>, Error> {
        let mut out = vec![self.expect(T::Identifier)?];
        while self.kind(self.p) == T::Comma {
            self.p += 1;
            out.push(self.expect(T::Identifier)?);
        }
        Ok(out)
    }

    #[expect(clippy::too_many_lines, reason = "mirrors Pele's topStatement")]
    /// Reads a statement at the top level.
    fn top_statement(&mut self) -> Result<(), Error> {
        let k = self.p;
        let t = self.kind(k);
        match t {
            T::AccTitleLine | T::AccDescrLine | T::AccDescrBlock => {
                self.p += 1;
                let text = self.image(k);
                let value = if t == T::AccDescrBlock {
                    let open = text.find('{').map_or(0, |at| at + 1);
                    let close = text.rfind('}').unwrap_or(text.len());
                    trim(text.get(open..close).unwrap_or(""))
                } else {
                    let colon = text.find(':').map_or(0, |at| at + 1);
                    trim(text.get(colon..).unwrap_or(""))
                }
                .to_owned();
                if t == T::AccTitleLine {
                    self.drafts.acc_title = value;
                } else {
                    self.drafts.acc_description = value;
                }
            }
            T::Direction => {
                self.p += 1;
                let value = self.kind(self.p);
                self.drafts.direction = Some(match value {
                    T::Td | T::Tb => "TB",
                    T::Bt => "BT",
                    T::Lr => "LR",
                    T::Rl => "RL",
                    _ => return Err(self.no_token(&[T::Td, T::Tb, T::Bt, T::Lr, T::Rl])),
                });
                self.p += 1;
            }
            T::Actor => return self.actors(true),
            T::SystemBoundary => return self.boundary(),
            T::Note => {
                self.p += 1;
                self.expect(T::For)?;
                let target = self.expect(T::Identifier)?;
                let label = self.label()?;
                self.drafts.notes.push(DraftNote {
                    label,
                    span: self.span(target),
                    target: self.image(target).to_owned(),
                });
            }
            T::JsonDeclarationStart => {
                self.p += 1;
                let literal = self.expect(T::JsonObjectLiteral)?;
                let mut classes = Vec::new();
                if self.kind(self.p) == T::ClassSeparator {
                    self.class_suffix(&mut classes)?;
                }
                let b = self.src.as_bytes();
                let mut from = self.start(k) + 4;
                while matches!(b.get(from), Some(b' ' | b'\t')) {
                    from += 1;
                }
                let mut to = from;
                while b.get(to).is_some_and(|&c| is_word(u32::from(c))) {
                    to += 1;
                }
                self.drafts.jsons.push(DraftJson {
                    classes,
                    id: self.slice((from, to)).to_owned(),
                    span: (from, to),
                    value: JsonDoc::default(),
                });
                self.pending_json
                    .push((self.drafts.jsons.len() - 1, literal));
            }
            T::ClassDef => {
                self.p += 1;
                let ids = self.identifiers()?;
                let styles = self.styles()?;
                let ids = ids.iter().map(|&id| self.image(id).to_owned()).collect();
                self.drafts.class_defs.push(DraftClassDef { ids, styles });
            }
            T::Class => {
                self.p += 1;
                let targets = self.identifiers()?;
                let classes = self.identifiers()?;
                let targets = targets
                    .iter()
                    .map(|&id| (self.image(id).to_owned(), self.span(id)))
                    .collect();
                let classes = classes
                    .iter()
                    .map(|&id| self.image(id).to_owned())
                    .collect();
                self.drafts.classes.push(DraftClasses { classes, targets });
            }
            T::Style => {
                self.p += 1;
                let target = self.expect(T::Identifier)?;
                let styles = self.styles()?;
                self.drafts.styles.push(DraftStyle {
                    span: self.span(target),
                    styles,
                    target: self.image(target).to_owned(),
                });
            }
            _ => {
                let next = self.kind(self.p + 1);
                if next == T::MetadataStart {
                    if !is_metadata_first(self.kind(self.p + 2)) {
                        return Err(self.no_alternative(&statement_sequences()));
                    }
                    self.p += 1;
                    let span = self.content(k);
                    let target = if t == T::Identifier {
                        self.image(k).to_owned()
                    } else {
                        generated_id(self.slice(span))
                    };
                    let properties = self.metadata()?;
                    self.drafts.metadata.push(DraftMetadata {
                        properties,
                        span,
                        target,
                    });
                } else {
                    let follows = follows_name(next)
                        || (t == T::Identifier && (next == T::LParen || next == T::LBracket));
                    // Mermaid turns away the PlantUML statements people are most likely to try.
                    let forbidden = t == T::Identifier
                        && FORBIDDEN
                            .iter()
                            .any(|word| word.eq_ignore_ascii_case(self.image(k)));
                    if !follows || forbidden {
                        return Err(self.no_alternative(&statement_sequences()));
                    }
                    return self.entity(true);
                }
            }
        }
        self.line_end()
    }

    /// Reads one line, at the top level or inside a boundary.
    fn line(&mut self, top: bool) -> Result<(), Error> {
        match self.kind(self.p) {
            T::Newline => {
                self.p += 1;
                Ok(())
            }
            T::Comment => {
                self.p += 1;
                self.line_end()
            }
            T::Actor if !top => self.actors(false),
            _ if top => self.top_statement(),
            _ => self.entity(false),
        }
    }
}

/// Parses preprocessed use case `source` into its model. `title` comes from front matter.
///
/// # Errors
///
/// Returns Mermaid's lexing, parsing, JSON, and semantic errors.
pub(crate) fn parse_usecase(source: &str, title: Option<&str>) -> Result<UsecaseModel, Error> {
    let mut parser = Parser {
        drafts: Drafts::default(),
        p: 0,
        parent: None,
        pending_json: Vec::new(),
        src: source,
        toks: tokenize(source)?,
    };
    parser.expect(T::Usecase)?;
    parser.line_end()?;
    while is_line(parser.kind(parser.p)) {
        parser.line(true)?;
    }
    if parser.kind(parser.p) != T::Eof {
        let mut detail = String::from("Redundant input, expecting EOF but found: ");
        detail.push_str(parser.image(parser.p));
        return Err(parser.fail(&detail));
    }
    // The whole text has to be well formed before the JSON in it is looked at, as in Mermaid.
    for (json, literal) in std::mem::take(&mut parser.pending_json) {
        let origin = line_column(source, parser.start(literal));
        let value = parse_ordered_json(parser.image(literal), origin)?;
        if let Some(draft) = parser.drafts.jsons.get_mut(json) {
            draft.value = value;
        }
    }
    resolve_usecase(source, parser.drafts, title)
}
