//! The pie model built from the parsed chart. Port of Pele's `src/diagrams/pie/model.ts`.

use std::collections::BTreeSet;

use super::parser::PieAst;
use crate::error::{Code, Error};
use crate::text::label::trim_start;
use crate::util::chars::is_space;
use crate::yaml::push_number;

/// A pie chart ready to draw: Pele's `PieModel`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PieModel {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    /// Slices by label in first-seen order. A repeated label keeps its first value.
    pub(crate) sections: Vec<(String, f64)>,
    /// Whether the legend shows values.
    pub(crate) show_data: bool,
    /// The title.
    pub(crate) title: Option<String>,
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

/// The error for a slice with a negative value.
#[cold]
fn negative(label: &str, value: f64) -> Error {
    let mut message = String::from("\"");
    message.push_str(label);
    message.push_str("\" has invalid value: ");
    push_number(&mut message, value);
    message.push_str(
        ". Negative values are not allowed in pie charts. All slice values must be >= 0.",
    );
    let mut error = Error::new(Code::Semantic, message);
    error.r#type = Some("pie");
    error
}

/// Builds the model from `ast`, as Pele's `buildPie` does. `title` comes from front matter.
///
/// # Errors
///
/// Returns a semantic error for a negative slice.
pub(crate) fn build_pie(ast: PieAst, title: Option<&str>) -> Result<PieModel, Error> {
    let mut seen = BTreeSet::new();
    let mut sections = Vec::new();
    for section in ast.sections {
        if section.value < 0.0 {
            return Err(negative(&section.label, section.value));
        }
        if seen.insert(section.label.clone()) {
            sections.push((section.label, section.value));
        }
    }

    Ok(PieModel {
        acc_descr: ast
            .acc_descr
            .filter(|text| !text.is_empty())
            .map(|text| dedent_lines(&text)),
        acc_title: ast
            .acc_title
            .filter(|text| !text.is_empty())
            .map(|text| trim_start(&text).to_owned()),
        sections,
        show_data: ast.show_data,
        title: ast
            .title
            .filter(|text| !text.is_empty())
            .or_else(|| title.map(str::to_owned)),
    })
}
