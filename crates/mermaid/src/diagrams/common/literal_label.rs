//! Labels that keep every character, labels broken between characters, and labels written from
//! their start. Port of Pele's `src/diagrams/common/literal-label.ts`.

use crate::svg::builder::{
    esc, js_round, label_svg, push_num, shared_style, span_style, text_style,
};
use crate::text::label::{LINE_HEIGHT, Label, Span, trim_end};
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
        let bounds: Vec<usize> = raw
            .char_indices()
            .map(|(at, _)| at)
            .chain([raw.len()])
            .collect();
        let count = bounds.len() - 1;
        let piece = |a: usize, b: usize| match (bounds.get(a), bounds.get(b)) {
            (Some(&a), Some(&b)) => raw.get(a..b).unwrap_or(""),
            _ => "",
        };
        let space = |i: usize| {
            piece(i, i + 1)
                .chars()
                .next()
                .is_some_and(|c| is_space(u32::from(c)))
        };

        if count == 0 {
            lines.push(String::new());
        }
        let mut from = 0;
        while from < count {
            let fits =
                |length: usize| measure::width(piece(from, from + length), size, style) <= room;
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
            lines.push(
                if wraps && !trimmed.is_empty() {
                    trimmed
                } else {
                    line
                }
                .to_owned(),
            );
            from += take;
            while wraps && from < count && space(from) {
                from += 1;
            }
        }
    }
    if text.is_empty() {
        lines.clear();
    }

    let widths: Vec<f64> = lines
        .iter()
        .map(|line| measure::width(line, size, style))
        .collect();
    let width = widths.iter().fold(0.0, |width: f64, &w| width.max(w));
    let line_height = js_round(size * LINE_HEIGHT);
    #[expect(
        clippy::cast_precision_loss,
        reason = "line counts stay far below 2^52"
    )]
    let height = lines.len() as f64 * line_height;
    Label {
        height,
        line_height,
        lines: lines
            .into_iter()
            .zip(&widths)
            .map(|(text, &width)| {
                vec![Span {
                    icon: None,
                    style,
                    text,
                    width,
                }]
            })
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
                let joins = |last: Option<&Span>| {
                    last.is_some_and(|last| last.icon.is_none() && last.style == span.style)
                };
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
                    _ => current.push(Span {
                        icon: None,
                        style: span.style,
                        text: ch.to_owned(),
                        width: added,
                    }),
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
    #[expect(
        clippy::cast_precision_loss,
        reason = "line counts stay far below 2^52"
    )]
    let height = lines.len() as f64 * label.line_height;
    Label {
        height,
        line_height: label.line_height,
        lines,
        size: label.size,
        width,
        widths,
    }
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
        #[expect(
            clippy::cast_precision_loss,
            reason = "line counts stay far below 2^52"
        )]
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
                text.push_str(own);
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

#[cfg(test)]
#[expect(
    clippy::unreadable_literal,
    reason = "hashes as the Pele scripts print them"
)]
mod tests {
    //! Checks against Pele (`obsidianmd/pele` at `458322a`). Every expected value comes from
    //! running Pele's `literalLabel`, `breakLabel` (of a markdown `layoutLabel` with a host that
    //! has one icon), and `leftLabelSvg` with `bun` and the monospace branch of its
    //! `metricsMeasurer`, never from this port. Pele's `pele-` class prefix reads as `ondine-`.
    //! Results compare FNV-1a hashes of the same serialization on both sides.

    use super::*;
    use crate::diagrams::common::fit::tests::label;
    use crate::diagrams::common::tokens::tests::{CORPUS, fnv, inputs};
    use crate::text::label::layout_label;

    fn literal(text: &str) -> String {
        let icons = |name: &str| (name == "fa:fa-car").then(|| "<path d=\"M1 1\"/>".to_owned());
        let resolver: &dyn Fn(&str) -> Option<String> = &icons;
        let mut out = Vec::new();
        for room in [0.0, 30.0, 80.0, 200.0] {
            for style in [0, 2] {
                let lit = literal_label(text, 14.0, room, style);
                out.push(label(&lit));
                out.push(left_label_svg(&lit, 10.0, 5.0, " class=\"a\""));
                let laid = layout_label(Some(text), true, 14.0, room, style, Some(resolver));
                let broken = break_label(&laid, room);
                out.push(label(&broken));
                out.push(left_label_svg(&broken, 3.5, 0.0, " font-weight=\"400\""));
            }
        }
        out.join("\n")
    }

    #[test]
    fn literal_label_matches_pele_on_edge_cases() {
        let cases: &[(&str, u32)] = &[
            ("", 0xde34fd4d),
            ("a", 0x1cf793bd),
            ("hello world", 0xca2f255),
            ("averyveryverylongwordthatcannotfitanywhere", 0x7e02b65d),
            ("src/components/VeryLongFileName.tsx", 0x7bf149d5),
            ("two  spaces   here and\ttab", 0x926a72f9),
            (
                "line one\nline two that is longer\n\nafter blank",
                0x1f6ab96f,
            ),
            ("trailing   \nspaces", 0xdc9aea91),
            ("<b>bold</b> and <br> break <BR/> and <br />x", 0x251fee7),
            ("a <script>x</script> fa:fa-car :fa-x", 0x5de6fbed),
            ("fa:fa-car drives fa:fa-none far away from here", 0x254455e7),
            (
                "**bold** and *italic* text in markdown that wraps",
                0x3b665f32,
            ),
            ("&amp; &lt;tag&gt; &#35; ﬂ°°35¶ß ﬂ°lt¶ß entity", 0x6416b7cb),
            ("ünïcödé wörds and 😀 emoji 😀😀😀😀😀😀😀😀😀", 0xbab9ff79),
            ("宽字符宽字符宽字符宽字符宽字符", 0xc37ec66d),
            ("e\u{301} combining and zero\u{200b}width", 0x2ed61a61),
            ("   leading spaces", 0xfedeeef7),
            ("x\u{3000}ideographic\u{3000}space", 0x1330b7f3),
            ("List~int~ and Map~K,V~", 0xcc75ea1f),
        ];
        for &(text, hash) in cases {
            assert_eq!(fnv(&literal(text)), hash, "{text:?}");
        }
    }

    #[test]
    fn literal_label_matches_pele_on_corpus() {
        // `(file, hash of whole sources, hash of each line)`.
        let expected: &[(&str, u32, u32)] = &[
            ("agentflow", 0x7bb3fa95, 0xf1daa7d5),
            ("architecture", 0x40d7f327, 0xe64316e7),
            ("block", 0x672793db, 0x9183e451),
            ("c4", 0x85be6705, 0xa170a925),
            ("class", 0x204af823, 0x6a459bed),
            ("cynefin", 0xbeab0085, 0x47b4d34f),
            ("er", 0xc0514efe, 0x154675c8),
            ("eventmodeling", 0x3869d289, 0x58d885f9),
            ("flowchart", 0x3cb85bb1, 0x27223867),
            ("gantt", 0x430fdadb, 0xe62b0d5d),
            ("git", 0x54e47df1, 0xbe00908b),
            ("info", 0xf2681a31, 0x630d8e09),
            ("ishikawa", 0x119a62b9, 0x8870277f),
            ("journey", 0xdd90674b, 0xb25cfe67),
            ("kanban", 0xa5252b51, 0xaac2a6bd),
            ("mindmap", 0x1c543056, 0xbe054f0c),
            ("packet", 0x54256487, 0xac5f5219),
            ("pie", 0x4a9b43a7, 0x3f998b9d),
            ("quadrant", 0xcce4a4f9, 0x9a0302b1),
            ("radar", 0x5e1618e5, 0xb4dd192d),
            ("railroad", 0xacafad6d, 0x458e06dd),
            ("requirement", 0x5619e0ac, 0xfa0d36c0),
            ("sankey", 0x22837425, 0xc5d4f8a1),
            ("sequence", 0x4c9c4beb, 0xee221a8d),
            ("state", 0xf70157cf, 0xab9189a3),
            ("swimlane", 0x5a418851, 0x94a289b3),
            ("timeline", 0xb88bfdff, 0x9cdfee29),
            ("treemap", 0x93435159, 0x5994c77d),
            ("treeview", 0xebe8cec5, 0x2eed47f1),
            ("usecase", 0x89761abb, 0x89b54f3e),
            ("venn", 0xeb18cbf5, 0x97bdb8ad),
            ("wardley", 0xcacdc81d, 0xeb987d01),
            ("xychart", 0xc9c9f7a5, 0xcf399345),
        ];
        for (&(name, whole, each), &(file, json)) in expected.iter().zip(CORPUS) {
            assert_eq!(name, file);
            let sources = inputs(json);
            let all: Vec<String> = sources.iter().map(|src| literal(src)).collect();
            assert_eq!(fnv(&all.join("\n\n")), whole, "{name} sources");
            let all: Vec<String> = sources
                .iter()
                .flat_map(|src| src.split('\n'))
                .map(literal)
                .collect();
            assert_eq!(fnv(&all.join("\n\n")), each, "{name} lines");
        }
    }
}
