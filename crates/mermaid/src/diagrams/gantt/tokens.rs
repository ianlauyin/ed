//! Gantt token kinds and their grammar names. Port of Pele's `src/diagrams/gantt/tokens.ts`.

/// A token kind. Variants keep the grammar's order, which `NAMES` and the weekday ranges rely on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum T {
    /// The end of the token list.
    End,
    /// Text no rule matches.
    Error,
    /// `%%{`, which opens a directive the grammar never reads.
    OpenDirective,
    /// `accTitle:`.
    AccTitle,
    /// The text after `accTitle:`.
    AccTitleValue,
    /// `accDescr:`.
    AccDescr,
    /// The text after `accDescr:`.
    AccDescrValue,
    /// The text inside `accDescr { ... }`.
    AccDescrMultilineValue,
    /// One or more newlines.
    Nl,
    /// The target of `href "..."`.
    Href,
    /// The function name after `call`.
    Callbackname,
    /// The arguments inside `call name(...)`.
    Callbackargs,
    /// The task ids after `click`.
    Click,
    /// `gantt`.
    Gantt,
    /// `dateFormat ...`.
    DateFormat,
    /// `inclusiveEndDates`.
    InclusiveEndDates,
    /// `topAxis`.
    TopAxis,
    /// `axisFormat ...`.
    AxisFormat,
    /// `tickInterval ...`.
    TickInterval,
    /// `includes ...`.
    Includes,
    /// `excludes ...`.
    Excludes,
    /// `todayMarker ...`.
    TodayMarker,
    /// `weekday monday`.
    WeekdayMonday,
    /// `weekday tuesday`.
    WeekdayTuesday,
    /// `weekday wednesday`.
    WeekdayWednesday,
    /// `weekday thursday`.
    WeekdayThursday,
    /// `weekday friday`.
    WeekdayFriday,
    /// `weekday saturday`.
    WeekdaySaturday,
    /// `weekday sunday`.
    WeekdaySunday,
    /// `weekend friday`.
    WeekendFriday,
    /// `weekend saturday`.
    WeekendSaturday,
    /// A `YYYY-MM-DD` date.
    Date,
    /// `title ...`.
    Title,
    /// `accDescription ...`.
    AccDescription,
    /// `section ...`.
    Section,
    /// A task's label.
    TaskTxt,
    /// A task's `:` and data.
    TaskData,
    /// A lone `:`.
    Colon,
    /// The end of the source.
    Eof,
}

/// Each kind's name in the grammar, as parse errors print it.
const NAMES: [&str; 39] = [
    "$end",
    "INVALID",
    "open_directive",
    "acc_title",
    "acc_title_value",
    "acc_descr",
    "acc_descr_value",
    "acc_descr_multiline_value",
    "NL",
    "href",
    "callbackname",
    "callbackargs",
    "click",
    "gantt",
    "dateFormat",
    "inclusiveEndDates",
    "topAxis",
    "axisFormat",
    "tickInterval",
    "includes",
    "excludes",
    "todayMarker",
    "weekday_monday",
    "weekday_tuesday",
    "weekday_wednesday",
    "weekday_thursday",
    "weekday_friday",
    "weekday_saturday",
    "weekday_sunday",
    "weekend_friday",
    "weekend_saturday",
    "date",
    "title",
    "accDescription",
    "section",
    "taskTxt",
    "taskData",
    ":",
    "EOF",
];

/// The weekday kinds, from Monday.
pub(crate) const WEEKDAYS: [T; 7] = [
    T::WeekdayMonday,
    T::WeekdayTuesday,
    T::WeekdayWednesday,
    T::WeekdayThursday,
    T::WeekdayFriday,
    T::WeekdaySaturday,
    T::WeekdaySunday,
];

impl T {
    /// The kind's grammar name.
    pub(crate) fn name(self) -> &'static str {
        NAMES.get(self as usize).copied().unwrap_or("")
    }
}
