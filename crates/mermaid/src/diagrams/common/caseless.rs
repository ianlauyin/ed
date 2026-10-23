//! Case-insensitive word tests for hand-written lexers. Port of Pele's
//! `src/diagrams/common/caseless.ts`.

/// Whether `word`, written in lower case ASCII, is at UTF-8 offset `at` of `src`. Only ASCII
/// letters match in either case, as with the `i` flag on a regular expression without the `u`
/// flag.
pub(crate) fn word_at(src: &str, at: usize, word: &str) -> bool {
    let bytes = src.as_bytes();
    word.bytes()
        .enumerate()
        .all(|(k, w)| bytes.get(at + k).is_some_and(|&c| c == w || (w.is_ascii_lowercase() && c == w - 32)))
}
