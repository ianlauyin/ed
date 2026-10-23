//! The diagram theme, author styles, and class names. Port of Pele's `src/svg/theme.ts`.
//!
//! Diagram modules write colors and line styles as `var(--_x)` placeholders, so they need no
//! theme at hand; [`resolve`] replaces every placeholder with the [`Theme`]'s value once the
//! document is drawn, leaving an SVG with no CSS variables or `color-mix()` in it.

use std::borrow::Cow;

use crate::svg::builder::esc;
use crate::text::label::trim;
use crate::util::chars::is_space;
use crate::yaml::push_number;

/// The root's stroke width and text color: shapes and edges inherit the stroke width, and an
/// icon is drawn in `currentColor`, which would otherwise be whatever color the host gives.
pub(crate) const ROOT_STYLE: &str = "stroke-width:var(--_sw);color:var(--_fg)";

/// The monospace font, as set on the root.
pub(crate) const FONT_MONO: &str = "var(--_fm)";
/// The corner radius of rectangles.
pub(crate) const RADIUS: &str = "var(--_r)";
/// The attribute that gives a rectangle the themed corner radius.
pub(crate) const ROUNDED: &str = " rx=\"var(--_r)\"";
/// The color of what a diagram marks as critical. It is not one of the series colors, as a theme
/// may put those in any order.
pub(crate) const CRITICAL: &str = "var(--_cr)";
/// The stroke width of shapes and edges, as set on the root.
pub(crate) const STROKE: &str = "var(--_sw)";

/// The font stack labels are drawn in by default.
const FONT: &str = "ui-monospace, SFMono-Regular, Menlo, Consolas, 'Liberation Mono', 'DejaVu Sans Mono', monospace";

/// The root's font declarations at `size` pixels. Every font is measured as monospace, so what
/// text inherits is pinned, as none of it was measured. Kerning and ligatures are off and glyphs
/// are placed at their exact size, so every character keeps the uniform advance that layout
/// assumed.
pub(crate) fn font_style(size: f64) -> String {
    let mut out = String::with_capacity(256);
    out.push_str(";font-size:");
    push_number(&mut out, size);
    out.push_str("px;font-family:var(--_fm);letter-spacing:normal;word-spacing:normal;text-transform:none;direction:ltr;font-kerning:none;font-variant-ligatures:none;text-rendering:geometricPrecision");
    out
}

/// An opaque sRGB color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
}

impl Color {
    /// White.
    pub const WHITE: Self = Self::rgb(0xff, 0xff, 0xff);
    /// Black.
    pub const BLACK: Self = Self::rgb(0, 0, 0);

    /// The color with the given channels.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// `self` mixed with `other`, `share` (0 to 1) of the way towards `self`.
    #[must_use]
    pub fn mix(self, other: Self, share: f64) -> Self {
        let share = share.clamp(0.0, 1.0);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the mix of two channels stays within 0..=255"
        )]
        let channel =
            |a: u8, b: u8| (f64::from(a) * share + f64::from(b) * (1.0 - share)).round() as u8;
        Self::rgb(
            channel(self.r, other.r),
            channel(self.g, other.g),
            channel(self.b, other.b),
        )
    }

    fn push_hex(self, out: &mut String) {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        out.push('#');
        for channel in [self.r, self.g, self.b] {
            out.push(char::from(DIGITS[usize::from(channel >> 4)]));
            out.push(char::from(DIGITS[usize::from(channel & 0xf)]));
        }
    }
}

/// The colors, font, and line style a diagram is drawn with.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// The canvas, and the fill of most shapes.
    pub background: Color,
    /// Text.
    pub foreground: Color,
    /// Secondary text.
    pub muted: Color,
    /// Edges and connectors.
    pub line: Color,
    /// The fill of nodes.
    pub surface: Color,
    /// Grids, graticules, excluded ranges, and finished work.
    pub surface_alt: Color,
    /// Shape outlines.
    pub border: Color,
    /// Highlights, such as the current time in a gantt chart.
    pub accent: Color,
    /// What a diagram marks as critical.
    pub critical: Color,
    /// Data series in charts, in order.
    pub series: [Color; 8],
    /// The CSS font family of labels. Labels are laid out as monospace (0.6 em per glyph).
    pub font_family: String,
    /// The stroke width of shapes and edges, in pixels.
    pub stroke_width: f64,
    /// The corner radius of rectangles, in pixels.
    pub corner_radius: f64,
}

/// The share of the foreground in each series' default shade. Every shade is at least half the
/// foreground, so thin lines, outlines under a wash, and labels drawn in the background color stay
/// readable on it. Neighboring series differ by at least 15 points.
const SERIES_SHADES: [f64; 8] = [1.0, 0.57, 0.86, 0.5, 0.79, 0.64, 0.93, 0.71];

impl Theme {
    /// A monochrome theme in `foreground` on `background`: lines, borders, and muted text in the
    /// foreground, fills in the background, and series in shades between the two.
    pub fn monochrome(background: Color, foreground: Color) -> Self {
        Self {
            background,
            foreground,
            muted: foreground,
            line: foreground,
            surface: background,
            surface_alt: foreground.mix(background, 0.2),
            border: foreground,
            accent: foreground,
            critical: foreground,
            series: SERIES_SHADES.map(|share| foreground.mix(background, share)),
            font_family: FONT.to_owned(),
            stroke_width: 2.0,
            corner_radius: 0.0,
        }
    }

    /// Appends the value of placeholder `name` (the part after `--_`), or returns false if there
    /// is no such placeholder.
    fn push_value(&self, name: &str, out: &mut String) -> bool {
        let color = match name {
            "bg" => self.background,
            "fg" => self.foreground,
            "m" => self.muted,
            "l" => self.line,
            "s" => self.surface,
            "a" => self.surface_alt,
            "b" => self.border,
            "c" => self.accent,
            "cr" => self.critical,
            "sw" | "r" => {
                let value = if name == "sw" {
                    self.stroke_width
                } else {
                    self.corner_radius
                };
                push_number(out, value);
                out.push_str("px");
                return true;
            }
            "fm" => {
                out.push_str(&esc(&self.font_family));
                return true;
            }
            "w" | "tw" | "hw" => {
                out.push_str("bold");
                return true;
            }
            _ => {
                let Some((kind, index)) = name.split_at_checked(1) else {
                    return false;
                };
                let Some(index) = index
                    .parse::<usize>()
                    .ok()
                    .filter(|index| (1..=8).contains(index))
                else {
                    return false;
                };
                match kind {
                    "p" => self.series[index - 1],
                    // A cluster title's halo: half of each enclosing cluster's fill over what is
                    // behind it.
                    "h" => {
                        #[expect(
                            clippy::cast_possible_truncation,
                            clippy::cast_possible_wrap,
                            reason = "depth is at most 8"
                        )]
                        let share = 1.0 - 0.5_f64.powi(index as i32);
                        self.surface_alt.mix(self.background, share)
                    }
                    _ => return false,
                }
            }
        };
        color.push_hex(out);
        true
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::monochrome(Color::WHITE, Color::BLACK)
    }
}

/// Replaces every `var(--_x)` placeholder in `svg` with the `theme`'s value.
pub(crate) fn resolve(svg: &str, theme: &Theme) -> String {
    const OPEN: &str = "var(--_";
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(start) = rest.find(OPEN) {
        out.push_str(rest.get(..start).unwrap_or(""));
        let after = rest.get(start + OPEN.len()..).unwrap_or("");
        let resolved = after
            .find(')')
            .filter(|end| theme.push_value(after.get(..*end).unwrap_or(""), &mut out));
        if let Some(end) = resolved {
            rest = after.get(end + 1..).unwrap_or("");
        } else {
            out.push_str(OPEN);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Properties of an author style that apply to a shape, in no particular order.
const SHAPE_PROPS: [&str; 12] = [
    "fill",
    "fill-opacity",
    "opacity",
    "rx",
    "ry",
    "stroke",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-opacity",
    "stroke-width",
];

/// Properties of an author style that apply to text, in no particular order.
const TEXT_PROPS: [&str; 9] = [
    "color",
    "font-family",
    "font-size",
    "font-style",
    "font-weight",
    "letter-spacing",
    "opacity",
    "text-decoration",
    "word-spacing",
];

/// Author styles split by what they apply to, as attributes ready to write.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ResolvedStyle {
    /// Whether the text is bold.
    pub(crate) bold: bool,
    /// A font size in pixels, when one in range was given.
    pub(crate) font_size: Option<f64>,
    /// Whether the text is italic.
    pub(crate) italic: bool,
    /// A `style` attribute for lines: the shape's declarations without fills.
    pub(crate) line: String,
    /// A `style` attribute for the shape.
    pub(crate) shape: String,
    /// The stroke color, when one was given.
    pub(crate) stroke: Option<String>,
    /// A `style` attribute for the text.
    pub(crate) text: String,
}

/// Whether `text` at `at` starts with the ASCII `word`, ignoring case.
fn starts_with_ci(text: &[u8], at: usize, word: &str) -> bool {
    text.get(at..at + word.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(word.as_bytes()))
}

/// Whether `word`, then optional whitespace, then `(` starts at `at`.
fn call_at(text: &str, at: usize, word: &str) -> bool {
    starts_with_ci(text.as_bytes(), at, word)
        && text.get(at + word.len()..).is_some_and(|rest| {
            rest.trim_start_matches(|c: char| is_space(u32::from(c)))
                .starts_with('(')
        })
}

/// Pele's `/url\s*\(|expression\s*\(|javascript:|[<>{};\\]|@import|image-set|\/\*/i`.
fn unsafe_value(value: &str) -> bool {
    let bytes = value.as_bytes();
    (0..bytes.len()).any(|at| {
        matches!(
            bytes.get(at),
            Some(b'<' | b'>' | b'{' | b'}' | b';' | b'\\')
        ) || call_at(value, at, "url")
            || call_at(value, at, "expression")
            || starts_with_ci(bytes, at, "javascript:")
            || starts_with_ci(bytes, at, "@import")
            || starts_with_ci(bytes, at, "image-set")
            || starts_with_ci(bytes, at, "/*")
    })
}

/// `text` without any `!important`, in any case.
fn strip_important(text: &str) -> Cow<'_, str> {
    const WORD: &str = "!important";
    let bytes = text.as_bytes();
    if !(0..bytes.len()).any(|at| starts_with_ci(bytes, at, WORD)) {
        return Cow::Borrowed(text);
    }

    let mut out = String::with_capacity(text.len());
    let mut start = 0;
    let mut at = 0;
    while at < bytes.len() {
        if starts_with_ci(bytes, at, WORD) {
            out.push_str(text.get(start..at).unwrap_or(""));
            at += WORD.len();
            start = at;
        } else {
            at += 1;
        }
    }
    out.push_str(text.get(start..).unwrap_or(""));
    Cow::Owned(out)
}

/// Sets `key` in an insertion-ordered map, keeping the position of an existing key as a
/// JavaScript `Map` does.
fn set(map: &mut Vec<(String, String)>, key: &str, value: &str) {
    if let Some(entry) = map.iter_mut().find(|(k, _)| k == key) {
        value.clone_into(&mut entry.1);
    } else {
        map.push((key.to_owned(), value.to_owned()));
    }
}

/// JavaScript's `parseFloat` over text of digits and dots: the longest leading decimal number.
fn parse_float(text: &str) -> Option<f64> {
    let bytes = text.as_bytes();
    let int = bytes.iter().take_while(|c| c.is_ascii_digit()).count();
    let mut end = int;
    let mut digits = int;
    if bytes.get(end) == Some(&b'.') {
        let frac = bytes
            .get(end + 1..)
            .unwrap_or(&[])
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count();
        digits += frac;
        if frac > 0 {
            end += 1 + frac;
        }
    }
    if digits == 0 {
        return None;
    }
    text.get(..end).and_then(crate::util::float::parse)
}

/// JavaScript's `Number(text)`, for the forms a font weight can take.
fn js_number(text: &str) -> f64 {
    let text = trim(text);
    if text.is_empty() {
        return 0.0;
    }

    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(digits) = text.strip_prefix(prefix) {
            return u64::from_str_radix(digits, radix)
                .ok()
                .filter(|_| !digits.starts_with(['+', '-']))
                .map_or(f64::NAN, |value| {
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "JavaScript numbers are doubles"
                    )]
                    let value = value as f64;
                    value
                });
        }
    }

    match text {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }

    // Rust also reads `inf` and `nan`, which JavaScript does not, so only number characters pass.
    if !text
        .bytes()
        .all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-'))
    {
        return f64::NAN;
    }
    crate::util::float::parse(text).unwrap_or(f64::NAN)
}

/// Lowercases as JavaScript's `toLowerCase` does, for property names.
fn lowercase(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

/// Splits Mermaid style declarations into what applies to a shape and what applies to its text.
pub(crate) fn resolve_style<S: AsRef<str>>(declarations: &[S]) -> ResolvedStyle {
    if declarations.is_empty() {
        return ResolvedStyle::default();
    }

    let mut shape: Vec<(String, String)> = Vec::new();
    let mut text: Vec<(String, String)> = Vec::new();
    for decl in declarations {
        let decl = decl.as_ref();
        let Some(at) = decl.find(':').filter(|&at| at > 0) else {
            continue;
        };
        let prop = lowercase(trim(decl.get(..at).unwrap_or("")));
        let value = strip_important(decl.get(at + 1..).unwrap_or(""));
        let value = trim(&value);
        if value.is_empty() || unsafe_value(value) {
            continue;
        }
        if SHAPE_PROPS.contains(&prop.as_str()) {
            set(&mut shape, &prop, value);
        }
        if TEXT_PROPS.contains(&prop.as_str()) {
            set(
                &mut text,
                if prop == "color" { "fill" } else { &prop },
                value,
            );
        }
    }
    if shape.is_empty() && text.is_empty() {
        return ResolvedStyle::default();
    }

    let mut shape_css = String::new();
    let mut line_css = String::new();
    for (prop, value) in &shape {
        push_declaration(&mut shape_css, prop, value);
        if prop != "fill" && prop != "fill-opacity" {
            push_declaration(&mut line_css, prop, value);
        }
    }
    let mut text_css = String::new();
    for (prop, value) in &text {
        push_declaration(&mut text_css, prop, value);
    }

    let get = |map: &[(String, String)], key: &str| {
        map.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    };
    let weight = get(&text, "font-weight");
    let px = get(&text, "font-size").and_then(|size| {
        let digits = size.strip_suffix("px").unwrap_or(&size);
        if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit() || c == b'.') {
            return None;
        }
        parse_float(digits)
    });
    ResolvedStyle {
        bold: weight
            .as_deref()
            .is_some_and(|w| w == "bold" || w == "bolder" || js_number(w) >= 600.0),
        font_size: px.filter(|&px| px > 0.0 && px <= 200.0),
        italic: get(&text, "font-style").as_deref() == Some("italic"),
        line: style_attr(&line_css),
        shape: style_attr(&shape_css),
        stroke: get(&shape, "stroke"),
        text: style_attr(&text_css),
    }
}

/// Appends `prop:value;`.
fn push_declaration(out: &mut String, prop: &str, value: &str) {
    out.push_str(prop);
    out.push(':');
    out.push_str(value);
    out.push(';');
}

/// A `style` attribute holding `css`, or nothing when it is empty.
fn style_attr(css: &str) -> String {
    if css.is_empty() {
        return String::new();
    }
    let mut out = String::from(" style=\"");
    out.push_str(&esc(css));
    out.push('"');
    out
}

/// Author class names as a ` class` suffix: every run of characters outside `[\w-]` becomes one
/// space. Empty when nothing is left.
pub(crate) fn class_names(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 1);
    let mut gap = true;
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            if gap {
                out.push(' ');
            }
            out.push(c);
            gap = false;
        } else {
            gap = true;
        }
    }
    out
}

/// Color for the nth data series in a chart, cycling through the theme's eight.
pub(crate) fn series_color(index: i64) -> String {
    let k = index.rem_euclid(8);
    let mut out = String::from("var(--_p");
    out.push(char::from(b'1' + u8::try_from(k).unwrap_or(0)));
    out.push(')');
    out
}

#[cfg(test)]
mod tests {
    //! Expectations come from Pele's `resolveStyle`, `classNames`, and `seriesColor`, with
    //! `--pele-*` series colors replaced by Ondine's monochrome ones.

    use super::{class_names, resolve_style, series_color};

    #[test]
    #[expect(clippy::type_complexity, reason = "a table of cases")]
    fn resolve_style_matches_pele() {
        let cases: &[(
            &[&str],
            &str,
            &str,
            &str,
            Option<&str>,
            bool,
            bool,
            Option<f64>,
        )] = &[
            (&[][..], "", "", "", None, false, false, None),
            (
                &["fill:#f9f", "stroke:#333", "stroke-width:4px"][..],
                " style=\"fill:#f9f;stroke:#333;stroke-width:4px;\"",
                " style=\"stroke:#333;stroke-width:4px;\"",
                "",
                Some("#333"),
                false,
                false,
                None,
            ),
            (
                &[
                    "color:red",
                    "font-weight:bold",
                    "font-size:20px",
                    "font-style:italic",
                ][..],
                "",
                "",
                " style=\"fill:red;font-weight:bold;font-size:20px;font-style:italic;\"",
                None,
                true,
                true,
                Some(20.0),
            ),
            (
                &["FILL : blue !important", "fill:green", "opacity:0.5"][..],
                " style=\"fill:green;opacity:0.5;\"",
                " style=\"opacity:0.5;\"",
                " style=\"opacity:0.5;\"",
                None,
                false,
                false,
                None,
            ),
            (
                &[
                    "fill:url(#x)",
                    "stroke:expression (1)",
                    "color:javascript:x",
                    "fill:a;b",
                    "fill:@IMPORT",
                    "fill:image-set(x)",
                    "fill:/*c*/",
                    "stroke:URL  (y)",
                ][..],
                "",
                "",
                "",
                None,
                false,
                false,
                None,
            ),
            (
                &[
                    ":nope",
                    "noval:",
                    "fill:",
                    "unknown:1",
                    "font-weight:600",
                    "font-size:300px",
                ][..],
                "",
                "",
                " style=\"font-weight:600;font-size:300px;\"",
                None,
                true,
                false,
                None,
            ),
            (
                &["font-weight:0x258", "font-size:1.5.5px"][..],
                "",
                "",
                " style=\"font-weight:0x258;font-size:1.5.5px;\"",
                None,
                true,
                false,
                Some(1.5),
            ),
            (
                &["font-weight:599", "font-size:.px"][..],
                "",
                "",
                " style=\"font-weight:599;font-size:.px;\"",
                None,
                false,
                false,
                None,
            ),
            (
                &["font-weight: Infinity ", "font-size:12"][..],
                "",
                "",
                " style=\"font-weight:Infinity;font-size:12;\"",
                None,
                true,
                false,
                Some(12.0),
            ),
            (
                &["font-weight:bolder", "font-size:0"][..],
                "",
                "",
                " style=\"font-weight:bolder;font-size:0;\"",
                None,
                true,
                false,
                None,
            ),
            (
                &["font-weight:7e2", "font-size:12pt"][..],
                "",
                "",
                " style=\"font-weight:7e2;font-size:12pt;\"",
                None,
                true,
                false,
                None,
            ),
            (
                &[
                    "stroke-dasharray:5 5",
                    "rx:10",
                    "ry:5",
                    "stroke-linecap:round",
                    "letter-spacing:2px",
                    "text-decoration:underline",
                    "font-family:Arial",
                ][..],
                " style=\"stroke-dasharray:5 5;rx:10;ry:5;stroke-linecap:round;\"",
                " style=\"stroke-dasharray:5 5;rx:10;ry:5;stroke-linecap:round;\"",
                " style=\"letter-spacing:2px;text-decoration:underline;font-family:Arial;\"",
                None,
                false,
                false,
                None,
            ),
            (
                &["stro\u{212a}e:#000", "fill:\"q\"&<"][..],
                " style=\"stroke:#000;\"",
                " style=\"stroke:#000;\"",
                "",
                Some("#000"),
                false,
                false,
                None,
            ),
            (
                &["fill:a!IMPORTANT!important b"][..],
                " style=\"fill:a b;\"",
                "",
                "",
                None,
                false,
                false,
                None,
            ),
        ];
        for &(decls, shape, line, text, stroke, bold, italic, font_size) in cases {
            let resolved = resolve_style(decls);
            assert_eq!(resolved.shape, shape, "{decls:?}");
            assert_eq!(resolved.line, line, "{decls:?}");
            assert_eq!(resolved.text, text, "{decls:?}");
            assert_eq!(resolved.stroke.as_deref(), stroke, "{decls:?}");
            assert_eq!(resolved.bold, bold, "{decls:?}");
            assert_eq!(resolved.italic, italic, "{decls:?}");
            assert_eq!(resolved.font_size, font_size, "{decls:?}");
        }
    }

    #[test]
    fn class_names_match_pele() {
        for &(raw, expected) in &[
            ("", ""),
            ("a", " a"),
            ("a b", " a b"),
            (" a,,b  c-d_e ", " a b c-d_e"),
            ("\u{e9} x", " x"),
            ("!!!", ""),
            ("x.y:z", " x y z"),
        ] {
            assert_eq!(class_names(raw), expected);
        }
    }
}
