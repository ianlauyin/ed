//! The agentflow parser: a hand-written equivalent of Mermaid's Jison grammar that reports the
//! same errors at the same tokens and records where each statement sits. Port of Pele's
//! `src/diagrams/agentflow/parser.ts`.

use std::borrow::Cow;

use super::db::{AgentflowDb, js_trim};
use super::lexer::{Token, tokenize};
use super::tokens::T;
use super::types::{DocItem, FlowText, LabelType, LinkInfo, Loc};
use crate::error::Error;
use crate::util::chars::is_space;

/// Tokens that continue an id.
fn is_id(t: T) -> bool {
    matches!(
        t,
        T::Num
            | T::NodeString
            | T::Down
            | T::Minus
            | T::Default
            | T::Comma
            | T::Colon
            | T::Amp
            | T::Brkt
            | T::Mult
            | T::UnicodeText
    )
}

/// Tokens of an alphanumeric name, such as a callback or curve.
fn is_alpha_num(t: T) -> bool {
    matches!(
        t,
        T::Num
            | T::UnicodeText
            | T::NodeString
            | T::Dir
            | T::Down
            | T::Minus
            | T::Comma
            | T::Colon
            | T::Amp
            | T::Brkt
            | T::Mult
    )
}

/// Tokens of a block id written without brackets.
fn is_no_tags(t: T) -> bool {
    matches!(
        t,
        T::Num
            | T::NodeString
            | T::Space
            | T::Minus
            | T::Amp
            | T::UnicodeText
            | T::Colon
            | T::Mult
            | T::Brkt
            | T::Style
            | T::LinkStyle
            | T::ClassDef
            | T::Class
            | T::Click
            | T::Graph
            | T::Dir
            | T::Flow
            | T::Connector
            | T::Global
            | T::BlockEnd
            | T::Down
            | T::Up
            | T::StartLink
    )
}

/// Tokens of one style declaration.
fn is_style_part(t: T) -> bool {
    matches!(
        t,
        T::Num | T::NodeString | T::Colon | T::Space | T::Brkt | T::Style
    )
}

/// Tokens that end a statement.
fn is_separator(t: T) -> bool {
    matches!(t, T::Newline | T::Semi | T::Eof | T::Comment)
}

/// Tokens that continue a node label.
fn is_text(t: T) -> bool {
    matches!(t, T::Text | T::TagStart | T::TagEnd | T::UnicodeText)
}

const TEXT_TOKENS: &str = "'UNICODE_TEXT', 'TEXT', 'TAGSTART'";

const SEPARATORS: &str = "'COMMENT', 'SEMI', 'NEWLINE', 'EOF'";

const STATEMENT_START: &str = "'COMMENT', 'SEMI', 'NEWLINE', 'SPACE', 'EOF', 'flow', 'global', 'connector', 'acc_title', 'acc_descr', 'acc_descr_multiline_value', 'STYLE', 'LINKSTYLE', 'CLASSDEF', 'CLASS', 'CLICK', 'NODE_STRING'";

/// An open `flow` or `global` block, or a `connector` header: its header and the body of its
/// parent.
struct Frame {
    /// Token index of the keyword.
    at: usize,
    collect: bool,
    data: Option<String>,
    data_from: usize,
    data_to: usize,
    global: bool,
    id: Option<FlowText>,
    list: Vec<DocItem>,
    title: Option<FlowText>,
}

struct Parser<'s, 'd> {
    data_from: usize,
    data_to: usize,
    db: &'d mut AgentflowDb,
    i: usize,
    src: &'s str,
    tokens: Vec<Token<'s>>,
}

/// Mermaid trims spaces and tabs between a closing brace and the end of its line before
/// parsing: `source.replace(/}[^\S\n]*\n/g, '}\n')`.
fn trim_brace_lines(source: &str) -> Cow<'_, str> {
    if !source.contains('}') {
        return Cow::Borrowed(source);
    }

    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(at) = rest.find('}') {
        out.push_str(rest.get(..=at).unwrap_or(""));
        let tail = rest.get(at + 1..).unwrap_or("");
        let run = tail
            .char_indices()
            .find(|&(_, c)| c == '\n' || !is_space(u32::from(c)))
            .map_or(tail.len(), |(end, _)| end);
        if tail.get(run..).is_some_and(|after| after.starts_with('\n')) {
            out.push('\n');
            rest = tail.get(run + 1..).unwrap_or("");
        } else {
            rest = tail;
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// Parses agentflow `source` into `db`.
pub(crate) fn parse_agentflow(source: &str, db: &mut AgentflowDb) -> Result<(), Error> {
    let src = trim_brace_lines(source);
    let mut parser = Parser {
        data_from: 0,
        data_to: 0,
        db,
        i: 0,
        src: &src,
        tokens: tokenize(&src),
    };
    parser.run()
}

impl Parser<'_, '_> {
    fn kind(&self) -> T {
        self.kind_at(self.i)
    }

    fn kind_at(&self, i: usize) -> T {
        self.tokens.get(i).map_or(T::End, |token| token.kind)
    }

    fn text_at(&self, i: usize) -> &str {
        self.tokens.get(i).map_or("", |token| &token.text)
    }

    /// Takes the current token's text.
    fn take(&mut self) -> String {
        let text = self.text_at(self.i).to_owned();
        self.i += 1;
        text
    }

    /// The span from the start of token `a` to the end of token `b`.
    fn span(&self, a: usize, b: usize) -> Loc {
        let first = self
            .tokens
            .get(a)
            .map(|token| token.loc)
            .unwrap_or_default();
        let last = self
            .tokens
            .get(b)
            .map(|token| token.loc)
            .unwrap_or_default();
        Loc {
            first_column: first.first_column,
            first_line: first.first_line,
            last_column: last.last_column,
            last_line: last.last_line,
        }
    }

    #[cold]
    fn fail(&self, expected: &str) -> Error {
        let t = self.kind();
        let start = self
            .tokens
            .get(self.i)
            .map_or(self.src.len(), |token| token.start);
        if t == T::Error {
            return Error::syntax("agentflow", self.src, start, "", true);
        }

        let mut detail = String::from("Expecting ");
        detail.push_str(expected);
        detail.push_str(", got '");
        detail.push_str(t.name());
        detail.push('\'');
        Error::syntax("agentflow", self.src, start, &detail, false)
    }

    fn expect(&mut self, kind: T, also: &str) -> Result<String, Error> {
        if self.kind() != kind {
            let mut expected = String::from("'");
            expected.push_str(kind.name());
            expected.push('\'');
            if !also.is_empty() {
                expected.push_str(", ");
                expected.push_str(also);
            }
            return Err(self.fail(&expected));
        }
        Ok(self.take())
    }

    fn separator(&mut self) -> Result<(), Error> {
        if !is_separator(self.kind()) {
            return Err(self.fail(SEPARATORS));
        }
        self.i += 1;
        Ok(())
    }

    /// Concatenates a run of tokens of one class, failing with `expected` when there is none.
    fn run_of(&mut self, class: fn(T) -> bool, expected: &str) -> Result<String, Error> {
        if !class(self.kind()) {
            return Err(self.fail(expected));
        }
        let mut s = self.take();
        while class(self.kind()) {
            s.push_str(self.text_at(self.i));
            self.i += 1;
        }
        Ok(s)
    }

    fn id_string(&mut self) -> Result<String, Error> {
        self.run_of(is_id, "'NODE_STRING'")
    }

    fn alpha_num(&mut self) -> Result<String, Error> {
        self.run_of(is_alpha_num, "'NODE_STRING'")
    }

    fn text(&mut self) -> Result<FlowText, Error> {
        let r#type = match self.kind() {
            T::Str => LabelType::String,
            T::MdStr => LabelType::Markdown,
            t if is_text(t) => LabelType::Text,
            _ => {
                let mut expected = String::from(TEXT_TOKENS);
                expected.push_str(", 'TAGEND', 'STR', 'MD_STR'");
                return Err(self.fail(&expected));
            }
        };
        let mut text = self.take();
        while is_text(self.kind()) {
            text.push_str(self.text_at(self.i));
            self.i += 1;
        }
        Ok(FlowText { text, r#type })
    }

    fn closed(&mut self, kind: T) -> Result<(), Error> {
        self.expect(kind, TEXT_TOKENS).map(drop)
    }

    fn edge_text(&mut self) -> Result<FlowText, Error> {
        let r#type = match self.kind() {
            T::Str => LabelType::String,
            T::MdStr => LabelType::Markdown,
            T::EdgeText | T::UnicodeText => LabelType::Text,
            _ => return Err(self.fail("'EDGE_TEXT', 'STR', 'MD_STR'")),
        };
        let mut text = self.take();
        while matches!(self.kind(), T::EdgeText | T::UnicodeText) {
            text.push_str(self.text_at(self.i));
            self.i += 1;
        }
        Ok(FlowText { text, r#type })
    }

    fn text_no_tags(&mut self) -> Result<FlowText, Error> {
        let r#type = match self.kind() {
            T::Str => LabelType::Text,
            T::MdStr => LabelType::Markdown,
            t if is_no_tags(t) => LabelType::Text,
            _ => return Err(self.fail("'NODE_STRING', 'STR', 'MD_STR'")),
        };
        let mut text = self.take();
        while is_no_tags(self.kind()) {
            text.push_str(self.text_at(self.i));
            self.i += 1;
        }
        Ok(FlowText { text, r#type })
    }

    /// Reads a metadata block, recording the tokens it spans.
    fn shape_data(&mut self) -> String {
        self.data_from = self.i;
        let mut s = self.take();
        while self.kind() == T::ShapeData {
            s.push_str(self.text_at(self.i));
            self.i += 1;
        }
        self.data_to = self.i - 1;
        s
    }

    /// Adds metadata to `id` and widens its span over the block.
    fn attach(&mut self, id: &str, data: &str, from: usize, to: usize) -> Result<(), Error> {
        let loc = self.span(from, to);
        self.db
            .add_vertex(id, None, None, None, None, Some((data, Some(loc))))?;
        self.db.extend_vertex_mapping(id, loc);
        Ok(())
    }

    /// Adds a block header's metadata to `id`.
    fn attach_data(&mut self, id: &str, frame: &Frame) -> Result<(), Error> {
        let Some(data) = &frame.data else {
            return Ok(());
        };
        let loc = self.span(frame.data_from, frame.data_to);
        self.db
            .add_vertex(id, None, None, None, None, Some((data, Some(loc))))
    }

    fn vertex(&mut self) -> Result<String, Error> {
        let from = self.i;
        let id = self.id_string()?;
        let mut label = None;
        let mut shape = None;
        let mut props = None;
        match self.kind() {
            T::Sqs => {
                self.i += 1;
                label = Some(self.text()?);
                self.closed(T::Sqe)?;
                shape = Some("square");
            }
            T::DoubleCircleStart => {
                self.i += 1;
                label = Some(self.text()?);
                self.closed(T::DoubleCircleEnd)?;
                shape = Some("doublecircle");
            }
            T::Ps => {
                self.i += 1;
                if self.kind() == T::Ps {
                    self.i += 1;
                    label = Some(self.text()?);
                    self.closed(T::Pe)?;
                    self.expect(T::Pe, "")?;
                    shape = Some("circle");
                } else {
                    label = Some(self.text()?);
                    self.closed(T::Pe)?;
                    shape = Some("round");
                }
            }
            T::EllipseStart => {
                self.i += 1;
                label = Some(self.text()?);
                self.closed(T::EllipseEnd)?;
                shape = Some("ellipse");
            }
            T::StadiumStart => {
                self.i += 1;
                label = Some(self.text()?);
                self.closed(T::StadiumEnd)?;
                shape = Some("stadium");
            }
            T::SubroutineStart => {
                self.i += 1;
                label = Some(self.text()?);
                self.closed(T::SubroutineEnd)?;
                shape = Some("subroutine");
            }
            T::VertexWithPropsStart => {
                self.i += 1;
                let field = self.expect(T::NodeString, "")?;
                self.expect(T::Colon, "")?;
                let value = self.expect(T::NodeString, "")?;
                self.expect(T::Pipe, "")?;
                label = Some(self.text()?);
                self.closed(T::Sqe)?;
                shape = Some("rect");
                props = Some((field, value));
            }
            T::CylinderStart => {
                self.i += 1;
                label = Some(self.text()?);
                self.closed(T::CylinderEnd)?;
                shape = Some("cylinder");
            }
            T::DiamondStart => {
                self.i += 1;
                if self.kind() == T::DiamondStart {
                    self.i += 1;
                    label = Some(self.text()?);
                    self.closed(T::DiamondStop)?;
                    self.expect(T::DiamondStop, "")?;
                    shape = Some("hexagon");
                } else {
                    label = Some(self.text()?);
                    self.closed(T::DiamondStop)?;
                    shape = Some("diamond");
                }
            }
            T::TagEnd => {
                self.i += 1;
                label = Some(self.text()?);
                self.closed(T::Sqe)?;
                shape = Some("odd");
            }
            T::TrapStart => {
                self.i += 1;
                label = Some(self.text()?);
                if self.kind() == T::InvTrapEnd {
                    self.i += 1;
                    shape = Some("lean_right");
                } else {
                    self.closed(T::TrapEnd)?;
                    shape = Some("trapezoid");
                }
            }
            T::InvTrapStart => {
                self.i += 1;
                label = Some(self.text()?);
                if self.kind() == T::TrapEnd {
                    self.i += 1;
                    shape = Some("lean_left");
                } else {
                    self.closed(T::InvTrapEnd)?;
                    shape = Some("inv_trapezoid");
                }
            }
            _ => {}
        }
        self.db.add_vertex(&id, label, shape, None, props, None)?;
        let loc = self.span(from, self.i - 1);
        self.db.add_vertex_mapping(&id, shape.is_some(), loc);
        Ok(id)
    }

    fn styled_vertex(&mut self) -> Result<String, Error> {
        let id = self.vertex()?;
        if self.kind() == T::StyleSeparator {
            self.i += 1;
            let class = self.id_string()?;
            self.db.set_class(&id, &class);
        }
        Ok(id)
    }

    /// A node group joined by `&`, and the metadata that trails its last node. Pele keeps the
    /// trailing data in `pendingData`.
    fn node(&mut self) -> Result<(Vec<String>, Option<String>), Error> {
        let mut ids = vec![self.styled_vertex()?];
        loop {
            let data = (self.kind() == T::ShapeData).then(|| self.shape_data());
            let (from, to) = (self.data_from, self.data_to);
            if self.kind() != T::Space {
                return Ok((ids, data));
            }

            self.skip_spaces();
            if self.kind() != T::Amp {
                if data.is_some() {
                    return Err(self.fail("'AMP'"));
                }
                return Ok((ids, None));
            }

            self.i += 1;
            if self.kind() != T::Space {
                return Err(self.fail("'SPACE'"));
            }
            self.skip_spaces();
            let next = self.styled_vertex()?;
            if let (Some(data), Some(last)) = (&data, ids.last().cloned()) {
                self.attach(&last, data, from, to)?;
            }
            ids.push(next);
        }
    }

    /// Steps past the current token and any spaces after it.
    fn skip_spaces(&mut self) {
        self.i += 1;
        while self.kind() == T::Space {
            self.i += 1;
        }
    }

    fn link(&mut self) -> Result<LinkInfo, Error> {
        let id = (self.kind() == T::LinkId).then(|| self.take());

        if self.kind() == T::Link {
            let text = self.take();
            let mut info = AgentflowDb::destruct_link(&text, None);
            if id.is_some() {
                info.id = id;
            }
            if self.kind() == T::Pipe {
                self.i += 1;
                info.text = Some(self.text()?);
                self.closed(T::Pipe)?;
                if self.kind() == T::Space {
                    self.i += 1;
                }
            }
            return Ok(info);
        }

        if self.kind() == T::StartLink {
            let start = self.take();
            let label = self.edge_text()?;
            let end = self.expect(T::Link, "'EDGE_TEXT'")?;
            let mut info = AgentflowDb::destruct_link(&end, Some(&start));
            info.text = Some(label);
            if id.is_some() {
                info.id = id;
            }
            return Ok(info);
        }

        Err(self.fail("'LINK', 'START_LINK'"))
    }

    /// A pending metadata block trails the last node of a group.
    fn attach_pending(&mut self, ids: &[String], data: Option<&String>) -> Result<(), Error> {
        if let (Some(data), Some(last)) = (data, ids.last()) {
            let (from, to) = (self.data_from, self.data_to);
            self.attach(last, data, from, to)?;
        }
        Ok(())
    }

    fn vertex_statement(&mut self) -> Result<Vec<String>, Error> {
        let from = self.i;
        let (mut stmt, data) = self.node()?;
        self.attach_pending(&stmt, data.as_ref())?;

        let mut nodes = stmt.clone();
        loop {
            if !matches!(self.kind(), T::Link | T::StartLink | T::LinkId) {
                return Ok(nodes);
            }
            let info = self.link()?;
            let (next, data) = self.node()?;
            self.attach_pending(&next, data.as_ref())?;
            self.db.add_link(&stmt, &next, &info)?;
            let loc = self.span(from, self.i - 1);
            self.db.add_edge_mapping(&next, loc);

            let mut joined = next.clone();
            joined.append(&mut nodes);
            nodes = joined;
            stmt = next;
        }
    }

    fn style(&mut self) -> Result<String, Error> {
        self.run_of(is_style_part, "'NODE_STRING', 'NUM', 'COLON', 'BRKT'")
    }

    fn styles_opt(&mut self) -> Result<Vec<String>, Error> {
        let mut out = vec![self.style()?];
        while self.kind() == T::Comma {
            self.i += 1;
            out.push(self.style()?);
        }
        Ok(out)
    }

    fn link_style_statement(&mut self) -> Result<(), Error> {
        self.i += 1;
        self.expect(T::Space, "")?;
        let mut positions = Vec::new();
        if self.kind() == T::Default {
            positions.push(self.take());
        } else {
            positions.push(self.expect(T::Num, "'DEFAULT'")?);
            while self.kind() == T::Comma {
                self.i += 1;
                positions.push(self.expect(T::Num, "")?);
            }
        }
        self.expect(T::Space, "")?;

        if self.kind() != T::Interpolate {
            let styles = self.styles_opt()?;
            return self.db.update_link(&positions, styles);
        }

        self.i += 1;
        self.expect(T::Space, "")?;
        let interpolate = self.alpha_num()?;
        if self.kind() == T::Space {
            self.i += 1;
            let styles = self.styles_opt()?;
            self.db.update_link_interpolate(&positions, &interpolate)?;
            self.db.update_link(&positions, styles)
        } else {
            self.db.update_link_interpolate(&positions, &interpolate)
        }
    }

    fn link_tail(&mut self, id: &str, url: &str) -> Result<(), Error> {
        if self.kind() != T::Space {
            self.db.set_link(id, url, None);
            return Ok(());
        }

        self.i += 1;
        if self.kind() == T::LinkTarget {
            let target = self.take();
            self.db.set_link(id, url, Some(&target));
            return Ok(());
        }

        let tooltip = self.expect(T::Str, "'LINK_TARGET'")?;
        if self.kind() == T::Space {
            self.i += 1;
            let target = self.expect(T::LinkTarget, "")?;
            self.db.set_link(id, url, Some(&target));
        } else {
            self.db.set_link(id, url, None);
        }
        self.db.set_tooltip(id, &tooltip);
        Ok(())
    }

    fn click_statement(&mut self) -> Result<(), Error> {
        let id = self.take();
        match self.kind() {
            T::CallbackName => {
                self.i += 1;
                if self.kind() == T::CallbackArgs {
                    self.i += 1;
                }
                let mut tooltip = None;
                if self.kind() == T::Space {
                    self.i += 1;
                    tooltip = Some(self.expect(T::Str, "")?);
                }
                self.db.set_click_event(&id);
                if let Some(tooltip) = tooltip {
                    self.db.set_tooltip(&id, &tooltip);
                }
                Ok(())
            }
            T::Href => {
                self.i += 1;
                let url = self.expect(T::Str, "")?;
                self.link_tail(&id, &url)
            }
            T::Str => {
                let url = self.take();
                self.link_tail(&id, &url)
            }
            _ => {
                self.alpha_num()?;
                if self.kind() == T::Space {
                    self.i += 1;
                    let tooltip = self.expect(T::Str, "")?;
                    self.db.set_click_event(&id);
                    self.db.set_tooltip(&id, &tooltip);
                } else {
                    self.db.set_click_event(&id);
                }
                Ok(())
            }
        }
    }

    /// Reads a `flow` or `connector` header up to its separator: an id, an optional bracketed
    /// title, and optional metadata. `at` is the keyword's token index.
    fn header(&mut self, at: usize) -> Result<Frame, Error> {
        self.expect(T::Space, SEPARATORS)?;
        let id = self.text_no_tags()?;
        let mut title = FlowText {
            text: String::new(),
            r#type: LabelType::Text,
        };
        if self.kind() == T::Sqs {
            self.i += 1;
            title = self.text()?;
            self.closed(T::Sqe)?;
        }
        let data = (self.kind() == T::ShapeData).then(|| self.shape_data());
        if !is_separator(self.kind()) {
            let mut expected = String::from(SEPARATORS);
            expected.push_str(", 'SHAPE_DATA'");
            return Err(self.fail(&expected));
        }
        Ok(Frame {
            at,
            collect: false,
            data,
            data_from: self.data_from,
            data_to: self.data_to,
            global: false,
            id: Some(id),
            list: Vec::new(),
            title: Some(title),
        })
    }

    /// One statement, or `None` for statements that add nothing to a block body.
    fn statement(&mut self) -> Result<Option<DocItem>, Error> {
        let t = self.kind();
        match t {
            T::Style => {
                self.i += 1;
                self.expect(T::Space, "")?;
                let id = self.id_string()?;
                self.expect(T::Space, "")?;
                let styles = self.styles_opt()?;
                self.db
                    .add_vertex(&id, None, None, Some(styles), None, None)?;
                self.separator()?;
                Ok(None)
            }
            T::LinkStyle => {
                self.link_style_statement()?;
                self.separator()?;
                Ok(None)
            }
            T::ClassDef => {
                self.i += 1;
                self.expect(T::Space, "")?;
                let id = self.id_string()?;
                self.expect(T::Space, "")?;
                let styles = self.styles_opt()?;
                self.db.add_class(&id, &styles);
                self.separator()?;
                Ok(None)
            }
            T::Class => {
                self.i += 1;
                self.expect(T::Space, "")?;
                let ids = self.id_string()?;
                self.expect(T::Space, "")?;
                let class = self.id_string()?;
                self.db.set_class(&ids, &class);
                self.separator()?;
                Ok(None)
            }
            T::Click => {
                self.click_statement()?;
                self.separator()?;
                Ok(None)
            }
            T::Connector => {
                let at = self.i;
                self.i += 1;
                let frame = self.header(at)?;
                let sep = self.i;
                self.i += 1;
                let id = self
                    .db
                    .add_connector(frame.id.as_ref(), frame.title.as_ref());
                self.attach_data(&id, &frame)?;
                let (start, end) = (self.span(frame.at, frame.at), self.span(sep, sep));
                self.db.add_connector_mapping(frame.id.as_ref(), start, end);
                Ok(Some(DocItem::Str(id)))
            }
            T::DirectionTb | T::DirectionBt | T::DirectionRl | T::DirectionLr | T::DirectionTd => {
                self.i += 1;
                let value = match t {
                    T::DirectionTb => "TB",
                    T::DirectionBt => "BT",
                    T::DirectionRl => "RL",
                    T::DirectionLr => "LR",
                    _ => "TD",
                };
                Ok(Some(DocItem::Dir(value)))
            }
            T::AccTitle => {
                self.i += 1;
                let value = js_trim(&self.expect(T::AccTitleValue, "")?).to_owned();
                self.db.set_acc_title(&value);
                Ok(Some(DocItem::Str(value)))
            }
            T::AccDescr => {
                self.i += 1;
                let value = js_trim(&self.expect(T::AccDescrValue, "")?).to_owned();
                self.db.set_acc_description(&value);
                Ok(Some(DocItem::Str(value)))
            }
            T::AccDescrMultilineValue => {
                let value = js_trim(&self.take()).to_owned();
                self.db.set_acc_description(&value);
                Ok(Some(DocItem::Str(value)))
            }
            _ => {
                if !is_id(t) {
                    return Err(self.fail(STATEMENT_START));
                }
                let nodes = self.vertex_statement()?;
                self.separator()?;
                Ok(Some(DocItem::Ids(nodes)))
            }
        }
    }

    /// Blocks nest without recursion: each open one keeps the list and collect flag of its
    /// parent.
    fn document(&mut self) -> Result<(), Error> {
        let mut open: Vec<Frame> = Vec::new();
        let mut list: Vec<DocItem> = Vec::new();
        let mut collect = false;
        loop {
            let t = self.kind();
            if t == T::End {
                break;
            }

            if t == T::BlockEnd {
                let Some(frame) = open.pop() else {
                    break;
                };
                let at = self.i;
                self.i += 1;
                if frame.global {
                    self.db.add_global(&list);
                    list = frame.list;
                    collect = frame.collect;
                    continue;
                }
                let id = self
                    .db
                    .add_subgraph(frame.id.as_ref(), &list, frame.title.as_ref());
                self.attach_data(&id, &frame)?;
                let (start, end) = (self.span(frame.at, frame.at), self.span(at, at));
                self.db.add_subgraph_mapping(frame.id.as_ref(), start, end);
                list = frame.list;
                collect = frame.collect;
                if collect {
                    list.push(DocItem::Str(id));
                }
            } else if matches!(t, T::Semi | T::Newline | T::Space | T::Eof) {
                if collect {
                    list.push(DocItem::Str(self.text_at(self.i).to_owned()));
                }
                self.i += 1;
            } else if t == T::Comment {
                self.i += 1;
            } else if t == T::Flow || t == T::Global {
                let at = self.i;
                self.i += 1;
                let mut frame = if t == T::Global || is_separator(self.kind()) {
                    let frame = Frame {
                        at,
                        collect,
                        data: None,
                        data_from: 0,
                        data_to: 0,
                        global: t == T::Global,
                        id: None,
                        list: Vec::new(),
                        title: None,
                    };
                    self.separator()?;
                    frame
                } else {
                    let mut frame = self.header(at)?;
                    frame.collect = collect;
                    self.i += 1;
                    frame
                };
                frame.list = std::mem::take(&mut list);
                open.push(frame);
                collect = true;
            } else if let Some(item) = self.statement()?
                && collect
                && !matches!(&item, DocItem::Ids(ids) if ids.is_empty())
            {
                list.push(item);
            }
        }

        if !open.is_empty() {
            return Err(self.fail("'end'"));
        }
        Ok(())
    }

    fn run(&mut self) -> Result<(), Error> {
        while matches!(self.kind(), T::Space | T::Newline | T::Comment) {
            self.i += 1;
        }
        self.expect(T::Graph, "'SPACE', 'NEWLINE', 'COMMENT'")?;

        if self.kind() == T::NoDir {
            self.i += 1;
            self.db.set_direction("TB");
        } else {
            let dir = self.expect(T::Dir, "'NODIR'")?;
            let t = self.kind();
            if t == T::Space {
                while self.kind() == T::Space {
                    self.i += 1;
                }
                if self.kind() != T::Newline {
                    return Err(self.fail("'NEWLINE', 'SPACE'"));
                }
            } else if t != T::Semi && t != T::Newline && t != T::Comment {
                return Err(self.fail("'COMMENT', 'SEMI', 'NEWLINE', 'SPACE'"));
            }
            self.i += 1;
            self.db.set_direction(&dir);
        }

        self.document()?;
        if self.kind() != T::End {
            return Err(self.fail(STATEMENT_START));
        }
        Ok(())
    }
}
