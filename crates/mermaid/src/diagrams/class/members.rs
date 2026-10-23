//! Class members: an attribute or a method line split into its parts. Port of Pele's
//! `src/diagrams/class/members.ts`.

use crate::diagrams::common::generics::parse_generic_types;
use crate::text::label::trim;
use crate::util::chars::{is_line_end, is_space};

/// Whether a member is a method or an attribute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MemberType {
    /// A field.
    Attribute,
    /// A method, written with parentheses.
    Method,
}

impl MemberType {
    /// The name Pele uses.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Attribute => "attribute",
            Self::Method => "method",
        }
    }
}

/// A parsed member line: Pele's `ClassMember`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ClassMember {
    /// `$` (static), `*` (abstract), or empty.
    pub(crate) classifier: String,
    /// The name, or for an attribute everything but its markers.
    pub(crate) id: String,
    /// Whether the member is a method or an attribute.
    pub(crate) member_type: MemberType,
    /// A method's parameters.
    pub(crate) parameters: String,
    /// A method's return type.
    pub(crate) return_type: String,
    /// `#`, `+`, `~`, `-`, or empty.
    pub(crate) visibility: String,
}

/// Whether `c` is a visibility marker.
fn is_visibility(c: u8) -> bool {
    matches!(c, b'#' | b'+' | b'~' | b'-')
}

/// The end of the line that starts at `from`.
fn line_end(input: &str, from: usize) -> usize {
    input
        .get(from..)
        .and_then(|rest| rest.find(|c: char| is_line_end(u32::from(c))))
        .map_or(input.len(), |at| from + at)
}

impl ClassMember {
    /// Parses `input` as a member of `member_type`.
    pub(crate) fn new(input: &str, member_type: MemberType) -> Self {
        let mut member = Self {
            classifier: String::new(),
            id: String::new(),
            member_type,
            parameters: String::new(),
            return_type: String::new(),
            visibility: String::new(),
        };
        // Mermaid throws a TypeError for a method with no opening parenthesis, such as `run)`.
        // It is kept here as an attribute, so it is shown as written.
        if member_type != MemberType::Method || !member.parse_method(input) {
            member.parse_attribute(input);
        }
        member.id = if member.id.starts_with(' ') {
            let mut id = String::from(" ");
            id.push_str(trim(&member.id));
            id
        } else {
            trim(&member.id).to_owned()
        };
        member
    }

    /// The same split as Mermaid's `/([#+~-])?(.+)\((.*)\)([\s$*])?(.*)([$*])?/`, found in one
    /// pass: the first line with an opening parenthesis after its first character and a closing
    /// one later.
    fn parse_method(&mut self, input: &str) -> bool {
        let bytes = input.as_bytes();
        let n = input.len();
        let mut start = 0;
        while start < n {
            let end = line_end(input, start);
            let close = bytes
                .get(start + 1..end)
                .and_then(|line| line.iter().rposition(|&b| b == b')'))
                .map(|at| start + 1 + at);
            let open = close.and_then(|close| {
                bytes
                    .get(start + 1..close)
                    .and_then(|line| line.iter().rposition(|&b| b == b'('))
                    .map(|at| start + 1 + at)
            });
            if let (Some(close), Some(open)) = (close, open) {
                let visible =
                    bytes.get(start).copied().is_some_and(is_visibility) && open > start + 1;
                if visible {
                    input
                        .get(start..=start)
                        .unwrap_or("")
                        .clone_into(&mut self.visibility);
                }
                input
                    .get(if visible { start + 1 } else { start }..open)
                    .unwrap_or("")
                    .clone_into(&mut self.id);
                trim(input.get(open + 1..close).unwrap_or("")).clone_into(&mut self.parameters);

                let mut rest = close + 1;
                let mut classifier = String::new();
                if let Some(next) = input.get(rest..).and_then(|tail| tail.chars().next())
                    && (next == '$' || next == '*' || is_space(u32::from(next)))
                {
                    if !is_space(u32::from(next)) {
                        classifier.push(next);
                    }
                    rest += next.len_utf8();
                }
                let stop = line_end(input, rest);
                trim(input.get(rest..stop).unwrap_or("")).clone_into(&mut self.return_type);
                if classifier.is_empty()
                    && let Some(last @ ('$' | '*')) = self.return_type.chars().last()
                {
                    classifier.push(last);
                    self.return_type.pop();
                }
                self.classifier = classifier;
                return true;
            }
            start = end
                + input
                    .get(end..)
                    .and_then(|t| t.chars().next())
                    .map_or(1, char::len_utf8);
        }
        self.member_type = MemberType::Attribute;
        false
    }

    /// Splits an attribute into its visibility, name, and classifier.
    fn parse_attribute(&mut self, input: &str) {
        let bytes = input.as_bytes();
        if let Some(&first) = bytes.first()
            && is_visibility(first)
        {
            input
                .get(..1)
                .unwrap_or("")
                .clone_into(&mut self.visibility);
        }
        if let Some(&last @ (b'$' | b'*')) = bytes.last() {
            self.classifier = String::from(char::from(last));
        }
        let from = usize::from(!self.visibility.is_empty());
        let to = if self.classifier.is_empty() {
            input.len()
        } else {
            input.len() - 1
        };
        input
            .get(from.min(to)..to.max(from))
            .unwrap_or("")
            .clone_into(&mut self.id);
    }

    /// The text shown for the member, as Pele's `getDisplayDetails().displayText`.
    pub(crate) fn display_text(&self) -> String {
        let mut text = self.visibility.clone();
        text.push_str(&parse_generic_types(&self.id));
        if self.member_type == MemberType::Method {
            text.push('(');
            text.push_str(&parse_generic_types(trim(&self.parameters)));
            text.push(')');
            if !self.return_type.is_empty() {
                text.push_str(" : ");
                text.push_str(&parse_generic_types(&self.return_type));
            }
        }
        trim(&text).to_owned()
    }
}
