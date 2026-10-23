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
use crate::util::chars::{is_blank, is_line_end, is_space};

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
    TokenType {
        first: None,
        hidden: false,
        longer: &[],
        name: text,
        opener: None,
        pattern: Pattern::Literal(text),
    }
}

/// A keyword token type. Mermaid ends a keyword only at whitespace, a comment, or the end of
/// input.
pub(crate) const fn keyword(word: &'static str) -> TokenType {
    TokenType {
        pattern: Pattern::Keyword(word),
        ..literal(word)
    }
}

/// `accDescr: text`, or `accDescr { text }` across lines.
pub(crate) const ACC_DESCR: TokenType = TokenType {
    first: Some("\t a"),
    name: "ACC_DESCR",
    pattern: Pattern::Match(acc_descr),
    ..literal("")
};

/// `accTitle: text`.
pub(crate) const ACC_TITLE: TokenType = TokenType {
    first: Some("\t a"),
    name: "ACC_TITLE",
    pattern: Pattern::Match(acc_title),
    ..literal("")
};

/// `title text`, or a bare `title`.
pub(crate) const TITLE: TokenType = TokenType {
    first: Some("\t t"),
    name: "TITLE",
    pattern: Pattern::Match(title),
    ..literal("")
};

/// A double- or single-quoted string with backslash escapes.
pub(crate) const STRING: TokenType = TokenType {
    first: Some("\"'"),
    name: "STRING",
    pattern: Pattern::Match(string),
    ..literal("")
};

/// A line break.
pub(crate) const NEWLINE: TokenType = TokenType {
    first: Some("\r\n"),
    name: "NEWLINE",
    pattern: Pattern::Match(newline),
    ..literal("")
};

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
pub(crate) fn tokenize<'a>(
    src: &'a str,
    types: &[TokenType],
    diagram: &'static str,
) -> Result<Tokens<'a>, Error> {
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
            let Some(mut end) =
                try_type(src, p, types, k, &mut dead, &mut memos).filter(|&end| end > p)
            else {
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
                tokens.push(Token {
                    end: offset(end),
                    kind: u16::try_from(kind).unwrap_or(EOF),
                    start: offset(p),
                });
            }
            p = end;
            continue 'scan;
        }
        return Err(Error::syntax(diagram, src, p, "", true));
    }

    tokens.push(Token {
        end: offset(n),
        kind: EOF,
        start: offset(n),
    });
    Ok(Tokens { src, tokens })
}

fn offset(p: usize) -> u32 {
    u32::try_from(p).unwrap_or(u32::MAX)
}

/// Whether type `ty` can start with ASCII character `c`, as Pele's `prepare` tables say.
fn can_start(ty: &TokenType, c: u8) -> bool {
    match ty.pattern {
        Pattern::Keyword(text) | Pattern::Literal(text) => text.as_bytes().first() == Some(&c),
        Pattern::Match(_) | Pattern::Scan(_) => {
            ty.first.is_none_or(|first| first.as_bytes().contains(&c))
        }
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
    src.get(p..)
        .and_then(|rest| rest.find(['\n', '\r']))
        .map_or(src.len(), |at| p + at)
}

/// `[^\n\r]*?(?=%%)|[^\n\r]*` from `p`: to the first `%%` on the line, or to the line's end.
fn rest_of_line(src: &str, p: usize) -> usize {
    let end = line_end(src, p);
    src.get(p..end)
        .and_then(|line| line.find("%%"))
        .map_or(end, |at| p + at)
}

/// `(?:(?=%%)|(?!\S))` at `p`.
fn word_ends(src: &str, p: usize) -> bool {
    src.get(p..).is_some_and(|rest| {
        rest.starts_with("%%") || rest.chars().next().is_none_or(|c| is_space(u32::from(c)))
    })
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
    if src
        .as_bytes()
        .get(r)
        .is_some_and(|&b| is_blank(u32::from(b)))
    {
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
    let at = if src.as_bytes().get(p) == Some(&b'\r') {
        p + 1
    } else {
        p
    };
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
    char_at(src, p)
        .is_none_or(|c| is_space(u32::from(c)))
        .then_some(p)
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
        if src.get(at..)?.starts_with("---") {
            block_end(src, at + 3)
        } else {
            None
        }
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
    src.get(skip_blanks(src, p)..)
        .is_some_and(|rest| rest.starts_with("%%{"))
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
    src.get(q..)?
        .starts_with("%%")
        .then(|| line_end(src, q + 2))
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
    if !text
        .as_bytes()
        .get(r)
        .is_some_and(|&b| is_blank(u32::from(b)))
    {
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
        let Some(body) = text
            .get(brace + 1..)
            .and_then(|rest| rest.find('}').and_then(|close| rest.get(..close)))
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
            (i + 1..e)
                .rev()
                .find(|&j| chars.get(j).is_some_and(|&c| is_line_end(u32::from(c))))
                .unwrap_or(i)
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
        Self {
            diagram,
            i: 0,
            tokens,
            types,
        }
    }

    /// The current token's kind. Past the end it reads as [`EOF`], where Pele reads `undefined`.
    pub(crate) fn kind(&self) -> u16 {
        self.tokens
            .tokens
            .get(self.i)
            .map_or(EOF, |token| token.kind)
    }

    /// The text of the current token.
    pub(crate) fn text(&self) -> &'a str {
        self.text_at(self.i)
    }

    /// The text of token `index`, or empty past the end.
    pub(crate) fn text_at(&self, index: usize) -> &'a str {
        let src: &'a str = self.tokens.src;
        self.tokens
            .tokens
            .get(index)
            .and_then(|token| src.get(token.start as usize..token.end as usize))
            .unwrap_or("")
    }

    /// The UTF-8 offset where the current token starts, or the end of the source past the end.
    pub(crate) fn start(&self) -> usize {
        self.tokens
            .tokens
            .get(self.i)
            .map_or(self.tokens.src.len(), |token| token.start as usize)
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

#[cfg(test)]
#[expect(
    clippy::unreadable_literal,
    reason = "hashes as the Pele scripts print them"
)]
pub(crate) mod tests {
    //! Checks against Pele (`obsidianmd/pele` at `458322a`). Every expected value comes from
    //! running Pele's `tokenize`, `Reader`, and value helpers with `bun` on the same tables and
    //! inputs, never from this port. Corpus results compare FNV-1a hashes of the same
    //! serialization on both sides: `kind:start-end[=value]` per token in UTF-16 offsets, or
    //! `E{line}:{column}:{message}` for an error.

    use super::*;
    use crate::yaml::{Value, parse};

    /// Every corpus file, for the shared modules' tests.
    pub(crate) const CORPUS: &[(&str, &str)] = &[
        (
            "agentflow",
            include_str!("../../../../../test/vectors/corpus/agentflow.json"),
        ),
        (
            "architecture",
            include_str!("../../../../../test/vectors/corpus/architecture.json"),
        ),
        (
            "block",
            include_str!("../../../../../test/vectors/corpus/block.json"),
        ),
        (
            "c4",
            include_str!("../../../../../test/vectors/corpus/c4.json"),
        ),
        (
            "class",
            include_str!("../../../../../test/vectors/corpus/class.json"),
        ),
        (
            "cynefin",
            include_str!("../../../../../test/vectors/corpus/cynefin.json"),
        ),
        (
            "er",
            include_str!("../../../../../test/vectors/corpus/er.json"),
        ),
        (
            "eventmodeling",
            include_str!("../../../../../test/vectors/corpus/eventmodeling.json"),
        ),
        (
            "flowchart",
            include_str!("../../../../../test/vectors/corpus/flowchart.json"),
        ),
        (
            "gantt",
            include_str!("../../../../../test/vectors/corpus/gantt.json"),
        ),
        (
            "git",
            include_str!("../../../../../test/vectors/corpus/git.json"),
        ),
        (
            "info",
            include_str!("../../../../../test/vectors/corpus/info.json"),
        ),
        (
            "ishikawa",
            include_str!("../../../../../test/vectors/corpus/ishikawa.json"),
        ),
        (
            "journey",
            include_str!("../../../../../test/vectors/corpus/journey.json"),
        ),
        (
            "kanban",
            include_str!("../../../../../test/vectors/corpus/kanban.json"),
        ),
        (
            "mindmap",
            include_str!("../../../../../test/vectors/corpus/mindmap.json"),
        ),
        (
            "packet",
            include_str!("../../../../../test/vectors/corpus/packet.json"),
        ),
        (
            "pie",
            include_str!("../../../../../test/vectors/corpus/pie.json"),
        ),
        (
            "quadrant",
            include_str!("../../../../../test/vectors/corpus/quadrant.json"),
        ),
        (
            "radar",
            include_str!("../../../../../test/vectors/corpus/radar.json"),
        ),
        (
            "railroad",
            include_str!("../../../../../test/vectors/corpus/railroad.json"),
        ),
        (
            "requirement",
            include_str!("../../../../../test/vectors/corpus/requirement.json"),
        ),
        (
            "sankey",
            include_str!("../../../../../test/vectors/corpus/sankey.json"),
        ),
        (
            "sequence",
            include_str!("../../../../../test/vectors/corpus/sequence.json"),
        ),
        (
            "state",
            include_str!("../../../../../test/vectors/corpus/state.json"),
        ),
        (
            "swimlane",
            include_str!("../../../../../test/vectors/corpus/swimlane.json"),
        ),
        (
            "timeline",
            include_str!("../../../../../test/vectors/corpus/timeline.json"),
        ),
        (
            "treemap",
            include_str!("../../../../../test/vectors/corpus/treemap.json"),
        ),
        (
            "treeview",
            include_str!("../../../../../test/vectors/corpus/treeview.json"),
        ),
        (
            "usecase",
            include_str!("../../../../../test/vectors/corpus/usecase.json"),
        ),
        (
            "venn",
            include_str!("../../../../../test/vectors/corpus/venn.json"),
        ),
        (
            "wardley",
            include_str!("../../../../../test/vectors/corpus/wardley.json"),
        ),
        (
            "xychart",
            include_str!("../../../../../test/vectors/corpus/xychart.json"),
        ),
    ];

    /// The sources of a corpus file.
    pub(crate) fn inputs(json: &str) -> Vec<String> {
        let Ok(Value::Array(items)) = parse(json) else {
            panic!("corpus file is not an array");
        };
        items
            .into_iter()
            .map(|item| match item {
                Value::String(text) => text,
                _ => panic!("corpus entry is not a string"),
            })
            .collect()
    }

    /// FNV-1a over UTF-8 bytes, as the Pele scripts compute it.
    pub(crate) fn fnv(text: &str) -> u32 {
        let mut hash: u32 = 0x811c_9dc5;
        for byte in text.bytes() {
            hash ^= u32::from(byte);
            hash = hash.wrapping_mul(0x0100_0193);
        }
        hash
    }

    /// The UTF-16 offset of UTF-8 offset `at`.
    pub(crate) fn utf16(src: &str, at: usize) -> usize {
        src[..at].encode_utf16().count()
    }

    /// An error as the Pele scripts print it.
    pub(crate) fn error(e: &Error) -> String {
        format!("E{}:{}:{}", e.line, e.column, e.message)
    }

    fn id(src: &str, p: usize) -> Option<usize> {
        let bytes = src.as_bytes();
        if !bytes
            .get(p)
            .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
        {
            return None;
        }
        let mut at = p + 1;
        while bytes
            .get(at)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
        {
            at += 1;
        }
        Some(at)
    }

    fn number(src: &str, p: usize) -> Option<usize> {
        let bytes = src.as_bytes();
        let digits = |from: usize| {
            from + bytes[from..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count()
        };
        let end = digits(p);
        if end == p {
            return None;
        }
        if bytes.get(end) == Some(&b'.') && digits(end + 1) > end + 1 {
            return Some(digits(end + 1));
        }
        Some(end)
    }

    fn any(src: &str, p: usize) -> Option<usize> {
        src[p..].chars().next().map(|c| p + c.len_utf8())
    }

    fn acc_descr_opener(src: &str, p: usize) -> bool {
        let q = skip_blanks(src, p);
        src[q..].starts_with("accDescr")
            && src.as_bytes().get(skip_space(src, q + 8)) == Some(&b'{')
    }

    const ID: TokenType = TokenType {
        first: Some("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_"),
        name: "ID",
        pattern: Pattern::Match(id),
        ..literal("")
    };
    const NUMBER: TokenType = TokenType {
        first: Some("0123456789"),
        name: "NUMBER",
        pattern: Pattern::Match(number),
        ..literal("")
    };

    /// Plain `ACC_DESCR` with an opener, and no catch-all.
    const A: &[TokenType] = &[
        TokenType {
            longer: &[16],
            ..keyword("graph")
        },
        keyword("pie"),
        keyword("showData"),
        TokenType {
            longer: &[13],
            ..literal("--")
        },
        TokenType {
            longer: &[13],
            ..literal("-")
        },
        literal(":"),
        TokenType {
            opener: Some(acc_descr_opener),
            ..ACC_DESCR
        },
        ACC_TITLE,
        TITLE,
        STRING,
        NUMBER,
        NEWLINE,
        WHITESPACE,
        YAML,
        DIRECTIVE,
        SINGLE_LINE_COMMENT,
        ID,
    ];

    /// The scanning `accDescr` and a catch-all.
    pub(crate) const B: &[TokenType] = &[
        TokenType {
            longer: &[14],
            ..keyword("graph")
        },
        keyword("pie"),
        TokenType {
            longer: &[11],
            ..literal("--")
        },
        TokenType {
            longer: &[11],
            ..literal("-")
        },
        crate::diagrams::common::acc_descr::acc_descr("EM_ACC_DESCR"),
        ACC_TITLE,
        TITLE,
        STRING,
        NUMBER,
        NEWLINE,
        WHITESPACE,
        YAML,
        DIRECTIVE,
        SINGLE_LINE_COMMENT,
        ID,
        TokenType {
            name: "ANY",
            pattern: Pattern::Match(any),
            ..literal("")
        },
    ];

    fn value(name: &str, text: &str) -> Option<String> {
        match name {
            "TITLE" => Some(title_value(text)),
            "ACC_TITLE" => Some(acc_title_value(text)),
            "ACC_DESCR" | "EM_ACC_DESCR" => Some(acc_descr_value(text)),
            "STRING" => Some(string_value(text).into_owned()),
            _ => None,
        }
    }

    /// Tokens as the Pele scripts print them.
    pub(crate) fn serialize(src: &str, types: &[TokenType]) -> String {
        let tokens = match tokenize(src, types, "test") {
            Ok(tokens) => tokens,
            Err(e) => return error(&e),
        };
        let mut lines = Vec::new();
        for token in &tokens.tokens {
            let (start, end) = (token.start as usize, token.end as usize);
            let kind = if token.kind == EOF {
                -1
            } else {
                i32::from(token.kind)
            };
            let mut line = format!("{kind}:{}-{}", utf16(src, start), utf16(src, end));
            if let Some(ty) = types.get(usize::from(token.kind))
                && let Some(value) = value(ty.name, &src[start..end])
            {
                line.push('=');
                line.push_str(&value);
            }
            lines.push(line);
        }
        lines.join("\n")
    }

    /// `pie` then lines of `"label" : number`, read with [`Reader`] as the Pele script does.
    fn read_pie(src: &str) -> String {
        let run = || -> Result<String, Error> {
            let tokens = tokenize(src, A, "test")?;
            let mut r = Reader::new(tokens, A, "test");
            let mut out = Vec::new();
            r.expect(1)?;
            while r.kind() != EOF {
                if r.accept(11) {
                    continue;
                }
                if r.kind() == 9 {
                    let label = string_value(r.take()).into_owned();
                    r.expect(5)?;
                    out.push(format!("{label}={}", r.expect(10)?));
                    continue;
                }
                return Err(r.fail("a slice"));
            }
            Ok(out.join(";"))
        };
        run().unwrap_or_else(|e| error(&e))
    }

    #[test]
    fn tokenize_matches_pele_on_edge_cases() {
        let cases: &[(&str, &str, &str, &str)] = &[
            (
                "pie\n\"a\" : 1\n'b\\'s' : 2.5\n",
                "1:0-3\n11:3-4\n9:4-7=a\n5:8-9\n10:10-11\n11:11-12\n9:12-18=b's\n5:19-20\n10:21-24\n11:24-25\n-1:25-25",
                "1:0-3\n9:3-4\n7:4-7=a\n15:8-9\n8:10-11\n9:11-12\n7:12-18=b's\n15:19-20\n8:21-24\n9:24-25\n-1:25-25",
                "a=1;b's=2.5",
            ),
            (
                "pie showData\n  title  My   pie  %% note\naccTitle : The   title\naccDescr: one  two\n",
                "1:0-3\n2:4-12\n11:12-13\n8:13-32=My pie\n11:39-40\n7:40-62=The title\n11:62-63\n6:63-81=one two\n11:81-82\n-1:82-82",
                "1:0-3\n14:4-12\n9:12-13\n6:13-32=My pie\n9:39-40\n5:40-62=The title\n9:62-63\n4:63-81=one two\n9:81-82\n-1:82-82",
                "E1:5:Parse error on line 1:\npie showData\n----^\nExpecting a slice but found `showData`.",
            ),
            (
                "pie\naccDescr {\n   line one   \n\n\n   line  two\t\t\n }\n\"x\":3",
                "1:0-3\n11:3-4\n6:4-49=line one\nline two\n11:49-50\n9:50-53=x\n5:53-54\n10:54-55\n-1:55-55",
                "1:0-3\n9:3-4\n4:4-49=line one\nline two\n9:49-50\n7:50-53=x\n15:53-54\n8:54-55\n-1:55-55",
                "E2:1:Parse error on line 2:\naccDescr {\n^\nExpecting a slice but found `accDescr {\n   line one   \n\n\n   line  two\t\t\n }`.",
            ),
            (
                "pie\naccDescr {\n never closed\naccDescr { again\n",
                "E2:10:Lexical error on line 2. Unrecognized text.\naccDescr {\n---------^",
                "1:0-3\n9:3-4\n14:4-12\n15:13-14\n9:14-15\n14:16-21\n14:22-28\n9:28-29\n14:29-37\n15:38-39\n14:40-45\n9:45-46\n-1:46-46",
                "E2:10:Lexical error on line 2. Unrecognized text.\naccDescr {\n---------^",
            ),
            (
                "graph graphs graph-x graph%%c\ngraph\n",
                "0:0-5\n16:6-12\n16:13-20\n0:21-26\n11:29-30\n0:30-35\n11:35-36\n-1:36-36",
                "0:0-5\n14:6-12\n14:13-20\n0:21-26\n9:29-30\n0:30-35\n9:35-36\n-1:36-36",
                "E1:1:Parse error on line 1:\ngraph graphs graph-x graph%%c\n^\nExpecting token of type 'pie' but found `graph`.",
            ),
            (
                "---\ntitle: x\n---\npie\n",
                "1:17-20\n11:20-21\n-1:21-21",
                "1:17-20\n9:20-21\n-1:21-21",
                "",
            ),
            (
                "---\r\ntitle: x\r\n---\r\npie",
                "1:20-23\n-1:23-23",
                "1:20-23\n-1:23-23",
                "",
            ),
            (
                "--- \nnever closed\npie\n--\n-",
                "3:0-2\n4:2-3\n11:4-5\n16:5-10\n16:11-17\n11:17-18\n1:18-21\n11:21-22\n3:22-24\n11:24-25\n4:25-26\n-1:26-26",
                "2:0-2\n3:2-3\n9:4-5\n14:5-10\n14:11-17\n9:17-18\n1:18-21\n9:21-22\n2:22-24\n9:24-25\n3:25-26\n-1:26-26",
                "E1:1:Parse error on line 1:\n--- \n^\nExpecting token of type 'pie' but found `--`.",
            ),
            (
                "%%{init: {}}%%\npie\n  %%{ x }%% y\n",
                "1:15-18\n11:18-19\n16:31-32\n11:32-33\n-1:33-33",
                "1:15-18\n9:18-19\n14:31-32\n9:32-33\n-1:33-33",
                "E3:13:Parse error on line 3:\n  %%{ x }%% y\n------------^\nExpecting a slice but found `y`.",
            ),
            (
                "%%{ unclosed\npie\n%%{ also\n",
                "11:12-13\n1:13-16\n11:16-17\n11:25-26\n-1:26-26",
                "9:12-13\n1:13-16\n9:16-17\n9:25-26\n-1:26-26",
                "E1:13:Parse error on line 1:\n%%{ unclosed\n------------^\nExpecting token of type 'pie' but found `\n`.",
            ),
            (
                "pie\n\"esc \\n \\t \\b \\f \\v \\0 \\q \\\\\" : 1\n",
                "1:0-3\n11:3-4\n9:4-33=esc \n \t \u{8} \u{c} \u{b} \u{0} q \\\n5:34-35\n10:36-37\n11:37-38\n-1:38-38",
                "1:0-3\n9:3-4\n7:4-33=esc \n \t \u{8} \u{c} \u{b} \u{0} q \\\n15:34-35\n8:36-37\n9:37-38\n-1:38-38",
                "esc \n \t \u{8} \u{c} \u{b} \u{0} q \\=1",
            ),
            (
                "pie\n\"line\nbreak\" : 1\n\"bad \\\nbreak\" : 2\n",
                "E4:1:Lexical error on line 4. Unrecognized text.\n\"bad \\\n^",
                "1:0-3\n9:3-4\n7:4-16=line\nbreak\n15:17-18\n8:19-20\n9:20-21\n15:21-22\n14:22-25\n15:26-27\n9:27-28\n14:28-33\n15:33-34\n15:35-36\n8:37-38\n9:38-39\n-1:39-39",
                "E4:1:Lexical error on line 4. Unrecognized text.\n\"bad \\\n^",
            ),
            (
                "pie\n\"é ünï 😀\" : 7\ntitle 😀  ok\n",
                "1:0-3\n11:3-4\n9:4-14=é ünï 😀\n5:15-16\n10:17-18\n11:18-19\n8:19-31=😀 ok\n11:31-32\n-1:32-32",
                "1:0-3\n9:3-4\n7:4-14=é ünï 😀\n15:15-16\n8:17-18\n9:18-19\n6:19-31=😀 ok\n9:31-32\n-1:32-32",
                "E3:1:Parse error on line 3:\ntitle 😀  ok\n^\nExpecting a slice but found `title 😀  ok`.",
            ),
            (
                "pie\r\n\"a\" : 1\r\n",
                "1:0-3\n11:3-5\n9:5-8=a\n5:9-10\n10:11-12\n11:12-14\n-1:14-14",
                "1:0-3\n9:3-5\n7:5-8=a\n15:9-10\n8:11-12\n9:12-14\n-1:14-14",
                "a=1",
            ),
            (
                "title\ntitlex\ntitle\tx  y\naccTitle:\naccTitle :%%c\n",
                "8:0-5=\n11:5-6\n8:6-11=\n16:11-12\n11:12-13\n8:13-23=x y\n11:23-24\n7:24-33=\n11:33-34\n7:34-44=\n11:47-48\n-1:48-48",
                "6:0-5=\n9:5-6\n6:6-11=\n14:11-12\n9:12-13\n6:13-23=x y\n9:23-24\n5:24-33=\n9:33-34\n5:34-44=\n9:47-48\n-1:48-48",
                "E1:1:Parse error on line 1:\ntitle\n^\nExpecting token of type 'pie' but found `title`.",
            ),
            (
                "pie\n\"a\" 1\n",
                "1:0-3\n11:3-4\n9:4-7=a\n10:8-9\n11:9-10\n-1:10-10",
                "1:0-3\n9:3-4\n7:4-7=a\n8:8-9\n9:9-10\n-1:10-10",
                "E2:5:Parse error on line 2:\n\"a\" 1\n----^\nExpecting token of type ':' but found `1`.",
            ),
            (
                "pie\n\"a\" :\n",
                "1:0-3\n11:3-4\n9:4-7=a\n5:8-9\n11:9-10\n-1:10-10",
                "1:0-3\n9:3-4\n7:4-7=a\n15:8-9\n9:9-10\n-1:10-10",
                "E2:6:Parse error on line 2:\n\"a\" :\n-----^\nExpecting token of type 'NUMBER' but found `\n`.",
            ),
            (
                "pie\n:\n",
                "1:0-3\n11:3-4\n5:4-5\n11:5-6\n-1:6-6",
                "1:0-3\n9:3-4\n15:4-5\n9:5-6\n-1:6-6",
                "E2:1:Parse error on line 2:\n:\n^\nExpecting a slice but found `:`.",
            ),
            ("pie", "1:0-3\n-1:3-3", "1:0-3\n-1:3-3", ""),
            (
                "",
                "-1:0-0",
                "-1:0-0",
                "E1:1:Parse error on line 1:\n\n^\nExpecting token of type 'pie' but found end of input.",
            ),
            (
                "accDescr\t{a}accDescr  :  z %% c\naccDescr {\n  a\n  b\n}\n",
                "6:0-12=a\n6:12-27=z\n11:31-32\n6:32-52=a\nb\n11:52-53\n-1:53-53",
                "4:0-12=a\n4:12-27=z\n9:31-32\n4:32-52=a\nb\n9:52-53\n-1:53-53",
                "E1:1:Parse error on line 1:\naccDescr\t{a}accDescr  :  z %% c\n^\nExpecting token of type 'pie' but found `accDescr\t{a}`.",
            ),
            (
                "accDescr\n\n{\n  first\n\n  second  \n}",
                "6:0-33=first\nsecond\n-1:33-33",
                "4:0-33=first\nsecond\n-1:33-33",
                "E1:1:Parse error on line 1:\naccDescr\n^\nExpecting token of type 'pie' but found `accDescr\n\n{\n  first\n\n  second  \n}`.",
            ),
            (
                "pie\n 😀 \n",
                "E2:2:Lexical error on line 2. Unrecognized text.\n 😀 \n-^",
                "1:0-3\n9:3-4\n15:5-7\n9:8-9\n-1:9-9",
                "E2:2:Lexical error on line 2. Unrecognized text.\n 😀 \n-^",
            ),
            (
                "pie\n\u{2028}\n",
                "E2:1:Lexical error on line 2. Unrecognized text.\n\u{2028}\n^",
                "1:0-3\n9:3-4\n15:4-5\n9:5-6\n-1:6-6",
                "E2:1:Lexical error on line 2. Unrecognized text.\n\u{2028}\n^",
            ),
            (
                "accTitle\u{3000}: x\n",
                "E1:9:Lexical error on line 1. Unrecognized text.\naccTitle\u{3000}: x\n--------^",
                "14:0-8\n15:8-9\n15:9-10\n14:11-12\n9:12-13\n-1:13-13",
                "E1:9:Lexical error on line 1. Unrecognized text.\naccTitle\u{3000}: x\n--------^",
            ),
            (
                "title\u{a0}x\n",
                "E1:6:Lexical error on line 1. Unrecognized text.\ntitle\u{a0}x\n-----^",
                "6:0-5=\n15:5-6\n14:6-7\n9:7-8\n-1:8-8",
                "E1:6:Lexical error on line 1. Unrecognized text.\ntitle\u{a0}x\n-----^",
            ),
        ];
        for &(src, a, b, pie) in cases {
            assert_eq!(serialize(src, A), a, "table A on {src:?}");
            assert_eq!(serialize(src, B), b, "table B on {src:?}");
            assert_eq!(read_pie(src), pie, "reader on {src:?}");
        }
    }

    #[test]
    fn tokenize_matches_pele_on_corpus() {
        // `(file, hash A, lines A, hash B, lines B, hash of the reader's results)`.
        let expected: &[(&str, u32, usize, u32, usize, u32)] = &[
            ("agentflow", 0xcd5eec63, 47, 0xcf03765f, 981, 0xcd5eec63),
            ("architecture", 0xa7201cfd, 23, 0x766b10ca, 674, 0xa7201cfd),
            ("block", 0x86414f9d, 191, 0xddf6873e, 879, 0xbf591b55),
            ("c4", 0x992c099c, 23, 0x5a8f30a6, 1798, 0x992c099c),
            ("class", 0x15e395e4, 178, 0x37be1d0f, 1513, 0xc1543532),
            ("cynefin", 0x743b9ef, 55, 0x8e2e082a, 106, 0x6016f3d3),
            ("er", 0x5b92ec56, 118, 0xeb146d1f, 1259, 0x6c950eed),
            ("eventmodeling", 0x52a4615, 79, 0x56aa5c61, 521, 0x7686803a),
            ("flowchart", 0xe3c4164c, 476, 0xb75c001e, 2560, 0x60128842),
            ("gantt", 0x478f4a1f, 50, 0x7a59a49e, 789, 0xa738e1a9),
            ("git", 0x1b1cb10a, 1972, 0x8d4ad13a, 1972, 0x2fa77857),
            ("info", 0xbad4c2fe, 8, 0x7dd3c35a, 8, 0x6532d4be),
            ("ishikawa", 0x76aea6a1, 62, 0xddb0731, 62, 0x89b17dc3),
            ("journey", 0xc7e61b87, 3, 0x62049bf4, 51, 0xc7e61b87),
            ("kanban", 0x83a21ac4, 11, 0x8e766acf, 246, 0x83a21ac4),
            ("mindmap", 0xc09b0616, 73, 0xfed7ead4, 257, 0xdecc5fae),
            ("packet", 0x8fbe822b, 145, 0xc22147e7, 175, 0xe3c1a006),
            ("pie", 0xbeca2550, 39, 0x8c40ee19, 39, 0x70606e61),
            ("quadrant", 0x666736d0, 11, 0x6fb39cba, 318, 0x666736d0),
            ("radar", 0x2c9ff1bf, 11, 0xa1f29f74, 234, 0x2c9ff1bf),
            ("railroad", 0xc4a74f6f, 55, 0x74e7824d, 709, 0xc4a74f6f),
            ("requirement", 0x6d577aa4, 35, 0xf1e87d40, 775, 0x6d577aa4),
            ("sankey", 0xefef6d59, 31, 0xf0bb16a1, 708, 0xefef6d59),
            ("sequence", 0xfb2f5a3a, 163, 0xd0158a20, 1894, 0x7dce066),
            ("state", 0xdcc53740, 102, 0xf21cea11, 1081, 0x43bd24fb),
            ("swimlane", 0xc4eda2ee, 51, 0x348365e7, 918, 0xc4eda2ee),
            ("timeline", 0x2c73181c, 331, 0x79140e2e, 690, 0x3e321bfd),
            ("treemap", 0xa2b29544, 161, 0x6f45559f, 255, 0xbc8bcc36),
            ("treeview", 0x93f10529, 67, 0x5f7d447a, 428, 0x85626bc2),
            ("usecase", 0x795dd380, 95, 0xa8669168, 1362, 0x795dd380),
            ("venn", 0xdb7278a0, 31, 0xfdefe7b0, 306, 0xdb7278a0),
            ("wardley", 0xe34f4200, 75, 0x4773131f, 1154, 0xe34f4200),
            ("xychart", 0x3ef4b970, 31, 0x8d4237b7, 504, 0x3ef4b970),
        ];
        assert_eq!(expected.len(), CORPUS.len());
        for (&(name, hash_a, lines_a, hash_b, lines_b, hash_pie), &(file, json)) in
            expected.iter().zip(CORPUS)
        {
            assert_eq!(name, file);
            let sources = inputs(json);
            let a = sources
                .iter()
                .map(|src| serialize(src, A))
                .collect::<Vec<_>>()
                .join("\n\n");
            let b = sources
                .iter()
                .map(|src| serialize(src, B))
                .collect::<Vec<_>>()
                .join("\n\n");
            let pie = sources
                .iter()
                .map(|src| read_pie(src))
                .collect::<Vec<_>>()
                .join("\n\n");
            assert_eq!(
                (fnv(&a), a.split('\n').count()),
                (hash_a, lines_a),
                "table A on {name}"
            );
            assert_eq!(
                (fnv(&b), b.split('\n').count()),
                (hash_b, lines_b),
                "table B on {name}"
            );
            assert_eq!(fnv(&pie), hash_pie, "reader on {name}");
        }
    }
}
