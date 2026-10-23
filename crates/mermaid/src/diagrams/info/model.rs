//! The info model built from the parsed diagram. Port of Pele's `src/diagrams/info/model.ts`.

use super::parser::InfoAst;
use crate::text::label::trim_start;
use crate::util::chars::is_space;

/// An info diagram ready to draw: Pele's `InfoModel`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct InfoModel {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
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

/// Builds the model from `ast`, as Pele's `buildInfo` does. `title` comes from front matter.
pub(crate) fn build_info(ast: InfoAst, title: Option<&str>) -> InfoModel {
    InfoModel {
        acc_descr: ast
            .acc_descr
            .filter(|text| !text.is_empty())
            .map(|text| dedent_lines(&text)),
        acc_title: ast
            .acc_title
            .filter(|text| !text.is_empty())
            .map(|text| trim_start(&text).to_owned()),
        title: ast
            .title
            .filter(|text| !text.is_empty())
            .or_else(|| title.map(str::to_owned)),
    }
}
