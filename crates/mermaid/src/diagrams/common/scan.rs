//! Lexer rules that Mermaid's journey and timeline grammars have in common, ported from the
//! compiled Jison rules: comments, whitespace, and the `accTitle` and `accDescr` statements.
//! Port of Pele's `src/diagrams/common/scan.ts`.
//!
//! Offsets are UTF-8. Pele reads UTF-16 code units, so a test of the unit after a character
//! sees the second half of a surrogate pair after a supplementary character, which no ASCII test
//! matches.

use crate::util::chars::is_space;

/// Tokens of a Jison-style lexer: parallel lists of types, texts, and UTF-8 starts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tokens<'a> {
    /// Where each token starts.
    pub(crate) starts: Vec<usize>,
    /// The text of each token.
    pub(crate) texts: Vec<&'a str>,
    /// The type of each token.
    pub(crate) types: Vec<u16>,
}

/// Where the line from `from` ends: at the next `\n`, or the end of `src`.
pub(crate) fn line_end(src: &str, from: usize) -> usize {
    src.get(from..)
        .and_then(|rest| rest.find('\n'))
        .map_or(src.len(), |at| from + at)
}

/// Whether `word`, in lowercase ASCII letters, is at `p`. The grammars are case-insensitive.
pub(crate) fn keyword_at(src: &str, p: usize, word: &str) -> bool {
    let bytes = src.as_bytes();
    word.bytes()
        .enumerate()
        .all(|(i, w)| bytes.get(p + i).is_some_and(|&c| c | 32 == w))
}

/// The character at `p`, when `p` is a character boundary inside `src`.
fn char_at(src: &str, p: usize) -> Option<char> {
    src.get(p..)?.chars().next()
}

/// Skips comments and whitespace, and stops at a line break that starts a token, which is a
/// `NEWLINE`. Jison compiles `\%%` to a single `%`, so one percent sign starts a comment, and the
/// second comment rule takes any character before `%%` with it, a line break included.
pub(crate) fn skip_trivia(src: &str, from: usize) -> usize {
    let bytes = src.as_bytes();
    let n = src.len();
    let mut p = from;
    while p < n {
        let Some(c) = char_at(src, p) else {
            return p;
        };
        // The unit after `c`. After a supplementary character it is a low surrogate.
        let next = p + c.len_utf8();
        let after = |k: usize| {
            if c.len_utf16() == 1 {
                bytes.get(next + k).copied()
            } else {
                None
            }
        };

        if c == '%' && after(0) != Some(b'{') {
            p = line_end(src, p);
        } else if c != '}' && after(0) == Some(b'%') && after(1) == Some(b'%') {
            p = line_end(src, next + 2);
        } else if c == '\n' {
            return p;
        } else if is_space(u32::from(c)) {
            p = next;
            while let Some(c) = char_at(src, p)
                && is_space(u32::from(c))
            {
                p += c.len_utf8();
            }
        } else if c == '#' {
            p = line_end(src, p);
        } else {
            return p;
        }
    }
    p
}

fn skip_space(src: &str, from: usize) -> usize {
    let mut p = from;
    while let Some(c) = char_at(src, p)
        && is_space(u32::from(c))
    {
        p += c.len_utf8();
    }
    p
}

/// Lexes an `accTitle` or `accDescr` statement at `p` into `out` and returns where it ends, or
/// `p` when there is none. The token types `acc_title`, `acc_title_value`, `acc_descr`,
/// `acc_descr_value`, and `acc_descr_multiline_value` are numbered consecutively from `first`.
pub(crate) fn accessibility<'a>(src: &'a str, p: usize, out: &mut Tokens<'a>, first: u16) -> usize {
    let descr = keyword_at(src, p, "accdescr");
    if !descr && !keyword_at(src, p, "acctitle") {
        return p;
    }
    let n = src.len();
    let mut q = skip_space(src, p + 8);
    let c = src.as_bytes().get(q).copied();
    let mut emit = |kind: u16, s: usize, e: usize| {
        out.types.push(kind);
        out.texts.push(src.get(s..e).unwrap_or(""));
        out.starts.push(s);
    };

    if c == Some(b':') {
        q = skip_space(src, q + 1);
        let end = line_end(src, q);
        emit(if descr { first + 2 } else { first }, p, q);
        emit(if descr { first + 3 } else { first + 1 }, q, end);
        return end;
    }
    if !descr || c != Some(b'{') {
        return p;
    }

    q = skip_space(src, q + 1);
    while src.as_bytes().get(q) != Some(&b'}') {
        let end = src
            .get(q..)
            .and_then(|rest| rest.find('}'))
            .map_or(n, |at| q + at);
        emit(first + 4, q, end);
        if q == n {
            return n;
        }
        q = end;
    }
    q + 1
}

#[cfg(test)]
#[expect(
    clippy::unreadable_literal,
    reason = "hashes as the Pele scripts print them"
)]
mod tests {
    //! Checks against Pele (`obsidianmd/pele` at `458322a`). Every expected value comes from
    //! running Pele's `skipTrivia`, `lineEnd`, `keywordAt`, and `accessibility` with `bun` at
    //! every character of the inputs, never from this port. Results compare FNV-1a hashes of
    //! the same serialization, in UTF-16 offsets.

    use super::*;
    use crate::diagrams::common::tokens::tests::{CORPUS, fnv, inputs, utf16};

    fn scan(src: &str) -> String {
        let u = |at: usize| utf16(src, at);
        let mut out = Vec::new();
        for p in src.char_indices().map(|(at, _)| at).chain([src.len()]) {
            let mut tokens = Tokens::default();
            let end = accessibility(src, p, &mut tokens, 10);
            let kw: String = ["accdescr", "journey", "title", "section"]
                .iter()
                .map(|w| if keyword_at(src, p, w) { '1' } else { '0' })
                .collect();
            let emitted: Vec<String> = tokens
                .types
                .iter()
                .enumerate()
                .map(|(i, t)| format!("{t}@{}:{}", u(tokens.starts[i]), tokens.texts[i]))
                .collect();
            out.push(format!(
                "{}>{}|{}|{kw}|{}|{}",
                u(p),
                u(skip_trivia(src, p)),
                u(line_end(src, p)),
                u(end),
                emitted.join(";")
            ));
        }
        out.join("\n")
    }

    #[test]
    fn scan_matches_pele_on_edge_cases() {
        let cases: &[(&str, u32)] = &[
            ("pie\n\"a\" : 1\n'b\\'s' : 2.5\n", 0xd3e39b26),
            (
                "pie showData\n  title  My   pie  %% note\naccTitle : The   title\naccDescr: one  two\n",
                0x74d26e8b,
            ),
            (
                "pie\naccDescr {\n   line one   \n\n\n   line  two\t\t\n }\n\"x\":3",
                0x4b6de14,
            ),
            (
                "pie\naccDescr {\n never closed\naccDescr { again\n",
                0x97dd6b89,
            ),
            ("graph graphs graph-x graph%%c\ngraph\n", 0x654402af),
            ("---\ntitle: x\n---\npie\n", 0x54e6ee6e),
            ("---\r\ntitle: x\r\n---\r\npie", 0xe6877118),
            ("--- \nnever closed\npie\n--\n-", 0xd591af61),
            ("%%{init: {}}%%\npie\n  %%{ x }%% y\n", 0xd31e4ff8),
            ("%%{ unclosed\npie\n%%{ also\n", 0x748574d2),
            (
                "pie\n\"esc \\n \\t \\b \\f \\v \\0 \\q \\\\\" : 1\n",
                0x1d844a2d,
            ),
            (
                "pie\n\"line\nbreak\" : 1\n\"bad \\\nbreak\" : 2\n",
                0xf3fb2c5c,
            ),
            ("pie\n\"é ünï 😀\" : 7\ntitle 😀  ok\n", 0x8e9a05f),
            ("pie\r\n\"a\" : 1\r\n", 0xb880b328),
            (
                "title\ntitlex\ntitle\tx  y\naccTitle:\naccTitle :%%c\n",
                0x432158d4,
            ),
            ("pie\n\"a\" 1\n", 0x3ada13fb),
            ("pie\n\"a\" :\n", 0x3ada13fb),
            ("pie\n:\n", 0x6eaba384),
            ("pie", 0xbaf08c09),
            ("", 0x189b19d1),
            (
                "accDescr\t{a}accDescr  :  z %% c\naccDescr {\n  a\n  b\n}\n",
                0x7838146d,
            ),
            ("accDescr\n\n{\n  first\n\n  second  \n}", 0xbd72db7c),
            ("pie\n 😀 \n", 0xea8b43a0),
            ("pie\n\u{2028}\n", 0x3e269056),
            ("accTitle\u{3000}: x\n", 0xdf63d0bd),
            ("title\u{a0}x\n", 0x9133b86b),
            ("a😀%%b\nc", 0xde0fb806),
            ("é%%x\n%{\n}%%y\n#c\n  \t\n", 0x251cf5f8),
            ("accDescr {\n a\n b", 0xeed3ad83),
            ("accdescr{}", 0xf48ca56e),
            ("ACCTITLE  :  v  \nx", 0x1e8a6988),
            ("accDescr", 0x9ec12576),
            ("accTitle", 0x7e402c86),
            ("accDescr :", 0xe4d75e8a),
            ("accDescr\u{3000}{x}", 0x3829b798),
        ];
        for &(text, hash) in cases {
            assert_eq!(fnv(&scan(text)), hash, "{text:?}");
        }
    }

    #[test]
    fn scan_matches_pele_on_corpus() {
        let expected: &[(&str, u32)] = &[
            ("agentflow", 0xc34f556),
            ("architecture", 0x8a63b679),
            ("block", 0x1df1a90c),
            ("c4", 0xf77bd937),
            ("class", 0xcf42d982),
            ("cynefin", 0xc70f8ddd),
            ("er", 0xb5a5590d),
            ("eventmodeling", 0x243bcc4b),
            ("flowchart", 0x22fa23e2),
            ("gantt", 0xdec8c131),
            ("git", 0x3a75a328),
            ("info", 0xb8b0339f),
            ("ishikawa", 0xb33e0f98),
            ("journey", 0xfca497a7),
            ("kanban", 0x4ad29c87),
            ("mindmap", 0x597f9a93),
            ("packet", 0xd09a0bcc),
            ("pie", 0x2f66cf1f),
            ("quadrant", 0x8a715a3f),
            ("radar", 0x57fa00b3),
            ("railroad", 0xbe881e90),
            ("requirement", 0x9ecdf59f),
            ("sankey", 0x885f476),
            ("sequence", 0xa3a49402),
            ("state", 0x2dc9f0cb),
            ("swimlane", 0x693fc92a),
            ("timeline", 0xb5409066),
            ("treemap", 0x11b031d0),
            ("treeview", 0x1cb45485),
            ("usecase", 0x4b5c428),
            ("venn", 0x1582f984),
            ("wardley", 0x1d9efe4),
            ("xychart", 0x2f56d1b6),
        ];
        for (&(name, hash), &(file, json)) in expected.iter().zip(CORPUS) {
            assert_eq!(name, file);
            let all: Vec<String> = inputs(json).iter().map(|src| scan(src)).collect();
            assert_eq!(fnv(&all.join("\n\n")), hash, "{name}");
        }
    }
}
