//! The `accDescr` token type for grammars where `accDescr` is also a name. Port of Pele's
//! `src/diagrams/common/accDescr.ts`.

use super::tokens::{ACC_DESCR, Memo, Pattern, TokenType, acc_descr_head};

/// The common `accDescr` token type, named `name` (Pele's default is `ACC_DESCR`). Lexing goes
/// on after an `accDescr {` that never closes, and each one would otherwise read to the end of
/// the text.
pub(crate) const fn acc_descr(name: &'static str) -> TokenType {
    TokenType {
        name,
        pattern: Pattern::Scan(scan),
        ..ACC_DESCR
    }
}

/// The braced form scans for its closing brace. Once none is found there is none further on, so
/// later braced descriptions fail without scanning the rest of the text again. `memo[0]` is 1
/// once a brace was found unclosed.
fn scan(src: &str, at: usize, memo: &mut Memo) -> Option<usize> {
    let (end, braced) = acc_descr_head(src, at)?;
    if !braced {
        return Some(end);
    }
    if memo[0] != 0 {
        return None;
    }

    let close = src.get(end..).and_then(|rest| rest.find('}'));
    memo[0] = usize::from(close.is_none());
    close.map(|close| end + close + 1)
}

#[cfg(test)]
mod tests {
    //! Checks against Pele (`obsidianmd/pele` at `458322a`). Every expected value comes from
    //! running Pele's `tokenize` with `accDescr('EM_ACC_DESCR')` in a table with a catch-all,
    //! with `bun`, never from this port. An unclosed brace stops later braced descriptions from
    //! matching, and a description with a colon still matches after it.

    use crate::diagrams::common::tokens::tests::{B, serialize};

    #[test]
    fn acc_descr_matches_pele() {
        let cases: &[(&str, &str)] = &[
            (
                "accDescr {\n never closed\naccDescr { again\naccDescr: fine\n",
                "14:0-8\n15:9-10\n9:10-11\n14:12-17\n14:18-24\n9:24-25\n14:25-33\n15:34-35\n14:36-41\n9:41-42\n4:42-56=fine\n9:56-57\n-1:57-57",
            ),
            (
                "accDescr{a}accDescr{b}\naccDescr  {\n  c\n}",
                "4:0-11=a\n4:11-22=b\n9:22-23\n4:23-40=c\n-1:40-40",
            ),
            (
                "accDescr {x\naccDescr: y %% z\naccDescr {",
                "14:0-8\n15:9-10\n14:10-11\n9:11-12\n4:12-24=y\n9:28-29\n14:29-37\n15:38-39\n-1:39-39",
            ),
            ("accDescr\n:x", "14:0-8\n9:8-9\n15:9-10\n14:10-11\n-1:11-11"),
            ("accDescr  :  spaced  out  ", "4:0-26=spaced out\n-1:26-26"),
        ];
        for &(src, expected) in cases {
            assert_eq!(serialize(src, B), expected, "{src:?}");
        }
    }
}
