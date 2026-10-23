//! Draws a kanban board: columns of cards, folded into rows when the host is narrow. Port of
//! Pele's `src/diagrams/kanban/render.ts`.

use super::db::{KanbanDb, KanbanNode};
use crate::diagrams::Rendered;
use crate::diagrams::common::fit_width::title_room;
use crate::diagrams::common::icon::icon_svg;
use crate::svg::builder::{esc, esc_text, js_round, label_svg, push_num, text_style};
use crate::svg::root::{Accessible, RenderOptions, svg_document};
use crate::svg::theme::{CRITICAL, ROUNDED, class_names};
use crate::text::icons::{IconResolver, icon_markup};
use crate::text::label::{Label, Span, layout_label, trim};
use crate::text::measure::{self, BOLD};
use crate::util::url::{LinkInfo, UrlOptions, link_url, rel_attr, safe_url};
use crate::yaml::{Value, push_number};

const COLUMN_GAP: f64 = 12.0;
const COLUMN_PAD: f64 = 8.0;
const CARD_PAD: f64 = 10.0;
const CARD_GAP: f64 = 8.0;
const META_GAP: f64 = 4.0;
const ICON_GAP: f64 = 6.0;

/// Mermaid names five priorities and gives Medium no color. Matching here ignores case. The high
/// ones are marked in the critical color and the low ones in the muted one, the nearer to Medium
/// the fainter. They are not series colors, as a theme may put those in any order.
fn priority_attrs(priority: &str) -> Option<String> {
    Some(match priority {
        "very high" => ["stroke=\"", CRITICAL, "\""].concat(),
        "high" => ["stroke=\"", CRITICAL, "\" opacity=\"0.5\""].concat(),
        "low" => "stroke=\"var(--_m)\" opacity=\"0.5\"".to_owned(),
        "very low" => "stroke=\"var(--_m)\"".to_owned(),
        _ => return None,
    })
}

/// A property of a YAML mapping, as JavaScript's `value.key` reads it.
fn property<'v>(value: &'v Value, key: &str) -> Option<&'v Value> {
    let Value::Object(entries) = value else {
        return None;
    };
    entries
        .iter()
        .find(|(own, _)| own == key)
        .map(|(_, value)| value)
}

/// `text` lowercased as JavaScript's `toLowerCase` does it, outside the final-sigma rule.
fn lower(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

/// Lays out text for a fixed width. A word too long for a line is broken between characters,
/// because a card cannot grow sideways to hold it.
fn fit_label(raw: Option<&str>, markdown: bool, size: f64, max: f64, base: u8) -> Label {
    let label = layout_label(raw, markdown, size, max, base, None);
    if label.width <= max {
        return label;
    }

    let mut lines: Vec<Vec<Span>> = Vec::new();
    let mut widths: Vec<f64> = Vec::new();
    let mut widest: f64 = 0.0;
    let mut line: Vec<Span> = Vec::new();
    let mut width = 0.0;
    let mut commit = |line: &mut Vec<Span>, width: &mut f64| {
        lines.push(std::mem::take(line));
        widths.push(*width);
        widest = widest.max(*width);
        *width = 0.0;
    };
    for (own, &own_width) in label.lines.iter().zip(&label.widths) {
        if own_width <= max {
            line.clone_from(own);
            width = own_width;
            commit(&mut line, &mut width);
            continue;
        }
        for span in own {
            if span.icon.is_some() {
                if width > 0.0 && width + span.width > max {
                    commit(&mut line, &mut width);
                }
                line.push(span.clone());
                width += span.width;
                continue;
            }
            let mut text = String::new();
            let mut from = width;
            for ch in span.text.chars() {
                let mut buf = [0u8; 4];
                let w = measure::width(ch.encode_utf8(&mut buf), size, span.style);
                if width > 0.0 && width + w > max {
                    if !text.is_empty() {
                        line.push(Span {
                            icon: None,
                            style: span.style,
                            text: std::mem::take(&mut text),
                            width: width - from,
                        });
                    }
                    commit(&mut line, &mut width);
                    from = 0.0;
                }
                text.push(ch);
                width += w;
            }
            if !text.is_empty() {
                line.push(Span {
                    icon: None,
                    style: span.style,
                    text,
                    width: width - from,
                });
            }
        }
        commit(&mut line, &mut width);
    }
    #[expect(clippy::cast_precision_loss, reason = "line counts are far below 2^52")]
    let height = lines.len() as f64 * label.line_height;
    Label {
        height,
        lines,
        width: widest,
        widths,
        ..label
    }
}

/// Writes a label with every line starting at `x`, or ending there when `end`, and the first
/// line's box starting at `y`.
fn text_block(
    label: &Label,
    x: f64,
    y: f64,
    attrs: &str,
    icons: Option<IconResolver<'_>>,
    end: bool,
) -> String {
    let mut out = String::new();
    let mut cy = y + 0.5 * label.line_height;
    for (line, &width) in label.lines.iter().zip(&label.widths) {
        if let [span] = line.as_slice()
            && span.icon.is_none()
        {
            out.push_str("<text");
            out.push_str(&text_style(attrs, span.style));
            out.push_str(" x=\"");
            push_num(&mut out, x);
            out.push_str("\" y=\"");
            push_num(&mut out, cy + label.size * 0.35);
            out.push('"');
            if end {
                out.push_str(" text-anchor=\"end\"");
            }
            out.push('>');
            out.push_str(&esc(&span.text));
            out.push_str("</text>");
        } else {
            let one = Label {
                height: label.line_height,
                line_height: label.line_height,
                lines: vec![line.clone()],
                size: label.size,
                width,
                widths: vec![width],
            };
            let shift = if end { -width } else { width };
            out.push_str(&label_svg(&one, x + shift / 2.0, cy, attrs, icons));
        }
        cy += label.line_height;
    }
    out
}

/// A card's laid-out text.
struct Card<'m> {
    assigned: Label,
    height: f64,
    item: &'m KanbanNode,
    label: Label,
    /// Height of the rows under the text that hold the ticket and the assignee.
    meta: f64,
    ticket: Label,
}

/// Draws `model` with the `kanban` section of `config`.
#[expect(
    clippy::cast_precision_loss,
    reason = "counts and indexes are far below 2^52"
)]
pub(crate) fn render(
    model: &KanbanDb,
    config: &Value,
    options: &RenderOptions,
    url: &UrlOptions<'_>,
    icons: Option<IconResolver<'_>>,
) -> Rendered {
    let has = |name: Option<&str>| {
        name.is_some_and(|name| !name.is_empty() && !icon_markup(icons, name).is_empty())
    };
    let size = options.font_size.unwrap_or(16.0);
    let small = js_round(size * 0.875);
    let line = js_round(size * 1.5);
    let small_line = js_round(small * 1.5);
    let pad = options.padding.unwrap_or(8.0);
    let conf = property(config, "kanban");
    let own_width = match conf.and_then(|conf| property(conf, "sectionWidth")) {
        Some(Value::Number(width)) if *width > 0.0 => width.clamp(80.0, 2000.0),
        _ => 200.0,
    };
    // A board wider than the width the host has is folded into rows of columns, which then share
    // the row between them.
    let room = match options.max_width {
        Some(width) if width > 0.0 => width - 2.0 * pad,
        _ => f64::INFINITY,
    };
    let sections = &model.sections;
    let count = sections.len() as f64;
    let fits = count * (own_width + COLUMN_GAP) - COLUMN_GAP <= room;
    let per_row = if fits {
        count.max(1.0)
    } else {
        1.0_f64.max(((room + COLUMN_GAP) / (own_width + COLUMN_GAP)).floor())
    };
    let column_width = if fits {
        own_width
    } else {
        own_width
            .min(room)
            .max((((room + COLUMN_GAP) / per_row).floor() - COLUMN_GAP).min(own_width * 2.0))
    };
    let base_url = match conf.and_then(|conf| property(conf, "ticketBaseUrl")) {
        Some(Value::String(base)) => base.as_str(),
        _ => "",
    };
    let card_width = column_width - 2.0 * COLUMN_PAD;
    let text_width = card_width - 2.0 * CARD_PAD;
    let glyph = small * 1.25;
    let mut links: Vec<LinkInfo> = Vec::new();

    let mut titles: Vec<Label> = Vec::with_capacity(sections.len());
    let mut count_widths: Vec<f64> = Vec::with_capacity(sections.len());
    let mut cards: Vec<Vec<Card<'_>>> = Vec::with_capacity(sections.len());
    let mut head_height = line;
    let mut body_heights: Vec<f64> = Vec::with_capacity(sections.len());
    let mut digits = String::new();
    for section in sections {
        digits.clear();
        push_number(&mut digits, section.items.len() as f64);
        let count_width = measure::width(&digits, small, 0);
        let reserved = count_width
            + 12.0
            + if has(section.node.icon.as_deref()) {
                glyph + ICON_GAP
            } else {
                0.0
            };
        let title = fit_label(
            Some(&section.node.label),
            true,
            size,
            card_width - 8.0 - reserved,
            BOLD,
        );
        head_height = head_height.max(title.height);
        count_widths.push(count_width);
        titles.push(title);

        let mut list = Vec::with_capacity(section.items.len());
        let mut height = 0.0;
        for item in &section.items {
            let icon_room = if has(item.icon.as_deref()) {
                glyph + ICON_GAP
            } else {
                0.0
            };
            let label = fit_label(Some(&item.label), true, small, text_width - icon_room, 0);
            let ticket = fit_label(item.ticket.as_deref(), false, small, text_width, 0);
            let assigned = fit_label(item.assigned.as_deref(), false, small, text_width, 0);
            let meta = if ticket.width + assigned.width + 8.0 > text_width {
                ticket.height + assigned.height
            } else {
                ticket.height.max(assigned.height)
            };
            let card_height = 2.0 * CARD_PAD
                + label.height.max(small_line)
                + if meta == 0.0 { 0.0 } else { META_GAP + meta };
            list.push(Card {
                assigned,
                height: card_height,
                item,
                label,
                meta,
                ticket,
            });
            height += card_height + CARD_GAP;
        }
        cards.push(list);
        body_heights.push(height);
    }
    head_height += 2.0 * COLUMN_PAD;

    // `per_row` is a whole number of at least one.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a whole number of at least one"
    )]
    let per_row_count = (per_row as usize).max(1);
    let mut row_heights: Vec<f64> = Vec::new();
    let mut row_tops: Vec<f64> = Vec::new();
    let mut y = 0.0;
    for row in body_heights.chunks(per_row_count) {
        let tallest = row.iter().fold(CARD_GAP, |tallest, &h| tallest.max(h));
        row_tops.push(y);
        row_heights.push(head_height + tallest);
        y += head_height + tallest + COLUMN_GAP;
    }
    let board_height = match (row_tops.last(), row_heights.last()) {
        (Some(top), Some(height)) => top + height,
        _ => 0.0,
    };

    let title = layout_label(
        model.title.as_deref(),
        false,
        size,
        title_room(options),
        BOLD,
        None,
    );
    let title_height = if title.height > 0.0 {
        title.height + 16.0
    } else {
        0.0
    };
    let board_width = (count.min(per_row) * (column_width + COLUMN_GAP) - COLUMN_GAP).max(0.0);
    let width = board_width.max(title.width);
    let left = pad + (width - board_width) / 2.0;
    let edge = column_width - COLUMN_PAD - 4.0;

    let mut out = String::new();
    for (index, section) in sections.iter().enumerate() {
        let row = index / per_row_count;
        let row_height = row_heights.get(row).copied().unwrap_or(0.0);
        let mut body = String::from("<rect width=\"");
        push_number(&mut body, column_width);
        body.push_str("\" height=\"");
        push_num(&mut body, row_height);
        body.push('"');
        body.push_str(ROUNDED);
        body.push_str(" fill=\"var(--_s)\"/>");
        if let Some(title) = titles.get(index) {
            body.push_str(&text_block(
                title,
                COLUMN_PAD + 4.0,
                COLUMN_PAD,
                " class=\"ondine-cluster-label\" font-weight=\"var(--_hw)\"",
                icons,
                false,
            ));
        }
        body.push_str("<text class=\"ondine-count\" x=\"");
        push_number(&mut body, edge);
        body.push_str("\" y=\"");
        push_num(&mut body, COLUMN_PAD + line / 2.0 + small * 0.35);
        body.push_str("\" text-anchor=\"end\" font-size=\"");
        push_number(&mut body, small);
        body.push_str("\" fill=\"var(--_m)\">");
        push_number(&mut body, section.items.len() as f64);
        body.push_str("</text>");
        if let Some(icon) = section.node.icon.as_deref()
            && has(Some(icon))
        {
            body.push_str(&icon_svg(
                icon,
                edge - count_widths.get(index).copied().unwrap_or(0.0) - ICON_GAP - glyph,
                COLUMN_PAD + (line - glyph) / 2.0,
                glyph,
                icons,
            ));
        }

        let mut y = head_height;
        for card in cards.get(index).map_or(&[][..], Vec::as_slice) {
            let item = card.item;
            let priority = lower(trim(item.priority.as_deref().unwrap_or("")));
            let color = priority_attrs(&priority);
            let mut inner = String::from("<rect width=\"");
            push_number(&mut inner, card_width);
            inner.push_str("\" height=\"");
            push_num(&mut inner, card.height);
            inner.push('"');
            inner.push_str(ROUNDED);
            inner.push_str(" fill=\"var(--_bg)\" stroke=\"var(--_b)\"/>");
            if let Some(color) = &color {
                inner.push_str("<path class=\"ondine-priority\" d=\"M2.5,5V");
                push_num(&mut inner, card.height - 5.0);
                inner.push_str("\" fill=\"none\" ");
                inner.push_str(color);
                inner.push_str(" stroke-width=\"3\" stroke-linecap=\"round\"/>");
            }
            inner.push_str(&text_block(
                &card.label,
                CARD_PAD,
                CARD_PAD,
                " class=\"ondine-label\"",
                icons,
                false,
            ));
            if let Some(icon) = item.icon.as_deref()
                && has(Some(icon))
            {
                inner.push_str(&icon_svg(
                    icon,
                    card_width - CARD_PAD - glyph,
                    CARD_PAD + (small_line - glyph) / 2.0,
                    glyph,
                    icons,
                ));
            }
            let bottom = card.height - CARD_PAD;
            if let Some(ticket) = item.ticket.as_deref().filter(|ticket| !ticket.is_empty()) {
                let top = bottom - card.meta;
                // `String.prototype.replace` with a string pattern replaces the first match only.
                let target = base_url.find("#TICKET#").map_or_else(
                    || base_url.to_owned(),
                    |at| {
                        [
                            base_url.get(..at).unwrap_or(""),
                            ticket,
                            base_url.get(at + 8..).unwrap_or(""),
                        ]
                        .concat()
                    },
                );
                let safe = if base_url.is_empty() {
                    String::new()
                } else {
                    safe_url(&target)
                };
                let href = if base_url.is_empty() {
                    None
                } else {
                    link_url(&safe, url)
                };
                if let Some(href) = href {
                    inner.push_str("<a href=\"");
                    inner.push_str(&esc(href));
                    inner.push_str("\" target=\"_blank\"");
                    inner.push_str(&rel_attr(url));
                    inner.push('>');
                    inner.push_str(&text_block(
                        &card.ticket,
                        CARD_PAD,
                        top,
                        " class=\"ondine-ticket\" fill=\"var(--_c)\"",
                        icons,
                        false,
                    ));
                    inner.push_str("</a>");
                    links.push(LinkInfo {
                        href: href.to_owned(),
                        id: item.id.clone(),
                        internal: false,
                    });
                } else {
                    inner.push_str(&text_block(
                        &card.ticket,
                        CARD_PAD,
                        top,
                        " class=\"ondine-ticket\" fill=\"var(--_m)\"",
                        icons,
                        false,
                    ));
                }
            }
            if item
                .assigned
                .as_deref()
                .is_some_and(|assigned| !assigned.is_empty())
            {
                inner.push_str(&text_block(
                    &card.assigned,
                    card_width - CARD_PAD,
                    bottom - card.assigned.height,
                    " class=\"ondine-assigned\" fill=\"var(--_m)\"",
                    icons,
                    true,
                ));
            }
            body.push_str("<g class=\"ondine-node ondine-card");
            if color.is_some() {
                body.push_str(" ondine-priority-");
                // `replace(' ', '-')` replaces the first space only.
                body.push_str(&priority.replacen(' ', "-", 1));
            }
            body.push_str(&class_names(item.css_classes.as_deref().unwrap_or("")));
            body.push_str("\" data-id=\"");
            body.push_str(&esc_text(&item.id));
            body.push_str("\" transform=\"translate(");
            push_number(&mut body, COLUMN_PAD);
            body.push(',');
            push_num(&mut body, y);
            body.push_str(")\" font-size=\"");
            push_number(&mut body, small);
            body.push_str("\">");
            body.push_str(&inner);
            body.push_str("</g>");
            y += card.height + CARD_GAP;
        }

        out.push_str("<g class=\"ondine-cluster ondine-column");
        out.push_str(&class_names(
            section.node.css_classes.as_deref().unwrap_or(""),
        ));
        out.push_str("\" data-id=\"");
        out.push_str(&esc_text(&section.node.id));
        out.push_str("\" transform=\"translate(");
        push_num(
            &mut out,
            left + (index % per_row_count) as f64 * (column_width + COLUMN_GAP),
        );
        out.push(',');
        push_num(
            &mut out,
            pad + title_height + row_tops.get(row).copied().unwrap_or(0.0),
        );
        out.push_str(")\">");
        out.push_str(&body);
        out.push_str("</g>");
    }

    let total_width = (width + 2.0 * pad).ceil();
    let total_height = (board_height + title_height + 2.0 * pad).ceil();
    let mut content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str(&out);

    Rendered {
        height: total_height,
        links,
        svg: svg_document(
            "kanban",
            total_width,
            total_height,
            size,
            options,
            Accessible::default(),
            &content,
        ),
        width: total_width,
    }
}
