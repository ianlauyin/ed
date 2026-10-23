//! The gantt model and its scheduler: tasks, their references to each other, and excluded days.
//! Port of Pele's `src/diagrams/gantt/db.ts`.
//!
//! Pele's database reports unknown task references through an optional `warn` callback. Its
//! renderer never sets one, so the messages are not ported.

use std::collections::{BTreeMap, BTreeSet};

use super::dates::{
    Unit, WEEKDAYS, add, add_days, format, index, is_valid, iso_date, parse_strict,
};
use super::js_date::{self, Date};
use super::types::{GanttTask, Raw};
use crate::error::{Code, Error};
use crate::text::label::{trim, trim_start};
use crate::util::chars::{is_digit, is_space, is_word};
use crate::util::url::safe_url;

/// Scheduling walks tasks day by day and parses dates with a format the diagram supplies. This
/// bounds the total, in rough character operations, so a hostile diagram fails instead of
/// hanging.
const WORK_LIMIT: f64 = 4e8;
/// The work of checking one day of a task.
const DAY_STEP: f64 = 400.0;
/// The work of matching one day against dated tokens.
const DATED_STEP: f64 = 200.0;
/// The base work of formatting one day in the chart's format.
const FORMAT_STEP: f64 = 400.0;

/// The time to schedule against: the host's `now` when it is finite. Pele reads the clock
/// otherwise, which the compiler does not, so it counts from the epoch.
pub(crate) fn clock(now: Option<f64>) -> f64 {
    now.filter(|now| now.is_finite()).unwrap_or(0.0)
}

/// A duration such as `3d` or `1.5h`, as Mermaid reads one. NaN and milliseconds when the text
/// is not a duration.
pub(crate) fn parse_duration(text: &str) -> (f64, Unit) {
    let text = trim(text);
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    let none = (f64::NAN, Unit::Millisecond);
    if digits == 0 {
        return none;
    }
    let mut end = digits;
    if text.as_bytes().get(end) == Some(&b'.') {
        let fraction = text
            .bytes()
            .skip(end + 1)
            .take_while(u8::is_ascii_digit)
            .count();
        if fraction > 0 {
            end += 1 + fraction;
        }
    }
    let unit = match text.get(end..) {
        Some("M") => Unit::Month,
        Some("d") => Unit::Day,
        Some("h") => Unit::Hour,
        Some("m") => Unit::Minute,
        Some("s") => Unit::Second,
        Some("w") => Unit::Week,
        Some("y") => Unit::Year,
        Some("ms") => Unit::Millisecond,
        _ => return none,
    };
    let value = text
        .get(..end)
        .and_then(|n| n.parse().ok())
        .unwrap_or(f64::NAN);
    (value, unit)
}

/// Lowercases as JavaScript's `toLowerCase` does, except for the final form of sigma.
fn lowercase(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

/// `existing` with the tokens of `text`, lowercased and split on whitespace and commas, added
/// in order without repeats.
fn merge_tokens(existing: &[String], text: &str) -> Vec<String> {
    let mut merged: Vec<String> = Vec::with_capacity(existing.len());
    for token in existing {
        if !merged.contains(token) {
            merged.push(token.clone());
        }
    }
    let lower = lowercase(text);
    for token in lower.split(|c: char| c == ',' || is_space(u32::from(c))) {
        if !token.is_empty() && !merged.iter().any(|own| own == token) {
            merged.push(token.to_owned());
        }
    }
    merged
}

/// A semantic error about the gantt chart.
#[cold]
fn semantic(message: String) -> Error {
    let mut error = Error::new(Code::Semantic, message);
    error.r#type = Some("gantt");
    error
}

/// `Invalid date:` and the text.
#[cold]
fn invalid_date(text: &str) -> Error {
    let mut message = String::from("Invalid date:");
    message.push_str(text);
    semantic(message)
}

/// The UTF-16 length of `text`, as a work count.
#[expect(clippy::cast_precision_loss, reason = "lengths stay far below 2^52")]
fn units(text: &str) -> f64 {
    text.encode_utf16().count() as f64
}

/// Whether a date format uses only numeric tokens and separators: `^[YMDHhmsSZXx\-/.:, T]*$`.
fn numeric_format(template: &str) -> bool {
    template.bytes().all(|b| b"YMDHhmsSZXx-/.:, T".contains(&b))
}

/// The id list of `after` or `until` at the start of `text`: `^word\s+([\d\w- ]+)`.
fn reference<'t>(text: &'t str, word: &str) -> Option<&'t str> {
    let rest = text.strip_prefix(word)?;
    let spaces: Vec<(usize, char)> = rest
        .char_indices()
        .take_while(|&(_, c)| is_space(u32::from(c)))
        .collect();
    let &(last_at, last) = spaces.last()?;
    let id_char = |b: &u8| is_word(u32::from(*b)) || *b == b'-' || *b == b' ';
    let run = |from: usize| {
        let length = rest
            .as_bytes()
            .get(from..)
            .unwrap_or(&[])
            .iter()
            .take_while(|b| id_char(b))
            .count();
        rest.get(from..from + length).unwrap_or("")
    };
    let ids = run(last_at + last.len_utf8());
    if !ids.is_empty() {
        return Some(ids);
    }
    // `\s+` gives characters back until the group can start on a space it took.
    spaces
        .iter()
        .skip(1)
        .rev()
        .find(|&&(_, c)| c == ' ')
        .map(|&(at, _)| run(at))
}

/// `+date` for a date that may be unset, as JavaScript compares `undefined`.
fn time_of(date: Option<Date>) -> f64 {
    date.map_or(f64::NAN, Date::time)
}

/// The gantt model: what the source sets, plus scheduling state. Pele's `GanttDb`.
#[derive(Clone, Debug, Default)]
pub(crate) struct GanttDb {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    /// The `axisFormat`, or empty.
    pub(crate) axis_format: String,
    /// The `dateFormat`, or empty.
    pub(crate) date_format: String,
    /// `compact` packs a section's tasks onto shared rows.
    pub(crate) display_mode: String,
    /// Lowercased `excludes` tokens, without repeats.
    pub(crate) excludes: Vec<String>,
    /// Whether `inclusiveEndDates` was given.
    pub(crate) inclusive_end_dates: bool,
    /// Lowercased `includes` tokens, without repeats.
    pub(crate) includes: Vec<String>,
    /// Link targets by task id, in insertion order.
    pub(crate) links: Vec<(String, String)>,
    /// Section names in order.
    pub(crate) sections: Vec<String>,
    /// Tasks in source order.
    pub(crate) tasks: Vec<GanttTask>,
    /// The `tickInterval`, when given.
    pub(crate) tick_interval: Option<String>,
    /// The diagram title.
    pub(crate) title: Option<String>,
    /// The `todayMarker` style, `off`, or empty.
    pub(crate) today_marker: String,
    /// Whether `topAxis` was given.
    pub(crate) top_axis: bool,
    /// The day week-based ticks start on, when the chart names one.
    pub(crate) weekday: Option<String>,
    /// The first day of the weekend: `friday` or `saturday`.
    pub(crate) weekend: String,

    /// Task indexes by id. A later task replaces an earlier one with the same id.
    by_id: BTreeMap<String, usize>,
    /// The section new tasks join.
    current_section: String,
    /// Whether excluded or included days must be matched by date.
    dated_tokens: bool,
    /// `excludes`, for lookups while scheduling.
    exclude_set: BTreeSet<String>,
    /// Excluded weekdays, from Sunday.
    excluded_days: [bool; 7],
    /// `includes`, for lookups while scheduling.
    include_set: BTreeSet<String>,
    /// The row the next task takes.
    last_order: i32,
    /// The id of the last task added.
    last_task_id: Option<String>,
    /// The time scheduling runs against.
    now: f64,
    /// The number of tasks given generated ids.
    task_count: u32,
    /// The work scheduling has done.
    work: f64,
}

impl GanttDb {
    /// An empty model.
    pub(crate) fn new() -> Self {
        Self {
            weekend: "saturday".to_owned(),
            ..Self::default()
        }
    }

    /// Sets the `dateFormat`.
    pub(crate) fn set_date_format(&mut self, text: &str) {
        text.clone_into(&mut self.date_format);
    }

    /// Adds `includes` tokens.
    pub(crate) fn set_includes(&mut self, text: &str) {
        self.includes = merge_tokens(&self.includes, text);
    }

    /// Adds `excludes` tokens.
    pub(crate) fn set_excludes(&mut self, text: &str) {
        self.excludes = merge_tokens(&self.excludes, text);
    }

    /// Sets the accessible title, without leading whitespace.
    pub(crate) fn set_acc_title(&mut self, text: &str) {
        self.acc_title = Some(trim_start(text).to_owned());
    }

    /// Sets the accessible description, without the indentation of its lines.
    pub(crate) fn set_acc_description(&mut self, text: &str) {
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            out.push(c);
            if c == '\n' {
                while chars.next_if(|&c| is_space(u32::from(c))).is_some() {}
            }
        }
        self.acc_descr = Some(out);
    }

    /// Starts a section.
    pub(crate) fn add_section(&mut self, text: &str) {
        text.clone_into(&mut self.current_section);
        self.sections.push(text.to_owned());
    }

    /// Adds a task from its label and data.
    ///
    /// # Errors
    ///
    /// Returns a semantic error when the data has no end or more than three fields, where
    /// Mermaid fails with a `TypeError`.
    pub(crate) fn add_task(&mut self, descr: &str, data_text: &str) -> Result<(), Error> {
        let data = data_text.strip_prefix(':').unwrap_or(data_text);
        let mut data: Vec<&str> = data.split(',').collect();
        let mut task = GanttTask {
            order: -1,
            prev_task_id: self.last_task_id.clone(),
            raw: Raw {
                data: data_text.to_owned(),
                ..Raw::default()
            },
            section: self.current_section.clone(),
            task: descr.to_owned(),
            ..GanttTask::default()
        };

        while let Some(first) = data.first() {
            match trim(first) {
                "active" => task.active = true,
                "done" => task.done = true,
                "crit" => task.crit = true,
                "milestone" => task.milestone = true,
                "vert" => task.vert = true,
                _ => break,
            }
            data.remove(0);
        }
        if data.is_empty() {
            let mut message = String::from("Task \"");
            message.push_str(trim(descr));
            message.push_str("\" needs a duration or an end date.");
            return Err(semantic(message));
        }
        if data.len() > 3 {
            let mut message = String::from("Task \"");
            message.push_str(trim(descr));
            message.push_str(
                "\" has too many fields. A task takes an id, a start, and an end or duration.",
            );
            return Err(semantic(message));
        }
        let data: Vec<&str> = data.into_iter().map(trim).collect();

        if let [id, _, _] = data.as_slice() {
            (*id).clone_into(&mut task.id);
        } else {
            self.task_count += 1;
            task.id = String::from("task");
            crate::error::push_u32(&mut task.id, self.task_count);
        }
        if data.len() > 1 {
            task.raw.start = data.get(data.len() - 2).map(|start| (*start).to_owned());
        }
        if let Some(end) = data.last() {
            (*end).clone_into(&mut task.raw.end);
        }

        if !task.vert {
            task.order = self.last_order;
            self.last_order += 1;
        }
        self.last_task_id = Some(task.id.clone());
        self.by_id.insert(task.id.clone(), self.tasks.len());
        self.tasks.push(task);
        Ok(())
    }

    /// Links the listed tasks to `link` and marks them clickable.
    pub(crate) fn set_link(&mut self, ids: &str, link: &str) {
        let href = safe_url(link);
        for id in ids.split(',') {
            if !self.by_id.contains_key(id) {
                continue;
            }
            if let Some(entry) = self.links.iter_mut().find(|(own, _)| own == id) {
                entry.1.clone_from(&href);
            } else {
                self.links.push((id.to_owned(), href.clone()));
            }
        }
        self.set_class(ids, "clickable");
    }

    /// Adds `class` to each listed task.
    pub(crate) fn set_class(&mut self, ids: &str, class: &str) {
        for id in ids.split(',') {
            if let Some(task) = self.by_id.get(id).and_then(|&i| self.tasks.get_mut(i)) {
                task.classes.push(class.to_owned());
            }
        }
    }

    /// The task with `id`, the last one defined when several share it.
    fn find(&self, id: &str) -> Option<&GanttTask> {
        self.by_id.get(id).and_then(|&i| self.tasks.get(i))
    }

    /// Resolves every task's start and end against `now`, from scratch on each call. A task may
    /// refer to tasks defined after it, so, like Mermaid, this makes up to eleven passes, each of
    /// which schedules every task again.
    ///
    /// # Errors
    ///
    /// Returns a semantic error for a start that is not a date, and a limit error when
    /// scheduling takes too much work.
    pub(crate) fn get_tasks(&mut self, now: f64) -> Result<&[GanttTask], Error> {
        self.now = now;
        self.work = 0.0;
        self.include_set = self.includes.iter().cloned().collect();
        self.exclude_set = self.excludes.iter().cloned().collect();
        let weekend_start = match self.weekend.as_str() {
            "friday" => 5,
            "saturday" => 6,
            _ => -10,
        };
        let weekends = self.exclude_set.contains("weekends");
        for (day, name) in WEEKDAYS.iter().enumerate() {
            let iso = if day == 0 { 7 } else { day };
            let iso = i32::try_from(iso).unwrap_or(0);
            if let Some(slot) = self.excluded_days.get_mut(day) {
                *slot = (weekends && (iso == weekend_start || iso == weekend_start + 1))
                    || self.exclude_set.contains(&lowercase(name));
            }
        }
        // A date written with only these tokens and separators can never read as a weekday
        // name, so when nothing else is excluded or included there is no need to format each day.
        self.dated_tokens = !self.includes.is_empty() || !numeric_format(trim(&self.date_format));
        for token in &self.excludes {
            if token != "weekends" && !WEEKDAYS.iter().any(|day| lowercase(day) == *token) {
                self.dated_tokens = true;
            }
        }

        for task in &mut self.tasks {
            task.start_time = None;
            task.end_time = None;
            task.render_end_time = None;
            task.processed = false;
            task.manual_end_time = false;
        }

        let mut all_processed = self.compile_tasks()?;
        let mut pass = 0;
        while !all_processed && pass < 10 {
            all_processed = self.compile_tasks()?;
            pass += 1;
        }
        Ok(&self.tasks)
    }

    /// Whether a day is excluded. A day matches a token by its weekday name, by its date as
    /// `YYYY-MM-DD`, or by its date written in the chart's own format.
    ///
    /// # Errors
    ///
    /// Returns a limit error when scheduling takes too much work.
    pub(crate) fn is_invalid_date(&mut self, date: Date) -> Result<bool, Error> {
        let weekday = index(date.weekday())
            .and_then(|day| self.excluded_days.get(day).copied())
            .unwrap_or(false);
        if !self.dated_tokens {
            return Ok(weekday);
        }
        let template = trim(&self.date_format).to_owned();
        let date_only = iso_date(date);
        let mut formatted = Some(date_only.clone());
        self.spend(DATED_STEP)?;
        if template != "YYYY-MM-DD" {
            self.spend(FORMAT_STEP + units(&template) * 8.0)?;
            formatted = format(date, &template);
        }
        let listed = |set: &BTreeSet<String>| {
            set.contains(&date_only) || formatted.as_ref().is_some_and(|f| set.contains(f))
        };
        if listed(&self.include_set) {
            return Ok(false);
        }
        Ok(weekday || listed(&self.exclude_set))
    }

    /// Counts `work`, and fails once the total passes the limit.
    fn spend(&mut self, work: f64) -> Result<(), Error> {
        self.work += work;
        if self.work > WORK_LIMIT {
            let mut error = Error::new(
                Code::Limit,
                "The gantt chart takes too much work to schedule.",
            );
            error.r#type = Some("gantt");
            return Err(error);
        }
        Ok(())
    }

    /// Parses `text` strictly with `template`, counting the work.
    fn parse(&mut self, text: &str, template: &str) -> Result<Option<Date>, Error> {
        self.spend((units(template) + 1.0) * (units(text) + 1.0))?;
        Ok(parse_strict(text, template, Date(self.now)))
    }

    /// The start of the day of `now`.
    fn today(&self) -> Date {
        let mut today = Date(js_date::time_clip(self.now));
        today.set_hours(0.0, 0.0, 0.0, 0.0);
        today
    }

    /// Schedules every task once. Whether all of them resolved.
    fn compile_tasks(&mut self) -> Result<bool, Error> {
        let mut all_processed = true;
        for i in 0..self.tasks.len() {
            self.compile_task(i)?;
            all_processed = all_processed && self.tasks.get(i).is_some_and(|task| task.processed);
        }
        Ok(all_processed)
    }

    /// Schedules the task at `i` from its data and the tasks it refers to.
    fn compile_task(&mut self, i: usize) -> Result<(), Error> {
        let Some(task) = self.tasks.get(i) else {
            return Ok(());
        };
        let raw = task.raw.clone();
        let prev = task.prev_task_id.clone();

        let start_time = match &raw.start {
            // Mermaid fails with a TypeError when the first task has no start. It starts today here.
            None => Some(match prev.as_deref().and_then(|id| self.find(id)) {
                Some(previous) => previous.end_time,
                None => Some(self.today()),
            }),
            Some(start) => self.get_start_date(start)?.map(Some),
        };
        let Some(task) = self.tasks.get_mut(i) else {
            return Ok(());
        };
        if let Some(start_time) = start_time {
            task.start_time = start_time;
        }
        let Some(start) = task.start_time else {
            return Ok(());
        };

        let end_time = self.get_end_date(start, &raw.end)?;
        let Some(task) = self.tasks.get_mut(i) else {
            return Ok(());
        };
        task.end_time = end_time;
        if end_time.is_none() {
            return Ok(());
        }
        task.processed = true;
        let manual = self.parse(&raw.end, "YYYY-MM-DD")?.is_some();
        if let Some(task) = self.tasks.get_mut(i) {
            task.manual_end_time = manual;
        }
        self.check_task_dates(i)
    }

    /// The tasks `list` names, skipping ids that do not exist.
    fn references(&self, list: &str) -> Vec<usize> {
        list.split(' ')
            .filter(|id| !id.is_empty())
            .filter_map(|id| self.by_id.get(id).copied())
            .collect()
    }

    /// The start a task's start field gives, or `None` while what it refers to is unresolved.
    fn get_start_date(&mut self, raw: &str) -> Result<Option<Date>, Error> {
        let text = trim(raw);
        let template = trim(&self.date_format).to_owned();

        if (template == "x" || template == "X")
            && !text.is_empty()
            && text.bytes().all(|b| is_digit(u32::from(b)))
        {
            let date = Date(js_date::time_clip(text.parse().unwrap_or(f64::NAN)));
            if !is_valid(date) {
                return Err(invalid_date(text));
            }
            return Ok(Some(date));
        }

        if let Some(list) = reference(text, "after") {
            let mut latest: Option<&GanttTask> = None;
            for i in self.references(list) {
                let Some(task) = self.tasks.get(i) else {
                    continue;
                };
                if latest.is_none_or(|own| time_of(task.end_time) > time_of(own.end_time)) {
                    latest = Some(task);
                }
            }
            return Ok(match latest {
                Some(task) => task.end_time,
                None => Some(self.today()),
            });
        }

        if let Some(parsed) = self.parse(text, &template)? {
            return Ok(Some(parsed));
        }

        // What does not fit the format gets a second chance with the JavaScript date parser, as
        // in Mermaid.
        let date = js_date::parse(text);
        if !is_valid(date) || date.year() < -10000.0 || date.year() > 10000.0 {
            return Err(invalid_date(text));
        }
        Ok(Some(date))
    }

    /// The end a task's end field gives from `start_time`.
    fn get_end_date(&mut self, start_time: Date, raw: &str) -> Result<Option<Date>, Error> {
        let text = trim(raw);

        if let Some(list) = reference(text, "until") {
            let mut earliest: Option<&GanttTask> = None;
            for i in self.references(list) {
                let Some(task) = self.tasks.get(i) else {
                    continue;
                };
                if earliest.is_none_or(|own| time_of(task.start_time) < time_of(own.start_time)) {
                    earliest = Some(task);
                }
            }
            return Ok(match earliest {
                Some(task) => task.start_time,
                None => Some(self.today()),
            });
        }

        let template = trim(&self.date_format).to_owned();
        if let Some(parsed) = self.parse(text, &template)? {
            return Ok(Some(if self.inclusive_end_dates {
                add_days(parsed, 1.0)
            } else {
                parsed
            }));
        }

        let (value, unit) = parse_duration(text);
        if value.is_nan() {
            return Ok(Some(start_time));
        }
        let end = add(start_time, value, unit);
        Ok(Some(if is_valid(end) { end } else { start_time }))
    }

    /// Pushes the end of a task out by one day for every excluded day it covers.
    fn check_task_dates(&mut self, i: usize) -> Result<(), Error> {
        let Some(task) = self.tasks.get(i) else {
            return Ok(());
        };
        if self.excludes.is_empty() || task.manual_end_time {
            return Ok(());
        }
        let (Some(start), Some(end)) = (task.start_time, task.end_time) else {
            return Ok(());
        };

        let mut cursor = add_days(start, 1.0);
        let mut end_time = end;
        let max_end_time = add_days(end_time, 10000.0).time();
        let mut invalid = false;
        let mut render_end_time: Option<f64> = None;
        while cursor.time() <= end_time.time() {
            self.spend(DAY_STEP)?;
            if !invalid {
                render_end_time = Some(end_time.time());
            }
            invalid = self.is_invalid_date(cursor)?;
            if invalid {
                end_time.set_date(end_time.date() + 1.0);
                if end_time.time() > max_end_time {
                    return Err(semantic(
                        "Failed to find a valid date that was not excluded by `excludes` after 10,000 iterations."
                            .to_owned(),
                    ));
                }
            }
            cursor.set_date(cursor.date() + 1.0);
        }

        if let Some(task) = self.tasks.get_mut(i) {
            task.end_time = Some(end_time);
            task.render_end_time = render_end_time.map(Date);
        }
        Ok(())
    }
}
