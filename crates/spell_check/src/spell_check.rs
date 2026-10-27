//! English spell checking for prose inside code: comments, strings, names.
//!
//! Word splitting and skip patterns follow codebook
//! (https://github.com/blopker/codebook): identifiers are split into their
//! camelCase / snake_case parts and each part is checked on its own.

use std::ops::Range;
use std::sync::LazyLock;

use collections::HashSet;
use regex::Regex;

/// Name of the optional per-project word list, at the worktree root. One word
/// per line; blank lines and lines starting with `#` are ignored.
pub const PROJECT_WORDS_FILE: &str = "spellcheck.txt";

const MIN_WORD_LEN: usize = 3;
const MAX_SUGGESTIONS: usize = 5;

/// en_US plus the code words. Code word lines are `.dic` entries, so they can
/// carry en_US affix flags (`addr/S` also accepts "addrs").
static DICTIONARY: LazyLock<spellbook::Dictionary> = LazyLock::new(|| {
    let mut dictionary = spellbook::Dictionary::new(
        include_str!("../dictionaries/en_US.aff"),
        include_str!("../dictionaries/en_US.dic"),
    )
    .expect("bundled en_US dictionary must parse");
    for line in include_str!("../dictionaries/code.dic").lines().filter(|line| !line.is_empty()) {
        dictionary.add(line).unwrap_or_else(|error| panic!("invalid code word entry {line:?}: {error}"));
    }
    dictionary
});

static SKIP_PATTERNS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        &[
            // URLs
            r"[a-zA-Z][a-zA-Z0-9+.-]*://[^\s]+",
            // Email addresses
            r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}",
            // Hex colors
            r"#[0-9a-fA-F]{3,8}\b",
            // Unix paths (and shebangs), anchored so "and/or" is still checked
            r#"(?m)(?:^|[\s"'(\[=<,!])\.{0,2}/[^\s]*"#,
            // UUIDs
            r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
            // Base64 with padding
            r"[A-Za-z0-9+/]{20,}={1,2}",
            // Markdown link targets
            r"\]\([^\s)]+\)",
            // Inline code
            r"`[^`\n]+`",
            // Quoted strings that look like paths or module specifiers
            r#""[^"\s]*/[^"\s]*"|'[^'\s]*/[^'\s]*'"#,
        ]
        .join("|"),
    )
    .expect("valid skip patterns")
});

/// Loads the bundled dictionary. It loads lazily on first use anyway; call
/// this from a background thread to keep that cost off the first check.
pub fn warm_up() {
    LazyLock::force(&DICTIONARY);
    LazyLock::force(&SKIP_PATTERNS);
}

/// Parses a project word list (see [`PROJECT_WORDS_FILE`]). Words match
/// case-insensitively.
pub fn parse_project_words(text: &str) -> HashSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_lowercase)
        .collect()
}

/// Where to insert `word` into a project word list, and the text to insert,
/// keeping the words sorted case-insensitively (comments stay put). `None`
/// when the word is already listed.
pub fn project_word_insertion(text: &str, word: &str) -> Option<(usize, String)> {
    let key = word.to_lowercase();
    let mut offset = 0;
    let mut insert_at = None;
    for line in text.split_inclusive('\n') {
        let listed = line.trim();
        if !listed.is_empty() && !listed.starts_with('#') {
            let listed = listed.to_lowercase();
            if listed == key {
                return None;
            }
            if insert_at.is_none() && listed > key {
                insert_at = Some(offset);
            }
        }
        offset += line.len();
    }
    Some(match insert_at {
        Some(offset) => (offset, format!("{word}\n")),
        None if text.is_empty() || text.ends_with('\n') => (text.len(), format!("{word}\n")),
        None => (text.len(), format!("\n{word}\n")),
    })
}

/// Returns the byte ranges of the misspelled words in `text`.
pub fn misspelled_words(text: &str, project_words: &HashSet<String>) -> Vec<Range<usize>> {
    let skip = SKIP_PATTERNS.find_iter(text).map(|m| m.range()).collect::<Vec<_>>();
    let mut skip = skip.iter().peekable();
    let mut parts = Vec::new();
    let mut result = Vec::new();
    for token in tokens(text) {
        while skip.next_if(|range| range.end <= token.start).is_some() {}
        if skip.peek().is_some_and(|range| range.start < token.end) {
            continue;
        }
        let token_text = &text[token.clone()];
        if is_hash(token_text) || !is_supported_script(token_text) {
            continue;
        }
        split_identifier(token_text, &mut parts);
        for part in &parts {
            let word = &token_text[part.clone()];
            if !is_correct(word, project_words) {
                result.push(token.start + part.start..token.start + part.end);
            }
        }
    }
    result
}

pub fn is_correct(word: &str, project_words: &HashSet<String>) -> bool {
    if word.chars().count() < MIN_WORD_LEN || word.chars().any(|c| c.is_numeric()) {
        return true;
    }
    if project_words.contains(&word.to_lowercase()) {
        return true;
    }
    DICTIONARY.check(word) || DICTIONARY.checker().check_lower_as_title(true).check_lower_as_upper(true).check(word)
}

/// Dictionary suggestions for `word`, in the word's case.
pub fn suggestions(word: &str) -> Vec<String> {
    let mut suggestions = Vec::new();
    DICTIONARY.suggest(word, &mut suggestions);
    suggestions.truncate(MAX_SUGGESTIONS);
    let all_caps = word.chars().all(char::is_uppercase);
    let all_lower = word.chars().all(char::is_lowercase);
    for suggestion in &mut suggestions {
        if all_caps {
            *suggestion = suggestion.to_uppercase();
        } else if all_lower {
            *suggestion = suggestion.to_lowercase();
        }
    }
    suggestions
}

/// Byte ranges of maximal runs of letters, digits, `_`, and apostrophes
/// between letters.
fn tokens(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut chars = text.char_indices().peekable();
    std::iter::from_fn(move || {
        let (start, _) = chars.find(|(_, c)| c.is_alphanumeric() || *c == '_')?;
        let mut end = text.len();
        let mut prev_is_letter = text[start..].starts_with(char::is_alphabetic);
        while let Some(&(ix, c)) = chars.peek() {
            let continues = if is_apostrophe(c) {
                prev_is_letter && text[ix + c.len_utf8()..].starts_with(char::is_alphabetic)
            } else {
                c.is_alphanumeric() || c == '_'
            };
            if !continues {
                end = ix;
                break;
            }
            prev_is_letter = c.is_alphabetic();
            chars.next();
        }
        Some(start..end)
    })
}

const fn is_apostrophe(c: char) -> bool {
    c == '\'' || c == '\u{2019}'
}

/// Git hashes and other hex runs: 7+ hex digits, at least one a decimal
/// digit so words spelled with a-f ("defaced") are still checked.
fn is_hash(token: &str) -> bool {
    token.len() >= 7 && token.chars().all(|c| c.is_ascii_hexdigit()) && token.chars().any(|c| c.is_ascii_digit())
}

/// The dictionary is English, so only Latin scripts are checked.
fn is_supported_script(token: &str) -> bool {
    token.chars().all(|c| c.is_ascii() || matches!(c, '\u{00C0}'..='\u{024F}' | '\u{2019}'))
}

#[derive(Clone, Copy, PartialEq)]
enum CharKind {
    Lower,
    Upper,
    Digit,
    Underscore,
}

/// Splits an identifier into its words: `parseHTTPResponse2` becomes
/// `parse`, `HTTP`, `Response`, `2`.
fn split_identifier(token: &str, parts: &mut Vec<Range<usize>>) {
    parts.clear();
    let mut start = 0;
    let mut prev = None;
    let mut chars = token.char_indices().peekable();
    while let Some((ix, c)) = chars.next() {
        let kind = if c == '_' {
            CharKind::Underscore
        } else if c.is_uppercase() {
            CharKind::Upper
        } else if c.is_numeric() {
            CharKind::Digit
        } else {
            CharKind::Lower
        };
        let boundary = match (prev, kind) {
            (_, CharKind::Underscore) => true,
            (Some(CharKind::Lower), CharKind::Upper) => true,
            // The last capital of a run starts the next word: HTTPResponse.
            (Some(CharKind::Upper), CharKind::Upper) => chars.peek().is_some_and(|(_, next)| next.is_lowercase()),
            (Some(prev), kind) => (prev == CharKind::Digit) != (kind == CharKind::Digit),
            (None, _) => false,
        };
        if boundary && ix > start {
            parts.push(start..ix);
        }
        if boundary {
            start = ix;
        }
        if kind == CharKind::Underscore {
            start = ix + c.len_utf8();
        }
        prev = Some(kind);
    }
    if start < token.len() {
        parts.push(start..token.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_code_dictionary() {
        let project_words = HashSet::default();
        assert!(is_correct("addr", &project_words));
        assert!(is_correct("ADDR", &project_words));
        assert!(is_correct("addrs", &project_words));
        assert!(is_correct("deserializable", &project_words));
        assert!(is_correct("deserializer", &project_words));
    }
}
