//! Sanitizes click targets as Mermaid does, and keeps links and images to the schemes a host
//! allows. Port of Pele's `src/util/url.ts`.

use crate::text::entities;
use crate::util::chars::{is_line_end, is_space, is_word};

const BLANK: &str = "about:blank";

/// A link in a drawing, as Pele's `LinkInfo` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LinkInfo {
    /// The sanitized target.
    pub(crate) href: String,
    /// The element the link wraps.
    pub(crate) id: String,
    /// Whether the link points inside the document.
    pub(crate) internal: bool,
}

/// The render options the link and image helpers read.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct UrlOptions<'a> {
    /// Schemes an image may use. `None` allows `http` and `https`.
    pub(crate) image_schemes: Option<&'a [&'a str]>,
    /// `Some(false)` leaves every image out.
    pub(crate) images: Option<bool>,
    /// The `rel` of each link. `None` writes `noopener`, and an empty value writes none.
    pub(crate) link_rel: Option<&'a str>,
    /// Schemes a link may use. `None` allows `http`, `https`, `mailto`, and `tel`.
    pub(crate) link_schemes: Option<&'a [&'a str]>,
    /// `Some(false)` leaves every link out.
    pub(crate) links: Option<bool>,
}

fn space(c: char) -> bool {
    is_space(u32::from(c))
}

fn unit_is(unit: Option<&u16>, ascii: u8) -> bool {
    unit.is_some_and(|&u| u == u16::from(ascii))
}

fn unit_word(unit: Option<&u16>) -> bool {
    unit.is_some_and(|&u| is_word(u32::from(u)))
}

fn unit_hex(unit: Option<&u16>) -> Option<u8> {
    let u = u8::try_from(*unit?).ok()?;
    char::from(u)
        .to_digit(16)
        .and_then(|digit| u8::try_from(digit).ok())
}

// /[\u0000-\u001F\u007F-\u009F -‍﻿]/
fn is_ctrl(u: u16) -> bool {
    u <= 0x1f || (0x7f..=0x9f).contains(&u) || (0x2000..=0x200d).contains(&u) || u == 0xfeff
}

// Whether `units` at `at` start with `ascii`, ignoring ASCII case.
fn starts_ignore_case(units: &[u16], at: usize, ascii: &str) -> bool {
    ascii.bytes().enumerate().all(|(i, b)| {
        units
            .get(at + i)
            .and_then(|&u| u8::try_from(u).ok())
            .is_some_and(|u| u.eq_ignore_ascii_case(&b))
    })
}

// `decodeURIComponent`, or `None` where it throws a URIError.
fn decode_uri(units: &[u16]) -> Option<Vec<u16>> {
    let byte_at = |at: usize| -> Option<u8> {
        if !unit_is(units.get(at), b'%') {
            return None;
        }
        Some(unit_hex(units.get(at + 1))? * 16 + unit_hex(units.get(at + 2))?)
    };

    let mut out = Vec::with_capacity(units.len());
    let mut k = 0;
    while let Some(&unit) = units.get(k) {
        if unit != u16::from(b'%') {
            out.push(unit);
            k += 1;
            continue;
        }
        let first = byte_at(k)?;
        k += 3;
        if first < 0x80 {
            out.push(u16::from(first));
            continue;
        }

        let n = first.leading_ones() as usize;
        if n == 1 || n > 4 {
            return None;
        }
        let mut octets = [first, 0, 0, 0];
        for slot in octets.iter_mut().take(n).skip(1) {
            let next = byte_at(k)?;
            if next & 0xc0 != 0x80 {
                return None;
            }
            *slot = next;
            k += 3;
        }
        let text = std::str::from_utf8(octets.get(..n)?).ok()?;
        out.extend(text.encode_utf16());
    }
    Some(out)
}

fn decode(units: &[u16]) -> Vec<u16> {
    decode_uri(units).unwrap_or_else(|| units.to_vec())
}

// `.replace(RE_CTRL, '')`, and whether anything matched.
fn remove_ctrl(units: &[u16]) -> (Vec<u16>, bool) {
    let out = units
        .iter()
        .copied()
        .filter(|&u| !is_ctrl(u))
        .collect::<Vec<_>>();
    let found = out.len() != units.len();
    (out, found)
}

// `.replace(/&#(\w+)(^\w|;)?/g, (_m, dec) => String.fromCharCode(dec))`. `^\w` cannot match
// after `&#`, so the group is an optional semicolon.
fn replace_entities(units: &[u16]) -> (Vec<u16>, bool) {
    let mut out = Vec::with_capacity(units.len());
    let mut found = false;
    let mut at = 0;
    while let Some(&unit) = units.get(at) {
        if unit_is(Some(&unit), b'&') && unit_is(units.get(at + 1), b'#') {
            let start = at + 2;
            let mut end = start;
            while unit_word(units.get(end)) {
                end += 1;
            }
            if end > start {
                let word = String::from_utf16_lossy(units.get(start..end).unwrap_or(&[]));
                out.push(char_code(word_number(&word)));
                found = true;
                at = if unit_is(units.get(end), b';') {
                    end + 1
                } else {
                    end
                };
                continue;
            }
        }
        out.push(unit);
        at += 1;
    }
    (out, found)
}

// JavaScript's `Number` of a `\w+` string.
fn word_number(word: &str) -> f64 {
    let prefix = word.get(..2).map(str::to_ascii_lowercase);
    let radix = match prefix.as_deref() {
        Some("0x") => Some(16),
        Some("0o") => Some(8),
        Some("0b") => Some(2),
        _ => None,
    };
    if let Some(base) = radix {
        let digits = word.get(2..).unwrap_or("");
        if digits.is_empty() || !digits.chars().all(|c| c.is_digit(base)) {
            return f64::NAN;
        }
        // Past 2^53 the value only feeds `fromCharCode`, which keeps its low 16 bits.
        return digits.chars().fold(0.0, |acc, c| {
            acc * f64::from(base) + f64::from(c.to_digit(base).unwrap_or(0))
        });
    }
    if word == "Infinity" {
        return f64::INFINITY;
    }

    let mantissa = word.bytes().take_while(u8::is_ascii_digit).count();
    let rest = word.get(mantissa..).unwrap_or("");
    let exponent = rest
        .strip_prefix(['e', 'E'])
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()));
    if mantissa > 0 && (rest.is_empty() || exponent) {
        return crate::util::float::parse(word).unwrap_or(f64::NAN);
    }
    f64::NAN
}

// `String.fromCharCode`, which keeps the value modulo 2^16.
fn char_code(value: f64) -> u16 {
    if !value.is_finite() {
        return 0;
    }
    let wrapped = value.trunc() % 65536.0;
    let wrapped = if wrapped < 0.0 {
        wrapped + 65536.0
    } else {
        wrapped
    };
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "wrapped into 0..65536"
    )]
    let code = wrapped as u16;
    code
}

// `.replace(/&(newline|tab);/gi, '')`.
fn remove_ctrl_entities(units: &[u16]) -> (Vec<u16>, bool) {
    let mut out = Vec::with_capacity(units.len());
    let mut found = false;
    let mut at = 0;
    while let Some(&unit) = units.get(at) {
        let length = ["&newline;", "&tab;"]
            .iter()
            .find(|name| starts_ignore_case(units, at, name))
            .map(|name| name.len());
        if let Some(length) = length {
            found = true;
            at += length;
            continue;
        }
        out.push(unit);
        at += 1;
    }
    (out, found)
}

// `.replace(/(\\|%5[cC])((%(6[eE]|72|74))|[nrt])/g, '')`.
fn remove_ws_escapes(units: &[u16]) -> (Vec<u16>, bool) {
    let length_at = |at: usize| -> Option<usize> {
        let slash = if unit_is(units.get(at), b'\\') {
            1
        } else if starts_ignore_case(units, at, "%5c") {
            3
        } else {
            return None;
        };
        let next = at + slash;
        if ["%6e", "%72", "%74"]
            .iter()
            .any(|code| starts_ignore_case(units, next, code))
        {
            return Some(slash + 3);
        }
        units
            .get(next)
            .and_then(|&u| u8::try_from(u).ok())
            .filter(|b| matches!(b, b'n' | b'r' | b't'))
            .map(|_| slash + 1)
    };

    let mut out = Vec::with_capacity(units.len());
    let mut found = false;
    let mut at = 0;
    while let Some(&unit) = units.get(at) {
        if let Some(length) = length_at(at) {
            found = true;
            at += length;
            continue;
        }
        out.push(unit);
        at += 1;
    }
    (out, found)
}

fn trim_units(units: &[u16]) -> &[u16] {
    let spaced = |u: &u16| is_space(u32::from(*u));
    let start = units.iter().position(|u| !spaced(u)).unwrap_or(units.len());
    let end = units
        .iter()
        .rposition(|u| !spaced(u))
        .map_or(start, |at| at + 1);
    units.get(start..end).unwrap_or(&[])
}

/// Follows the algorithm of `@braintree/sanitize-url`, which Mermaid applies to click targets.
/// A dangerous or malformed URL becomes `about:blank`.
pub(crate) fn sanitize_url(url: &str) -> String {
    if url.is_empty() {
        return BLANK.to_owned();
    }

    // The loop runs over UTF-16 units so entity codes can pair surrogates as JavaScript does.
    let mut decoded = decode(&url.trim_matches(space).encode_utf16().collect::<Vec<_>>());
    let mut rounds = 0;
    loop {
        // Each round peels one layer of encoding. A URL wrapped this many times is not a real one.
        rounds += 1;
        if rounds > 32 {
            return BLANK.to_owned();
        }
        let (step, _) = remove_ctrl(&decoded);
        let (step, _) = replace_entities(&step);
        let (step, _) = remove_ctrl_entities(&step);
        let (step, _) = remove_ctrl(&step);
        let (step, _) = remove_ws_escapes(&step);
        decoded = decode(trim_units(&step));

        let more = remove_ctrl(&decoded).1
            || replace_entities(&decoded).1
            || remove_ctrl_entities(&decoded).1
            || remove_ws_escapes(&decoded).1;
        if !more {
            break;
        }
    }

    // A lone surrogate becomes U+FFFD here, where JavaScript would keep it.
    let decoded = String::from_utf16_lossy(&decoded);
    if decoded.is_empty() {
        return BLANK.to_owned();
    }
    if decoded.starts_with(['.', '/']) {
        return decoded;
    }

    let trimmed = decoded.trim_start_matches(space);
    let Some(scheme) = scheme(trimmed) else {
        return decoded;
    };
    let lower = scheme
        .chars()
        .flat_map(char::to_lowercase)
        .collect::<String>();
    let protocol = lower.trim_matches(space);
    if invalid_protocol(protocol) {
        return BLANK.to_owned();
    }

    let normalized = trimmed
        .chars()
        .map(|c| if c == '\\' { '/' } else { c })
        .collect::<String>();
    if protocol == "mailto:" || protocol.contains("://") {
        return normalized;
    }
    if protocol == "http:" || protocol == "https:" {
        return parse_http(&normalized).unwrap_or_else(|| BLANK.to_owned());
    }
    normalized
}

// The first match of `/^.+(:|&colon;)/gim`: on the first line that has one, the text up to its
// last colon or `&colon;` after the first character.
fn scheme(text: &str) -> Option<&str> {
    let mut start = 0;
    loop {
        let rest = text.get(start..)?;
        let (length, width) = rest
            .char_indices()
            .find(|&(_, c)| is_line_end(u32::from(c)))
            .map_or((rest.len(), 0), |(at, c)| (at, c.len_utf8()));
        let line = rest.get(..length).unwrap_or("");

        let mut end = None;
        for (p, _) in line.char_indices().skip(1) {
            let tail = line.get(p..).unwrap_or("");
            if tail.starts_with(':') {
                end = Some(p + 1);
            } else if tail
                .get(..7)
                .is_some_and(|head| head.eq_ignore_ascii_case("&colon;"))
            {
                end = Some(p + 7);
            }
        }
        if let Some(end) = end {
            return line.get(..end);
        }
        if width == 0 {
            return None;
        }
        start += length + width;
    }
}

// /^([^\w]*)(javascript|data|vbscript)/im
fn invalid_protocol(protocol: &str) -> bool {
    let rest = protocol.trim_start_matches(|c: char| !is_word(u32::from(c)));
    ["javascript", "data", "vbscript"].iter().any(|name| {
        rest.get(..name.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(name))
    })
}

// Parses an `http:` or `https:` URL and serializes it as the WHATWG URL Standard does, for
// `new URL(url).toString()`.
fn parse_http(input: &str) -> Option<String> {
    let input = input
        .trim_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect::<String>();
    let colon = input.find(':')?;
    let scheme = input.get(..colon)?;
    let (scheme, default_port) = if scheme.eq_ignore_ascii_case("http") {
        ("http", 80)
    } else if scheme.eq_ignore_ascii_case("https") {
        ("https", 443)
    } else {
        return None;
    };

    let rest = input.get(colon + 1..)?.trim_start_matches(['/', '\\']);
    let authority_end = rest.find(['/', '\\', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    let (credentials, host_port) = match authority.rfind('@') {
        Some(at) => (authority.get(..at), authority.get(at + 1..)?),
        None => (None, authority),
    };

    // Only a URL whose sole colon on its first line ends the scheme reaches this parser, so a
    // bracketed IPv6 host, which needs colons, is never valid here. Any other `[` is forbidden.
    if host_port.contains('[') {
        return None;
    }
    let (host, port) = match host_port.find(':') {
        Some(at) => (host_port.get(..at)?, Some(host_port.get(at + 1..)?)),
        None => (host_port, None),
    };
    if host.is_empty() {
        return None;
    }
    let host = parse_host(host)?;
    let port = match port {
        Some(digits) if !digits.is_empty() => {
            if !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let value = digits.bytes().fold(0u32, |acc, b| {
                acc.saturating_mul(10).saturating_add(u32::from(b - b'0'))
            });
            if value > 65535 {
                return None;
            }
            (value != default_port).then_some(value)
        }
        Some(_) | None => None,
    };

    let mut out = String::from(scheme);
    out.push_str("://");
    if let Some(credentials) = credentials {
        let (user, password) = credentials.split_once(':').unwrap_or((credentials, ""));
        if !user.is_empty() || !password.is_empty() {
            encode(&mut out, user, userinfo_set);
            if !password.is_empty() {
                out.push(':');
                encode(&mut out, password, userinfo_set);
            }
            out.push('@');
        }
    }
    out.push_str(&host);
    if let Some(port) = port {
        out.push(':');
        crate::error::push_u32(&mut out, port);
    }

    let (before, fragment) = match tail.split_once('#') {
        Some((before, fragment)) => (before, Some(fragment)),
        None => (tail, None),
    };
    let (path, query) = match before.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (before, None),
    };

    let mut segments: Vec<String> = Vec::new();
    let path = path.strip_prefix(['/', '\\']).unwrap_or(path);
    let count = path.split(['/', '\\']).count();
    for (i, raw) in path.split(['/', '\\']).enumerate() {
        let at_end = i + 1 == count;
        let mut buffer = String::with_capacity(raw.len());
        encode(&mut buffer, raw, path_set);
        if is_double_dot(&buffer) {
            segments.pop();
            if at_end {
                segments.push(String::new());
            }
        } else if is_single_dot(&buffer) {
            if at_end {
                segments.push(String::new());
            }
        } else {
            segments.push(buffer);
        }
    }
    for segment in &segments {
        out.push('/');
        out.push_str(segment);
    }

    if let Some(query) = query {
        out.push('?');
        encode(&mut out, query, |b| query_set(b) || b == b'\'');
    }
    if let Some(fragment) = fragment {
        out.push('#');
        encode(&mut out, fragment, fragment_set);
    }
    Some(out)
}

fn is_single_dot(segment: &str) -> bool {
    segment == "." || segment.eq_ignore_ascii_case("%2e")
}

fn is_double_dot(segment: &str) -> bool {
    ["..", ".%2e", "%2e.", "%2e%2e"]
        .iter()
        .any(|dots| segment.eq_ignore_ascii_case(dots))
}

fn c0_control_set(b: u8) -> bool {
    !(0x20..=0x7e).contains(&b)
}

fn fragment_set(b: u8) -> bool {
    c0_control_set(b) || matches!(b, b' ' | b'"' | b'<' | b'>' | b'`')
}

fn query_set(b: u8) -> bool {
    c0_control_set(b) || matches!(b, b' ' | b'"' | b'#' | b'<' | b'>')
}

fn path_set(b: u8) -> bool {
    query_set(b) || matches!(b, b'?' | b'^' | b'`' | b'{' | b'}')
}

fn userinfo_set(b: u8) -> bool {
    path_set(b) || matches!(b, b'/' | b':' | b';' | b'=' | b'@' | b'['..=b'^' | b'|')
}

fn encode(out: &mut String, text: &str, set: impl Fn(u8) -> bool) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for b in text.bytes() {
        if set(b) {
            out.push('%');
            out.push(char::from(HEX[usize::from(b >> 4)]));
            out.push(char::from(HEX[usize::from(b & 15)]));
        } else {
            // Every set encodes bytes above 0x7e, so what is left is ASCII.
            out.push(char::from(b));
        }
    }
}

fn parse_host(input: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(input.len());
    let raw = input.as_bytes();
    let mut at = 0;
    while let Some(&b) = raw.get(at) {
        let hex = |i: usize| raw.get(i).and_then(|&h| char::from(h).to_digit(16));
        if b == b'%'
            && let (Some(high), Some(low)) = (hex(at + 1), hex(at + 2))
        {
            bytes.push(u8::try_from(high * 16 + low).unwrap_or(0));
            at += 3;
        } else {
            bytes.push(b);
            at += 1;
        }
    }

    let domain = String::from_utf8_lossy(&bytes);
    let ascii = domain_to_ascii(&domain)?;
    if ascii.chars().any(forbidden_domain) {
        return None;
    }
    if ends_in_number(&ascii) {
        return ipv4(&ascii);
    }
    Some(ascii)
}

// ponytail: lowercases, Punycode-encodes, and checks `xn--` labels decode, without UTS 46 mapping,
// normalization, or validity tables. Add the tables if hosts beyond plain letters matter.
fn domain_to_ascii(domain: &str) -> Option<String> {
    // UTS 46 maps non-ASCII spaces to a forbidden space and disallows line separators.
    if domain.chars().any(|c| !c.is_ascii() && space(c)) {
        return None;
    }

    let mapped = domain
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            '\u{3002}' | '\u{ff0e}' | '\u{ff61}' => '.',
            c => c,
        })
        .collect::<String>();

    let mut out = String::with_capacity(mapped.len());
    for (i, label) in mapped.split('.').enumerate() {
        if i > 0 {
            out.push('.');
        }
        let encoded = label.starts_with("xn--");
        if encoded && (!label.is_ascii() || !punycode_label(label.get(4..)?)) {
            return None;
        }
        if label.is_ascii() {
            out.push_str(label);
        } else {
            out.push_str("xn--");
            punycode(label, &mut out)?;
        }
    }
    (!out.is_empty()).then_some(out)
}

fn forbidden_domain(c: char) -> bool {
    c <= '\u{1f}'
        || c == '\u{7f}'
        || matches!(
            c,
            ' ' | '#' | '%' | '/' | ':' | '<' | '>' | '?' | '@' | '[' | '\\' | ']' | '^' | '|'
        )
}

// Encodes a label with RFC 3492 Punycode.
const BASE: u32 = 36;

// Punycode's bias adaptation (RFC 3492, section 6.1).
fn adapt(delta: u32, points: u32, first: bool) -> u32 {
    let mut delta = if first { delta / 700 } else { delta / 2 };
    delta += delta / points;
    let mut k = 0;
    while delta > (BASE - 1) * 26 / 2 {
        delta /= BASE - 1;
        k += BASE;
    }
    k + BASE * delta / (delta + 38)
}

fn threshold(step: u32, bias: u32) -> u32 {
    if step <= bias {
        1
    } else if step >= bias + 26 {
        26
    } else {
        step - bias
    }
}

// Whether the part of an `xn--` label after the prefix decodes to a label that is not plain ASCII.
// Code points below U+00A0 stand in for the UTS 46 validity tables.
fn punycode_label(encoded: &str) -> bool {
    let (basic, deltas) = encoded.rsplit_once('-').unwrap_or(("", encoded));
    let mut output = basic.chars().map(u32::from).collect::<Vec<_>>();
    let mut code: u32 = 0x80;
    let mut bias = 72;
    let mut i: u32 = 0;
    let mut digits = deltas.bytes();
    let mut pending = digits.len() > 0;
    while pending {
        let old = i;
        let mut weight: u32 = 1;
        let mut step = BASE;
        loop {
            let Some(digit) = digits.next().and_then(|b| match b {
                b'a'..=b'z' => Some(u32::from(b - b'a')),
                b'A'..=b'Z' => Some(u32::from(b - b'A')),
                b'0'..=b'9' => Some(u32::from(b - b'0') + 26),
                _ => None,
            }) else {
                return false;
            };
            let Some(next) = digit.checked_mul(weight).and_then(|add| i.checked_add(add)) else {
                return false;
            };
            i = next;
            let t = threshold(step, bias);
            if digit < t {
                break;
            }
            let Some(next) = weight.checked_mul(BASE - t) else {
                return false;
            };
            weight = next;
            step += BASE;
        }
        let Ok(points) = u32::try_from(output.len() + 1) else {
            return false;
        };
        bias = adapt(i - old, points, old == 0);
        let Some(next) = code.checked_add(i / points) else {
            return false;
        };
        code = next;
        i %= points;
        if char::from_u32(code).is_none() {
            return false;
        }
        output.insert(usize::try_from(i).unwrap_or(output.len()), code);
        i += 1;
        pending = digits.len() > 0;
    }
    output.iter().any(|&c| c >= 0x80) && output.iter().all(|&c| !(0x80..0xa0).contains(&c))
}

fn punycode(label: &str, out: &mut String) -> Option<()> {
    // Digits 0 to 25 are `a` to `z`, and 26 to 35 are `0` to `9`.
    let digit = |d: u32| match d {
        0..26 => char::from_u32(u32::from(b'a') + d),
        _ => char::from_u32(u32::from(b'0') + d - 26),
    };

    let input = label.chars().map(u32::from).collect::<Vec<_>>();
    let length = u32::try_from(input.len()).ok()?;
    let basic = u32::try_from(input.iter().filter(|&&c| c < 0x80).count()).ok()?;
    for &c in &input {
        if c < 0x80 {
            out.push(char::from_u32(c)?);
        }
    }
    if basic > 0 {
        out.push('-');
    }

    let mut code = 0x80;
    let mut delta: u32 = 0;
    let mut bias = 72;
    let mut handled = basic;
    while handled < length {
        let next = input.iter().copied().filter(|&c| c >= code).min()?;
        delta = delta.checked_add((next - code).checked_mul(handled + 1)?)?;
        code = next;
        for &c in &input {
            if c < code {
                delta = delta.checked_add(1)?;
            }
            if c == code {
                let mut rest = delta;
                let mut step = BASE;
                loop {
                    let t = threshold(step, bias);
                    if rest < t {
                        break;
                    }
                    out.push(digit(t + (rest - t) % (BASE - t))?);
                    rest = (rest - t) / (BASE - t);
                    step += BASE;
                }
                out.push(digit(rest)?);
                bias = adapt(delta, handled + 1, handled == basic);
                delta = 0;
                handled += 1;
            }
        }
        delta = delta.checked_add(1)?;
        code += 1;
    }
    Some(())
}

fn ipv4_number(text: &str) -> Option<u64> {
    if text.is_empty() {
        return None;
    }
    let (base, digits) = match text.get(..2) {
        Some("0x" | "0X") => (16, text.get(2..).unwrap_or("")),
        _ if text.len() >= 2 && text.starts_with('0') => (8, text.get(1..).unwrap_or("")),
        _ => (10, text),
    };
    digits.chars().try_fold(0u64, |acc, c| {
        Some(
            acc.saturating_mul(u64::from(base))
                .saturating_add(u64::from(c.to_digit(base)?)),
        )
    })
}

fn ends_in_number(domain: &str) -> bool {
    let mut labels = domain.split('.').collect::<Vec<_>>();
    if labels.last() == Some(&"") {
        if labels.len() == 1 {
            return false;
        }
        labels.pop();
    }
    let last = labels.last().copied().unwrap_or("");
    (!last.is_empty() && last.bytes().all(|b| b.is_ascii_digit())) || ipv4_number(last).is_some()
}

fn ipv4(domain: &str) -> Option<String> {
    let mut parts = domain.split('.').collect::<Vec<_>>();
    if parts.len() > 1 && parts.last() == Some(&"") {
        parts.pop();
    }
    if parts.len() > 4 {
        return None;
    }
    let numbers = parts
        .iter()
        .map(|part| ipv4_number(part))
        .collect::<Option<Vec<_>>>()?;
    let (&last, init) = numbers.split_last()?;
    if init.iter().any(|&n| n > 255) {
        return None;
    }
    let width = u32::try_from(5 - numbers.len()).ok()?;
    if last >= 256u64.pow(width) {
        return None;
    }

    let mut address = last;
    for (i, &n) in init.iter().enumerate() {
        address += n * 256u64.pow(u32::try_from(3 - i).ok()?);
    }
    let mut out = String::with_capacity(15);
    for shift in [24, 16, 8, 0] {
        if shift != 24 {
            out.push('.');
        }
        crate::error::push_u32(&mut out, u32::try_from((address >> shift) & 255).ok()?);
    }
    Some(out)
}

/// A URL as written in a diagram: entity codes are read first, so they cannot hide a scheme.
pub(crate) fn safe_url(url: &str) -> String {
    sanitize_url(&entities::decode(url))
}

const LINK_SCHEMES: [&str; 4] = ["http", "https", "mailto", "tel"];
const IMAGE_SCHEMES: [&str; 2] = ["http", "https"];

// Stricter than Mermaid, which lets any scheme but three through: a diagram in a shared note
// could otherwise open a local file, a network share, or another application.
fn allow<'a>(url: &'a str, schemes: &[&str]) -> Option<&'a str> {
    let bytes = url.as_bytes();
    if matches!(bytes.first(), Some(b'/' | b'\\')) && matches!(bytes.get(1), Some(b'/' | b'\\')) {
        return None;
    }

    let name_length = if bytes.first().is_some_and(u8::is_ascii_alphabetic) {
        1 + bytes
            .iter()
            .skip(1)
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'.' | b'-'))
            .count()
    } else {
        0
    };
    if name_length == 0 || bytes.get(name_length) != Some(&b':') {
        return Some(url);
    }
    let name = url.get(..name_length)?;
    schemes
        .iter()
        .any(|scheme| {
            scheme
                .bytes()
                .eq(name.bytes().map(|b| b.to_ascii_lowercase()))
        })
        .then_some(url)
}

/// Keeps a URL that `sanitize_url` has passed when it is relative or its scheme is one the host
/// allows for links. A refused link is left out of the drawing: written as `about:blank`, it
/// would still navigate the page when clicked.
pub(crate) fn link_url<'a>(url: &'a str, options: &UrlOptions<'_>) -> Option<&'a str> {
    if options.links == Some(false) {
        return None;
    }
    allow(url, options.link_schemes.unwrap_or(&LINK_SCHEMES))
}

/// Keeps a URL that `sanitize_url` has passed when it is relative or its scheme is one the host
/// allows for images.
pub(crate) fn image_url<'a>(url: &'a str, options: &UrlOptions<'_>) -> Option<&'a str> {
    if options.images == Some(false) {
        return None;
    }
    allow(url, options.image_schemes.unwrap_or(&IMAGE_SCHEMES))
}

/// Wraps `body` in a link to `url` and records it in `links`, or returns `body` alone when the
/// link is refused. `attrs` is written into the `<a>` tag as is.
pub(crate) fn linked(
    body: &str,
    url: &str,
    id: &str,
    links: &mut Vec<LinkInfo>,
    options: &UrlOptions<'_>,
    attrs: &str,
) -> String {
    let Some(href) = link_url(url, options) else {
        return body.to_owned();
    };
    links.push(LinkInfo {
        href: href.to_owned(),
        id: id.to_owned(),
        internal: false,
    });

    let mut out = String::with_capacity(body.len() + href.len() + 32);
    out.push_str("<a href=\"");
    push_escaped(&mut out, href);
    out.push('"');
    out.push_str(attrs);
    out.push_str(&rel_attr(options));
    out.push('>');
    out.push_str(body);
    out.push_str("</a>");
    out
}

/// The ` rel="..."` attribute for links, or nothing when the host sets an empty `rel`.
pub(crate) fn rel_attr(options: &UrlOptions<'_>) -> String {
    let rel = options.link_rel.unwrap_or("noopener");
    if rel.is_empty() {
        return String::new();
    }
    let mut out = String::from(" rel=\"");
    push_escaped(&mut out, rel);
    out.push('"');
    out
}

// Pele's `esc`: escapes XML text and drops the control characters XML does not allow. Kept here
// until `svg::builder` is ported.
fn push_escaped(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{7f}' => {}
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    //! Expectations come from running Pele's URL helpers at 458322a, over hand-picked URLs and every
    //! quoted string on a `click`, `href`, or `link` line in `test/vectors`.

    use super::{
        LinkInfo, UrlOptions, image_url, link_url, linked, rel_attr, safe_url, sanitize_url,
    };

    fn options(name: &str) -> UrlOptions<'static> {
        match name {
            "default" => UrlOptions::default(),
            "off" => UrlOptions {
                images: Some(false),
                links: Some(false),
                ..UrlOptions::default()
            },
            "ftp" => UrlOptions {
                image_schemes: Some(&["data"]),
                link_schemes: Some(&["ftp", "HTTP"]),
                ..UrlOptions::default()
            },
            "empty" => UrlOptions {
                link_rel: Some(""),
                ..UrlOptions::default()
            },
            "quoted" => UrlOptions {
                link_rel: Some("a\"b&c\u{1}"),
                ..UrlOptions::default()
            },
            _ => panic!("unknown options {name}"),
        }
    }

    #[test]
    fn sanitize_url_matches_pele() {
        for &(url, expected) in SANITIZE {
            assert_eq!(sanitize_url(url), expected, "{url:?}");
        }
    }

    #[test]
    fn safe_url_matches_pele() {
        for &(url, expected) in SAFE {
            assert_eq!(safe_url(url), expected, "{url:?}");
        }
    }

    #[test]
    fn link_and_image_url_match_pele() {
        for &(case, expected) in ALLOW {
            let (name, url) = case.split_once('|').unwrap_or_default();
            let options = options(name);
            let actual = format!(
                "{}|{}",
                link_url(url, &options).unwrap_or("null"),
                image_url(url, &options).unwrap_or("null"),
            );
            assert_eq!(actual, expected, "{case:?}");
        }
    }

    #[test]
    fn linked_matches_pele() {
        for &(case, expected) in LINKED {
            let (name, url) = case.split_once('|').unwrap_or_default();
            let options = options(name);
            let mut links: Vec<LinkInfo> = Vec::new();
            let html = linked("<g/>", url, "n1", &mut links, &options, " class=\"x\"");
            let links = links
                .iter()
                .map(|link| {
                    format!(
                        "{{\"id\":\"{}\",\"href\":\"{}\",\"internal\":{}}}",
                        link.id,
                        link.href.split('"').collect::<Vec<_>>().join("\\\""),
                        link.internal
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            let actual = format!("{html}|[{links}]|{}", rel_attr(&options));
            assert_eq!(actual, expected, "{case:?}");
        }
    }

    const SANITIZE: &[(&str, &str)] = &[
        ("https://example.com", "https://example.com/"),
        (
            "http://EXAMPLE.com/Path?q=1#frag",
            "http://example.com/Path?q=1#frag",
        ),
        ("click.html", "click.html"),
        ("./relative/path", "./relative/path"),
        ("/absolute/path", "/absolute/path"),
        ("mailto:someone@example.com", "mailto:someone@example.com"),
        (
            "obsidian://open?vault=Notes&file=Idea",
            "obsidian://open?vault=Notes&file=Idea",
        ),
        ("javascript:alert(1)", "about:blank"),
        ("JaVaScRiPt:alert(1)", "about:blank"),
        ("  javascript:alert(1)", "about:blank"),
        ("java\nscript:alert(1)", "about:blank"),
        ("jav&#x09;ascript:alert(1)", "about:blank"),
        ("&#106;avascript:alert(1)", "about:blank"),
        ("data:text/html,<script>alert(1)</script>", "about:blank"),
        ("vbscript:msgbox(1)", "about:blank"),
        ("javascript&colon;alert(1)", "about:blank"),
        ("%6A%61%76%61%73%63%72%69%70%74:alert(1)", "about:blank"),
        ("https://example.com/a b", "https://example.com/a%20b"),
        ("http://", "about:blank"),
        ("ftp://example.com/file", "ftp://example.com/file"),
        ("www.example.com", "www.example.com"),
        ("C:\\Users\\file.txt", "C:/Users/file.txt"),
        ("\\\\server\\share", "\\\\server\\share"),
        ("note name with spaces", "note name with spaces"),
        ("#heading", "#heading"),
        ("?query=1", "?query=1"),
        ("", "about:blank"),
        ("about:blank", "about:blank"),
        ("tel:+123456", "tel:+123456"),
        (
            "https://example.com/%E0%A4%A",
            "https://example.com/%E0%A4%A",
        ),
        (
            "https://a.com/^{}`|'\"<>[]?q=^{}'\"<>`|#f^{}`'\"<> x",
            "https://a.com/%5E%7B%7D%60|'%22%3C%3E[]?q=^{}%27%22%3C%3E`|#f^{}%60'%22%3C%3E%20x",
        ),
        ("https://u:p@a.com:80/x", "https://u:p@a.com:80/x"),
        ("https://u@b:c@A.com:443/", "https://u@b:c@A.com:443/"),
        ("http://0x7f.1/", "http://127.0.0.1/"),
        ("http://[::1]/", "http://[::1]/"),
        ("http://münchen.de/ä", "http://xn--mnchen-3ya.de/%C3%A4"),
        ("HTTP:EXAMPLE.com", "http://example.com/"),
        ("http://a/b/../c/./d/%2e%2E/e", "http://a/c/e"),
        ("http://a.b./", "http://a.b./"),
        ("http://xn--a/", "about:blank"),
        ("http:\\\\a\\b", "http://a/b"),
        ("http://a b/", "about:blank"),
        ("http://a%41b/", "http://aab/"),
        ("http://1.2.3.256/", "about:blank"),
        ("http://999999999/", "http://59.154.201.255/"),
        ("http://a/?", "http://a/?"),
        ("http://a/#", "http://a/#"),
        ("http://%E2%80%A8/", "about:blank"),
        ("http://a.0x/", "about:blank"),
        ("http://u@a.com/x", "http://u@a.com/x"),
        ("http://@a.com/", "http://a.com/"),
        ("http://u@/x", "about:blank"),
        ("http:a", "http://a/"),
        ("http:/a", "http://a/"),
        ("http:///a", "http://a/"),
        ("https://A.COM", "https://a.com/"),
        ("http://a/..", "http://a/"),
        ("http://a/.", "http://a/"),
        ("http://a/b/..", "http://a/"),
        ("http://a/b/.?x", "http://a/b/?x"),
        ("http://a/%2e%2e/b", "http://a/b"),
        ("http://a/x?é#é", "http://a/x?%C3%A9#%C3%A9"),
        ("http://a.com/日本", "http://a.com/%E6%97%A5%E6%9C%AC"),
        ("http://日本.jp/", "http://xn--wgv71a.jp/"),
        ("http://ÉXAMPLE.com/", "http://xn--xample-9ua.com/"),
        ("http://a%2Fb/", "http://a/b/"),
        ("http://a%25b/", "about:blank"),
        ("http://0/", "http://0.0.0.0/"),
        ("http://0x/", "http://0.0.0.0/"),
        ("http://1.2/", "http://1.0.0.2/"),
        ("http://1.2.3/", "http://1.2.0.3/"),
        ("http://0300.0250.1.1/", "http://192.168.1.1/"),
        ("http://4294967296/", "about:blank"),
        ("http://1.2.3.4.5/", "about:blank"),
        ("http://1..2/", "about:blank"),
        ("http://a.1/", "about:blank"),
        ("http://1.a/", "http://1.a/"),
        ("http://a。b/", "http://a.b/"),
        ("http://a\u{2028}:80/", "about:blank"),
        ("http://u\u{2028}@a:80/", "http://u%E2%80%A8@a/"),
        ("https://u\u{2028}@a:443/x", "https://u%E2%80%A8@a/x"),
        ("http://a\u{2028}b/", "about:blank"),
        ("http://a/\u{2028}x:y", "http://a/%E2%80%A8x:y"),
        ("a\u{2028}http://x", "about:blank"),
        ("  https://example.com  ", "https://example.com/"),
        ("https://example.com/%20", "https://example.com/"),
        ("%2568ttp://x", "http://x/"),
        ("&#104;ttp://x", "http://x/"),
        ("&#x6A;avascript:x", "avascript:x"),
        ("&#0000106avascript:x", ":x"),
        ("&#1e2;", "d"),
        ("&#Infinity;x", "x"),
        ("&#65536;", "about:blank"),
        ("&#55357;&#56832;", "😀"),
        ("jav&NewLine;ascript:x", "about:blank"),
        ("jav&tab;ascript:x", "about:blank"),
        ("a\\nb", "ab"),
        ("a%5Cnb", "ab"),
        ("a%5c%6Eb", "ab"),
        ("a\\%72b", "ab"),
        ("a\\xb", "a\\xb"),
        ("%", "%"),
        ("%zz", "%zz"),
        ("%C3", "%C3"),
        ("%C3%28", "%C3%28"),
        ("%F0%9F%98%80", "😀"),
        ("%ED%A0%80", "%ED%A0%80"),
        ("%C0%80", "%C0%80"),
        ("\u{0}javascript:x", "about:blank"),
        ("\u{200b}javascript:x", "about:blank"),
        ("\u{feff}javascript:x", "about:blank"),
        ("İjavascript:x", "İjavascript:x"),
        ("Kjavascript:x", "Kjavascript:x"),
        ("-javascript:x", "about:blank"),
        ("_javascript:x", "_javascript:x"),
        (" data:x", "about:blank"),
        ("DATA:x", "about:blank"),
        ("x:javascript:y", "x:javascript:y"),
        ("mailto:a", "mailto:a"),
        ("MAILTO:a", "MAILTO:a"),
        ("tel:1", "tel:1"),
        ("https:", "about:blank"),
        ("https:x", "https://x/"),
        ("https:x:y", "https:x:y"),
        ("http:x&colon;y", "http:x&colon;y"),
        ("a&COLON;b", "a&COLON;b"),
        ("a:", "a:"),
        (":a", ":a"),
        ("..", ".."),
        (".", "."),
        ("/", "/"),
        ("//evil.com", "//evil.com"),
        ("\\\\evil", "\\\\evil"),
        ("foo/bar:baz", "foo/bar:baz"),
        ("&#46;/x", "./x"),
        ("%2e/x", "./x"),
        ("&#x2F;&#x2F;evil", "evil"),
        ("https://example.com/aaaaa", "https://example.com/aaaaa"),
        ("%25%25%2541", "%%%41"),
        (
            "%2525252525252525252525252525252525252525252525252525252525252525252541",
            "%252525252525252525252525252525252525252525252525252525252525252541",
        ),
        ("https://www.github.com", "https://www.github.com/"),
        (
            "This is a tooltip for a link",
            "This is a tooltip for a link",
        ),
        (
            "This is a tooltip for a callback",
            "This is a tooltip for a callback",
        ),
        ("This is a link", "This is a link"),
        ("Callback tooltip", "Callback tooltip"),
        ("Communication link", "Communication link"),
        ("Tooltip for a callback", "Tooltip for a callback"),
        ("Open this in a new tab", "Open this in a new tab"),
        ("Dashboard", "Dashboard"),
        (
            "https://dashboard.contoso.com/alice",
            "https://dashboard.contoso.com/alice",
        ),
        ("Wiki", "Wiki"),
        (
            "https://wiki.contoso.com/alice",
            "https://wiki.contoso.com/alice",
        ),
        (
            "https://dashboard.contoso.com/john",
            "https://dashboard.contoso.com/john",
        ),
        (
            "https://wiki.contoso.com/john",
            "https://wiki.contoso.com/john",
        ),
    ];
    const SAFE: &[(&str, &str)] = &[
        ("https://example.com", "https://example.com/"),
        (
            "http://EXAMPLE.com/Path?q=1#frag",
            "http://example.com/Path?q=1#frag",
        ),
        ("click.html", "click.html"),
        ("./relative/path", "./relative/path"),
        ("/absolute/path", "/absolute/path"),
        ("mailto:someone@example.com", "mailto:someone@example.com"),
        (
            "obsidian://open?vault=Notes&file=Idea",
            "obsidian://open?vault=Notes&file=Idea",
        ),
        ("javascript:alert(1)", "about:blank"),
        ("JaVaScRiPt:alert(1)", "about:blank"),
        ("  javascript:alert(1)", "about:blank"),
        ("java\nscript:alert(1)", "about:blank"),
        ("jav&#x09;ascript:alert(1)", "about:blank"),
        ("&#106;avascript:alert(1)", "about:blank"),
        ("data:text/html,<script>alert(1)</script>", "about:blank"),
        ("vbscript:msgbox(1)", "about:blank"),
        ("javascript&colon;alert(1)", "about:blank"),
        ("%6A%61%76%61%73%63%72%69%70%74:alert(1)", "about:blank"),
        ("https://example.com/a b", "https://example.com/a%20b"),
        ("http://", "about:blank"),
        ("ftp://example.com/file", "ftp://example.com/file"),
        ("www.example.com", "www.example.com"),
        ("C:\\Users\\file.txt", "C:/Users/file.txt"),
        ("\\\\server\\share", "\\\\server\\share"),
        ("note name with spaces", "note name with spaces"),
        ("#heading", "#heading"),
        ("?query=1", "?query=1"),
        ("", "about:blank"),
        ("about:blank", "about:blank"),
        ("tel:+123456", "tel:+123456"),
        (
            "https://example.com/%E0%A4%A",
            "https://example.com/%E0%A4%A",
        ),
        (
            "https://a.com/^{}`|'\"<>[]?q=^{}'\"<>`|#f^{}`'\"<> x",
            "https://a.com/%5E%7B%7D%60|'%22%3C%3E[]?q=^{}%27%22%3C%3E`|#f^{}%60'%22%3C%3E%20x",
        ),
        ("https://u:p@a.com:80/x", "https://u:p@a.com:80/x"),
        ("https://u@b:c@A.com:443/", "https://u@b:c@A.com:443/"),
        ("http://0x7f.1/", "http://127.0.0.1/"),
        ("http://[::1]/", "http://[::1]/"),
        ("http://münchen.de/ä", "http://xn--mnchen-3ya.de/%C3%A4"),
        ("HTTP:EXAMPLE.com", "http://example.com/"),
        ("http://a/b/../c/./d/%2e%2E/e", "http://a/c/e"),
        ("http://a.b./", "http://a.b./"),
        ("http://xn--a/", "about:blank"),
        ("http:\\\\a\\b", "http://a/b"),
        ("http://a b/", "about:blank"),
        ("http://a%41b/", "http://aab/"),
        ("http://1.2.3.256/", "about:blank"),
        ("http://999999999/", "http://59.154.201.255/"),
        ("http://a/?", "http://a/?"),
        ("http://a/#", "http://a/#"),
        ("http://%E2%80%A8/", "about:blank"),
        ("http://a.0x/", "about:blank"),
        ("http://u@a.com/x", "http://u@a.com/x"),
        ("http://@a.com/", "http://a.com/"),
        ("http://u@/x", "about:blank"),
        ("http:a", "http://a/"),
        ("http:/a", "http://a/"),
        ("http:///a", "http://a/"),
        ("https://A.COM", "https://a.com/"),
        ("http://a/..", "http://a/"),
        ("http://a/.", "http://a/"),
        ("http://a/b/..", "http://a/"),
        ("http://a/b/.?x", "http://a/b/?x"),
        ("http://a/%2e%2e/b", "http://a/b"),
        ("http://a/x?é#é", "http://a/x?%C3%A9#%C3%A9"),
        ("http://a.com/日本", "http://a.com/%E6%97%A5%E6%9C%AC"),
        ("http://日本.jp/", "http://xn--wgv71a.jp/"),
        ("http://ÉXAMPLE.com/", "http://xn--xample-9ua.com/"),
        ("http://a%2Fb/", "http://a/b/"),
        ("http://a%25b/", "about:blank"),
        ("http://0/", "http://0.0.0.0/"),
        ("http://0x/", "http://0.0.0.0/"),
        ("http://1.2/", "http://1.0.0.2/"),
        ("http://1.2.3/", "http://1.2.0.3/"),
        ("http://0300.0250.1.1/", "http://192.168.1.1/"),
        ("http://4294967296/", "about:blank"),
        ("http://1.2.3.4.5/", "about:blank"),
        ("http://1..2/", "about:blank"),
        ("http://a.1/", "about:blank"),
        ("http://1.a/", "http://1.a/"),
        ("http://a。b/", "http://a.b/"),
        ("http://a\u{2028}:80/", "about:blank"),
        ("http://u\u{2028}@a:80/", "http://u%E2%80%A8@a/"),
        ("https://u\u{2028}@a:443/x", "https://u%E2%80%A8@a/x"),
        ("http://a\u{2028}b/", "about:blank"),
        ("http://a/\u{2028}x:y", "http://a/%E2%80%A8x:y"),
        ("a\u{2028}http://x", "about:blank"),
        ("  https://example.com  ", "https://example.com/"),
        ("https://example.com/%20", "https://example.com/"),
        ("%2568ttp://x", "http://x/"),
        ("&#104;ttp://x", "http://x/"),
        ("&#x6A;avascript:x", "about:blank"),
        ("&#0000106avascript:x", ":x"),
        ("&#1e2;", "d"),
        ("&#Infinity;x", "x"),
        ("&#65536;", "𐀀"),
        ("&#55357;&#56832;", "😀"),
        ("jav&NewLine;ascript:x", "about:blank"),
        ("jav&tab;ascript:x", "about:blank"),
        ("a\\nb", "ab"),
        ("a%5Cnb", "ab"),
        ("a%5c%6Eb", "ab"),
        ("a\\%72b", "ab"),
        ("a\\xb", "a\\xb"),
        ("%", "%"),
        ("%zz", "%zz"),
        ("%C3", "%C3"),
        ("%C3%28", "%C3%28"),
        ("%F0%9F%98%80", "😀"),
        ("%ED%A0%80", "%ED%A0%80"),
        ("%C0%80", "%C0%80"),
        ("\u{0}javascript:x", "about:blank"),
        ("\u{200b}javascript:x", "about:blank"),
        ("\u{feff}javascript:x", "about:blank"),
        ("İjavascript:x", "İjavascript:x"),
        ("Kjavascript:x", "Kjavascript:x"),
        ("-javascript:x", "about:blank"),
        ("_javascript:x", "_javascript:x"),
        (" data:x", "about:blank"),
        ("DATA:x", "about:blank"),
        ("x:javascript:y", "x:javascript:y"),
        ("mailto:a", "mailto:a"),
        ("MAILTO:a", "MAILTO:a"),
        ("tel:1", "tel:1"),
        ("https:", "about:blank"),
        ("https:x", "https://x/"),
        ("https:x:y", "https:x:y"),
        ("http:x&colon;y", "http:x:y"),
        ("a&COLON;b", "a&COLON;b"),
        ("a:", "a:"),
        (":a", ":a"),
        ("..", ".."),
        (".", "."),
        ("/", "/"),
        ("//evil.com", "//evil.com"),
        ("\\\\evil", "\\\\evil"),
        ("foo/bar:baz", "foo/bar:baz"),
        ("&#46;/x", "./x"),
        ("%2e/x", "./x"),
        ("&#x2F;&#x2F;evil", "//evil"),
        ("https://example.com/aaaaa", "https://example.com/aaaaa"),
        ("%25%25%2541", "%%%41"),
        (
            "%2525252525252525252525252525252525252525252525252525252525252525252541",
            "%252525252525252525252525252525252525252525252525252525252525252541",
        ),
        ("https://www.github.com", "https://www.github.com/"),
        (
            "This is a tooltip for a link",
            "This is a tooltip for a link",
        ),
        (
            "This is a tooltip for a callback",
            "This is a tooltip for a callback",
        ),
        ("This is a link", "This is a link"),
        ("Callback tooltip", "Callback tooltip"),
        ("Communication link", "Communication link"),
        ("Tooltip for a callback", "Tooltip for a callback"),
        ("Open this in a new tab", "Open this in a new tab"),
        ("Dashboard", "Dashboard"),
        (
            "https://dashboard.contoso.com/alice",
            "https://dashboard.contoso.com/alice",
        ),
        ("Wiki", "Wiki"),
        (
            "https://wiki.contoso.com/alice",
            "https://wiki.contoso.com/alice",
        ),
        (
            "https://dashboard.contoso.com/john",
            "https://dashboard.contoso.com/john",
        ),
        (
            "https://wiki.contoso.com/john",
            "https://wiki.contoso.com/john",
        ),
    ];

    const ALLOW: &[(&str, &str)] = &[
        ("default|https://a", "https://a|https://a"),
        ("default|HTTP://a", "HTTP://a|HTTP://a"),
        ("default|mailto:x", "mailto:x|null"),
        ("default|tel:1", "tel:1|null"),
        ("default|ftp://a", "null|null"),
        ("default|file:///etc", "null|null"),
        ("default|//a", "null|null"),
        ("default|\\\\a", "null|null"),
        ("default|/\\a", "null|null"),
        ("default|a/b", "a/b|a/b"),
        ("default|a:b", "null|null"),
        ("default|1a:b", "1a:b|1a:b"),
        ("default|a+b.c-d:x", "null|null"),
        ("default|about:blank", "null|null"),
        ("default|#x", "#x|#x"),
        ("default|C:\\x", "null|null"),
        ("default|obsidian://x", "null|null"),
        ("default|", "|"),
        ("off|https://a", "null|null"),
        ("off|HTTP://a", "null|null"),
        ("off|mailto:x", "null|null"),
        ("off|tel:1", "null|null"),
        ("off|ftp://a", "null|null"),
        ("off|file:///etc", "null|null"),
        ("off|//a", "null|null"),
        ("off|\\\\a", "null|null"),
        ("off|/\\a", "null|null"),
        ("off|a/b", "null|null"),
        ("off|a:b", "null|null"),
        ("off|1a:b", "null|null"),
        ("off|a+b.c-d:x", "null|null"),
        ("off|about:blank", "null|null"),
        ("off|#x", "null|null"),
        ("off|C:\\x", "null|null"),
        ("off|obsidian://x", "null|null"),
        ("off|", "null|null"),
        ("ftp|https://a", "null|null"),
        ("ftp|HTTP://a", "null|null"),
        ("ftp|mailto:x", "null|null"),
        ("ftp|tel:1", "null|null"),
        ("ftp|ftp://a", "ftp://a|null"),
        ("ftp|file:///etc", "null|null"),
        ("ftp|//a", "null|null"),
        ("ftp|\\\\a", "null|null"),
        ("ftp|/\\a", "null|null"),
        ("ftp|a/b", "a/b|a/b"),
        ("ftp|a:b", "null|null"),
        ("ftp|1a:b", "1a:b|1a:b"),
        ("ftp|a+b.c-d:x", "null|null"),
        ("ftp|about:blank", "null|null"),
        ("ftp|#x", "#x|#x"),
        ("ftp|C:\\x", "null|null"),
        ("ftp|obsidian://x", "null|null"),
        ("ftp|", "|"),
    ];
    const LINKED: &[(&str, &str)] = &[
        (
            "default|https://a.com/?x=\"1\"&y=<2>",
            "<a href=\"https://a.com/?x=&quot;1&quot;&amp;y=&lt;2&gt;\" class=\"x\" rel=\"noopener\"><g/></a>|[{\"id\":\"n1\",\"href\":\"https://a.com/?x=\\\"1\\\"&y=<2>\",\"internal\":false}]| rel=\"noopener\"",
        ),
        ("default|ftp://a", "<g/>|[]| rel=\"noopener\""),
        (
            "default|rel",
            "<a href=\"rel\" class=\"x\" rel=\"noopener\"><g/></a>|[{\"id\":\"n1\",\"href\":\"rel\",\"internal\":false}]| rel=\"noopener\"",
        ),
        (
            "empty|https://a.com/?x=\"1\"&y=<2>",
            "<a href=\"https://a.com/?x=&quot;1&quot;&amp;y=&lt;2&gt;\" class=\"x\"><g/></a>|[{\"id\":\"n1\",\"href\":\"https://a.com/?x=\\\"1\\\"&y=<2>\",\"internal\":false}]|",
        ),
        ("empty|ftp://a", "<g/>|[]|"),
        (
            "empty|rel",
            "<a href=\"rel\" class=\"x\"><g/></a>|[{\"id\":\"n1\",\"href\":\"rel\",\"internal\":false}]|",
        ),
        (
            "quoted|https://a.com/?x=\"1\"&y=<2>",
            "<a href=\"https://a.com/?x=&quot;1&quot;&amp;y=&lt;2&gt;\" class=\"x\" rel=\"a&quot;b&amp;c\"><g/></a>|[{\"id\":\"n1\",\"href\":\"https://a.com/?x=\\\"1\\\"&y=<2>\",\"internal\":false}]| rel=\"a&quot;b&amp;c\"",
        ),
        ("quoted|ftp://a", "<g/>|[]| rel=\"a&quot;b&amp;c\""),
        (
            "quoted|rel",
            "<a href=\"rel\" class=\"x\" rel=\"a&quot;b&amp;c\"><g/></a>|[{\"id\":\"n1\",\"href\":\"rel\",\"internal\":false}]| rel=\"a&quot;b&amp;c\"",
        ),
        (
            "off|https://a.com/?x=\"1\"&y=<2>",
            "<g/>|[]| rel=\"noopener\"",
        ),
        ("off|ftp://a", "<g/>|[]| rel=\"noopener\""),
        ("off|rel", "<g/>|[]| rel=\"noopener\""),
    ];
}
