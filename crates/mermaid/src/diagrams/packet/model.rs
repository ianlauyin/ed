//! The packet model: fields split into rows of bits. Port of Pele's
//! `src/diagrams/packet/model.ts`.

use super::parser::PacketAst;
use crate::error::{Code, Error};
use crate::text::label::trim_start;
use crate::util::chars::is_space;
use crate::yaml::{Value, push_number};

/// A field, or the piece of one in a row.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PacketBlock {
    /// As Mermaid counts it: one less than the width for the pieces of a field split across rows.
    pub(crate) bits: f64,
    /// The last bit.
    pub(crate) end: f64,
    /// The label.
    pub(crate) label: String,
    /// The first bit.
    pub(crate) start: f64,
}

/// A packet diagram ready to draw: Pele's `PacketModel`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PacketModel {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    /// Bits in each row.
    pub(crate) bits_per_row: f64,
    /// The rows of fields.
    pub(crate) rows: Vec<Vec<PacketBlock>>,
    /// The title.
    pub(crate) title: Option<String>,
}

/// The most rows a packet is split into.
const MAX_ROWS: usize = 10_000;

/// The `packet` option `key` of `config` when it is a number at least `min`, capped at `max`, or
/// `fallback`.
pub(crate) fn packet_option(config: &Value, key: &str, fallback: f64, min: f64, max: f64) -> f64 {
    match property(config, "packet").and_then(|packet| property(packet, key)) {
        Some(Value::Number(value)) if *value >= min => value.min(max),
        _ => fallback,
    }
}

/// A property of a YAML mapping, as JavaScript's `value.key` reads it.
pub(crate) fn property<'v>(value: &'v Value, key: &str) -> Option<&'v Value> {
    let Value::Object(entries) = value else {
        return None;
    };
    entries
        .iter()
        .find(|(own, _)| own == key)
        .map(|(_, value)| value)
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

/// A semantic error whose message interleaves `parts` with `numbers`.
#[cold]
fn invalid(parts: &[&str], numbers: &[f64]) -> Error {
    let mut message = String::new();
    for (i, part) in parts.iter().enumerate() {
        message.push_str(part);
        if let Some(&number) = numbers.get(i) {
            push_number(&mut message, number);
        }
    }
    let mut error = Error::new(Code::Semantic, message);
    error.r#type = Some("packet");
    error
}

/// Builds the model from `ast`, as Pele's `buildPacket` does. `title` comes from front matter.
///
/// # Errors
///
/// Returns a semantic error for a field that ends before it starts, does not follow the one
/// before it, or has no bits.
#[expect(clippy::float_cmp, reason = "bit numbers compare exactly, as in Pele")]
pub(crate) fn build_packet(
    ast: PacketAst,
    config: &Value,
    title: Option<&str>,
) -> Result<PacketModel, Error> {
    let bits_per_row = packet_option(config, "bitsPerRow", 32.0, 1.0, 4096.0).floor();
    let mut rows: Vec<Vec<PacketBlock>> = Vec::new();
    let mut last_bit = -1.0;
    let mut word: Vec<PacketBlock> = Vec::new();
    let mut row = 1.0;

    for block in ast.blocks {
        if let (Some(start), Some(end)) = (block.start, block.end)
            && end < start
        {
            return Err(invalid(
                &[
                    "Packet block ",
                    " - ",
                    " is invalid. End must be greater than start.",
                ],
                &[start, end],
            ));
        }
        let mut start = block.start.unwrap_or(last_bit + 1.0);
        if start != last_bit + 1.0 {
            return Err(invalid(
                &[
                    "Packet block ",
                    " - ",
                    " is not contiguous. It should start from ",
                    ".",
                ],
                &[start, block.end.unwrap_or(start), last_bit + 1.0],
            ));
        }
        if block.bits == Some(0.0) {
            return Err(invalid(
                &[
                    "Packet block ",
                    " is invalid. Cannot have a zero bit field.",
                ],
                &[start],
            ));
        }
        let end = block.end.unwrap_or(start + block.bits.unwrap_or(1.0) - 1.0);
        let mut bits = block.bits.unwrap_or(end - start + 1.0);
        last_bit = end;

        #[expect(clippy::cast_precision_loss, reason = "rows are short")]
        while (word.len() as f64) <= bits_per_row + 1.0 && rows.len() < MAX_ROWS {
            let row_end = row * bits_per_row - 1.0;
            if end <= row_end {
                word.push(PacketBlock {
                    bits,
                    end,
                    label: block.label.clone(),
                    start,
                });
                if end == row_end {
                    rows.push(std::mem::take(&mut word));
                    row += 1.0;
                }
                break;
            }
            word.push(PacketBlock {
                bits: row_end - start,
                end: row_end,
                label: block.label.clone(),
                start,
            });
            rows.push(std::mem::take(&mut word));
            row += 1.0;
            start = row_end + 1.0;
            bits = end - start;
        }
    }
    if !word.is_empty() {
        rows.push(word);
    }

    Ok(PacketModel {
        acc_descr: ast
            .acc_descr
            .filter(|text| !text.is_empty())
            .map(|text| dedent_lines(&text)),
        acc_title: ast
            .acc_title
            .filter(|text| !text.is_empty())
            .map(|text| trim_start(&text).to_owned()),
        bits_per_row,
        rows,
        title: ast
            .title
            .filter(|text| !text.is_empty())
            .or_else(|| title.map(str::to_owned)),
    })
}
