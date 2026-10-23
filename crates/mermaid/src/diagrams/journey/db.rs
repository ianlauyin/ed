//! The user journey database. Port of Pele's `src/diagrams/journey/db.ts`.

use crate::text::label::{trim, trim_start};
use crate::util::chars::is_space;

/// A task: its score and the people taking part.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct JourneyTask {
    /// The actors, as written.
    pub(crate) people: Vec<String>,
    /// As written: any number, or NaN when the score is not one.
    pub(crate) score: f64,
    /// Name of the section in force when the task was declared, empty before the first one.
    pub(crate) section: String,
    /// The task's name.
    pub(crate) task: String,
}

/// The parsed user journey: Pele's `JourneyDb`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct JourneyDb {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    current_section: String,
    /// The section names, in source order.
    pub(crate) sections: Vec<String>,
    /// The tasks, in source order.
    pub(crate) tasks: Vec<JourneyTask>,
    /// The title.
    pub(crate) title: Option<String>,
}

/// Everyone named on a task, once each, in UTF-16 code unit order.
pub(crate) fn journey_actors(tasks: &[JourneyTask]) -> Vec<&str> {
    let mut names: Vec<&str> = Vec::new();
    for name in tasks.iter().flat_map(|task| &task.people) {
        if !names.contains(&name.as_str()) {
            names.push(name);
        }
    }
    names.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    names
}

/// JavaScript's `StringToNumber`.
fn string_to_number(text: &str) -> f64 {
    let text = trim(text);
    if text.is_empty() {
        return 0.0;
    }

    let bytes = text.as_bytes();
    if let (Some(b'0'), Some(&prefix)) = (bytes.first(), bytes.get(1)) {
        let radix = match prefix {
            b'x' | b'X' => 16,
            b'o' | b'O' => 8,
            b'b' | b'B' => 2,
            _ => 0,
        };
        if radix != 0 {
            let digits = text.get(2..).unwrap_or("");
            if digits.is_empty() {
                return f64::NAN;
            }
            let mut value = 0.0;
            for c in digits.chars() {
                let Some(digit) = c.to_digit(radix) else {
                    return f64::NAN;
                };
                value = value * f64::from(radix) + f64::from(digit);
            }
            return value;
        }
    }

    let unsigned = text
        .strip_prefix('+')
        .or_else(|| text.strip_prefix('-'))
        .unwrap_or(text);
    if unsigned == "Infinity" {
        return if text.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }

    // StrDecimalLiteral: digits with an optional fraction, then an optional exponent.
    let b = unsigned.as_bytes();
    let mut i = 0;
    let mut digits = 0;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
        digits += 1;
    }
    if b.get(i) == Some(&b'.') {
        i += 1;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return f64::NAN;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let start = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return f64::NAN;
        }
    }
    if i != b.len() {
        return f64::NAN;
    }
    text.parse::<f64>().unwrap_or(f64::NAN)
}

/// `text.replace(/\n\s+/g, '\n')`: each line break followed by whitespace, line breaks included,
/// becomes one line break.
fn collapse_breaks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\n' {
            while chars.next_if(|&next| is_space(u32::from(next))).is_some() {}
        }
    }
    out
}

impl JourneyDb {
    /// Sets the title from a `title` statement.
    pub(crate) fn set_diagram_title(&mut self, text: &str) {
        self.title = Some(text.to_owned());
    }

    /// Sets the accessible title, without leading whitespace.
    pub(crate) fn set_acc_title(&mut self, text: &str) {
        self.acc_title = Some(trim_start(text).to_owned());
    }

    /// Sets the accessible description, without the indentation of its lines.
    pub(crate) fn set_acc_description(&mut self, text: &str) {
        self.acc_descr = Some(collapse_breaks(text));
    }

    /// Starts a section.
    pub(crate) fn add_section(&mut self, text: &str) {
        text.clone_into(&mut self.current_section);
        self.sections.push(text.to_owned());
    }

    /// Adds a task. `data` is the rest of the line from its first colon: `:score` or
    /// `:score:actor, actor`.
    pub(crate) fn add_task(&mut self, task: &str, data: &str) {
        let mut pieces = data.get(1..).unwrap_or("").split(':');
        let score = string_to_number(pieces.next().unwrap_or(""));
        let people = pieces
            .next()
            .map(|actors| {
                actors
                    .split(',')
                    .map(|name| trim(name).to_owned())
                    .collect()
            })
            .unwrap_or_default();
        self.tasks.push(JourneyTask {
            people,
            score,
            section: self.current_section.clone(),
            task: task.to_owned(),
        });
    }
}
