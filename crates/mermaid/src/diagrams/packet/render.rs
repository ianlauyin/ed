//! Draws a packet diagram: rows of fields with their bit numbers. Port of Pele's
//! `src/diagrams/packet/render.ts`.

use super::model::{PacketBlock, PacketModel, packet_option, property};
use crate::diagrams::Rendered;
use crate::diagrams::common::fit_text::fit_text;
use crate::diagrams::common::fit_width::{title_room, turn_to_fit};
use crate::diagrams::common::literal_label::literal_label;
use crate::svg::builder::{esc, js_round, label_svg, push_num};
use crate::svg::root::{Accessible, RenderOptions, svg_document};
use crate::svg::theme::RADIUS;
use crate::text::entities::decode;
use crate::text::label::{Label, layout_label, trim};
use crate::text::measure::BOLD;
use crate::yaml::{Value, push_number};

/// Draws `model` with the `packet` section of `config`, running down when it does not fit the
/// host's width.
pub(crate) fn render(model: &PacketModel, config: &Value, options: &RenderOptions) -> Rendered {
    turn_to_fit(options, |down| draw(model, config, options, down))
}

/// The rows of `model` regrouped `bits_per_row` to a row. The model has split each field at its
/// own rows, so its pieces are joined again first, and a field is only split where one of the
/// new rows ends. A piece counts one bit short.
#[expect(clippy::float_cmp, reason = "bit numbers compare exactly, as in Pele")]
fn regroup(model: &PacketModel, bits_per_row: f64) -> Vec<Vec<PacketBlock>> {
    let piece = |block: &PacketBlock| block.bits == block.end - block.start;
    let mut whole: Vec<PacketBlock> = Vec::new();
    for block in model.rows.iter().flatten() {
        if let Some(prev) = whole.last_mut()
            && piece(prev)
            && piece(block)
            && prev.label == block.label
            && block.start == prev.end + 1.0
            && block.start % model.bits_per_row == 0.0
        {
            prev.end = block.end;
            prev.bits = prev.end - prev.start;
        } else {
            whole.push(block.clone());
        }
    }

    let mut rows: Vec<Vec<PacketBlock>> = Vec::new();
    for block in &whole {
        let mut start = block.start;
        while start <= block.end {
            let end = block
                .end
                .min(((start / bits_per_row).floor() + 1.0) * bits_per_row - 1.0);
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "row indices are small and not negative"
            )]
            let index = (start / bits_per_row).floor() as usize;
            // Fields are contiguous from bit 0, so a new row is always the next one.
            if rows.len() <= index {
                rows.resize_with(index + 1, Vec::new);
            }
            if let Some(row) = rows.get_mut(index) {
                row.push(PacketBlock {
                    end,
                    start,
                    ..block.clone()
                });
            }
            start = end + 1.0;
        }
    }
    rows
}

#[expect(clippy::float_cmp, reason = "sizes compare exactly, as in Pele")]
/// Draws `model`, running down when `down` is set.
fn draw(model: &PacketModel, config: &Value, options: &RenderOptions, down: bool) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let label_size = js_round(size * 0.875);
    let bit_size = js_round(size * 0.625);
    let pad = options.padding.unwrap_or(8.0);

    let packet = property(config, "packet");
    let row_height = packet_option(config, "rowHeight", 32.0, 1.0, 400.0);
    let mut bit_width = packet_option(config, "bitWidth", 32.0, 1.0, 400.0);
    let padding_x = packet_option(config, "paddingX", 5.0, 0.0, 400.0);
    let padding_y = packet_option(config, "paddingY", 5.0, 0.0, 400.0);
    let show_bits = !matches!(
        packet.and_then(|packet| property(packet, "showBits")),
        Some(Value::Bool(false))
    );
    let descending = matches!(
        packet.and_then(|packet| property(packet, "bitOrder")),
        Some(Value::String(order)) if order == "descending"
    );
    let mut bits_per_row = model.bits_per_row;
    let rows = if down {
        let room = options.max_width.unwrap_or(320.0) - 2.0 * pad;
        let count = (room / bit_width.min(18.0)).floor().max(1.0);
        // A byte is the shortest row. Below that every label is one character to a line, and a
        // long packet is a row to a bit.
        #[expect(clippy::cast_possible_truncation, reason = "a small exponent")]
        let power = 2.0_f64.powi(count.log2().floor() as i32);
        bits_per_row = bits_per_row.min(8.0_f64.max(power));
        let last = model
            .rows
            .last()
            .and_then(|row| row.last())
            .map_or(0.0, |block| block.end);
        if ((last + 1.0) / bits_per_row).ceil() > 10000.0 {
            bits_per_row = model.bits_per_row;
        }
        bit_width = bit_width.min(18.0_f64.max(room / bits_per_row));
        regroup(model, bits_per_row)
    } else {
        model.rows.clone()
    };

    let labels: Vec<Vec<Label>> = if down {
        rows.iter()
            .map(|row| {
                row.iter()
                    .map(|block| {
                        let room = 1.0_f64.max(
                            (block.end - block.start + 1.0) * bit_width
                                - padding_x.min(bit_width - 1.0)
                                - 6.0,
                        );
                        literal_label(trim(&decode(&block.label)), label_size, room, 0)
                    })
                    .collect()
            })
            .collect()
    } else {
        Vec::new()
    };
    // Bit numbers sit above each row and need room of their own.
    let numbers = if show_bits { bit_size + 4.0 } else { 0.0 };
    let row_heights: Vec<f64> = (0..rows.len())
        .map(|index| {
            let mut height = row_height;
            for label in labels.get(index).into_iter().flatten() {
                height = height.max(label.height + 10.0);
            }
            height
        })
        .collect();
    let mut tops = Vec::with_capacity(rows.len());
    let mut rows_height = 0.0;
    for height in &row_heights {
        tops.push(rows_height);
        rows_height += numbers + height + padding_y;
    }
    if !rows.is_empty() {
        rows_height -= padding_y;
    }

    let gap = padding_x.min(bit_width - 1.0);
    let width = if rows.is_empty() {
        0.0
    } else {
        bits_per_row * bit_width - gap
    };
    let title = layout_label(
        model.title.as_deref(),
        false,
        size,
        if down { title_room(options) } else { 4000.0 },
        BOLD,
        None,
    );
    let title_height = if title.height > 0.0 {
        title.height + if rows_height > 0.0 { 12.0 } else { 0.0 }
    } else {
        0.0
    };

    let left = pad + 0.0_f64.max((title.width - width) / 2.0);
    let mut fields = String::new();
    let mut bits = String::new();
    for (index, row) in rows.iter().enumerate() {
        let y = pad + tops.get(index).copied().unwrap_or(0.0) + numbers;
        let row_height = row_heights.get(index).copied().unwrap_or(row_height);
        for (at, block) in row.iter().enumerate() {
            let count = block.end - block.start + 1.0;
            let first = block.start % bits_per_row;
            let x = left
                + if descending {
                    bits_per_row - first - count
                } else {
                    first
                } * bit_width;
            let w = count * bit_width - gap;

            fields.push_str("<g class=\"ondine-node\" data-id=\"");
            push_number(&mut fields, block.start);
            fields.push('-');
            push_number(&mut fields, block.end);
            fields.push_str("\"><rect x=\"");
            push_num(&mut fields, x);
            fields.push_str("\" y=\"");
            push_num(&mut fields, y);
            fields.push_str("\" width=\"");
            push_num(&mut fields, w);
            fields.push_str("\" height=\"");
            push_num(&mut fields, row_height);
            fields.push_str("\" rx=\"");
            fields.push_str(RADIUS);
            fields.push_str("\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>");
            if down {
                if let Some(label) = labels.get(index).and_then(|row| row.get(at)) {
                    fields.push_str(&label_svg(
                        label,
                        x + w / 2.0,
                        y + row_height / 2.0,
                        "",
                        None,
                    ));
                }
            } else if let Some(text) = fit_text(
                trim(&decode(&block.label)),
                w - 6.0,
                label_size,
                js_round(size * 0.5),
                0,
            ) && text.size <= row_height
            {
                fields.push_str("<text x=\"");
                push_num(&mut fields, x + w / 2.0);
                fields.push_str("\" y=\"");
                push_num(&mut fields, y + row_height / 2.0 + text.size * 0.35);
                fields.push('"');
                if text.size != label_size {
                    fields.push_str(" font-size=\"");
                    push_num(&mut fields, text.size);
                    fields.push('"');
                }
                fields.push('>');
                fields.push_str(&esc(&text.text));
                fields.push_str("</text>");
            }
            fields.push_str("</g>");

            if !show_bits {
                continue;
            }
            let (leading, trailing) = if descending {
                (block.end, block.start)
            } else {
                (block.start, block.end)
            };
            if count == 1.0 {
                bits.push_str("<text x=\"");
                push_num(&mut bits, x + w / 2.0);
                bits.push_str("\" y=\"");
                push_num(&mut bits, y - 4.0);
                bits.push_str("\" text-anchor=\"middle\">");
                push_number(&mut bits, leading);
                bits.push_str("</text>");
            } else {
                bits.push_str("<text x=\"");
                push_num(&mut bits, x);
                bits.push_str("\" y=\"");
                push_num(&mut bits, y - 4.0);
                bits.push_str("\">");
                push_number(&mut bits, leading);
                bits.push_str("</text><text x=\"");
                push_num(&mut bits, x + w);
                bits.push_str("\" y=\"");
                push_num(&mut bits, y - 4.0);
                bits.push_str("\" text-anchor=\"end\">");
                push_number(&mut bits, trailing);
                bits.push_str("</text>");
            }
        }
    }

    let total_width = (width.max(title.width) + 2.0 * pad).ceil();
    let total_height = (rows_height + title_height + 2.0 * pad).ceil();
    let mut content = String::new();
    if !fields.is_empty() {
        content.push_str("<g class=\"ondine-packet-fields\" font-size=\"");
        push_number(&mut content, label_size);
        content.push_str("\" text-anchor=\"middle\">");
        content.push_str(&fields);
        content.push_str("</g>");
    }
    if !bits.is_empty() {
        content.push_str("<g class=\"ondine-packet-bits\" font-size=\"");
        push_number(&mut content, bit_size);
        content.push_str("\" fill=\"var(--_m)\">");
        content.push_str(&bits);
        content.push_str("</g>");
    }
    // Mermaid puts a packet's title under it.
    content.push_str(&label_svg(
        &title,
        total_width / 2.0,
        total_height - pad - title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    ));

    let svg = svg_document(
        "packet",
        total_width,
        total_height,
        size,
        options,
        Accessible {
            acc_descr: model.acc_descr.as_deref(),
            acc_title: model.acc_title.as_deref(),
        },
        &content,
    );
    Rendered {
        height: total_height,
        links: Vec::new(),
        svg,
        width: total_width,
    }
}
