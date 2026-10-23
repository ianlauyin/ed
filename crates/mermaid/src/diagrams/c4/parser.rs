//! The C4 parser. Port of Pele's `src/diagrams/c4/parser.ts`.

use super::db::C4Db;
use super::lexer::{Token, tokenize};
use super::tokens::T;
use super::types::{C4Attr, C4Named};
use crate::error::Error;
use crate::text::label::trim;

/// Mermaid's typeC4Shape for each shape keyword, in token order from `Person`.
const KINDS: [&str; 20] = [
    "person",
    "external_person",
    "system",
    "system_db",
    "system_queue",
    "external_system",
    "external_system_db",
    "external_system_queue",
    "container",
    "container_db",
    "container_queue",
    "external_container",
    "external_container_db",
    "external_container_queue",
    "component",
    "component_db",
    "component_queue",
    "external_component",
    "external_component_db",
    "external_component_queue",
];

const HEADERS: &str = "'direction_tb', 'direction_bt', 'direction_rl', 'direction_lr', 'C4_CONTEXT', 'C4_CONTAINER', 'C4_COMPONENT', 'C4_DYNAMIC', 'C4_DEPLOYMENT'";
const ATTRIBUTES: &str = "'STR', 'STR_KEY', 'ATTRIBUTE', 'ATTRIBUTE_EMPTY'";

/// What may come next.
#[derive(Clone, Copy, PartialEq, Eq)]
enum P {
    /// A statement is required.
    Item,
    /// Only a diagram statement may come next: the first one in a boundary, or one written
    /// straight after a title.
    Diagram,
    ItemOrEnd,
    AfterOther,
    AfterDiagram,
}

fn is_other(t: T) -> bool {
    matches!(
        t,
        T::Title | T::AccDescription | T::AccTitle | T::AccDescr | T::AccDescrMultilineValue
    )
}

fn is_diagram(t: T) -> bool {
    t >= T::Person && t <= T::UpdateLayoutConfig
}

/// The kind of a shape keyword.
fn kind(t: T) -> &'static str {
    let at = match t {
        T::Person => 0,
        T::PersonExt => 1,
        T::System => 2,
        T::SystemDb => 3,
        T::SystemQueue => 4,
        T::SystemExt => 5,
        T::SystemExtDb => 6,
        T::SystemExtQueue => 7,
        T::Container => 8,
        T::ContainerDb => 9,
        T::ContainerQueue => 10,
        T::ContainerExt => 11,
        T::ContainerExtDb => 12,
        T::ContainerExtQueue => 13,
        T::Component => 14,
        T::ComponentDb => 15,
        T::ComponentQueue => 16,
        T::ComponentExt => 17,
        T::ComponentExtDb => 18,
        _ => 19,
    };
    KINDS.get(at).copied().unwrap_or("")
}

/// Parser state over the token list.
struct Parser<'a> {
    i: usize,
    src: &'a str,
    tokens: Vec<Token>,
}

impl<'a> Parser<'a> {
    fn t(&self) -> T {
        self.tokens.get(self.i).map_or(T::End, |token| token.t)
    }

    fn text(&self, at: usize) -> &'a str {
        self.tokens
            .get(at)
            .and_then(|token| self.src.get(token.start..token.end))
            .unwrap_or("")
    }

    #[cold]
    fn fail(&self, expected: &str) -> Error {
        let start = self
            .tokens
            .get(self.i)
            .map_or(self.src.len(), |token| token.start);
        let t = self.t();
        if t == T::Error {
            return Error::syntax("c4", self.src, start, "", true);
        }
        let mut detail = String::from("Expecting ");
        detail.push_str(expected);
        detail.push_str(", got '");
        detail.push_str(t.name());
        detail.push('\'');
        Error::syntax("c4", self.src, start, &detail, false)
    }

    fn expect(&mut self, t: T) -> Result<(), Error> {
        if self.t() != t {
            let mut expected = String::from("'");
            expected.push_str(t.name());
            expected.push('\'');
            return Err(self.fail(&expected));
        }
        self.i += 1;
        Ok(())
    }

    fn attributes(&mut self) -> Result<Vec<C4Attr>, Error> {
        let mut attrs = Vec::new();
        loop {
            match self.t() {
                T::Str => {
                    attrs.push(C4Attr::Text(trim(self.text(self.i)).to_owned()));
                    self.i += 1;
                }
                T::AttributeEmpty => {
                    attrs.push(C4Attr::Text(String::new()));
                    self.i += 1;
                }
                T::StrKey => {
                    self.i += 1;
                    if self.t() != T::StrValue {
                        return Err(self.fail("'STR_VALUE'"));
                    }
                    attrs.push(C4Attr::Named(C4Named {
                        key: trim(self.text(self.i - 1)).to_owned(),
                        value: trim(self.text(self.i)).to_owned(),
                    }));
                    self.i += 1;
                }
                _ => break,
            }
        }
        if attrs.is_empty() {
            return Err(self.fail(ATTRIBUTES));
        }
        Ok(attrs)
    }

    fn other(&mut self, t: T, db: &mut C4Db) -> Result<(), Error> {
        match t {
            T::Title => {
                db.set_title(after(self.text(self.i), 5));
                self.i += 1;
            }
            T::AccDescription => {
                db.set_acc_description(after(self.text(self.i), 14));
                self.i += 1;
            }
            T::AccDescrMultilineValue => {
                db.set_acc_description(trim(self.text(self.i)));
                self.i += 1;
            }
            _ => {
                self.i += 1;
                if t == T::AccTitle {
                    if self.t() != T::AccTitleValue {
                        return Err(self.fail("'acc_title_value'"));
                    }
                    // The grammar hands `accTitle:` to setTitle, so it becomes the visible title.
                    db.set_title(trim(self.text(self.i)));
                } else {
                    if self.t() != T::AccDescrValue {
                        return Err(self.fail("'acc_descr_value'"));
                    }
                    db.set_acc_description(trim(self.text(self.i)));
                }
                self.i += 1;
            }
        }
        Ok(())
    }

    /// Reads a diagram statement. Returns true when it opens a boundary.
    fn diagram(&mut self, t: T, db: &mut C4Db) -> Result<bool, Error> {
        self.i += 1;
        let mut attrs = self.attributes()?;
        if t <= T::SystemExtQueue {
            db.add_person_or_system(kind(t), &attrs);
        } else if t <= T::ComponentExtQueue {
            db.add_container(kind(t), &attrs);
        } else if t <= T::NodeR {
            let inserted = match t {
                T::EnterpriseBoundary => Some("ENTERPRISE"),
                T::SystemBoundary => Some("SYSTEM"),
                T::ContainerBoundary => Some("CONTAINER"),
                _ => None,
            };
            if let Some(inserted) = inserted {
                // `splice(2, 0, ...)` inserts at the end of a shorter list.
                attrs.insert(attrs.len().min(2), C4Attr::Text(inserted.to_owned()));
            }
            match t {
                T::Node => db.add_deployment_node("node", &attrs),
                T::NodeL => db.add_deployment_node("nodeL", &attrs),
                T::NodeR => db.add_deployment_node("nodeR", &attrs),
                T::ContainerBoundary => db.add_container_boundary(&attrs),
                _ => db.add_person_or_system_boundary(&attrs, "system"),
            }
            if self.t() == T::Lbrace {
                self.i += 1;
                self.expect(T::Newline)?;
            } else if self.t() == T::Newline {
                self.i += 1;
                self.expect(T::Lbrace)?;
                if self.t() == T::Newline {
                    self.i += 1;
                }
            } else {
                return Err(self.fail("'NEWLINE', 'LBRACE'"));
            }
            return Ok(true);
        } else if t <= T::RelIndex {
            // RelIndex takes a sequence number first, which Mermaid ignores.
            if t == T::RelIndex && !attrs.is_empty() {
                attrs.remove(0);
            }
            let r#type = match t {
                T::Birel => "birel",
                T::RelU => "rel_u",
                T::RelD => "rel_d",
                T::RelL => "rel_l",
                T::RelR => "rel_r",
                T::RelB => "rel_b",
                _ => "rel",
            };
            db.add_rel(r#type, &attrs);
        } else if t == T::UpdateElStyle {
            db.update_el_style(&attrs);
        } else if t == T::UpdateRelStyle {
            db.update_rel_style(&attrs);
        } else {
            db.update_layout_config(&attrs);
        }
        Ok(false)
    }
}

/// `text.substring(skip + 1)` in UTF-16: the text after an ASCII keyword of `skip` bytes and the
/// one whitespace character after it.
fn after(text: &str, skip: usize) -> &str {
    let space = text
        .get(skip..)
        .and_then(|rest| rest.chars().next())
        .map_or(0, char::len_utf8);
    text.get(skip + space..).unwrap_or("")
}

/// Accepts what the parser Jison generates from `c4Diagram.jison` accepts, and calls the model
/// builder in the order its semantic actions do. Boundaries nest through a counter, not
/// recursion.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse_c4(src: &str, db: &mut C4Db) -> Result<(), Error> {
    let mut p = Parser {
        i: 0,
        src,
        tokens: tokenize(src),
    };

    let header = p.t();
    if header < T::C4Context || header > T::C4Deployment {
        return Err(p.fail(HEADERS));
    }
    p.i += 1;
    p.expect(T::Newline)?;

    let mut depth: usize = 0;
    let mut state = P::Item;
    loop {
        let t = p.t();
        if state == P::AfterOther || state == P::AfterDiagram {
            if t == T::Newline {
                p.i += 1;
                state = P::ItemOrEnd;
                continue;
            }
            if state == P::AfterOther && is_diagram(t) {
                state = P::Diagram;
                continue;
            }
        } else if is_diagram(t) {
            if p.diagram(t, db)? {
                depth += 1;
                state = P::Diagram;
            } else {
                state = P::AfterDiagram;
            }
            continue;
        } else if is_other(t) && state != P::Diagram {
            p.other(t, db)?;
            state = P::AfterOther;
            continue;
        } else if state != P::ItemOrEnd {
            return Err(p.fail(
                "'title', 'accDescription', 'acc_title', 'acc_descr', 'acc_descr_multiline_value', 'PERSON', 'REL'",
            ));
        }

        if depth > 0 {
            if t != T::Rbrace {
                return Err(p.fail("'NEWLINE', 'RBRACE'"));
            }
            p.i += 1;
            db.pop_boundary_parse_stack();
            depth -= 1;
            state = P::AfterDiagram;
        } else {
            if t != T::Eof {
                return Err(p.fail("'NEWLINE', 'EOF'"));
            }
            p.i += 1;
            p.expect(T::End)?;
            db.set_c4_type(p.text(0));
            return Ok(());
        }
    }
}
