//! The sequence parser: a port of Mermaid's sequence grammar. Port of Pele's
//! `src/diagrams/sequence/parser.ts`.
//!
//! Pele builds Mermaid's nested statement lists and walks them once the document has parsed.
//! The walk visits statements in document order, and the texts Pele reads at the end of a block
//! are pure functions of the source, so this port lists the statements flat as it reads them.

use super::db::{Declare, Menu, SeqDb, Statement, extract_wrap};
use super::lexer::{Token, tokenize};
use super::tokens::T;
use super::types::{NoteActor, SeqNumbering, SeqText, line, placement};
use crate::error::Error;
use crate::text::label::trim;

const SIGNAL_TOKENS: &str = "'SOLID_OPEN_ARROW', 'DOTTED_OPEN_ARROW', 'SOLID_ARROW', 'DOTTED_ARROW', 'SOLID_CROSS', 'DOTTED_CROSS', 'SOLID_POINT', 'DOTTED_POINT'";

/// The message kind of an arrow token.
fn signal_of(kind: T) -> Option<u8> {
    Some(match kind {
        T::SolidOpenArrow => line::SOLID_OPEN,
        T::DottedOpenArrow => line::DOTTED_OPEN,
        T::SolidArrow => line::SOLID,
        T::DottedArrow => line::DOTTED,
        T::BidirectionalSolidArrow => line::BIDIRECTIONAL_SOLID,
        T::BidirectionalDottedArrow => line::BIDIRECTIONAL_DOTTED,
        T::SolidCross => line::SOLID_CROSS,
        T::DottedCross => line::DOTTED_CROSS,
        T::SolidPoint => line::SOLID_POINT,
        T::DottedPoint => line::DOTTED_POINT,
        T::SolidArrowTop => line::SOLID_TOP,
        T::SolidArrowBottom => line::SOLID_BOTTOM,
        T::StickArrowTop => line::STICK_TOP,
        T::StickArrowBottom => line::STICK_BOTTOM,
        T::SolidArrowTopDotted => line::SOLID_TOP_DOTTED,
        T::SolidArrowBottomDotted => line::SOLID_BOTTOM_DOTTED,
        T::StickArrowTopDotted => line::STICK_TOP_DOTTED,
        T::StickArrowBottomDotted => line::STICK_BOTTOM_DOTTED,
        T::SolidArrowTopReverse => line::SOLID_ARROW_TOP_REVERSE,
        T::SolidArrowBottomReverse => line::SOLID_ARROW_BOTTOM_REVERSE,
        T::StickArrowTopReverse => line::STICK_ARROW_TOP_REVERSE,
        T::StickArrowBottomReverse => line::STICK_ARROW_BOTTOM_REVERSE,
        T::SolidArrowTopReverseDotted => line::SOLID_ARROW_TOP_REVERSE_DOTTED,
        T::SolidArrowBottomReverseDotted => line::SOLID_ARROW_BOTTOM_REVERSE_DOTTED,
        T::StickArrowTopReverseDotted => line::STICK_ARROW_TOP_REVERSE_DOTTED,
        T::StickArrowBottomReverseDotted => line::STICK_ARROW_BOTTOM_REVERSE_DOTTED,
        _ => return None,
    })
}

/// The kind of an open block.
#[derive(Clone, Copy, PartialEq, Eq)]
enum K {
    Box,
    Loop,
    Rect,
    Opt,
    Break,
    Alt,
    Par,
    Critical,
}

/// Parser state: the tokens, the position, and the statements read so far.
struct Parser<'a> {
    i: usize,
    out: Vec<Statement>,
    src: &'a str,
    tokens: Vec<Token<'a>>,
}

impl<'a> Parser<'a> {
    fn token(&self, i: usize) -> Token<'a> {
        self.tokens
            .get(i)
            .or_else(|| self.tokens.last())
            .copied()
            .unwrap_or(Token {
                kind: T::Eof,
                start: self.src.len(),
                text: "",
            })
    }

    fn kind(&self) -> T {
        self.token(self.i).kind
    }

    #[cold]
    fn fail(&self, expected: &str) -> Error {
        let token = self.token(self.i);
        if token.kind == T::Error {
            return Error::syntax("sequence", self.src, token.start, "", true);
        }
        let detail = ["Expecting ", expected, ", got '", token.kind.name(), "'"].concat();
        Error::syntax("sequence", self.src, token.start, &detail, false)
    }

    fn expect(&mut self, wanted: T) -> Result<&'a str, Error> {
        if self.kind() != wanted {
            return Err(self.fail(&["'", wanted.name(), "'"].concat()));
        }
        let text = self.token(self.i).text;
        self.i += 1;
        Ok(text)
    }

    fn actor(&mut self) -> Result<&'a str, Error> {
        if !matches!(self.kind(), T::Actor | T::Link) {
            return Err(self.fail("'ACTOR'"));
        }
        let text = self.token(self.i).text;
        self.i += 1;
        Ok(text)
    }

    /// Adds the statement that mentions a participant.
    fn mention(&mut self, actor: &str) {
        self.out.push(Statement::Participant {
            actor: actor.to_owned(),
            config: None,
            description: None,
            draw: None,
            how: Declare::Add,
        });
    }

    fn text2(&mut self) -> Result<SeqText, Error> {
        let text = trim(self.expect(T::Txt)?);
        Ok(extract_wrap(text.get(1..).unwrap_or("")))
    }

    fn participant(&mut self, how: Declare) -> Result<(), Error> {
        let t = self.kind();
        self.i += 1;
        if t == T::Destroy {
            let actor = self.actor()?;
            self.expect(T::Newline)?;
            self.out.push(Statement::Participant {
                actor: actor.to_owned(),
                config: None,
                description: None,
                draw: None,
                how: if how == Declare::Create {
                    Declare::Create
                } else {
                    Declare::Destroy
                },
            });
            return Ok(());
        }
        let actor = self.actor()?;
        let mut config = None;
        if self.kind() == T::ConfigStart && self.token(self.i - 1).kind == T::Actor {
            self.i += 1;
            let content = self.expect(T::ConfigContent)?;
            self.expect(T::ConfigEnd)?;
            config = Some(trim(content).to_owned());
        }
        let draw = if t == T::Participant {
            "participant"
        } else {
            "actor"
        };
        let mut description = None;
        if self.kind() == T::As {
            self.i += 1;
            let alias = self.expect(T::RestOfLine)?;
            self.expect(T::Newline)?;
            description = Some(extract_wrap(alias));
        } else if self.kind() == T::Newline {
            self.i += 1;
        } else {
            return Err(self.fail("'AS', 'NEWLINE'"));
        }
        self.out.push(Statement::Participant {
            actor: actor.to_owned(),
            config,
            description,
            draw: Some(draw),
            how,
        });
        Ok(())
    }

    fn signal(&mut self, from: &str) -> Result<(), Error> {
        let mut central_from = false;
        if self.kind() == T::Central {
            central_from = true;
            self.i += 1;
        }
        let Some(signal_type) = signal_of(self.kind()) else {
            return Err(self.fail(SIGNAL_TOKENS));
        };
        self.i += 1;
        let mark = self.kind();
        if mark == T::Central || (!central_from && matches!(mark, T::Plus | T::Minus)) {
            self.i += 1;
        }
        let to = self.actor()?;
        let msg = self.text2()?;
        self.mention(from);
        self.mention(to);
        let marker = |actor: &str, signal_type: u8| Statement::Marker {
            actor: actor.to_owned(),
            signal_type,
        };
        let mut activate = false;
        let mut central_connection = None;
        let mut extra = Vec::new();
        if central_from {
            let dual = mark == T::Central;
            activate = dual;
            central_connection = Some(if dual {
                line::CENTRAL_CONNECTION_DUAL
            } else {
                line::CENTRAL_CONNECTION_REVERSE
            });
            if dual {
                extra.push(marker(to, line::CENTRAL_CONNECTION));
            }
            extra.push(marker(from, line::CENTRAL_CONNECTION_REVERSE));
        } else if mark == T::Plus {
            activate = true;
            extra.push(marker(to, line::ACTIVE_START));
        } else if mark == T::Minus {
            extra.push(marker(from, line::ACTIVE_END));
        } else if mark == T::Central {
            activate = true;
            central_connection = Some(line::CENTRAL_CONNECTION);
            extra.push(marker(to, line::CENTRAL_CONNECTION));
        }
        self.out.push(Statement::Message {
            activate,
            central_connection,
            from: from.to_owned(),
            msg,
            signal_type,
            to: to.to_owned(),
        });
        self.out.append(&mut extra);
        Ok(())
    }

    fn menu(&mut self, kind: Menu) -> Result<(), Error> {
        self.i += 1;
        let actor = self.actor()?;
        self.mention(actor);
        let text = self.text2()?;
        self.out.push(Statement::Menu {
            actor: actor.to_owned(),
            kind,
            text,
        });
        Ok(())
    }

    fn note(&mut self) -> Result<(), Error> {
        self.i += 1;
        let t = self.kind();
        if t == T::Over {
            self.i += 1;
            let first = self.actor()?;
            self.mention(first);
            let mut second = first;
            if self.kind() == T::Comma {
                self.i += 1;
                second = self.actor()?;
                self.mention(second);
            }
            let text = self.text2()?;
            self.out.push(Statement::Note {
                actor: NoteActor::Over(first.to_owned(), second.to_owned()),
                placement: placement::OVER,
                text,
            });
            return Ok(());
        }
        if t != T::LeftOf && t != T::RightOf {
            return Err(self.fail("'over', 'left_of', 'right_of'"));
        }
        self.i += 1;
        let who = self.actor()?;
        self.mention(who);
        let text = self.text2()?;
        self.out.push(Statement::Note {
            actor: NoteActor::Single(who.to_owned()),
            placement: if t == T::LeftOf {
                placement::LEFT_OF
            } else {
                placement::RIGHT_OF
            },
            text,
        });
        Ok(())
    }

    fn autonumber(&mut self) -> Result<(), Error> {
        self.i += 1;
        let mut numbering = SeqNumbering {
            start: None,
            step: None,
            visible: true,
        };
        if self.kind() == T::Num {
            numbering.start = Some(js_number(self.token(self.i).text));
            self.i += 1;
            numbering.step = Some(if self.kind() == T::Num {
                let step = js_number(self.token(self.i).text);
                self.i += 1;
                step
            } else {
                1.0
            });
        } else if self.kind() == T::Off {
            self.i += 1;
            numbering.visible = false;
        }
        if self.kind() != T::Newline {
            return Err(self.fail("'NEWLINE', 'NUM', 'off'"));
        }
        self.i += 1;
        self.out.push(Statement::SequenceIndex(numbering));
        Ok(())
    }

    fn block(&mut self, signal_type: u8, text: &str) {
        self.out.push(Statement::Block {
            signal_type,
            text: Some(extract_wrap(text)),
        });
    }

    /// Adds the statement that opens a block of `kind`.
    fn open(&mut self, kind: K, t: T, text: &str) {
        match kind {
            K::Box => self
                .out
                .push(Statement::BoxStart(SeqDb::parse_box_data(text))),
            K::Loop => self.block(line::LOOP_START, text),
            K::Rect => self.block(line::RECT_START, text),
            K::Opt => self.block(line::OPT_START, text),
            K::Break => self.block(line::BREAK_START, text),
            K::Alt => self.block(line::ALT_START, text),
            K::Par => self.block(
                if t == T::ParOver {
                    line::PAR_OVER_START
                } else {
                    line::PAR_START
                },
                text,
            ),
            K::Critical => self.block(line::CRITICAL_START, text),
        }
    }

    /// Adds the statement that closes a block of `kind`.
    fn close(&mut self, kind: K) {
        let signal_type = match kind {
            K::Box => {
                self.out.push(Statement::BoxEnd);
                return;
            }
            K::Loop => line::LOOP_END,
            K::Rect => line::RECT_END,
            K::Opt => line::OPT_END,
            K::Break => line::BREAK_END,
            K::Alt => line::ALT_END,
            K::Par => line::PAR_END,
            K::Critical => line::CRITICAL_END,
        };
        self.out.push(Statement::Block {
            signal_type,
            text: None,
        });
    }
}

/// JavaScript's `Number` of a numeric token.
fn js_number(text: &str) -> f64 {
    text.parse().unwrap_or(f64::NAN)
}

/// Parses preprocessed sequence diagram `src` into `db`.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and the errors of [`SeqDb::apply`].
pub(crate) fn parse_sequence(src: &str, db: &mut SeqDb) -> Result<(), Error> {
    let mut p = Parser {
        i: 0,
        out: Vec::new(),
        src,
        tokens: tokenize(src),
    };

    while p.kind() == T::Newline {
        p.i += 1;
    }
    p.expect(T::Sd)?;

    let mut stack: Vec<K> = Vec::new();
    loop {
        let t = p.kind();
        let frame = stack.last().copied();
        if frame == Some(K::Box) {
            match t {
                T::Participant | T::ParticipantActor | T::Destroy => {
                    p.participant(Declare::Add)?;
                }
                T::Newline => p.i += 1,
                T::End => {
                    p.i += 1;
                    stack.pop();
                    p.close(K::Box);
                }
                _ => {
                    return Err(
                        p.fail("'end', 'NEWLINE', 'participant', 'participant_actor', 'destroy'")
                    );
                }
            }
            continue;
        }
        match t {
            T::Newline | T::Invalid => p.i += 1,
            T::Participant | T::ParticipantActor | T::Destroy => p.participant(Declare::Add)?,
            T::Create => {
                p.i += 1;
                if !matches!(p.kind(), T::Participant | T::ParticipantActor | T::Destroy) {
                    return Err(p.fail("'participant', 'participant_actor', 'destroy'"));
                }
                p.participant(Declare::Create)?;
            }
            T::Actor => {
                let from = p.actor()?;
                p.signal(from)?;
                p.expect(T::Newline)?;
            }
            T::Link => {
                let next = p.token(p.i + 1).kind;
                if matches!(next, T::Actor | T::Link) {
                    p.menu(Menu::ALink)?;
                } else {
                    let from = p.actor()?;
                    p.signal(from)?;
                }
                p.expect(T::Newline)?;
            }
            T::Links | T::Properties | T::Details => {
                p.menu(match t {
                    T::Links => Menu::Links,
                    T::Properties => Menu::Properties,
                    _ => Menu::Details,
                })?;
                p.expect(T::Newline)?;
            }
            T::Note => {
                p.note()?;
                p.expect(T::Newline)?;
            }
            T::Autonumber => p.autonumber()?,
            T::Activate | T::Deactivate => {
                p.i += 1;
                let who = p.actor()?;
                p.expect(T::Newline)?;
                p.out.push(Statement::Marker {
                    actor: who.to_owned(),
                    signal_type: if t == T::Activate {
                        line::ACTIVE_START
                    } else {
                        line::ACTIVE_END
                    },
                });
            }
            T::Title | T::LegacyTitle => {
                let text = p.token(p.i).text;
                p.i += 1;
                // The token starts with `title` or `title:` and one whitespace character.
                let head = if t == T::Title { 5 } else { 6 };
                let rest = text.get(head..).unwrap_or("");
                let skip = rest.chars().next().map_or(0, char::len_utf8);
                db.set_diagram_title(rest.get(skip..).unwrap_or(""));
            }
            T::AccTitle => {
                p.i += 1;
                let value = trim(p.expect(T::AccTitleValue)?);
                db.set_acc_title(value);
            }
            T::AccDescr => {
                p.i += 1;
                let value = trim(p.expect(T::AccDescrValue)?);
                db.set_acc_description(value);
            }
            T::AccDescrMultilineValue => {
                let value = trim(p.token(p.i).text);
                p.i += 1;
                db.set_acc_description(value);
            }
            T::Loop
            | T::Rect
            | T::Opt
            | T::Break
            | T::Alt
            | T::Par
            | T::ParOver
            | T::Critical
            | T::Box => {
                p.i += 1;
                let text = p.expect(T::RestOfLine)?;
                let kind = match t {
                    T::Loop => K::Loop,
                    T::Rect => K::Rect,
                    T::Opt => K::Opt,
                    T::Break => K::Break,
                    T::Alt => K::Alt,
                    T::Par | T::ParOver => K::Par,
                    T::Critical => K::Critical,
                    _ => K::Box,
                };
                p.open(kind, t, text);
                stack.push(kind);
            }
            T::Else | T::And | T::Option => {
                let (wanted, signal_type) = match t {
                    T::Else => (K::Alt, line::ALT_ELSE),
                    T::And => (K::Par, line::PAR_AND),
                    _ => (K::Critical, line::CRITICAL_OPTION),
                };
                if frame != Some(wanted) {
                    return Err(p.fail("'end'"));
                }
                p.i += 1;
                let text = p.expect(T::RestOfLine)?;
                p.block(signal_type, text);
            }
            T::End => {
                let Some(kind) = frame else {
                    return Err(p.fail("'NEWLINE', 'EOF'"));
                };
                p.i += 1;
                stack.pop();
                p.close(kind);
            }
            T::Eof => {
                if frame.is_some() {
                    return Err(p.fail("'end'"));
                }
                return db.apply(p.out);
            }
            _ => return Err(p.fail("'NEWLINE', 'ACTOR', 'participant', 'note', 'end'")),
        }
    }
}
