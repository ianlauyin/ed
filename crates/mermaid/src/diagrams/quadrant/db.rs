//! The quadrant chart database. Port of Pele's `src/diagrams/quadrant/db.ts`.

use crate::error::{Code, Error};
use crate::text::label::{trim, trim_end, trim_start};
use crate::util::chars::{is_hex, is_space};

/// A label: its text, and whether it is markdown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct QuadrantText {
    /// Whether the text came from a markdown string.
    pub(crate) markdown: bool,
    /// The text.
    pub(crate) text: String,
}

/// How a point is drawn, from its own styles or its class.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PointStyle {
    /// A fill color: a hex code with or without `#`.
    pub(crate) color: Option<String>,
    /// A radius in pixels.
    pub(crate) radius: Option<f64>,
    /// An outline color: a hex code with or without `#`.
    pub(crate) stroke_color: Option<String>,
    /// An outline width, such as `2px`.
    pub(crate) stroke_width: Option<String>,
}

/// A point of the chart.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct QuadrantPoint {
    /// The class after `:::`, or empty.
    pub(crate) class_name: String,
    /// Its own styles.
    pub(crate) style: PointStyle,
    /// The label.
    pub(crate) text: QuadrantText,
    /// From 0 at the left to 1 at the right. `NaN` when JavaScript's `Number` cannot read it.
    pub(crate) x: f64,
    /// From 0 at the bottom to 1 at the top.
    pub(crate) y: f64,
}

/// A semantic error of a quadrant chart.
#[cold]
fn semantic(message: String) -> Error {
    let mut error = Error::new(Code::Semantic, message);
    error.r#type = Some("quadrantChart");
    error
}

#[cold]
fn invalid(style: &str, value: &str, kind: &str) -> Error {
    semantic(
        [
            "value for ",
            style,
            " ",
            value,
            " is invalid, please use a valid ",
            kind,
        ]
        .concat(),
    )
}

/// The first two parts of Mermaid's `style.trim().split(/\s*:\s*/)`, found in one pass.
fn key_value(style: &str) -> (&str, &str) {
    let s = trim(style);
    let Some(colon) = s.find(':') else {
        return (s, "undefined");
    };
    let rest = trim_start(s.get(colon + 1..).unwrap_or(""));
    let key = trim_end(s.get(..colon).unwrap_or(""));
    match rest.find(':') {
        None => (key, rest),
        Some(next) => (key, trim_end(rest.get(..next).unwrap_or(""))),
    }
}

/// Whether `text` is one or more ASCII digits.
fn digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|c| c.is_ascii_digit())
}

/// Mermaid's `/^#?([\dA-Fa-f]{6}|[\dA-Fa-f]{3})$/`.
fn hex(text: &str) -> bool {
    let body = text.strip_prefix('#').unwrap_or(text);
    matches!(body.len(), 3 | 6) && body.bytes().all(|c| is_hex(u32::from(c)))
}

/// Reads Mermaid's point styles: `radius`, `color`, `stroke-color` and `stroke-width`, later ones
/// winning.
///
/// # Errors
///
/// Returns a semantic error for an invalid value or an unknown style.
pub(crate) fn parse_styles<S: AsRef<str>>(styles: &[S]) -> Result<PointStyle, Error> {
    let mut out = PointStyle::default();
    for style in styles {
        let (key, value) = key_value(style.as_ref());
        match key {
            "radius" => {
                if !digits(value) {
                    return Err(invalid(key, value, "number"));
                }
                out.radius = Some(value.parse().unwrap_or(f64::NAN));
            }
            "color" => {
                if !hex(value) {
                    return Err(invalid(key, value, "hex code"));
                }
                out.color = Some(value.to_owned());
            }
            "stroke-color" => {
                if !hex(value) {
                    return Err(invalid(key, value, "hex code"));
                }
                out.stroke_color = Some(value.to_owned());
            }
            "stroke-width" => {
                if !value.strip_suffix("px").is_some_and(digits) {
                    return Err(invalid(key, value, "number of pixels (eg. 10px)"));
                }
                out.stroke_width = Some(value.to_owned());
            }
            _ => {
                return Err(semantic(
                    ["style named ", key, " is not supported."].concat(),
                ));
            }
        }
    }
    Ok(out)
}

/// The value of digits in `radix`, as JavaScript reads a `0x`, `0o` or `0b` literal, or `NaN`.
fn radix_value(digits: &str, radix: u32) -> f64 {
    if digits.is_empty() {
        return f64::NAN;
    }
    let mut exact: u128 = 0;
    let mut rough = 0.0;
    let mut overflow = false;
    for c in digits.chars() {
        let Some(digit) = c.to_digit(radix) else {
            return f64::NAN;
        };
        // ponytail: past 128 bits the value is summed in floating point, which may round
        // differently from V8 in the last place.
        match exact
            .checked_mul(u128::from(radix))
            .and_then(|value| value.checked_add(u128::from(digit)))
        {
            Some(value) if !overflow => exact = value,
            _ => {
                if !overflow {
                    #[expect(clippy::cast_precision_loss, reason = "rounds as JavaScript does")]
                    let value = exact as f64;
                    rough = value;
                    overflow = true;
                }
                rough = rough * f64::from(radix) + f64::from(digit);
            }
        }
    }
    if overflow {
        return rough;
    }
    #[expect(clippy::cast_precision_loss, reason = "rounds as JavaScript does")]
    let value = exact as f64;
    value
}

/// JavaScript's `Number` of a point coordinate as the lexer reads one: `1`, `0`, or `0`, any one
/// character, and digits. The character makes it a decimal fraction, a hex, octal or binary
/// literal, an exponent, or more leading digits, and anything else is `NaN`.
pub(crate) fn coordinate(text: &str) -> f64 {
    let Some(rest) = text.strip_prefix('0') else {
        return text.parse().unwrap_or(f64::NAN);
    };
    let mut chars = rest.chars();
    let Some(kind) = chars.next() else {
        return 0.0;
    };
    let tail = chars.as_str();
    match kind {
        'x' | 'X' => radix_value(tail, 16),
        'o' | 'O' => radix_value(tail, 8),
        'b' | 'B' => radix_value(tail, 2),
        '.' | 'e' | 'E' | '0'..='9' => text.parse().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

/// `text.replace(/\n\s+/g, '\n')`: whitespace after each line break removed, line breaks within
/// it included.
pub(crate) fn acc_description(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\n' {
            while chars.next_if(|&c| is_space(u32::from(c))).is_some() {}
        }
    }
    out
}

fn trimmed(mut label: QuadrantText) -> QuadrantText {
    let text = trim(&label.text);
    if text.len() != label.text.len() {
        label.text = text.to_owned();
    }
    label
}

/// The parsed quadrant chart: Pele's `QuadrantDb`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct QuadrantDb {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    /// Point classes by name, in the order they were first defined.
    pub(crate) classes: Vec<(String, PointStyle)>,
    /// Points, in order.
    pub(crate) points: Vec<QuadrantPoint>,
    /// Quadrants 1 to 4: top right, top left, bottom left, bottom right.
    pub(crate) quadrants: [Option<QuadrantText>; 4],
    /// The title.
    pub(crate) title: Option<String>,
    /// The text at the left of the x axis.
    pub(crate) x_axis_left: Option<QuadrantText>,
    /// The text at the right of the x axis.
    pub(crate) x_axis_right: Option<QuadrantText>,
    /// The text at the bottom of the y axis.
    pub(crate) y_axis_bottom: Option<QuadrantText>,
    /// The text at the top of the y axis.
    pub(crate) y_axis_top: Option<QuadrantText>,
}

impl QuadrantDb {
    /// Sets the text of quadrant `index`, from 0 for `quadrant-1`.
    pub(crate) fn set_quadrant_text(&mut self, index: usize, label: QuadrantText) {
        if let Some(slot) = self.quadrants.get_mut(index) {
            *slot = Some(trimmed(label));
        }
    }

    /// Sets the text at the left of the x axis.
    pub(crate) fn set_x_axis_left_text(&mut self, label: QuadrantText) {
        self.x_axis_left = Some(trimmed(label));
    }

    /// Sets the text at the right of the x axis.
    pub(crate) fn set_x_axis_right_text(&mut self, label: QuadrantText) {
        self.x_axis_right = Some(trimmed(label));
    }

    /// Sets the text at the bottom of the y axis.
    pub(crate) fn set_y_axis_bottom_text(&mut self, label: QuadrantText) {
        self.y_axis_bottom = Some(trimmed(label));
    }

    /// Sets the text at the top of the y axis.
    pub(crate) fn set_y_axis_top_text(&mut self, label: QuadrantText) {
        self.y_axis_top = Some(trimmed(label));
    }

    /// Adds a point.
    ///
    /// # Errors
    ///
    /// Returns a semantic error for an invalid style.
    pub(crate) fn add_point(
        &mut self,
        label: QuadrantText,
        class_name: String,
        x: &str,
        y: &str,
        styles: &[String],
    ) -> Result<(), Error> {
        let style = parse_styles(styles)?;
        self.points.push(QuadrantPoint {
            class_name,
            style,
            text: trimmed(label),
            x: coordinate(x),
            y: coordinate(y),
        });
        Ok(())
    }

    /// Defines a point class, replacing one of the same name in place.
    ///
    /// # Errors
    ///
    /// Returns a semantic error for an invalid style.
    pub(crate) fn add_class(&mut self, class_name: String, styles: &[String]) -> Result<(), Error> {
        let style = parse_styles(styles)?;
        match self
            .classes
            .iter_mut()
            .find(|(name, _)| *name == class_name)
        {
            Some(entry) => entry.1 = style,
            None => self.classes.push((class_name, style)),
        }
        Ok(())
    }

    /// The class named `name`.
    pub(crate) fn class(&self, name: &str) -> Option<&PointStyle> {
        self.classes
            .iter()
            .find(|(own, _)| own == name)
            .map(|(_, style)| style)
    }

    /// Sets the title from a `title` statement.
    pub(crate) fn set_diagram_title(&mut self, text: &str) {
        self.title = Some(text.to_owned());
    }

    /// Sets the accessible title, without the whitespace it starts with.
    pub(crate) fn set_acc_title(&mut self, text: &str) {
        self.acc_title = Some(trim_start(text).to_owned());
    }

    /// Sets the accessible description, without the indentation of its lines.
    pub(crate) fn set_acc_description(&mut self, text: &str) {
        self.acc_descr = Some(acc_description(text));
    }
}
