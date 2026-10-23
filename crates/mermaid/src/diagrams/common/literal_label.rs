//! Labels that keep every character, labels broken between characters, and labels written from
//! their start. Port of Pele's `src/diagrams/common/literal-label.ts`.

use crate::svg::builder::esc;
use crate::svg::builder::js_round;
use crate::svg::builder::label_svg;
use crate::svg::builder::push_num;
use crate::svg::builder::shared_style;
use crate::svg::builder::span_style;
use crate::svg::builder::text_style;
use crate::text::label::LINE_HEIGHT;
use crate::text::label::Label;
use crate::text::label::Span;
use crate::text::label::trim_end;
use crate::text::measure;
use crate::util::chars::is_space;

/// Lays out `text` at font `size` in lines no wider than `room`. Literal labels, including
/// filenames and protocol fields, keep every character but the spaces a line breaks at. Long
/// words break between characters, and measuring stays bounded by one line of text.
pub(crate) fn literal_label(text: &str, size: f64, room: f64, style: u8) -> Label {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        // Byte offsets of each character and of the end, so characters `a..b` are
        // `raw[bounds[a]..bounds[b]]`.
        let bounds: Vec<usize> = raw.char_indices().map(|(at, _)| at).chain([raw.len()]).collect();
        let count = bounds.len() - 1;
        let piece = |a: usize, b: usize| match (bounds.get(a), bounds.get(b)) {
            (Some(&a), Some(&b)) => raw.get(a..b).unwrap_or(""),
            _ => "",
        };
        let space = |i: usize| piece(i, i + 1).chars().next().is_some_and(|c| is_space(u32::from(c)));

        if count == 0 {
            lines.push(String::new());
        }
        let mut from = 0;
        while from < count {
            let fits = |length: usize| measure::width(piece(from, from + length), size, style) <= room;
            let mut hi = 1;
            while hi < count - from && fits(hi) {
                hi = (count - from).min(hi * 2);
            }
            let mut lo = 1;
            while lo < hi {
                let mid = (lo + hi).div_ceil(2);
                if fits(mid) {
                    lo = mid;
                } else {
                    hi = mid - 1;
                }
            }

            let mut take = lo;
            let wraps = from + take < count;
            if wraps
                && !space(from + take)
                && let Some(i) = (1..take).rev().find(|&i| space(from + i))
            {
                take = i + 1;
            }

            let line = piece(from, from + take);
            let trimmed = trim_end(line);
            lines.push(if wraps && !trimmed.is_empty() { trimmed } else { line }.to_owned());
            from += take;
            while wraps && from < count && space(from) {
                from += 1;
            }
        }
    }
    if text.is_empty() {
        lines.clear();
    }

    let widths: Vec<f64> = lines.iter().map(|line| measure::width(line, size, style)).collect();
    let width = widths.iter().fold(0.0, |width: f64, &w| width.max(w));
    let line_height = js_round(size * LINE_HEIGHT);
    #[expect(clippy::cast_precision_loss, reason = "line counts stay far below 2^52")]
    let height = lines.len() as f64 * line_height;
    Label {
        height,
        line_height,
        lines: lines
            .into_iter()
            .zip(&widths)
            .map(|(text, &width)| vec![Span { icon: None, style, text, width }])
            .collect(),
        size,
        width,
        widths,
    }
}

/// Breaks the lines of `label` wider than `room` between characters, keeping icons whole.
pub(crate) fn break_label(label: &Label, room: f64) -> Label {
    if label.width <= room {
        return label.clone();
    }

    let mut lines: Vec<Vec<Span>> = Vec::new();
    let mut widths: Vec<f64> = Vec::new();
    for (i, spans) in label.lines.iter().enumerate() {
        let line_width = label.widths.get(i).copied().unwrap_or(f64::NAN);
        if line_width <= room {
            lines.push(spans.clone());
            widths.push(line_width);
            continue;
        }

        let mut current: Vec<Span> = Vec::new();
        let mut width = 0.0;
        for span in spans {
            if span.icon.is_some() {
                if !current.is_empty() && width + span.width > room {
                    lines.push(std::mem::take(&mut current));
                    widths.push(width);
                    width = 0.0;
                }
                current.push(span.clone());
                width += span.width;
                continue;
            }

            let mut buf = [0u8; 4];
            for c in span.text.chars() {
                let ch: &str = c.encode_utf8(&mut buf);
                let joins =
                    |last: Option<&Span>| last.is_some_and(|last| last.icon.is_none() && last.style == span.style);
                let mut joined = joins(current.last());
                let mut added = match current.last() {
                    Some(last) if joined => {
                        let mut text = last.text.clone();
                        text.push_str(ch);
                        measure::width(&text, label.size, span.style) - last.width
                    }
                    _ => measure::width(ch, label.size, span.style),
                };
                if !current.is_empty() && width + added > room {
                    lines.push(std::mem::take(&mut current));
                    widths.push(width);
                    width = 0.0;
                    joined = false;
                    added = measure::width(ch, label.size, span.style);
                }
                match current.last_mut() {
                    Some(last) if joined => {
                        last.text.push_str(ch);
                        last.width += added;
                    }
                    _ => current.push(Span { icon: None, style: span.style, text: ch.to_owned(), width: added }),
                }
                width += added;
            }
        }
        if !current.is_empty() || spans.is_empty() {
            lines.push(current);
            widths.push(width);
        }
    }

    let width = widths.iter().fold(0.0, |width: f64, &w| width.max(w));
    #[expect(clippy::cast_precision_loss, reason = "line counts stay far below 2^52")]
    let height = lines.len() as f64 * label.line_height;
    Label { height, line_height: label.line_height, lines, size: label.size, width, widths }
}

/// Writes `label` with every line starting at `x`, from `top` down. `attrs` go on each text
/// element. A line is anchored at its start and keeps its spaces, so where it begins does not
/// depend on how its text was measured. A line with icons is centered on its width and, as in
/// Pele, draws them without the host's markup.
pub(crate) fn left_label_svg(label: &Label, x: f64, top: f64, attrs: &str) -> String {
    let shared = shared_style(label);
    let mut open = String::from("<text");
    open.push_str(&text_style(attrs, shared));

    let mut body = String::new();
    for (i, spans) in label.lines.iter().enumerate() {
        #[expect(clippy::cast_precision_loss, reason = "line counts stay far below 2^52")]
        let line = i as f64;
        let cy = top + (line + 0.5) * label.line_height;
        if spans.iter().any(|span| span.icon.is_some()) {
            let width = label.widths.get(i).copied().unwrap_or(f64::NAN);
            let one = Label {
                height: label.line_height,
                line_height: label.line_height,
                lines: vec![spans.clone()],
                size: label.size,
                width,
                widths: vec![width],
            };
            body.push_str(&label_svg(&one, x + width / 2.0, cy, attrs, None));
            continue;
        }

        let mut text = String::new();
        for span in spans {
            let own = span_style(span.style & !shared);
            if own.is_empty() {
                text.push_str(&esc(&span.text));
            } else {
                text.push_str("<tspan");
                text.push_str(&own);
                text.push('>');
                text.push_str(&esc(&span.text));
                text.push_str("</tspan>");
            }
        }
        if !text.is_empty() {
            body.push_str(&open);
            body.push_str(" x=\"");
            push_num(&mut body, x);
            body.push_str("\" y=\"");
            push_num(&mut body, cy + label.size * 0.35);
            body.push_str("\" text-anchor=\"start\" xml:space=\"preserve\">");
            body.push_str(&text);
            body.push_str("</text>");
        }
    }
    body
}
