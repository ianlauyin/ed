//! The timeline database. Port of Pele's `src/diagrams/timeline/db.ts`.

use crate::error::{Code, Error};
use crate::text::label::trim_start;
use crate::util::chars::is_space;

/// A time period and its events.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TimelinePeriod {
    /// The events, in source order.
    pub(crate) events: Vec<String>,
    /// Name of the section in force when the period was declared, empty before the first one.
    pub(crate) section: String,
    /// Index into `sections` of that section, -1 before the first one.
    pub(crate) section_index: i64,
    /// The period's text.
    pub(crate) text: String,
}

/// The parsed timeline: Pele's `TimelineDb`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TimelineDb {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    current_section: String,
    /// `LR` or `TD`.
    pub(crate) direction: &'static str,
    /// The periods, in source order.
    pub(crate) periods: Vec<TimelinePeriod>,
    /// The section names, in source order.
    pub(crate) sections: Vec<String>,
    /// The title.
    pub(crate) title: Option<String>,
}

impl Default for TimelineDb {
    fn default() -> Self {
        Self {
            acc_descr: None,
            acc_title: None,
            current_section: String::new(),
            direction: "LR",
            periods: Vec::new(),
            sections: Vec::new(),
            title: None,
        }
    }
}

/// `text.replace(/\n\s+/g, '\n')`: each line break followed by whitespace, line breaks included,
/// becomes one line break.
pub(crate) fn collapse_breaks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\n' && chars.peek().is_some_and(|&next| is_space(u32::from(next))) {
            while chars.next_if(|&next| is_space(u32::from(next))).is_some() {}
        }
    }
    out
}

impl TimelineDb {
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

    /// Sets the direction: `LR` or `TD`.
    pub(crate) fn set_direction(&mut self, direction: &'static str) {
        self.direction = direction;
    }

    /// Starts a section.
    pub(crate) fn add_section(&mut self, text: &str) {
        text.clone_into(&mut self.current_section);
        self.sections.push(text.to_owned());
    }

    /// Adds a time period, with `event` as its first event when it is not empty.
    pub(crate) fn add_task(&mut self, period: &str, event: &str) {
        let section_index = i64::try_from(self.sections.len()).unwrap_or(i64::MAX) - 1;
        self.periods.push(TimelinePeriod {
            events: if event.is_empty() {
                Vec::new()
            } else {
                vec![event.to_owned()]
            },
            section: self.current_section.clone(),
            section_index,
            text: period.to_owned(),
        });
    }

    /// Adds an event to the last period.
    ///
    /// # Errors
    ///
    /// Returns a semantic error when no period came before. Mermaid fails here too, with a
    /// `TypeError`.
    pub(crate) fn add_event(&mut self, event: &str) -> Result<(), Error> {
        let Some(period) = self.periods.last_mut() else {
            let mut error = Error::new(Code::Semantic, "An event needs a time period before it.");
            error.r#type = Some("timeline");
            return Err(error);
        };
        period.events.push(event.to_owned());
        Ok(())
    }
}
