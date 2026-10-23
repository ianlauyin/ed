//! Case-insensitive word tests for hand-written lexers. Port of Pele's
//! `src/diagrams/common/caseless.ts`.

/// Whether `word`, written in lower case ASCII, is at UTF-8 offset `at` of `src`. Only ASCII
/// letters match in either case, as with the `i` flag on a regular expression without the `u`
/// flag.
pub(crate) fn word_at(src: &str, at: usize, word: &str) -> bool {
    let bytes = src.as_bytes();
    word.bytes().enumerate().all(|(k, w)| {
        bytes
            .get(at + k)
            .is_some_and(|&c| c == w || (w.is_ascii_lowercase() && c == w - 32))
    })
}

#[cfg(test)]
#[expect(
    clippy::unreadable_literal,
    reason = "hashes as the Pele scripts print them"
)]
mod tests {
    //! Checks against Pele (`obsidianmd/pele` at `458322a`). Every expected value comes from
    //! running Pele's `wordAt` with `bun` at every character of the inputs, never from this port.

    use super::*;
    use crate::diagrams::common::tokens::tests::{CORPUS, fnv, inputs};

    const WORDS: [&str; 8] = [
        "acc", "descr", "title", "vertical", "accdescr", "acctitle", "journey", "section",
    ];

    fn caseless(src: &str) -> String {
        src.char_indices()
            .map(|(at, _)| at)
            .chain([src.len()])
            .map(|p| {
                WORDS
                    .iter()
                    .map(|w| if word_at(src, p, w) { '1' } else { '0' })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    #[test]
    fn word_at_matches_pele_on_edge_cases() {
        let cases: &[(&str, u32)] = &[
            ("", 0xff37de5),
            ("a", 0x7dc0b9cb),
            ("hello world", 0x53ecdbab),
            ("averyveryverylongwordthatcannotfitanywhere", 0xb3ce7e45),
            ("src/components/VeryLongFileName.tsx", 0xa90f722b),
            ("two  spaces   here and\ttab", 0x48a61945),
            (
                "line one\nline two that is longer\n\nafter blank",
                0x1c7cbb0b,
            ),
            ("trailing   \nspaces", 0x1ff7dec5),
            ("<b>bold</b> and <br> break <BR/> and <br />x", 0xdbb5c7a5),
            ("a <script>x</script> fa:fa-car :fa-x", 0x7c388925),
            ("fa:fa-car drives fa:fa-none far away from here", 0xa07a8985),
            (
                "**bold** and *italic* text in markdown that wraps",
                0xc357eecb,
            ),
            ("&amp; &lt;tag&gt; &#35; ﬂ°°35¶ß ﬂ°lt¶ß entity", 0x1c7cbb0b),
            ("ünïcödé wörds and 😀 emoji 😀😀😀😀😀😀😀😀😀", 0xa90f722b),
            ("宽字符宽字符宽字符宽字符宽字符", 0xfe22a96b),
            ("e\u{301} combining and zero\u{200b}width", 0x9950faab),
            ("   leading spaces", 0x89ad60cb),
            ("x\u{3000}ideographic\u{3000}space", 0x5c0f732b),
            ("List~int~ and Map~K,V~", 0xdb97b205),
        ];
        for &(text, hash) in cases {
            assert_eq!(fnv(&caseless(text)), hash, "{text:?}");
        }
    }

    #[test]
    fn word_at_matches_pele_on_corpus() {
        let expected: &[(&str, u32)] = &[
            ("agentflow", 0x66b696aa),
            ("architecture", 0x95770893),
            ("block", 0x717a0163),
            ("c4", 0x293daf70),
            ("class", 0x89d6714f),
            ("cynefin", 0x5dbc6ee2),
            ("er", 0x1960482f),
            ("eventmodeling", 0x85aa82b7),
            ("flowchart", 0x7adfb412),
            ("gantt", 0xacebec6d),
            ("git", 0xdf9f5630),
            ("info", 0x894eedf3),
            ("ishikawa", 0x71771405),
            ("journey", 0xb6746845),
            ("kanban", 0xd66c03e8),
            ("mindmap", 0x5677800b),
            ("packet", 0x4266cfa1),
            ("pie", 0x862ae63),
            ("quadrant", 0xe652324d),
            ("radar", 0xb4252f9f),
            ("railroad", 0xa7bfcaeb),
            ("requirement", 0x4f9fb3eb),
            ("sankey", 0x40dfc013),
            ("sequence", 0x79b54e5e),
            ("state", 0xe6f504b0),
            ("swimlane", 0x8b1884c6),
            ("timeline", 0x1ea9710b),
            ("treemap", 0xb24ea93b),
            ("treeview", 0xabd4b08b),
            ("usecase", 0x2b2082c5),
            ("venn", 0x76b242fe),
            ("wardley", 0xa69c11f0),
            ("xychart", 0x56c7331c),
        ];
        for (&(name, hash), &(file, json)) in expected.iter().zip(CORPUS) {
            assert_eq!(name, file);
            let all: Vec<String> = inputs(json).iter().map(|src| caseless(src)).collect();
            assert_eq!(fnv(&all.join("\n")), hash, "{name}");
        }
    }
}
