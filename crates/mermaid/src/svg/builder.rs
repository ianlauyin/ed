//! Escaping, number formatting, and label text for SVG output. Port of Pele's
//! `src/svg/builder.ts`.

use std::borrow::Cow;

use crate::text::entities::decode;
use crate::text::icons::IconResolver;
use crate::text::label::Label;
use crate::text::measure::{BOLD, ITALIC};

/// Rounds as JavaScript's `Math.round` does: halves toward positive infinity.
pub(crate) fn js_round(value: f64) -> f64 {
    let floor = value.floor();
    if value - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

/// Whether XML text needs escaping: markup characters, or control characters XML does not allow.
fn needs_esc(c: char) -> bool {
    matches!(c, '&' | '<' | '>' | '"' | '\0'..='\x08' | '\x0b' | '\x0c' | '\x0e'..='\x1f' | '\x7f')
}

/// Escapes text for XML and drops the control characters XML does not allow.
pub(crate) fn esc(text: &str) -> Cow<'_, str> {
    if !text.contains(needs_esc) {
        return Cow::Borrowed(text);
    }

    let mut out = String::with_capacity(text.len() + 16);
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c if needs_esc(c) => {}
            c => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// Escapes text that is only ever read as text: a tooltip, an accessible name, a `data-id`.
/// Entity codes become their characters first. A value that was checked before it is written,
/// such as a style, a color, or a URL, goes through [`esc`] instead, so what was checked is what
/// is written.
pub(crate) fn esc_text(text: &str) -> Cow<'_, str> {
    match decode(text) {
        Cow::Borrowed(text) => esc(text),
        Cow::Owned(text) => Cow::Owned(esc(&text).into_owned()),
    }
}

/// Appends an unsigned integer in decimal.
fn push_u64(out: &mut String, mut value: u64) {
    let mut digits = [0u8; 20];
    let mut at = digits.len();
    loop {
        at -= 1;
        if let Some(digit) = digits.get_mut(at) {
            *digit = b'0' + u8::try_from(value % 10).unwrap_or(0);
        }
        value /= 10;
        if value == 0 || at == 0 {
            break;
        }
    }
    for &digit in digits.get(at..).unwrap_or(&[]) {
        out.push(char::from(digit));
    }
}

/// Appends a coordinate rounded to two decimals, without trailing zeros or negative zero.
pub(crate) fn push_num(out: &mut String, value: f64) {
    let n = js_round(value * 100.0);
    if n == 0.0 {
        out.push('0');
        return;
    }
    if n.is_nan() {
        out.push_str("NaN");
        return;
    }
    if n.is_infinite() {
        out.push_str(if n > 0.0 { "Infinity" } else { "-Infinity" });
        return;
    }

    let n = if n < 0.0 {
        out.push('-');
        -n
    } else {
        n
    };

    // ponytail: Pele prints hundredths of 1e11 and more with `String(n / 100)`, which turns to
    // exponents past 1e21. This keeps the two-decimal form, saturating at `u64::MAX`.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "n is a positive whole number"
    )]
    let n = n as u64;
    push_u64(out, n / 100);

    let fraction = n % 100;
    if fraction == 0 {
        return;
    }
    out.push('.');
    out.push(char::from(b'0' + u8::try_from(fraction / 10).unwrap_or(0)));
    if !fraction.is_multiple_of(10) {
        out.push(char::from(b'0' + u8::try_from(fraction % 10).unwrap_or(0)));
    }
}

/// A coordinate rounded to two decimals, without trailing zeros or negative zero.
pub(crate) fn num(value: f64) -> String {
    let mut out = String::new();
    push_num(&mut out, value);
    out
}

/// Writes a measured label as SVG text centered on (`cx`, `cy`). `attrs` go on the text element;
/// `icons` draws the label's icons.
pub(crate) fn label_svg(
    label: &Label,
    cx: f64,
    cy: f64,
    attrs: &str,
    icons: Option<IconResolver<'_>>,
) -> String {
    let lines = &label.lines;
    if lines.is_empty() {
        return String::new();
    }

    let lh = label.line_height;
    let shift = label.size * 0.35;
    let mut y = cy - label.height / 2.0 + lh / 2.0 + shift;
    let mut out = String::new();

    if let [line] = lines.as_slice()
        && let [span] = line.as_slice()
        && span.icon.is_none()
        && span.style == 0
    {
        out.push_str("<text");
        out.push_str(attrs);
        out.push_str(" x=\"");
        push_num(&mut out, cx);
        out.push_str("\" y=\"");
        push_num(&mut out, y);
        out.push_str("\" text-anchor=\"middle\">");
        out.push_str(&esc(&span.text));
        out.push_str("</text>");
        return out;
    }

    let shared = shared_style(label);
    let mut body = String::new();
    let mut extra = String::new();
    for (i, line) in lines.iter().enumerate() {
        if line.iter().any(|span| span.icon.is_some()) {
            let mut x = cx - label.widths.get(i).copied().unwrap_or(f64::NAN) / 2.0;
            for span in line {
                if let Some(icon) = &span.icon {
                    let side = label.size * 1.1;
                    extra.push_str("<svg class=\"ondine-icon\" data-icon=\"");
                    extra.push_str(&esc_text(icon));
                    extra.push_str("\" x=\"");
                    push_num(&mut extra, x);
                    extra.push_str("\" y=\"");
                    push_num(&mut extra, y - shift - side / 2.0);
                    extra.push_str("\" width=\"");
                    push_num(&mut extra, side);
                    extra.push_str("\" height=\"");
                    push_num(&mut extra, side);
                    extra.push_str(
                        "\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\">",
                    );
                    if let Some(resolve) = icons
                        && let Some(inner) = resolve(icon)
                    {
                        extra.push_str(&inner);
                    }
                    extra.push_str("</svg>");
                } else if !span.text.is_empty() {
                    body.push_str("<tspan x=\"");
                    push_num(&mut body, x);
                    body.push_str("\" y=\"");
                    push_num(&mut body, y);
                    body.push_str("\" text-anchor=\"start\"");
                    body.push_str(span_style(span.style & !shared));
                    body.push('>');
                    body.push_str(&esc(&span.text));
                    body.push_str("</tspan>");
                }
                x += span.width;
            }
        } else {
            for (j, span) in line.iter().enumerate() {
                body.push_str("<tspan");
                if j == 0 {
                    body.push_str(" x=\"");
                    push_num(&mut body, cx);
                    body.push_str("\" y=\"");
                    push_num(&mut body, y);
                    body.push('"');
                }
                body.push_str(span_style(span.style & !shared));
                body.push('>');
                body.push_str(&esc(&span.text));
                body.push_str("</tspan>");
            }
            if line.is_empty() {
                body.push_str("<tspan x=\"");
                push_num(&mut body, cx);
                body.push_str("\" y=\"");
                push_num(&mut body, y);
                body.push_str("\"> </tspan>");
            }
        }
        y += lh;
    }

    out.push_str("<text");
    out.push_str(&text_style(attrs, shared));
    out.push_str(" text-anchor=\"middle\" xml:space=\"preserve\">");
    out.push_str(&body);
    out.push_str("</text>");
    out.push_str(&extra);
    out
}

/// The attribute that makes text bold, at the weight the root sets.
const BOLD_ATTR: &str = " font-weight=\"var(--_w)\"";
/// The attribute that makes text italic.
const ITALIC_ATTR: &str = " font-style=\"italic\"";

/// The attributes for a span's emphasis bits.
pub(crate) fn span_style(style: u8) -> &'static str {
    match (style & BOLD != 0, style & ITALIC != 0) {
        (true, true) => " font-weight=\"var(--_w)\" font-style=\"italic\"",
        (true, false) => BOLD_ATTR,
        (false, true) => ITALIC_ATTR,
        (false, false) => "",
    }
}

/// The emphasis every span of a label has. It is written once, on the text element, where a CSS
/// rule for the label reaches it.
pub(crate) fn shared_style(label: &Label) -> u8 {
    let mut shared = BOLD | ITALIC;
    let mut any = false;
    for span in label.lines.iter().flatten() {
        if span.icon.is_some() || span.text.is_empty() {
            continue;
        }
        shared &= span.style;
        any = true;
    }
    if any { shared } else { 0 }
}

/// Adds that emphasis to a text element's attributes, unless they give a weight or style of
/// their own.
pub(crate) fn text_style(attrs: &str, shared: u8) -> Cow<'_, str> {
    let bold = shared & BOLD != 0 && !attrs.contains(" font-weight=\"");
    let italic = shared & ITALIC != 0 && !attrs.contains(" font-style=\"");
    if !bold && !italic {
        return Cow::Borrowed(attrs);
    }

    let mut out = String::with_capacity(attrs.len() + 48);
    out.push_str(attrs);
    if bold {
        out.push_str(BOLD_ATTR);
    }
    if italic {
        out.push_str(ITALIC_ATTR);
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    //! Expectations come from Pele's `num`, `esc`, `escText`, `textStyle`, and `spanStyle`.

    use super::{esc, esc_text, num, span_style, text_style};

    #[test]
    #[expect(
        clippy::unreadable_literal,
        reason = "values as Pele's tests print them"
    )]
    fn num_matches_pele() {
        for &(value, expected) in &[
            (0.0, "0"),
            (0.0, "0"),
            (0.004, "0"),
            (0.005, "0.01"),
            (-0.005, "0"),
            (0.015, "0.02"),
            (1.005, "1"),
            (2.675, "2.68"),
            (0.1, "0.1"),
            (0.25, "0.25"),
            (0.5, "0.5"),
            (-0.5, "-0.5"),
            (-0.499, "-0.5"),
            (1.0, "1"),
            (10.1, "10.1"),
            (12.34, "12.34"),
            (99.995, "100"),
            (123.456, "123.46"),
            (-123.456, "-123.46"),
            (1e-7, "0"),
            (10000000000.0, "10000000000"),
            (99999999.99, "99999999.99"),
            (1000000000000.0, "1000000000000"),
            (-7.3, "-7.3"),
            (0.07, "0.07"),
            (3.1, "3.1"),
            (0.3333333333333333, "0.33"),
            (-0.6666666666666666, "-0.67"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
        ] {
            assert_eq!(num(value), expected, "{value}");
        }
    }

    #[test]
    fn esc_matches_pele() {
        for &(text, escaped, as_text) in &[
            ("plain", "plain", "plain"),
            ("a & b", "a &amp; b", "a &amp; b"),
            (
                "<tag attr=\"v\">",
                "&lt;tag attr=&quot;v&quot;&gt;",
                "&lt;tag attr=&quot;v&quot;&gt;",
            ),
            ("\u{0}\u{1}x\u{8}\u{b}\u{c}\u{e}\u{1f}\u{7f}y", "xy", "xy"),
            ("tab\tnl\ncr\r", "tab\tnl\ncr\r", "tab\tnl\ncr\r"),
            (
                "caf\u{e9} \u{1f600}",
                "caf\u{e9} \u{1f600}",
                "caf\u{e9} \u{1f600}",
            ),
            ("&amp;", "&amp;amp;", "&amp;"),
            (
                "&lt;b&gt; #35; \u{fb02}\u{b0}\u{b0}35\u{b6}\u{df}",
                "&amp;lt;b&amp;gt; #35; \u{fb02}\u{b0}\u{b0}35\u{b6}\u{df}",
                "&lt;b&gt; #35; #",
            ),
            ("\"q\"", "&quot;q&quot;", "&quot;q&quot;"),
        ] {
            assert_eq!(esc(text), escaped);
            assert_eq!(esc_text(text), as_text);
        }
    }

    #[test]
    fn styles_match_pele() {
        for &(attrs, shared, expected, span) in &[
            ("", 0, "", ""),
            (
                "",
                1,
                " font-weight=\"var(--_w)\"",
                " font-weight=\"var(--_w)\"",
            ),
            (
                "",
                3,
                " font-weight=\"var(--_w)\" font-style=\"italic\"",
                " font-weight=\"var(--_w)\" font-style=\"italic\"",
            ),
            (
                " font-weight=\"400\"",
                3,
                " font-weight=\"400\" font-style=\"italic\"",
                " font-weight=\"var(--_w)\" font-style=\"italic\"",
            ),
            (
                " font-style=\"normal\"",
                3,
                " font-style=\"normal\" font-weight=\"var(--_w)\"",
                " font-weight=\"var(--_w)\" font-style=\"italic\"",
            ),
            (
                " x",
                2,
                " x font-style=\"italic\"",
                " font-style=\"italic\"",
            ),
        ] {
            assert_eq!(text_style(attrs, shared), expected);
            assert_eq!(span_style(shared), span);
        }
    }
}
