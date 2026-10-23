//! Architecture diagrams: Mermaid's `architecture-beta` services, junctions, groups, and edges
//! between their sides. Port of Pele's `src/diagrams/architecture/index.ts`.

pub(crate) mod db;
pub(crate) mod icons;
pub(crate) mod layout;
pub(crate) mod parser;
pub(crate) mod render;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::Options;
use crate::error::Error;

/// The `architecture` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render_architecture(&model, input.config, options))
}

/// The parsed architecture model: Pele's `ArchitectureDb`.
pub(crate) use db::ArchitectureDb as Model;

/// Parses preprocessed architecture `source`, as Pele's `architecture.parse` does. `title` comes
/// from front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for a model Mermaid's
/// database refuses.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::default();
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        db.title = Some(title.to_owned());
    }
    db::populate(parser::parse_architecture(source)?, &mut db)?;
    Ok(db)
}
