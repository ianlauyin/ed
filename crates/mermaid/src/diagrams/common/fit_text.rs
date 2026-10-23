//! Fits one line of text into a width. Port of Pele's `src/diagrams/common/fit-text.ts`.

use crate::text::label::trim_end;
use crate::text::measure;

/// Text fitted into a width.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Fitted {
    /// Font size.
    pub(crate) size: f64,
    /// The text, cut and ended with an ellipsis when it did not fit whole.
    pub(crate) text: String,
    /// The measured width.
    pub(crate) width: f64,
}

/// Fits `text` into `max`: first by shrinking the font from `size` down to `min`, then by
/// cutting the text and adding an ellipsis. Returns `None` when not even one character fits.
pub(crate) fn fit_text(text: &str, max: f64, size: f64, min: f64, style: u8) -> Option<Fitted> {
    if text.is_empty() || max <= 0.0 {
        return None;
    }
    let mut width = measure::width(text, size, style);
    if width <= max {
        return Some(Fitted {
            size,
            text: text.to_owned(),
            width,
        });
    }

    // Width grows with font size, so one division lands on the right size or just above it.
    let mut fitted = min.max((size * max / width).floor());
    while fitted >= min {
        width = measure::width(text, fitted, style);
        if width <= max {
            return Some(Fitted {
                size: fitted,
                text: text.to_owned(),
                width,
            });
        }
        fitted -= 1.0;
    }

    // Byte offsets after each character, so a cut of `k` characters is `text[..ends[k - 1]]`.
    let ends: Vec<usize> = text
        .char_indices()
        .map(|(at, c)| at + c.len_utf8())
        .collect();
    let cut = |count: usize| {
        let end = count
            .checked_sub(1)
            .and_then(|k| ends.get(k))
            .copied()
            .unwrap_or(0);
        let mut cut = String::from(trim_end(text.get(..end).unwrap_or("")));
        cut.push('…');
        cut
    };

    let mut low = 0;
    let mut high = ends.len().saturating_sub(1);
    while low < high {
        let mid = (low + high + 1) >> 1;
        if measure::width(&cut(mid), min, style) <= max {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    if low == 0 {
        return None;
    }

    let text = cut(low);
    width = measure::width(&text, min, style);
    (width <= max).then_some(Fitted {
        size: min,
        text,
        width,
    })
}

#[cfg(test)]
#[expect(
    clippy::unreadable_literal,
    reason = "hashes as the Pele scripts print them"
)]
mod tests {
    //! Checks against Pele (`obsidianmd/pele` at `458322a`). Every expected value comes from
    //! running Pele's `fitText` with `bun` and the monospace branch of its `metricsMeasurer`,
    //! never from this port.

    use super::*;
    use crate::diagrams::common::tokens::tests::{CORPUS, fnv, inputs};

    fn fitted(text: &str) -> String {
        let mut out = Vec::new();
        for max in [0.0, 20.0, 50.0, 120.0] {
            for style in [0, 1] {
                out.push(match fit_text(text, max, 14.0, 9.0, style) {
                    Some(f) => format!("F{}|{}|{}", f.text, f.size, f.width),
                    None => "none".to_owned(),
                });
            }
        }
        out.join("\n")
    }

    #[test]
    fn fit_text_matches_pele_on_edge_cases() {
        let cases: &[(&str, &str)] = &[
            ("", "none\nnone\nnone\nnone\nnone\nnone\nnone\nnone"),
            (
                "a",
                "none\nnone\nFa|14|8.4\nFa|14|8.4\nFa|14|8.4\nFa|14|8.4\nFa|14|8.4\nFa|14|8.4",
            ),
            (
                "hello world",
                "none\nnone\nFhe…|9|16.2\nFhe…|9|16.2\nFhello wo…|9|48.6\nFhello wo…|9|48.6\nFhello world|14|92.4\nFhello world|14|92.4",
            ),
            (
                "averyveryverylongwordthatcannotfitanywhere",
                "none\nnone\nFav…|9|16.2\nFav…|9|16.2\nFaveryver…|9|48.6\nFaveryver…|9|48.6\nFaveryveryverylongword…|9|118.8\nFaveryveryverylongword…|9|118.8",
            ),
            (
                "src/components/VeryLongFileName.tsx",
                "none\nnone\nFsr…|9|16.2\nFsr…|9|16.2\nFsrc/comp…|9|48.6\nFsrc/comp…|9|48.6\nFsrc/components/VeryLo…|9|118.8\nFsrc/components/VeryLo…|9|118.8",
            ),
            (
                "two  spaces   here and\ttab",
                "none\nnone\nFtw…|9|16.2\nFtw…|9|16.2\nFtwo  spa…|9|48.6\nFtwo  spa…|9|48.6\nFtwo  spaces   here an…|9|118.8\nFtwo  spaces   here an…|9|118.8",
            ),
            (
                "line one\nline two that is longer\n\nafter blank",
                "none\nnone\nFli…|9|16.2\nFli…|9|16.2\nFline one…|9|48.6\nFline one…|9|48.6\nFline one\nline two that…|9|118.8\nFline one\nline two that…|9|118.8",
            ),
            (
                "trailing   \nspaces",
                "none\nnone\nFtr…|9|16.2\nFtr…|9|16.2\nFtrailing…|9|48.6\nFtrailing…|9|48.6\nFtrailing   \nspaces|11|112.2\nFtrailing   \nspaces|11|112.2",
            ),
            (
                "<b>bold</b> and <br> break <BR/> and <br />x",
                "none\nnone\nF<b…|9|16.2\nF<b…|9|16.2\nF<b>bold<…|9|48.6\nF<b>bold<…|9|48.6\nF<b>bold</b> and <br>…|9|113.4\nF<b>bold</b> and <br>…|9|113.4",
            ),
            (
                "a <script>x</script> fa:fa-car :fa-x",
                "none\nnone\nFa…|9|10.8\nFa…|9|10.8\nFa <scrip…|9|48.6\nFa <scrip…|9|48.6\nFa <script>x</script>…|9|113.4\nFa <script>x</script>…|9|113.4",
            ),
            (
                "fa:fa-car drives fa:fa-none far away from here",
                "none\nnone\nFfa…|9|16.2\nFfa…|9|16.2\nFfa:fa-ca…|9|48.6\nFfa:fa-ca…|9|48.6\nFfa:fa-car drives fa:f…|9|118.8\nFfa:fa-car drives fa:f…|9|118.8",
            ),
            (
                "**bold** and *italic* text in markdown that wraps",
                "none\nnone\nF**…|9|16.2\nF**…|9|16.2\nF**bold**…|9|48.6\nF**bold**…|9|48.6\nF**bold** and *italic*…|9|118.8\nF**bold** and *italic*…|9|118.8",
            ),
            (
                "&amp; &lt;tag&gt; &#35; ﬂ°°35¶ß ﬂ°lt¶ß entity",
                "none\nnone\nF&a…|9|16.2\nF&a…|9|16.2\nF&amp; &l…|9|48.6\nF&amp; &l…|9|48.6\nF&amp; &lt;tag&gt; &#3…|9|118.8\nF&amp; &lt;tag&gt; &#3…|9|118.8",
            ),
            (
                "ünïcödé wörds and 😀 emoji 😀😀😀😀😀😀😀😀😀",
                "none\nnone\nFün…|9|16.2\nFün…|9|16.2\nFünïcödé…|9|43.2\nFünïcödé…|9|43.2\nFünïcödé wörds and 😀…|9|113.4\nFünïcödé wörds and 😀…|9|113.4",
            ),
            (
                "宽字符宽字符宽字符宽字符宽字符",
                "none\nnone\nF宽…|9|16.2\nF宽…|9|16.2\nF宽字符宽…|9|48.6\nF宽字符宽…|9|48.6\nF宽字符宽字符宽字符宽…|9|113.4\nF宽字符宽字符宽字符宽…|9|113.4",
            ),
            (
                "e\u{301} combining and zero\u{200b}width",
                "none\nnone\nFe\u{301}…|9|10.8\nFe\u{301}…|9|10.8\nFe\u{301} combin…|9|48.6\nFe\u{301} combin…|9|48.6\nFe\u{301} combining and zero\u{200b}w…|9|118.8\nFe\u{301} combining and zero\u{200b}w…|9|118.8",
            ),
            (
                "   leading spaces",
                "none\nnone\nF…|9|5.4\nF…|9|5.4\nF   leadi…|9|48.6\nF   leadi…|9|48.6\nF   leading spaces|11|112.2\nF   leading spaces|11|112.2",
            ),
            (
                "x\u{3000}ideographic\u{3000}space",
                "none\nnone\nFx…|9|10.8\nFx…|9|10.8\nFx\u{3000}ideog…|9|48.6\nFx\u{3000}ideog…|9|48.6\nFx\u{3000}ideographic\u{3000}space|9|113.4\nFx\u{3000}ideographic\u{3000}space|9|113.4",
            ),
            (
                "List~int~ and Map~K,V~",
                "none\nnone\nFLi…|9|16.2\nFLi…|9|16.2\nFList~int…|9|48.6\nFList~int…|9|48.6\nFList~int~ and Map~K,V~|9|118.8\nFList~int~ and Map~K,V~|9|118.8",
            ),
        ];
        for &(text, expected) in cases {
            assert_eq!(fitted(text), expected, "{text:?}");
        }
    }

    #[test]
    fn fit_text_matches_pele_on_corpus_lines() {
        let expected: &[(&str, u32)] = &[
            ("agentflow", 0x50b6ab5b),
            ("architecture", 0x8b15259),
            ("block", 0x9995edc7),
            ("c4", 0x66bd84f7),
            ("class", 0xb404331d),
            ("cynefin", 0x6a09d545),
            ("er", 0x8601b4bd),
            ("eventmodeling", 0xe527d4a1),
            ("flowchart", 0x4e8ba383),
            ("gantt", 0xa43d4c3b),
            ("git", 0x2349d415),
            ("info", 0x8cb2c029),
            ("ishikawa", 0x4ccdfdb5),
            ("journey", 0xdc54ad8b),
            ("kanban", 0xfeee403b),
            ("mindmap", 0x3f064a65),
            ("packet", 0x2e1fd853),
            ("pie", 0x930f462f),
            ("quadrant", 0x857cd683),
            ("radar", 0xf4001567),
            ("railroad", 0xd153ac51),
            ("requirement", 0xb2c36c89),
            ("sankey", 0x7022b663),
            ("sequence", 0xa362d5d),
            ("state", 0x93dd5d77),
            ("swimlane", 0xc924647),
            ("timeline", 0x88ec4f2f),
            ("treemap", 0x4282e0ad),
            ("treeview", 0xbf3f5525),
            ("usecase", 0x6eddfa0d),
            ("venn", 0x4844d1d3),
            ("wardley", 0xc7e29325),
            ("xychart", 0x88a7f611),
        ];
        for (&(name, hash), &(file, json)) in expected.iter().zip(CORPUS) {
            assert_eq!(name, file);
            let sources = inputs(json);
            let all: Vec<String> = sources
                .iter()
                .flat_map(|src| src.split('\n'))
                .map(fitted)
                .collect();
            assert_eq!(fnv(&all.join("\n\n")), hash, "{name}");
        }
    }
}
