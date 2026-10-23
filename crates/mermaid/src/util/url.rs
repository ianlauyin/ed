//! Sanitizes URLs as Mermaid does, and keeps images to relative, `http`, and `https` addresses.
//! Port of Pele's `src/util/url.ts`.

use crate::text::entities;
use crate::util::chars::is_line_end;
use crate::util::chars::is_space;
use crate::util::chars::is_word;

const BLANK: &str = "about:blank";

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
    char::from(u).to_digit(16).and_then(|digit| u8::try_from(digit).ok())
}

// /[\u0000-\u001F\u007F-\u009F -‍﻿]/
fn is_ctrl(u: u16) -> bool {
    u <= 0x1f || (0x7f..=0x9f).contains(&u) || (0x2000..=0x200d).contains(&u) || u == 0xfeff
}

// Whether `units` at `at` start with `ascii`, ignoring ASCII case.
fn starts_ignore_case(units: &[u16], at: usize, ascii: &str) -> bool {
    ascii
        .bytes()
        .enumerate()
        .all(|(i, b)| units.get(at + i).and_then(|&u| u8::try_from(u).ok()).is_some_and(|u| u.eq_ignore_ascii_case(&b)))
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
    let out = units.iter().copied().filter(|&u| !is_ctrl(u)).collect::<Vec<_>>();
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
                at = if unit_is(units.get(end), b';') { end + 1 } else { end };
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
        return digits.chars().fold(0.0, |acc, c| acc * f64::from(base) + f64::from(c.to_digit(base).unwrap_or(0)));
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
    let wrapped = if wrapped < 0.0 { wrapped + 65536.0 } else { wrapped };
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "wrapped into 0..65536")]
    let code = wrapped as u16;
    code
}

// `.replace(/&(newline|tab);/gi, '')`.
fn remove_ctrl_entities(units: &[u16]) -> (Vec<u16>, bool) {
    let mut out = Vec::with_capacity(units.len());
    let mut found = false;
    let mut at = 0;
    while let Some(&unit) = units.get(at) {
        let length =
            ["&newline;", "&tab;"].iter().find(|name| starts_ignore_case(units, at, name)).map(|name| name.len());
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
        if ["%6e", "%72", "%74"].iter().any(|code| starts_ignore_case(units, next, code)) {
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
    let end = units.iter().rposition(|u| !spaced(u)).map_or(start, |at| at + 1);
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
    let lower = scheme.chars().flat_map(char::to_lowercase).collect::<String>();
    let protocol = lower.trim_matches(space);
    if invalid_protocol(protocol) {
        return BLANK.to_owned();
    }

    let normalized = trimmed.chars().map(|c| if c == '\\' { '/' } else { c }).collect::<String>();
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
            } else if tail.get(..7).is_some_and(|head| head.eq_ignore_ascii_case("&colon;")) {
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
    ["javascript", "data", "vbscript"]
        .iter()
        .any(|name| rest.get(..name.len()).is_some_and(|head| head.eq_ignore_ascii_case(name)))
}

// Parses an `http:` or `https:` URL and serializes it as the WHATWG URL Standard does, for
// `new URL(url).toString()`.
fn parse_http(input: &str) -> Option<String> {
    let input =
        input.trim_matches(|c: char| c <= ' ').chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect::<String>();
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
            let value = digits.bytes().fold(0u32, |acc, b| acc.saturating_mul(10).saturating_add(u32::from(b - b'0')));
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
    ["..", ".%2e", "%2e.", "%2e%2e"].iter().any(|dots| segment.eq_ignore_ascii_case(dots))
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

const fn forbidden_domain(c: char) -> bool {
    c <= '\u{1f}'
        || c == '\u{7f}'
        || matches!(c, ' ' | '#' | '%' | '/' | ':' | '<' | '>' | '?' | '@' | '[' | '\\' | ']' | '^' | '|')
}

// Encodes a label with RFC 3492 Punycode.
const BASE: u32 = 36;

// Punycode's bias adaptation (RFC 3492, section 6.1).
const fn adapt(delta: u32, points: u32, first: bool) -> u32 {
    let mut delta = if first { delta / 700 } else { delta / 2 };
    delta += delta / points;
    let mut k = 0;
    while delta > (BASE - 1) * 26 / 2 {
        delta /= BASE - 1;
        k += BASE;
    }
    k + BASE * delta / (delta + 38)
}

const fn threshold(step: u32, bias: u32) -> u32 {
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
    digits
        .chars()
        .try_fold(0u64, |acc, c| Some(acc.saturating_mul(u64::from(base)).saturating_add(u64::from(c.to_digit(base)?))))
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
    let numbers = parts.iter().map(|part| ipv4_number(part)).collect::<Option<Vec<_>>>()?;
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

const IMAGE_SCHEMES: [&str; 2] = ["http", "https"];

// Stricter than Mermaid, which lets any scheme but three through: a diagram in a shared note
// could otherwise reach a local file, a network share, or another application.
fn allow<'a>(url: &'a str, schemes: &[&str]) -> Option<&'a str> {
    let bytes = url.as_bytes();
    if matches!(bytes.first(), Some(b'/' | b'\\')) && matches!(bytes.get(1), Some(b'/' | b'\\')) {
        return None;
    }

    let name_length = if bytes.first().is_some_and(u8::is_ascii_alphabetic) {
        1 + bytes.iter().skip(1).take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'.' | b'-')).count()
    } else {
        0
    };
    if name_length == 0 || bytes.get(name_length) != Some(&b':') {
        return Some(url);
    }
    let name = url.get(..name_length)?;
    schemes.iter().any(|scheme| scheme.bytes().eq(name.bytes().map(|b| b.to_ascii_lowercase()))).then_some(url)
}

/// Keeps a URL that `sanitize_url` has passed when it is relative or uses `http` or `https`.
pub(crate) fn image_url(url: &str) -> Option<&str> {
    allow(url, &IMAGE_SCHEMES)
}
