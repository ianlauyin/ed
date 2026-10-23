//! Tree views: files and folders nested by indentation or box-drawing prefixes. Port of Pele's
//! `src/diagrams/treeview/index.ts`.

pub(crate) mod box_drawing;
pub(crate) mod icons;
pub(crate) mod model;
pub(crate) mod parser;
pub(crate) mod render;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::Options;
use crate::error::Error;

/// The `treeView` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

/// Parses and draws the diagram.
fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render(&model, input.config, options))
}

/// The parsed tree view: Pele's `TreeViewModel`.
pub(crate) use model::TreeViewModel as Model;

/// Parses preprocessed tree view `source`, as Pele's `treeView.parse` does. `title` comes from
/// front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    Ok(model::build_tree_view(model::read_tree_view(source)?, title))
}
