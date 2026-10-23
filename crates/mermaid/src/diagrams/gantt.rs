//! Gantt charts: tasks on a time axis, with sections, dependencies, excluded days, and
//! milestones. Port of Pele's `src/diagrams/gantt/index.ts`.
//!
//! Pele reads and writes dates in the host's local time zone. Ondine works in UTC, so it matches
//! Pele running with `TZ=UTC`. Pele schedules against the current time when the host gives no
//! `now`. The compiler reads no clock, so it schedules against the epoch instead.

pub(crate) mod axis;
pub(crate) mod dates;
pub(crate) mod db;
pub(crate) mod js_date;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod render;
pub(crate) mod tokens;
pub(crate) mod types;

use super::{Diagram, Input, Rendered};
use crate::error::Error;
use crate::svg::root::RenderOptions;
use crate::util::url::UrlOptions;
use crate::yaml::Value;

/// The `gantt` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram {
    keep_comments: false,
    render: render_input,
    section: None,
};

/// Parses and draws a gantt chart.
fn render_input(
    input: &Input<'_>,
    options: &RenderOptions,
    url: &UrlOptions<'_>,
) -> Result<Rendered, Error> {
    let mut db = parse(input.source, input.config, input.title)?;
    render::render(&mut db, input.config, options, url)
}

/// The parsed gantt model: Pele's `GanttDb`.
pub(crate) use db::GanttDb as Model;

/// Parses preprocessed gantt `source`, as Pele's `gantt.parse` does. `config` is the diagram
/// configuration, of which `gantt.displayMode` is read. `title` comes from front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for a task with no
/// end or too many fields.
pub(crate) fn parse(source: &str, config: &Value, title: Option<&str>) -> Result<Model, Error> {
    let mut db = Model::new();
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        db.title = Some(title.to_owned());
    }
    if let Some(Value::String(mode)) =
        property(config, "gantt").and_then(|g| property(g, "displayMode"))
    {
        mode.clone_into(&mut db.display_mode);
    }
    parser::parse_gantt(source, &mut db)?;
    Ok(db)
}

/// A property of a YAML mapping, as JavaScript's `value.key` reads it.
fn property<'v>(value: &'v Value, key: &str) -> Option<&'v Value> {
    let Value::Object(entries) = value else {
        return None;
    };
    entries
        .iter()
        .find(|(own, _)| own == key)
        .map(|(_, value)| value)
}

/// Serializes `model`, scheduled against `now`, as `JSON.stringify` serializes Pele's parsed
/// `GanttDb` after this normalization, which the parity tests apply on Pele's side:
///
/// - The top level lists the model's public fields in the order below. `links` becomes a plain
///   object with `Object.fromEntries`, so integer-like ids come first.
/// - Each task lists its fields in the order below, with dates as their time values.
/// - `getTasks(now)` runs first. When it throws, `schedule` holds the error's code and message,
///   and tasks keep the state the scheduler left them in.
///
/// ```ts
/// const time = (d) => (d == null ? d : +d);
/// let schedule;
/// try { db.getTasks(now); } catch (e) { schedule = { code: e.code, message: e.message }; }
/// JSON.stringify({
///   type: 'gantt', title: db.title, accTitle: db.accTitle, accDescr: db.accDescr,
///   dateFormat: db.dateFormat, axisFormat: db.axisFormat, tickInterval: db.tickInterval,
///   todayMarker: db.todayMarker, includes: db.includes, excludes: db.excludes,
///   inclusiveEndDates: db.inclusiveEndDates, topAxis: db.topAxis, displayMode: db.displayMode,
///   weekday: db.weekday, weekend: db.weekend, sections: db.sections,
///   links: Object.fromEntries(db.links),
///   tasks: db.tasks.map((t) => ({ id: t.id, task: t.task, section: t.section,
///     startTime: time(t.startTime), endTime: time(t.endTime), renderEndTime: time(t.renderEndTime),
///     manualEndTime: t.manualEndTime, processed: t.processed, active: t.active, done: t.done,
///     crit: t.crit, milestone: t.milestone, vert: t.vert, order: t.order, classes: t.classes,
///     prevTaskId: t.prevTaskId, raw: { data: t.raw.data, start: t.raw.start, end: t.raw.end } })),
///   schedule,
/// });
/// ```
#[cfg(feature = "model")]
pub(crate) fn to_json(model: &Model, now: f64) -> String {
    let mut model = model.clone();
    let schedule = model.get_tasks(now).err();

    let mut json = json::Writer::default();
    json.open('{');
    json.key("type");
    json.string("gantt");
    json.optional_string("title", model.title.as_deref());
    json.optional_string("accTitle", model.acc_title.as_deref());
    json.optional_string("accDescr", model.acc_descr.as_deref());
    json.key("dateFormat");
    json.string(&model.date_format);
    json.key("axisFormat");
    json.string(&model.axis_format);
    json.optional_string("tickInterval", model.tick_interval.as_deref());
    json.key("todayMarker");
    json.string(&model.today_marker);
    json.key("includes");
    json.strings(&model.includes);
    json.key("excludes");
    json.strings(&model.excludes);
    json.key("inclusiveEndDates");
    json.boolean(model.inclusive_end_dates);
    json.key("topAxis");
    json.boolean(model.top_axis);
    json.key("displayMode");
    json.string(&model.display_mode);
    json.optional_string("weekday", model.weekday.as_deref());
    json.key("weekend");
    json.string(&model.weekend);
    json.key("sections");
    json.strings(&model.sections);

    json.key("links");
    json.open('{');
    for (id, href) in json::property_order(&model.links) {
        json.key(id);
        json.string(href);
    }
    json.close('}');

    json.key("tasks");
    json.open('[');
    for task in &model.tasks {
        json.item();
        json.open('{');
        json.key("id");
        json.string(&task.id);
        json.key("task");
        json.string(&task.task);
        json.key("section");
        json.string(&task.section);
        if let Some(start) = task.start_time {
            json.key("startTime");
            json.number(start.time());
        }
        if let Some(end) = task.end_time {
            json.key("endTime");
            json.number(end.time());
        }
        json.key("renderEndTime");
        json.number(task.render_end_time.map_or(f64::NAN, js_date::Date::time));
        for (key, value) in [
            ("manualEndTime", task.manual_end_time),
            ("processed", task.processed),
            ("active", task.active),
            ("done", task.done),
            ("crit", task.crit),
            ("milestone", task.milestone),
            ("vert", task.vert),
        ] {
            json.key(key);
            json.boolean(value);
        }
        json.key("order");
        json.number(f64::from(task.order));
        json.key("classes");
        json.strings(&task.classes);
        json.optional_string("prevTaskId", task.prev_task_id.as_deref());
        json.key("raw");
        json.open('{');
        json.key("data");
        json.string(&task.raw.data);
        json.optional_string("start", task.raw.start.as_deref());
        json.key("end");
        json.string(&task.raw.end);
        json.close('}');
        json.close('}');
    }
    json.close(']');

    if let Some(error) = schedule {
        json.key("schedule");
        json.open('{');
        json.key("code");
        json.string(error.code.as_str());
        json.key("message");
        json.string(&error.message);
        json.close('}');
    }
    json.close('}');
    json.out
}

/// A JSON writer that matches `JSON.stringify` output.
#[cfg(feature = "model")]
mod json {
    use crate::yaml::push_number;

    /// Output and comma state.
    #[derive(Default)]
    pub(super) struct Writer {
        /// The JSON text.
        pub(super) out: String,
        /// Whether the next member or item is the first of its container.
        first: bool,
    }

    impl Writer {
        /// Opens an object or array.
        pub(super) fn open(&mut self, bracket: char) {
            self.out.push(bracket);
            self.first = true;
        }

        /// Closes an object or array.
        pub(super) fn close(&mut self, bracket: char) {
            self.out.push(bracket);
            self.first = false;
        }

        /// Starts an array item.
        pub(super) fn item(&mut self) {
            if !self.first {
                self.out.push(',');
            }
            self.first = false;
        }

        /// Starts an object member.
        pub(super) fn key(&mut self, key: &str) {
            self.item();
            self.string(key);
            self.out.push(':');
        }

        /// Writes `true` or `false`.
        pub(super) fn boolean(&mut self, value: bool) {
            self.out.push_str(if value { "true" } else { "false" });
        }

        /// Writes a string as `JSON.stringify` quotes it.
        pub(super) fn string(&mut self, text: &str) {
            self.out.push('"');
            for c in text.chars() {
                match c {
                    '"' => self.out.push_str("\\\""),
                    '\\' => self.out.push_str("\\\\"),
                    '\u{8}' => self.out.push_str("\\b"),
                    '\u{c}' => self.out.push_str("\\f"),
                    '\n' => self.out.push_str("\\n"),
                    '\r' => self.out.push_str("\\r"),
                    '\t' => self.out.push_str("\\t"),
                    c if u32::from(c) < 0x20 => {
                        let code = u32::from(c);
                        self.out.push_str("\\u00");
                        for nibble in [code >> 4, code & 15] {
                            self.out.push(char::from_digit(nibble, 16).unwrap_or('0'));
                        }
                    }
                    c => self.out.push(c),
                }
            }
            self.out.push('"');
        }

        /// Writes a number, or `null` for one JSON cannot hold.
        pub(super) fn number(&mut self, value: f64) {
            if value.is_finite() {
                push_number(&mut self.out, value);
            } else {
                self.out.push_str("null");
            }
        }

        /// Writes an array of strings.
        pub(super) fn strings(&mut self, items: &[String]) {
            self.open('[');
            for item in items {
                self.item();
                self.string(item);
            }
            self.close(']');
        }

        /// Writes a string member, or nothing for an unset one, as `undefined` is dropped.
        pub(super) fn optional_string(&mut self, key: &str, value: Option<&str>) {
            if let Some(value) = value {
                self.key(key);
                self.string(value);
            }
        }
    }

    /// The array index a property key names, as JavaScript orders own properties.
    fn array_index(key: &str) -> Option<u32> {
        if key != "0" && (key.is_empty() || key.starts_with('0')) {
            return None;
        }
        if !key.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        key.parse::<u32>().ok().filter(|&index| index != u32::MAX)
    }

    /// Entries in JavaScript property order: array-index keys ascending, then the rest in order.
    pub(super) fn property_order(entries: &[(String, String)]) -> Vec<(&str, &str)> {
        let mut indexed: Vec<(u32, &(String, String))> = entries
            .iter()
            .filter_map(|entry| array_index(&entry.0).map(|index| (index, entry)))
            .collect();
        indexed.sort_by_key(|&(index, _)| index);
        let rest = entries
            .iter()
            .filter(|entry| array_index(&entry.0).is_none());
        indexed
            .into_iter()
            .map(|(_, entry)| entry)
            .chain(rest)
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write;

    use super::parse;
    use crate::yaml::Value;

    /// The parity fixture: Pele's parser input with its normalized model JSON or error,
    /// generated by running Pele's TypeScript with `TZ=UTC`.
    const FIXTURE: &str = include_str!("../../tests/fixtures/gantt-parse.json");

    #[derive(Debug)]
    enum Json {
        Arr(Vec<Json>),
        Bool(bool),
        Null,
        Num(f64),
        Obj(Vec<(String, Json)>),
        Str(String),
    }

    impl Json {
        fn get(&self, key: &str) -> Option<&Json> {
            let Json::Obj(entries) = self else {
                return None;
            };
            entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
        }

        fn str(&self, key: &str) -> Option<&str> {
            match self.get(key) {
                Some(Json::Str(s)) => Some(s),
                _ => None,
            }
        }

        fn num(&self, key: &str) -> Option<f64> {
            match self.get(key) {
                Some(Json::Num(n)) => Some(*n),
                _ => None,
            }
        }
    }

    struct Reader<'a> {
        bytes: &'a [u8],
        at: usize,
    }

    impl Reader<'_> {
        fn ws(&mut self) {
            while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
                self.at += 1;
            }
        }

        fn value(&mut self) -> Json {
            self.ws();
            match self.bytes[self.at] {
                b'{' => {
                    self.at += 1;
                    let mut entries = Vec::new();
                    loop {
                        self.ws();
                        match self.bytes[self.at] {
                            b'}' => {
                                self.at += 1;
                                return Json::Obj(entries);
                            }
                            b',' => self.at += 1,
                            _ => {
                                let Json::Str(key) = self.value() else {
                                    panic!("key")
                                };
                                self.ws();
                                assert_eq!(self.bytes[self.at], b':');
                                self.at += 1;
                                entries.push((key, self.value()));
                            }
                        }
                    }
                }
                b'[' => {
                    self.at += 1;
                    let mut items = Vec::new();
                    loop {
                        self.ws();
                        match self.bytes[self.at] {
                            b']' => {
                                self.at += 1;
                                return Json::Arr(items);
                            }
                            b',' => self.at += 1,
                            _ => items.push(self.value()),
                        }
                    }
                }
                b'"' => {
                    self.at += 1;
                    let mut units: Vec<u16> = Vec::new();
                    loop {
                        let c = self.bytes[self.at];
                        self.at += 1;
                        match c {
                            b'"' => break,
                            b'\\' => {
                                let e = self.bytes[self.at];
                                self.at += 1;
                                units.push(match e {
                                    b'n' => 10,
                                    b'r' => 13,
                                    b't' => 9,
                                    b'b' => 8,
                                    b'f' => 12,
                                    b'u' => {
                                        let hex =
                                            std::str::from_utf8(&self.bytes[self.at..self.at + 4])
                                                .unwrap();
                                        self.at += 4;
                                        u16::from_str_radix(hex, 16).unwrap()
                                    }
                                    other => u16::from(other),
                                });
                            }
                            _ => {
                                let start = self.at - 1;
                                let len = match c {
                                    0..0x80 => 1,
                                    0xc0..0xe0 => 2,
                                    0xe0..0xf0 => 3,
                                    _ => 4,
                                };
                                let s =
                                    std::str::from_utf8(&self.bytes[start..start + len]).unwrap();
                                units.extend(s.encode_utf16());
                                self.at = start + len;
                            }
                        }
                    }
                    Json::Str(String::from_utf16(&units).unwrap())
                }
                b't' => {
                    self.at += 4;
                    Json::Bool(true)
                }
                b'f' => {
                    self.at += 5;
                    Json::Bool(false)
                }
                b'n' => {
                    self.at += 4;
                    Json::Null
                }
                _ => {
                    let start = self.at;
                    while self
                        .bytes
                        .get(self.at)
                        .is_some_and(|b| b"+-.eE0123456789".contains(b))
                    {
                        self.at += 1;
                    }
                    let text = std::str::from_utf8(&self.bytes[start..self.at]).unwrap();
                    Json::Num(text.parse().unwrap())
                }
            }
        }
    }

    /// A model's JSON (when the `model` feature is on, otherwise empty), or an error's code,
    /// message, line, and column.
    type Outcome = Result<String, (String, String, u32, u32)>;

    fn actual(case: &Json) -> Outcome {
        let input = case.str("input").unwrap();
        let mut gantt = Vec::new();
        if let Some(mode) = case.str("displayMode") {
            gantt.push(("displayMode".to_owned(), Value::String(mode.to_owned())));
        }
        let config = Value::Object(vec![("gantt".to_owned(), Value::Object(gantt))]);
        match parse(input, &config, case.str("title")) {
            #[cfg(feature = "model")]
            Ok(model) => Ok(super::to_json(&model, case.num("now").unwrap())),
            #[cfg(not(feature = "model"))]
            Ok(_) => Ok(String::new()),
            Err(error) => Err((
                error.code.as_str().to_owned(),
                error.message,
                error.line,
                error.column,
            )),
        }
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "test data"
    )]
    fn expected(case: &Json) -> Outcome {
        if let Some(model) = case.str("model") {
            return Ok(if cfg!(feature = "model") {
                model.to_owned()
            } else {
                String::new()
            });
        }
        let error = case.get("error").unwrap();
        Err((
            error.str("code").unwrap().to_owned(),
            error.str("message").unwrap().to_owned(),
            error.num("line").unwrap() as u32,
            error.num("column").unwrap() as u32,
        ))
    }

    #[test]
    fn matches_pele() {
        let Json::Arr(cases) = (Reader {
            bytes: FIXTURE.as_bytes(),
            at: 0,
        })
        .value() else {
            panic!("fixture is not an array");
        };

        let mut report = String::new();
        let mut differing = 0;
        for case in &cases {
            let (actual, expected) = (actual(case), expected(case));
            if actual == expected {
                continue;
            }
            differing += 1;
            if differing <= 20 {
                write!(
                    report,
                    "\n{}\n  input: {:?}\n  pele:  {expected:?}\n  ondine: {actual:?}",
                    case.str("origin").unwrap_or("?"),
                    case.str("input").unwrap(),
                )
                .unwrap();
            }
        }
        assert!(
            differing == 0,
            "{differing} of {} cases differ from Pele:{report}",
            cases.len()
        );
    }
}
