//! Decodes Mermaid's entity placeholders and HTML entities, as Pele's `decodeEntities` does.

/// Named entities Pele recognizes, sorted by name for binary search.
const NAMED: [(&str, u32); 176] = [
    ("Alpha", 913),
    ("Beta", 914),
    ("Chi", 935),
    ("Dagger", 8225),
    ("Delta", 916),
    ("Epsilon", 917),
    ("Eta", 919),
    ("Gamma", 915),
    ("Iota", 921),
    ("Kappa", 922),
    ("Lambda", 923),
    ("Mu", 924),
    ("Nu", 925),
    ("Omega", 937),
    ("Omicron", 927),
    ("Phi", 934),
    ("Pi", 928),
    ("Prime", 8243),
    ("Psi", 936),
    ("Rho", 929),
    ("Sigma", 931),
    ("Tau", 932),
    ("Theta", 920),
    ("Upsilon", 933),
    ("Xi", 926),
    ("Zeta", 918),
    ("alpha", 945),
    ("amp", 38),
    ("and", 8743),
    ("apos", 39),
    ("ast", 42),
    ("asymp", 8776),
    ("bdquo", 8222),
    ("beta", 946),
    ("bsol", 92),
    ("bull", 8226),
    ("cap", 8745),
    ("cent", 162),
    ("check", 10003),
    ("chi", 967),
    ("clubs", 9827),
    ("colon", 58),
    ("comma", 44),
    ("commat", 64),
    ("cong", 8773),
    ("copy", 169),
    ("cross", 10007),
    ("cup", 8746),
    ("curren", 164),
    ("dArr", 8659),
    ("dagger", 8224),
    ("darr", 8595),
    ("deg", 176),
    ("delta", 948),
    ("diams", 9830),
    ("divide", 247),
    ("dollar", 36),
    ("empty", 8709),
    ("epsilon", 949),
    ("equals", 61),
    ("equiv", 8801),
    ("eta", 951),
    ("euro", 8364),
    ("excl", 33),
    ("exist", 8707),
    ("forall", 8704),
    ("frac12", 189),
    ("frac14", 188),
    ("frac34", 190),
    ("gamma", 947),
    ("ge", 8805),
    ("grave", 96),
    ("gt", 62),
    ("hArr", 8660),
    ("harr", 8596),
    ("hat", 94),
    ("hearts", 9829),
    ("hellip", 8230),
    ("iexcl", 161),
    ("infin", 8734),
    ("int", 8747),
    ("iota", 953),
    ("iquest", 191),
    ("isin", 8712),
    ("kappa", 954),
    ("lArr", 8656),
    ("lambda", 955),
    ("laquo", 171),
    ("larr", 8592),
    ("lbrace", 123),
    ("lbrack", 91),
    ("ldquo", 8220),
    ("le", 8804),
    ("lowbar", 95),
    ("loz", 9674),
    ("lpar", 40),
    ("lsaquo", 8249),
    ("lsquo", 8216),
    ("lt", 60),
    ("mdash", 8212),
    ("micro", 181),
    ("middot", 183),
    ("minus", 8722),
    ("mu", 956),
    ("nabla", 8711),
    ("nbsp", 160),
    ("ndash", 8211),
    ("ne", 8800),
    ("ni", 8715),
    ("not", 172),
    ("notin", 8713),
    ("nu", 957),
    ("num", 35),
    ("omega", 969),
    ("omicron", 959),
    ("oplus", 8853),
    ("or", 8744),
    ("otimes", 8855),
    ("para", 182),
    ("part", 8706),
    ("percnt", 37),
    ("period", 46),
    ("permil", 8240),
    ("perp", 8869),
    ("phi", 966),
    ("pi", 960),
    ("plus", 43),
    ("plusmn", 177),
    ("pound", 163),
    ("prime", 8242),
    ("prod", 8719),
    ("psi", 968),
    ("quest", 63),
    ("quot", 34),
    ("rArr", 8658),
    ("radic", 8730),
    ("raquo", 187),
    ("rarr", 8594),
    ("rbrace", 125),
    ("rbrack", 93),
    ("rdquo", 8221),
    ("reg", 174),
    ("rho", 961),
    ("rpar", 41),
    ("rsaquo", 8250),
    ("rsquo", 8217),
    ("sbquo", 8218),
    ("sdot", 8901),
    ("sect", 167),
    ("semi", 59),
    ("sigma", 963),
    ("sigmaf", 962),
    ("sim", 8764),
    ("sol", 47),
    ("spades", 9824),
    ("sub", 8834),
    ("sube", 8838),
    ("sum", 8721),
    ("sup", 8835),
    ("sup1", 185),
    ("sup2", 178),
    ("sup3", 179),
    ("supe", 8839),
    ("tau", 964),
    ("there4", 8756),
    ("theta", 952),
    ("tilde", 126),
    ("times", 215),
    ("trade", 8482),
    ("uArr", 8657),
    ("uarr", 8593),
    ("upsilon", 965),
    ("vert", 124),
    ("xi", 958),
    ("yen", 165),
    ("zeta", 950),
];

/// Opens a Mermaid placeholder (`ﬂ°`); a second `°` marks a numeric code.
const OPEN: &str = "\u{fb02}\u{b0}";
/// Closes a Mermaid placeholder (`¶ß`).
const CLOSE: &str = "\u{b6}\u{df}";

/// Turns Mermaid's `#35;` and `#quot;` codes, as hidden by preprocessing, and HTML entities into
/// characters. Unknown or invalid references stay as written.
pub(crate) fn decode(text: &str) -> std::borrow::Cow<'_, str> {
    let placeholders = if text.contains(CLOSE) {
        decode_placeholders(text)
    } else {
        std::borrow::Cow::Borrowed(text)
    };

    if !placeholders.contains('&') {
        return placeholders;
    }
    std::borrow::Cow::Owned(decode_html(&placeholders))
}

fn lookup(name: &str) -> Option<char> {
    NAMED
        .binary_search_by(|&(entry, _)| entry.cmp(name))
        .ok()
        .and_then(|at| NAMED.get(at))
        .and_then(|&(_, code)| char::from_u32(code))
}

fn code_point(value: u64) -> Option<char> {
    if value == 0 || value > 0x10_ffff {
        return None;
    }
    u32::try_from(value).ok().and_then(char::from_u32)
}

fn decode_placeholders(text: &str) -> std::borrow::Cow<'_, str> {
    let mut out = String::new();
    let mut last = 0;
    let mut from = text.find(OPEN);

    while let Some(start) = from {
        let body_start = start + OPEN.len();
        let Some(stop) = text[body_start..].find('\u{b6}').map(|at| body_start + at) else {
            break;
        };
        let after = stop + '\u{b6}'.len_utf8();

        if text[after..].starts_with('\u{df}') {
            let end = after + '\u{df}'.len_utf8();
            let numeric = text[body_start..].starts_with('\u{b0}');
            let body = &text[if numeric {
                body_start + '\u{b0}'.len_utf8()
            } else {
                body_start
            }..stop];

            out.push_str(&text[last..start]);
            if numeric {
                match parse_decimal(body).and_then(code_point) {
                    Some(c) => out.push(c),
                    None => out.push_str(&text[start..end]),
                }
            } else if let Some(c) = lookup(body) {
                out.push(c);
            } else {
                out.push('&');
                out.push_str(body);
                out.push(';');
            }
            last = end;
        }

        from = text[after..].find(OPEN).map(|at| after + at);
    }

    if last == 0 {
        return std::borrow::Cow::Borrowed(text);
    }
    out.push_str(&text[last..]);
    std::borrow::Cow::Owned(out)
}

// Matches `&(?:#(\d+)|#[xX]([0-9a-fA-F]+)|(\w+));`, replacing references that resolve.
fn decode_html(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut at = 0;

    while let Some(found) = text[at..].find('&') {
        let start = at + found;
        let mut end = start + 1;
        let decoded = if bytes.get(end) == Some(&b'#') {
            end += 1;
            if matches!(bytes.get(end), Some(b'x' | b'X')) {
                end += 1;
                let digits = end;
                while bytes.get(end).is_some_and(u8::is_ascii_hexdigit) {
                    end += 1;
                }
                (end > digits).then(|| Some(parse_hex(&text[digits..end])))
            } else {
                let digits = end;
                while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                    end += 1;
                }
                (end > digits).then(|| parse_decimal(&text[digits..end]))
            }
            .map(|value| value.and_then(code_point))
        } else {
            let name = end;
            while bytes
                .get(end)
                .is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_')
            {
                end += 1;
            }
            (end > name).then(|| lookup(&text[name..end]))
        };

        match decoded {
            Some(resolved) if bytes.get(end) == Some(&b';') => {
                if let Some(c) = resolved {
                    out.push_str(&text[last..start]);
                    out.push(c);
                    last = end + 1;
                }
                at = end + 1;
            }
            _ => at = start + 1,
        }
    }

    out.push_str(&text[last..]);
    out
}

// Saturates, so an overlong number is rejected as out of range, as `Number()` gives a huge value.
fn parse_decimal(digits: &str) -> Option<u64> {
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(digits.bytes().fold(0u64, |n, b| {
        n.saturating_mul(10).saturating_add(u64::from(b - b'0'))
    }))
}

fn parse_hex(digits: &str) -> u64 {
    digits.bytes().fold(0u64, |n, b| {
        let d = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            _ => b - b'A' + 10,
        };
        n.saturating_mul(16).saturating_add(u64::from(d))
    })
}
