//! Tokenizer for the diagram types Mermaid defines with Langium grammars. Port of Pele's
//! `src/diagrams/common/tokens.ts`.
//!
//! Token types are tried in order and the first match wins, as in Chevrotain. Pele's sticky
//! regular expressions become [`Pattern`] values: literal text, a keyword, or a hand-written
//! matcher that each diagram supplies. Positions are UTF-8 offsets of the source.
//! [`Error::syntax`] reports them in UTF-16 columns, as Pele does.

use std::borrow::Cow;

use crate::error::Error;
use crate::text::label::trim;
use crate::util::chars::is_blank;
use crate::util::chars::is_line_end;
use crate::util::chars::is_space;

/// Answers as a sticky regular expression does: the end of a match that starts at the offset,
/// or `None`. The end must be on a character boundary.
pub(crate) type Matcher = fn(&str, usize) -> Option<usize>;

/// Memory a [`Scanner`] keeps between calls during one [`tokenize`] call. It starts zeroed.
pub(crate) type Memo = [usize; 4];

/// A [`Matcher`] that may remember what it has scanned, for a pattern that would otherwise rescan
/// the text from every position. Pele makes one with a `scanner` factory per text.
pub(crate) type Scanner = fn(&str, usize, &mut Memo) -> Option<usize>;

/// What a token type matches.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Pattern {
    /// A word that Mermaid ends only at whitespace, a comment, or the end of input. See
    /// [`keyword`].
    Keyword(&'static str),
    /// Literal text.
    Literal(&'static str),
    /// A hand-written matcher.
    Match(Matcher),
    /// A hand-written matcher with memory.
    Scan(Scanner),
}

/// A token type, as Pele's `TokenType` describes one.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TokenType {
    /// Every ASCII character a [`Pattern::Match`] or [`Pattern::Scan`] match can start with,
    /// when that is known. The type is then tried only at those characters and at characters
    /// outside ASCII. Literals and keywords derive it from their text.
    pub(crate) first: Option<&'static str>,
    /// Whether matched text is dropped instead of becoming a token.
    pub(crate) hidden: bool,
    /// Chevrotain's `LONGER_ALT`, as indices into the same table. When this type matches, these
    /// are tried at the same place, and the first one that matches more text becomes the token.
    pub(crate) longer: &'static [u16],
    /// The name messages print.
    pub(crate) name: &'static str,
    /// Start of a construct that needs a closer, for a [`Pattern::Match`]. When this matches but
    /// the pattern does not, no closer exists in the rest of the text, so the pattern is not
    /// tried again.
    pub(crate) opener: Option<fn(&str, usize) -> bool>,
    /// What the type matches.
    pub(crate) pattern: Pattern,
}

/// A token: its kind as an index into the token types, and its UTF-8 span.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    /// End offset.
    pub(crate) end: u32,
    /// Index of the token type, or [`EOF`].
    pub(crate) kind: u16,
    /// Start offset.
    pub(crate) start: u32,
}

const _: () = assert!(size_of::<Token>() == 12);

/// The kind of the token that ends every token list. Pele uses `-1`.
pub(crate) const EOF: u16 = u16::MAX;

/// Tokens of a source, ending with an [`EOF`] token at its end.
#[derive(Clone, Debug)]
pub(crate) struct Tokens<'a> {
    /// The source.
    pub(crate) src: &'a str,
    /// The tokens, hidden ones left out.
    pub(crate) tokens: Vec<Token>,
}

/// A token type that matches `text`, named after it.
pub(crate) const fn literal(text: &'static str) -> TokenType {
    TokenType { first: None, hidden: false, longer: &[], name: text, opener: None, pattern: Pattern::Literal(text) }
}

/// A keyword token type. Mermaid ends a keyword only at whitespace, a comment, or the end of
/// input.
pub(crate) const fn keyword(word: &'static str) -> TokenType {
    TokenType { pattern: Pattern::Keyword(word), ..literal(word) }
}

/// `accDescr: text`, or `accDescr { text }` across lines.
pub(crate) const ACC_DESCR: TokenType =
    TokenType { first: Some("\t a"), name: "ACC_DESCR", pattern: Pattern::Match(acc_descr), ..literal("") };

/// `accTitle: text`.
pub(crate) const ACC_TITLE: TokenType =
    TokenType { first: Some("\t a"), name: "ACC_TITLE", pattern: Pattern::Match(acc_title), ..literal("") };

/// `title text`, or a bare `title`.
pub(crate) const TITLE: TokenType =
    TokenType { first: Some("\t t"), name: "TITLE", pattern: Pattern::Match(title), ..literal("") };

/// A double- or single-quoted string with backslash escapes.
pub(crate) const STRING: TokenType =
    TokenType { first: Some("\"'"), name: "STRING", pattern: Pattern::Match(string), ..literal("") };

/// A line break.
pub(crate) const NEWLINE: TokenType =
    TokenType { first: Some("\r\n"), name: "NEWLINE", pattern: Pattern::Match(newline), ..literal("") };

/// Spaces and tabs, hidden.
pub(crate) const WHITESPACE: TokenType = TokenType {
    first: Some("\t "),
    hidden: true,
    name: "WHITESPACE",
    pattern: Pattern::Match(whitespace),
    ..literal("")
};

/// A front matter block, hidden.
pub(crate) const YAML: TokenType = TokenType {
    first: Some("-"),
    hidden: true,
    name: "YAML",
    opener: Some(yaml_opener),
    pattern: Pattern::Match(yaml),
    ..literal("")
};

/// A `%%{ ... }%%` directive, hidden.
pub(crate) const DIRECTIVE: TokenType = TokenType {
    first: Some("\t %"),
    hidden: true,
    name: "DIRECTIVE",
    opener: Some(directive_opener),
    pattern: Pattern::Match(directive),
    ..literal("")
};

/// A `%%` comment to the end of its line, hidden.
pub(crate) const SINGLE_LINE_COMMENT: TokenType = TokenType {
    first: Some("\t %"),
    hidden: true,
    name: "SINGLE_LINE_COMMENT",
    pattern: Pattern::Match(single_line_comment),
    ..literal("")
};

/// Splits `src` into tokens of `types`. At each place the types that can start there are tried in
/// order and the first match wins.
///
/// # Errors
///
/// Returns a lexical syntax error for `diagram` where no type matches.
pub(crate) fn tokenize<'a>(src: &'a str, types: &[TokenType], diagram: &'static str) -> Result<Tokens<'a>, Error> {
    let n = src.len();
    let mut dead = vec![false; types.len()];
    let mut memos = vec![[0usize; 4]; types.len()];
    let mut tokens = Vec::new();

    let mut p = 0;
    'scan: while p < n {
        let c = src.as_bytes().get(p).copied().unwrap_or(0);
        for (k, ty) in types.iter().enumerate() {
            if c < 128 && !can_start(ty, c) {
                continue;
            }
            let Some(mut end) = try_type(src, p, types, k, &mut dead, &mut memos).filter(|&end| end > p) else {
                continue;
            };

            let mut kind = k;
            for &alt in ty.longer {
                let alt = usize::from(alt);
                if let Some(alt_end) = try_type(src, p, types, alt, &mut dead, &mut memos)
                    && alt_end > end
                {
                    kind = alt;
                    end = alt_end;
                    break;
                }
            }

            if !types.get(kind).is_some_and(|ty| ty.hidden) {
                tokens.push(Token { end: offset(end), kind: u16::try_from(kind).unwrap_or(EOF), start: offset(p) });
            }
            p = end;
            continue 'scan;
        }
        return Err(Error::syntax(diagram, src, p, "", true));
    }

    tokens.push(Token { end: offset(n), kind: EOF, start: offset(n) });
    Ok(Tokens { src, tokens })
}

fn offset(p: usize) -> u32 {
    u32::try_from(p).unwrap_or(u32::MAX)
}

/// Whether type `ty` can start with ASCII character `c`, as Pele's `prepare` tables say.
fn can_start(ty: &TokenType, c: u8) -> bool {
    match ty.pattern {
        Pattern::Keyword(text) | Pattern::Literal(text) => text.as_bytes().first() == Some(&c),
        Pattern::Match(_) | Pattern::Scan(_) => ty.first.is_none_or(|first| first.as_bytes().contains(&c)),
    }
}

/// Tries type `k` at `p`, marking it dead when its opener matches but its pattern does not.
fn try_type(
    src: &str,
    p: usize,
    types: &[TokenType],
    k: usize,
    dead: &mut [bool],
    memos: &mut [Memo],
) -> Option<usize> {
    if dead.get(k).copied().unwrap_or(true) {
        return None;
    }
    let ty = types.get(k)?;
    match ty.pattern {
        Pattern::Keyword(word) => keyword_end(src, p, word),
        Pattern::Literal(text) => src.get(p..)?.starts_with(text).then_some(p + text.len()),
        Pattern::Match(matcher) => {
            let end = matcher(src, p);
            if end.is_none()
                && let Some(opener) = ty.opener
                && opener(src, p)
                && let Some(flag) = dead.get_mut(k)
            {
                *flag = true;
            }
            end
        }
        Pattern::Scan(scanner) => scanner(src, p, memos.get_mut(k)?),
    }
}

/// The character at `p`, when `p` is a character boundary inside `src`.
fn char_at(src: &str, p: usize) -> Option<char> {
    src.get(p..)?.chars().next()
}

/// Where a run of spaces and tabs from `p` ends.
pub(crate) fn skip_blanks(src: &str, p: usize) -> usize {
    let bytes = src.as_bytes();
    let mut at = p;
    while bytes.get(at).is_some_and(|&b| is_blank(u32::from(b))) {
        at += 1;
    }
    at
}

/// Where a run of JavaScript `\s` from `p` ends.
fn skip_space(src: &str, p: usize) -> usize {
    let mut at = p;
    while let Some(c) = char_at(src, at)
        && is_space(u32::from(c))
    {
        at += c.len_utf8();
    }
    at
}

/// Where the line from `p` ends: at the first `\n` or `\r`, or the end of `src`.
fn line_end(src: &str, p: usize) -> usize {
    src.get(p..).and_then(|rest| rest.find(['\n', '\r'])).map_or(src.len(), |at| p + at)
}

/// `[^\n\r]*?(?=%%)|[^\n\r]*` from `p`: to the first `%%` on the line, or to the line's end.
fn rest_of_line(src: &str, p: usize) -> usize {
    let end = line_end(src, p);
    src.get(p..end).and_then(|line| line.find("%%")).map_or(end, |at| p + at)
}

/// `(?:(?=%%)|(?!\S))` at `p`.
fn word_ends(src: &str, p: usize) -> bool {
    src.get(p..)
        .is_some_and(|rest| rest.starts_with("%%") || rest.chars().next().is_none_or(|c| is_space(u32::from(c))))
}

fn keyword_end(src: &str, p: usize, word: &str) -> Option<usize> {
    let end = p + word.len();
    (src.get(p..)?.starts_with(word) && word_ends(src, end)).then_some(end)
}

/// The start of an `accDescr` statement: `[\t ]*accDescr(?:[\t ]*:(?:[^\n\r]*?(?=%%)|[^\n\r]*)|\s*({))`.
/// Returns the end and whether the statement is the braced form, which ends after the `{`.
pub(crate) fn acc_descr_head(src: &str, p: usize) -> Option<(usize, bool)> {
    let q = skip_blanks(src, p);
    if !src.get(q..)?.starts_with("accDescr") {
        return None;
    }
    let r = q + 8;

    let colon = skip_blanks(src, r);
    if src.as_bytes().get(colon) == Some(&b':') {
        return Some((rest_of_line(src, colon + 1), false));
    }

    let brace = skip_space(src, r);
    (src.as_bytes().get(brace) == Some(&b'{')).then_some((brace + 1, true))
}

fn acc_descr(src: &str, p: usize) -> Option<usize> {
    let (end, braced) = acc_descr_head(src, p)?;
    if !braced {
        return Some(end);
    }
    src.get(end..)?.find('}').map(|close| end + close + 1)
}

fn acc_title(src: &str, p: usize) -> Option<usize> {
    let q = skip_blanks(src, p);
    if !src.get(q..)?.starts_with("accTitle") {
        return None;
    }
    let colon = skip_blanks(src, q + 8);
    (src.as_bytes().get(colon) == Some(&b':')).then(|| rest_of_line(src, colon + 1))
}

fn title(src: &str, p: usize) -> Option<usize> {
    let q = skip_blanks(src, p);
    if !src.get(q..)?.starts_with("title") {
        return None;
    }
    let r = q + 5;
    if src.as_bytes().get(r).is_some_and(|&b| is_blank(u32::from(b))) {
        return Some(rest_of_line(src, r + 1));
    }
    Some(r)
}

fn string(src: &str, p: usize) -> Option<usize> {
    let quote = *src.as_bytes().get(p)?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }

    let mut at = p + 1;
    loop {
        let c = char_at(src, at)?;
        at += c.len_utf8();
        if c == char::from(quote) {
            return Some(at);
        }
        if c == '\\' {
            let next = char_at(src, at).filter(|&next| !is_line_end(u32::from(next)))?;
            at += next.len_utf8();
        }
    }
}

fn newline(src: &str, p: usize) -> Option<usize> {
    let at = if src.as_bytes().get(p) == Some(&b'\r') { p + 1 } else { p };
    (src.as_bytes().get(at) == Some(&b'\n')).then_some(at + 1)
}

#[expect(clippy::unnecessary_wraps, reason = "a matcher answers with an option")]
fn whitespace(src: &str, p: usize) -> Option<usize> {
    Some(skip_blanks(src, p))
}

/// `(?:\r?\n|(?!\S))` at `p`.
fn block_end(src: &str, p: usize) -> Option<usize> {
    if let Some(end) = newline(src, p) {
        return Some(end);
    }
    char_at(src, p).is_none_or(|c| is_space(u32::from(c))).then_some(p)
}

/// `---[\t ]*\r?\n`.
fn yaml_head(src: &str, p: usize) -> Option<usize> {
    if !src.get(p..)?.starts_with("---") {
        return None;
    }
    newline(src, skip_blanks(src, p + 3))
}

fn yaml_opener(src: &str, p: usize) -> bool {
    yaml_head(src, p).is_some()
}

/// `---[\t ]*\r?\n(?:[\S\s]*?\r?\n)?---(?:\r?\n|(?!\S))`.
fn yaml(src: &str, p: usize) -> Option<usize> {
    let head = yaml_head(src, p)?;
    let close = |at: usize| {
        if src.get(at..)?.starts_with("---") { block_end(src, at + 3) } else { None }
    };

    // The optional group comes first, and its lazy body ends at the earliest line break that a
    // closing `---` follows.
    let mut from = head;
    while let Some(at) = src.get(from..).and_then(|rest| rest.find('\n')) {
        let after = from + at + 1;
        if let Some(end) = close(after) {
            return Some(end);
        }
        from = after;
    }
    close(head)
}

/// `[\t ]*%%{`.
fn directive_opener(src: &str, p: usize) -> bool {
    src.get(skip_blanks(src, p)..).is_some_and(|rest| rest.starts_with("%%{"))
}

/// `[\t ]*%%{[\S\s]*?}%%(?:\r?\n|(?!\S))`.
fn directive(src: &str, p: usize) -> Option<usize> {
    let q = skip_blanks(src, p);
    if !src.get(q..)?.starts_with("%%{") {
        return None;
    }

    let mut from = q + 3;
    while let Some(at) = src.get(from..).and_then(|rest| rest.find("}%%")) {
        if let Some(end) = block_end(src, from + at + 3) {
            return Some(end);
        }
        from += at + 1;
    }
    None
}

/// `[\t ]*%%[^\n\r]*`.
fn single_line_comment(src: &str, p: usize) -> Option<usize> {
    let q = skip_blanks(src, p);
    src.get(q..)?.starts_with("%%").then(|| line_end(src, q + 2))
}

/// `text` trimmed, with each run of two or more spaces and tabs made one space.
fn single(text: &str) -> String {
    collapse(trim(text), is_blank, ' ')
}

/// `text` with each run of two or more characters that `class` takes replaced by `with`.
fn collapse(text: &str, class: fn(u32) -> bool, with: char) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    for c in text.chars() {
        if class(u32::from(c)) {
            run.push(c);
            continue;
        }
        flush(&mut out, &mut run, with);
        out.push(c);
    }
    flush(&mut out, &mut run, with);
    out
}

fn flush(out: &mut String, run: &mut String, with: char) {
    if run.chars().nth(1).is_some() {
        out.push(with);
    } else {
        out.push_str(run);
    }
    run.clear();
}

/// The value of a `title` token: Pele's `titleValue`. Empty when `text` has no `title`.
pub(crate) fn title_value(text: &str) -> String {
    let Some(at) = text.find("title") else {
        return String::new();
    };
    let r = at + 5;
    if !text.as_bytes().get(r).is_some_and(|&b| is_blank(u32::from(b))) {
        return String::new();
    }
    single(text.get(r..line_end(text, r)).unwrap_or(""))
}

/// The value of an `accTitle` token: Pele's `accTitleValue`. Empty when `text` has none.
pub(crate) fn acc_title_value(text: &str) -> String {
    for (at, _) in text.match_indices("accTitle") {
        let colon = skip_blanks(text, at + 8);
        if text.as_bytes().get(colon) == Some(&b':') {
            return single(text.get(colon + 1..line_end(text, colon + 1)).unwrap_or(""));
        }
    }
    String::new()
}

/// The value of an `accDescr` token: Pele's `accDescrValue`. The braced form keeps its lines,
/// without their indentation, trailing whitespace, or blank lines between them. Empty when
/// `text` has none.
pub(crate) fn acc_descr_value(text: &str) -> String {
    for (at, _) in text.match_indices("accDescr") {
        let r = at + 8;
        let colon = skip_blanks(text, r);
        if text.as_bytes().get(colon) == Some(&b':') {
            return single(text.get(colon + 1..line_end(text, colon + 1)).unwrap_or(""));
        }

        let brace = skip_space(text, r);
        if text.as_bytes().get(brace) != Some(&b'{') {
            continue;
        }
        let Some(body) = text.get(brace + 1..).and_then(|rest| rest.find('}').and_then(|close| rest.get(..close)))
        else {
            continue;
        };
        let body = strip_line_ends(&strip_line_starts(body));
        let body = collapse(&body, is_blank, ' ');
        return collapse(&body, |c| c == 10 || c == 13, '\n');
    }
    String::new()
}

/// `text.replace(/^\s*/gm, '')`: whitespace removed where each line starts. A run removed this
/// way takes the blank lines after it too.
fn strip_line_starts(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut line_start = true;
    let mut skipping = false;
    for c in text.chars() {
        let code = u32::from(c);
        if line_start && is_space(code) {
            skipping = true;
        }
        if skipping && is_space(code) {
            line_start = is_line_end(code);
            continue;
        }
        skipping = false;
        out.push(c);
        line_start = is_line_end(code);
    }
    out
}

/// `text.replace(/\s+$/gm, '')`: whitespace removed where each line ends. A run of whitespace
/// keeps what follows its last line break, or goes entirely at the end of `text`.
fn strip_line_ends(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if !is_space(u32::from(c)) {
            out.push(c);
            i += 1;
            continue;
        }

        let mut e = i;
        while chars.get(e).is_some_and(|&c| is_space(u32::from(c))) {
            e += 1;
        }
        let keep = if e == chars.len() {
            e
        } else {
            (i + 1..e).rev().find(|&j| chars.get(j).is_some_and(|&c| is_line_end(u32::from(c)))).unwrap_or(i)
        };
        out.extend(chars.get(keep..e).unwrap_or(&[]));
        i = e;
    }
    out
}

/// The value of a quoted string token, with its escapes read: Pele's `stringValue`.
pub(crate) fn string_value(text: &str) -> Cow<'_, str> {
    let mut chars = text.chars();
    let first = chars.next().map_or(0, char::len_utf8);
    let Some(last) = chars.next_back() else {
        return Cow::Borrowed("");
    };
    let close = text.len() - last.len_utf8();
    if !text.contains('\\') {
        return Cow::Borrowed(text.get(first..close).unwrap_or(""));
    }

    // An escape just before the closing quote takes the quote, as in Pele.
    let mut out = String::with_capacity(close);
    let mut iter = text.char_indices().skip(1);
    while let Some((at, c)) = iter.next() {
        if at >= close {
            break;
        }
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some((_, next)) = iter.next() else {
            break;
        };
        out.push(match next {
            'b' => '\u{8}',
            'f' => '\u{c}',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'v' => '\u{b}',
            '0' => '\0',
            other => other,
        });
    }
    Cow::Owned(out)
}

/// Reads tokens for a hand-written parser, with Chevrotain-style messages on failure.
pub(crate) struct Reader<'a> {
    /// The diagram type errors name.
    pub(crate) diagram: &'static str,
    /// Index of the current token.
    pub(crate) i: usize,
    /// The tokens.
    pub(crate) tokens: Tokens<'a>,
    /// The token types the kinds index.
    pub(crate) types: &'a [TokenType],
}

impl<'a> Reader<'a> {
    /// A reader at the first token.
    pub(crate) fn new(tokens: Tokens<'a>, types: &'a [TokenType], diagram: &'static str) -> Self {
        Self { diagram, i: 0, tokens, types }
    }

    /// The current token's kind. Past the end it reads as [`EOF`], where Pele reads `undefined`.
    pub(crate) fn kind(&self) -> u16 {
        self.tokens.tokens.get(self.i).map_or(EOF, |token| token.kind)
    }

    /// The text of the current token.
    pub(crate) fn text(&self) -> &'a str {
        self.text_at(self.i)
    }

    /// The text of token `index`, or empty past the end.
    pub(crate) fn text_at(&self, index: usize) -> &'a str {
        let src: &'a str = self.tokens.src;
        self.tokens.tokens.get(index).and_then(|token| src.get(token.start as usize..token.end as usize)).unwrap_or("")
    }

    /// The UTF-8 offset where the current token starts, or the end of the source past the end.
    pub(crate) fn start(&self) -> usize {
        self.tokens.tokens.get(self.i).map_or(self.tokens.src.len(), |token| token.start as usize)
    }

    /// The current token's text, moving past it.
    pub(crate) fn take(&mut self) -> &'a str {
        let text = self.text();
        self.i += 1;
        text
    }

    /// Moves past the current token when it is of `kind`.
    pub(crate) fn accept(&mut self, kind: u16) -> bool {
        if self.kind() != kind {
            return false;
        }
        self.i += 1;
        true
    }

    /// The current token's text when it is of `kind`, moving past it.
    ///
    /// # Errors
    ///
    /// Returns a syntax error naming `kind` otherwise.
    pub(crate) fn expect(&mut self, kind: u16) -> Result<&'a str, Error> {
        if self.kind() != kind {
            let name = self.types.get(usize::from(kind)).map_or("", |ty| ty.name);
            let mut expected = String::from("token of type '");
            expected.push_str(name);
            expected.push('\'');
            return Err(self.fail(&expected));
        }
        Ok(self.take())
    }

    /// A syntax error at the current token: `Expecting {expected} but found ...`.
    #[cold]
    pub(crate) fn fail(&self, expected: &str) -> Error {
        let mut detail = String::from("Expecting ");
        detail.push_str(expected);
        detail.push_str(" but found ");
        if self.kind() == EOF {
            detail.push_str("end of input");
        } else {
            detail.push('`');
            detail.push_str(self.text());
            detail.push('`');
        }
        detail.push('.');
        Error::syntax(self.diagram, self.tokens.src, self.start(), &detail, false)
    }
}
