//! The venn diagram database. Port of Pele's `src/diagrams/venn/db.ts`.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use super::types::{VennStyle, VennSubset, VennText};
use crate::error::{Code, Error};
use crate::text::label::trim;

/// Mermaid takes one pair of quotes off an id, label or style value.
pub(crate) fn normalize_text(text: &str) -> &str {
    let trimmed = trim(text);
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        return trimmed.get(1..trimmed.len() - 1).unwrap_or("");
    }
    trimmed
}

/// Compares strings as JavaScript's default sort does: by UTF-16 code units.
pub(crate) fn utf16_cmp(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

fn normalize_ids(ids: &[&str]) -> Vec<String> {
    let mut sets: Vec<String> = ids.iter().map(|id| normalize_text(id).to_owned()).collect();
    sets.sort_by(|a, b| utf16_cmp(a, b));
    sets
}

/// A semantic error of a venn diagram.
#[cold]
pub(crate) fn semantic(message: String) -> Error {
    let mut error = Error::new(Code::Semantic, message);
    error.r#type = Some("venn");
    error
}

/// The parsed venn diagram: Pele's `VennDb`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct VennDb {
    current: Option<Vec<String>>,
    /// Whether an indented `text` line belongs to the set or union before it. The lexer reads this.
    indent: bool,
    known: BTreeSet<String>,
    /// Styles, in order.
    pub(crate) styles: Vec<VennStyle>,
    /// Sets and overlaps, in order.
    pub(crate) subsets: Vec<VennSubset>,
    /// Texts, in order.
    pub(crate) text_nodes: Vec<VennText>,
    /// The title.
    pub(crate) title: Option<String>,
}

impl VennDb {
    /// Sets the title from a `title` statement.
    pub(crate) fn set_diagram_title(&mut self, title: &str) {
        self.title = Some(title.to_owned());
    }

    /// Adds a set or overlap. A set without a size gets 10, and an overlap of n sets 10 / n².
    pub(crate) fn add_subset_data(&mut self, ids: &[&str], label: Option<&str>, size: Option<f64>) {
        let sets = normalize_ids(ids);
        self.current = Some(sets.clone());
        if let [only] = sets.as_slice() {
            self.known.insert(only.clone());
        }
        let count = ids.iter().fold(0.0, |count, _| count + 1.0);
        self.subsets.push(VennSubset {
            label: label
                .filter(|label| !label.is_empty())
                .map(|label| normalize_text(label).to_owned()),
            sets,
            size: size.unwrap_or(10.0 / (count * count)),
        });
    }

    /// Adds a text inside the region of `ids`.
    pub(crate) fn add_text_data(&mut self, ids: &[&str], id: &str, label: Option<&str>) {
        self.text_nodes.push(VennText {
            id: normalize_text(id).to_owned(),
            label: label
                .filter(|label| !label.is_empty())
                .map(|label| normalize_text(label).to_owned()),
            sets: normalize_ids(ids),
        });
    }

    /// Adds a style for `ids`. A later declaration of a property replaces an earlier one in place.
    pub(crate) fn add_style_data(&mut self, ids: &[&str], data: &[(&str, String)]) {
        let mut styles: Vec<(String, String)> = Vec::new();
        for (key, value) in data {
            let value = if value.is_empty() {
                String::new()
            } else {
                normalize_text(value).to_owned()
            };
            match styles.iter_mut().find(|(own, _)| own == key) {
                Some(entry) => entry.1 = value,
                None => styles.push(((*key).to_owned(), value)),
            }
        }
        self.styles.push(VennStyle {
            styles,
            targets: normalize_ids(ids),
        });
    }

    /// Checks that every id of a union names a set.
    ///
    /// # Errors
    ///
    /// Returns a semantic error naming the unknown ids.
    pub(crate) fn validate_union_identifiers(&self, ids: &[&str]) -> Result<(), Error> {
        let unknown: Vec<&str> = ids
            .iter()
            .map(|id| normalize_text(id))
            .filter(|id| !self.known.contains(*id))
            .collect();
        if unknown.is_empty() {
            return Ok(());
        }
        Err(semantic(
            ["unknown set identifier: ", &unknown.join(", ")].concat(),
        ))
    }

    /// The sets of the last set or union.
    pub(crate) fn current_sets(&self) -> Option<&[String]> {
        self.current.as_deref()
    }

    /// Whether an indented `text` line belongs to the set or union before it.
    pub(crate) fn indent_mode(&self) -> bool {
        self.indent
    }

    /// Sets whether an indented `text` line belongs to the set or union before it.
    pub(crate) fn set_indent_mode(&mut self, enabled: bool) {
        self.indent = enabled;
    }
}
