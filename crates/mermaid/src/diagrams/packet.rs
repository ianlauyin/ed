//! Packet diagrams: protocol fields drawn as rows of bits. Port of Pele's
//! `src/diagrams/packet/index.ts`.

pub(crate) mod model;
pub(crate) mod parser;
pub(crate) mod render;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::Options;
use crate::error::Error;
use crate::yaml::Value;

/// The `packet` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input };

fn render_input(input: &Input<'_>, options: &Options) -> Result<Rendered, Error> {
    let model = parse(input.source, input.config, input.title)?;
    Ok(render::render(&model, input.config, options))
}

/// The parsed packet diagram: Pele's `PacketModel`.
pub(crate) use model::PacketModel as Model;

/// Parses preprocessed packet `source`, as Pele's `packet.parse` does, with the `packet` section
/// of `config`. `title` comes from front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for fields that do not
/// follow each other or have no bits.
pub(crate) fn parse(source: &str, config: &Value, title: Option<&str>) -> Result<Model, Error> {
    model::build_packet(parser::parse_packet(source)?, config, title)
}
