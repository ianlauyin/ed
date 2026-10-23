//! The `accDescr` token type for grammars where `accDescr` is also a name. Port of Pele's
//! `src/diagrams/common/accDescr.ts`.

use super::tokens::ACC_DESCR;
use super::tokens::Memo;
use super::tokens::Pattern;
use super::tokens::TokenType;
use super::tokens::acc_descr_head;

/// The common `accDescr` token type, named `name` (Pele's default is `ACC_DESCR`). Lexing goes
/// on after an `accDescr {` that never closes, and each one would otherwise read to the end of
/// the text.
pub(crate) const fn acc_descr(name: &'static str) -> TokenType {
    TokenType { name, pattern: Pattern::Scan(scan), ..ACC_DESCR }
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
