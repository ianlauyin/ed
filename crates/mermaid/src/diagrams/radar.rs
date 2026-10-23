//! Radar charts: series of values drawn as closed curves over radial axes. Port of Pele's
//! `src/diagrams/radar/index.ts`.

pub(crate) mod model;
pub(crate) mod parser;
pub(crate) mod render;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::Options;
use crate::error::Error;

/// The `radar` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let model = parse(input.source, input.title)?;
    Ok(render::render(&model, input.config, options))
}

/// The parsed radar chart: Pele's `RadarModel`.
pub(crate) use model::RadarModel as Model;

/// Parses preprocessed radar `source`, as Pele's `radar.parse` does. `title` comes from front
/// matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for a curve whose axis
/// references cannot be resolved.
pub(crate) fn parse(source: &str, title: Option<&str>) -> Result<Model, Error> {
    model::build_radar(parser::parse_radar(source)?, title)
}
