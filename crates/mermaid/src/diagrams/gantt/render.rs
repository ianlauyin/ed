//! Lays out and draws a gantt chart: a time axis, one row per task (or packed rows in compact
//! mode), section bands, excluded days, vertical markers, and the today marker. Port of Pele's
//! `src/diagrams/gantt/render.ts`.

use std::cmp::Ordering;

use super::axis::{auto_ticks, interval_ticks, time_format};
use super::dates::{Unit, add, add_days, int_string, js_max, js_min};
use super::db::{GanttDb, clock};
use super::js_date::Date;
use super::types::GanttTask;
use crate::diagrams::Rendered;
use crate::diagrams::common::fit_width::cap_width;
use crate::error::Error;
use crate::svg::builder::{esc, esc_text, js_round, label_svg, push_num};
use crate::svg::root::{Accessible, RenderOptions, svg_document};
use crate::svg::theme::{CRITICAL, RADIUS, resolve_style};
use crate::text::label::{Label, layout_label};
use crate::text::measure::{self, BOLD};
use crate::util::chars::is_space;
use crate::util::url::{LinkInfo, UrlOptions, linked};
use crate::yaml::Value;

/// The default width of a chart.
const WIDTH: f64 = 800.0;
/// The narrowest the plot gets.
const MIN_PLOT: f64 = 240.0;
/// Space between a bar and its label.
const LABEL_GAP: f64 = 8.0;

/// The `gantt` section of the configuration.
fn section(config: &Value) -> Option<&Value> {
    let Value::Object(entries) = config else {
        return None;
    };
    entries
        .iter()
        .find(|(key, _)| key == "gantt")
        .map(|(_, value)| value)
}

/// A setting of the `gantt` section.
fn get<'v>(config: &'v Value, key: &str) -> Option<&'v Value> {
    let Some(Value::Object(entries)) = section(config) else {
        return None;
    };
    entries
        .iter()
        .find(|(own, _)| own == key)
        .map(|(_, value)| value)
}

/// A finite number setting, clamped to `min` and `max`.
fn setting(config: &Value, key: &str, min: f64, max: f64) -> Option<f64> {
    match get(config, key) {
        Some(Value::Number(n)) if n.is_finite() => Some(n.max(min).min(max)),
        _ => None,
    }
}

/// A string setting.
fn text<'v>(config: &'v Value, key: &str) -> Option<&'v str> {
    match get(config, key) {
        Some(Value::String(s)) => Some(s),
        _ => None,
    }
}

/// Whether a setting is `true`.
fn flag(config: &Value, key: &str) -> bool {
    matches!(get(config, key), Some(Value::Bool(true)))
}

/// `raw` with each `<br>`, `<br/>`, or `<br />` (any case) replaced by a space.
fn no_breaks(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut start = 0;
    let mut at = 0;
    while at < bytes.len() {
        let end = (bytes.get(at) == Some(&b'<')
            && bytes
                .get(at + 1)
                .is_some_and(|b| b.eq_ignore_ascii_case(&b'b'))
            && bytes
                .get(at + 2)
                .is_some_and(|b| b.eq_ignore_ascii_case(&b'r')))
        .then(|| {
            let mut end = at + 3;
            while let Some(c) = raw.get(end..).and_then(|rest| rest.chars().next()) {
                if !is_space(u32::from(c)) {
                    break;
                }
                end += c.len_utf8();
            }
            if bytes.get(end) == Some(&b'/') {
                end += 1;
            }
            (bytes.get(end) == Some(&b'>')).then_some(end + 1)
        })
        .flatten();
        if let Some(end) = end {
            out.push_str(raw.get(start..at).unwrap_or(""));
            out.push(' ');
            start = end;
            at = end;
        } else {
            at += 1;
        }
    }
    out.push_str(raw.get(start..).unwrap_or(""));
    out
}

/// A task label sits on one row, so only its first line is kept.
fn one_line(raw: &str, size: f64) -> Label {
    let mut label = layout_label(Some(&no_breaks(raw)), false, size, 4000.0, 0, None);
    if label.lines.len() <= 1 {
        return label;
    }
    label.lines.truncate(1);
    label.widths.truncate(1);
    label.width = label.widths.first().copied().unwrap_or(0.0);
    label.height = label.line_height;
    label
}

/// The task's start as a time value, NaN when unset.
fn start_of(task: &GanttTask) -> f64 {
    task.start_time.map_or(f64::NAN, Date::time)
}

/// The task's end as a time value, NaN when unset.
fn end_of(task: &GanttTask) -> f64 {
    task.end_time.map_or(f64::NAN, Date::time)
}

/// Whether scheduling gave the task a start and an end. One that waits on a task that never
/// resolves has neither.
fn scheduled(task: &GanttTask) -> bool {
    start_of(task).is_finite() && end_of(task).is_finite()
}

/// Mermaid's compact mode: within a section, a task goes on the first row that is free when it
/// starts. Returns the number of rows the section takes.
fn pack_rows(
    tasks: &[GanttTask],
    group: &[usize],
    offset: usize,
    row_of: &mut [Option<usize>],
) -> usize {
    let mut sorted: Vec<usize> = group
        .iter()
        .copied()
        .filter(|&i| tasks.get(i).is_some_and(scheduled))
        .collect();
    let key = |i: usize| {
        tasks
            .get(i)
            .map_or((f64::NAN, 0), |t| (start_of(t), t.order))
    };
    sorted.sort_by(|&a, &b| {
        let (a_start, a_order) = key(a);
        let (b_start, b_order) = key(b);
        a_start
            .partial_cmp(&b_start)
            .unwrap_or(Ordering::Equal)
            .then(a_order.cmp(&b_order))
    });

    let mut free: Vec<f64> = Vec::new();
    for i in sorted {
        let Some(task) = tasks.get(i) else {
            continue;
        };
        let start = start_of(task);
        let mut row = 0;
        while free.get(row).is_some_and(|&end| start < end) {
            row += 1;
        }
        if let Some(slot) = free.get_mut(row) {
            *slot = end_of(task);
        } else {
            free.push(end_of(task));
        }
        if let Some(slot) = row_of.get_mut(i) {
            *slot = Some(offset + row);
        }
    }
    let mut rows = free.len();
    for &i in group {
        if let Some(slot) = row_of.get_mut(i).filter(|slot| slot.is_none()) {
            *slot = Some(offset + rows);
            rows += 1;
        }
    }
    rows.max(1)
}

/// A drawn task bar and where its label goes.
struct Bar {
    /// Whether the label fits inside the bar.
    inside: bool,
    /// The task label.
    label: Label,
    /// The center of the label.
    label_x: f64,
    /// The row the bar is on.
    row: usize,
    /// The index of the task.
    task: usize,
    /// The left edge, NaN for an unscheduled task.
    x0: f64,
    /// The right edge, NaN for an unscheduled task.
    x1: f64,
}

/// The `x` attribute value of a `<text>` and its `y`, written as `x="..." y="..."`.
fn push_xy(out: &mut String, x: f64, y: f64) {
    out.push_str("<text x=\"");
    push_num(out, x);
    out.push_str("\" y=\"");
    push_num(out, y);
    out.push_str("\" ");
}

/// Draws a gantt chart, scheduling its tasks against the host's `now`.
///
/// # Errors
///
/// Returns the scheduler's errors: a start that is not a date, or too much work.
#[expect(
    clippy::too_many_lines,
    reason = "mirrors Pele's single render function"
)]
pub(crate) fn render(
    model: &mut GanttDb,
    config: &Value,
    options: &RenderOptions,
    url: &UrlOptions<'_>,
) -> Result<Rendered, Error> {
    let size = options.font_size.unwrap_or(16.0);
    let pad = options.padding.unwrap_or(8.0);
    let now = clock(options.now);

    let label_size =
        setting(config, "fontSize", 4.0, 72.0).unwrap_or_else(|| js_round(size * 0.8125));
    let section_size = setting(config, "sectionFontSize", 4.0, 72.0).unwrap_or(label_size);
    let tick_size = js_round(size * 0.75);
    let bar_height =
        setting(config, "barHeight", 4.0, 400.0).unwrap_or_else(|| js_round(label_size * 1.85));
    let bar_gap =
        setting(config, "barGap", 0.0, 400.0).unwrap_or_else(|| js_round(bar_height / 4.0));
    let row_height = bar_height + bar_gap;
    let section_styles =
        js_round(setting(config, "numberSectionStyles", 1.0, 1000.0).unwrap_or(4.0));
    let compact = model.display_mode == "compact";
    let top_axis = model.top_axis || flag(config, "topAxis");

    let tasks: Vec<GanttTask> = model.get_tasks(now)?.to_vec();
    let row_tasks: Vec<usize> = (0..tasks.len())
        .filter(|&i| tasks.get(i).is_some_and(|t| !t.vert))
        .collect();
    let mut verts: Vec<usize> = (0..tasks.len())
        .filter(|&i| tasks.get(i).is_some_and(|t| t.vert && scheduled(t)))
        .collect();

    let mut categories: Vec<&str> = Vec::new();
    for &i in &row_tasks {
        if let Some(task) = tasks.get(i)
            && !categories.contains(&task.section.as_str())
        {
            categories.push(&task.section);
        }
    }
    let category = |section: &str| {
        categories
            .iter()
            .position(|own| *own == section)
            .unwrap_or(0)
    };

    let mut row_of: Vec<Option<usize>> = vec![None; tasks.len()];
    let mut row_section: Vec<&str> = Vec::new();
    if compact {
        let mut groups: Vec<(&str, Vec<usize>)> = Vec::new();
        for &i in &row_tasks {
            let Some(task) = tasks.get(i) else {
                continue;
            };
            if let Some((_, group)) = groups.iter_mut().find(|(own, _)| *own == task.section) {
                group.push(i);
            } else {
                groups.push((&task.section, vec![i]));
            }
        }
        for (section, group) in &groups {
            let rows = pack_rows(&tasks, group, row_section.len(), &mut row_of);
            for _ in 0..rows {
                row_section.push(section);
            }
        }
    } else {
        for &i in &row_tasks {
            if let (Some(slot), Some(task)) = (row_of.get_mut(i), tasks.get(i)) {
                *slot = Some(row_section.len());
                row_section.push(&task.section);
            }
        }
    }
    let row_count = row_section.len();

    let mut start = f64::INFINITY;
    let mut stop = f64::NEG_INFINITY;
    for task in tasks.iter().filter(|t| scheduled(t)) {
        start = start.min(start_of(task));
        stop = stop.max(end_of(task));
    }
    let has_plot = start <= stop;

    let title = layout_label(model.title.as_deref(), false, size, 4000.0, BOLD, None);
    let title_height = if title.height > 0.0 {
        title.height + 16.0
    } else {
        0.0
    };
    let (acc_descr, acc_title) = (model.acc_descr.clone(), model.acc_title.clone());
    let acc = Accessible {
        acc_descr: acc_descr.as_deref(),
        acc_title: acc_title.as_deref(),
    };

    if !has_plot {
        let width = (title.width.max(1.0) + 2.0 * pad).ceil();
        let height = (title.height.max(1.0) + 2.0 * pad).ceil();
        let content = label_svg(
            &title,
            width / 2.0,
            pad + title.height / 2.0,
            " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
            None,
        );
        return Ok(Rendered {
            height,
            links: Vec::new(),
            svg: svg_document("gantt", width, height, size, options, acc, &content),
            width,
        });
    }

    let mut section_labels: Vec<Label> = Vec::with_capacity(categories.len());
    let mut section_width: f64 = 0.0;
    for section in &categories {
        let label = layout_label(Some(section), false, section_size, 180.0, 0, None);
        section_width = js_max(section_width, label.width);
        section_labels.push(label);
    }

    let axis_format = if !model.axis_format.is_empty() {
        model.axis_format.as_str()
    } else if model.date_format == "D" {
        "%d"
    } else {
        text(config, "axisFormat").unwrap_or("%Y-%m-%d")
    };
    let tick_label = |t: f64| time_format(axis_format, Date(t));
    let sample_width = js_max(
        measure::width(&tick_label(start), tick_size, 0),
        measure::width(&tick_label(stop), tick_size, 0),
    );

    let left = pad
        + setting(config, "leftPadding", 0.0, 2000.0).unwrap_or_else(|| {
            js_max(
                if section_width > 0.0 {
                    section_width + 24.0
                } else {
                    0.0
                },
                sample_width / 2.0,
            )
        });
    let right_padding = setting(config, "rightPadding", 0.0, 2000.0)
        .unwrap_or_else(|| js_max(16.0, sample_width / 2.0));
    let target = cap_width(
        options,
        setting(config, "useWidth", 100.0, 20000.0).unwrap_or(WIDTH),
        280.0,
        0.0,
    );
    let plot_width = js_max(MIN_PLOT, target - left - right_padding - pad);
    let right = left + plot_width;
    let scale = if stop > start {
        plot_width / (stop - start)
    } else {
        0.0
    };
    let x = |t: f64| {
        if scale == 0.0 {
            left + plot_width / 2.0
        } else {
            left + (t - start) * scale
        }
    };

    let tick_height = js_round(tick_size * 1.75);
    let top_padding = setting(config, "topPadding", 0.0, 2000.0);
    let grid_start = setting(config, "gridLineStartPadding", 0.0, 2000.0);
    let head_height = pad + title_height + if top_axis { tick_height } else { 0.0 };
    let plot_top = js_max(head_height, top_padding.unwrap_or(0.0));
    // Mermaid starts its grid lines above the first row by the difference between these two
    // settings. Here that only uses room a larger `topPadding` leaves free.
    let grid_lead = match (top_padding, grid_start) {
        (Some(top), Some(grid)) => js_max(0.0, js_min(top - grid, plot_top - head_height)),
        _ => 0.0,
    };
    #[expect(clippy::cast_precision_loss, reason = "row counts stay small")]
    let plot_height = row_count.max(1) as f64 * row_height;
    let plot_bottom = plot_top + plot_height;

    // Bars and where their labels go: inside when they fit, otherwise beside the bar on the side
    // with room.
    let mut bars: Vec<Bar> = Vec::with_capacity(row_tasks.len());
    let mut extent = right + right_padding;
    for &i in &row_tasks {
        let Some(task) = tasks.get(i) else {
            continue;
        };
        let label = one_line(&task.task, label_size);
        let row = row_of.get(i).copied().flatten().unwrap_or(0);
        if !scheduled(task) {
            let label_x = left + LABEL_GAP + label.width / 2.0;
            extent = js_max(extent, label_x + label.width / 2.0);
            bars.push(Bar {
                inside: false,
                label,
                label_x,
                row,
                task: i,
                x0: f64::NAN,
                x1: f64::NAN,
            });
            continue;
        }
        let mut x0 = x(start_of(task));
        let render_end = task
            .render_end_time
            .map_or_else(|| end_of(task), Date::time);
        let mut x1 = js_max(x(render_end), x0 + if task.milestone { 0.0 } else { 2.0 });
        if task.milestone {
            let mid = x0 + (x(end_of(task)) - x0) / 2.0;
            x0 = mid - bar_height / 2.0;
            x1 = mid + bar_height / 2.0;
        }
        let inside = !task.milestone && label.width + 2.0 * LABEL_GAP <= x1 - x0;
        let mut label_x = f64::midpoint(x0, x1);
        if !inside {
            let fits_right = x1 + LABEL_GAP + label.width <= right + right_padding;
            let fits_left = x0 - LABEL_GAP - label.width >= left;
            label_x = if fits_right || !fits_left {
                x1 + LABEL_GAP + label.width / 2.0
            } else {
                x0 - LABEL_GAP - label.width / 2.0
            };
            extent = js_max(extent, label_x + label.width / 2.0);
        }
        bars.push(Bar {
            inside,
            label,
            label_x,
            row,
            task: i,
            x0,
            x1,
        });
    }

    let interval = model
        .tick_interval
        .as_deref()
        .filter(|interval| !interval.is_empty())
        .or_else(|| text(config, "tickInterval").filter(|interval| !interval.is_empty()))
        .unwrap_or("");
    let weekday = model
        .weekday
        .as_deref()
        .or_else(|| text(config, "weekday"))
        .unwrap_or("sunday");
    let count = js_max(
        2.0,
        js_min(10.0, (plot_width / (sample_width + 24.0)).floor()),
    );
    let ticks = interval_ticks(interval, weekday, start, stop, (plot_width / 3.0).ceil())
        .unwrap_or_else(|| auto_ticks(start, stop, count));

    let mut grid = String::new();
    let mut axis_bottom = String::new();
    let mut axis_top = String::new();
    let mut last_right = f64::NEG_INFINITY;
    let mut last_label = String::new();
    let bottom_y = plot_bottom + tick_height / 2.0 + 2.0;
    let top_y = plot_top - grid_lead - tick_height / 2.0 - 2.0;
    for &t in &ticks {
        let tx = x(t);
        grid.push('M');
        push_num(&mut grid, tx);
        grid.push(',');
        push_num(&mut grid, plot_top - grid_lead);
        grid.push('V');
        push_num(&mut grid, plot_bottom);
        let label = tick_label(t);
        let width = measure::width(&label, tick_size, 0);
        // A label is skipped when it would run into the one before it or only repeat it. Its
        // grid line stays.
        if tx - width / 2.0 < last_right + 8.0 || label == last_label {
            continue;
        }
        last_right = tx + width / 2.0;
        extent = js_max(extent, last_right);
        let mut body = String::from("text-anchor=\"middle\">");
        body.push_str(&esc(&label));
        body.push_str("</text>");
        push_xy(&mut axis_bottom, tx, bottom_y + tick_size * 0.35);
        axis_bottom.push_str(&body);
        if top_axis {
            push_xy(&mut axis_top, tx, top_y + tick_size * 0.35);
            axis_top.push_str(&body);
        }
        last_label = label;
    }

    // Vertical markers take no row. Their labels go under the axis, on as many lines as it takes
    // to keep them apart.
    let mut vert_lines = String::new();
    let mut vert_labels = String::new();
    let mut vert_ends: Vec<f64> = Vec::new();
    let vert_top = plot_bottom + tick_height + 4.0;
    let vert_line = js_round(label_size * 1.5);
    verts.sort_by(|&a, &b| {
        let key = |i: usize| tasks.get(i).map_or(f64::NAN, start_of);
        key(a).partial_cmp(&key(b)).unwrap_or(Ordering::Equal)
    });
    for &i in &verts {
        let Some(task) = tasks.get(i) else {
            continue;
        };
        let vx = x(start_of(task));
        let label = one_line(&task.task, label_size);
        let cx = js_max(pad + label.width / 2.0, vx);
        let mut line = 0;
        while vert_ends
            .get(line)
            .is_some_and(|&end| cx - label.width / 2.0 < end + 8.0)
        {
            line += 1;
        }
        let end = cx + label.width / 2.0;
        if let Some(slot) = vert_ends.get_mut(line) {
            *slot = end;
        } else {
            vert_ends.push(end);
        }
        extent = js_max(extent, end);
        let id = esc_text(&task.id);
        vert_lines.push_str("<path class=\"ondine-vert\" data-id=\"");
        vert_lines.push_str(&id);
        vert_lines.push_str("\" d=\"M");
        push_num(&mut vert_lines, vx);
        vert_lines.push(',');
        push_num(&mut vert_lines, plot_top - grid_lead);
        vert_lines.push('V');
        push_num(&mut vert_lines, plot_bottom);
        vert_lines.push_str("\"/>");
        let mut attrs = String::from(" class=\"ondine-vert-label\" data-id=\"");
        attrs.push_str(&id);
        attrs.push('"');
        #[expect(clippy::cast_precision_loss, reason = "line counts stay small")]
        let line = line as f64;
        vert_labels.push_str(&label_svg(
            &label,
            cx,
            vert_top + line * vert_line + vert_line / 2.0,
            &attrs,
            None,
        ));
    }

    let width = (js_max(extent, pad + title.width) + pad).ceil();
    #[expect(clippy::cast_precision_loss, reason = "line counts stay small")]
    let vert_height = if vert_ends.is_empty() {
        0.0
    } else {
        4.0 + vert_ends.len() as f64 * vert_line
    };
    let height = (plot_bottom + tick_height + vert_height + pad).ceil();

    // Section bands, one per run of rows that share a section, shaded on alternate sections.
    let mut bands = String::new();
    let mut section_out = String::new();
    let mut row = 0;
    while let Some(&section) = row_section.get(row) {
        let mut end = row + 1;
        while row_section.get(end) == Some(&section) {
            end += 1;
        }
        let category_index = category(section);
        #[expect(clippy::cast_precision_loss, reason = "section counts stay small")]
        let style = category_index as f64 % section_styles;
        #[expect(clippy::cast_precision_loss, reason = "row counts stay small")]
        let (y, h) = (
            plot_top + row as f64 * row_height,
            (end - row) as f64 * row_height,
        );
        if categories.len() > 1 || !section.is_empty() {
            bands.push_str("<rect class=\"ondine-section ondine-section-");
            bands.push_str(&int_string(style));
            bands.push_str("\" x=\"0\" y=\"");
            push_num(&mut bands, y);
            bands.push_str("\" width=\"");
            bands.push_str(&int_string(width));
            bands.push_str("\" height=\"");
            push_num(&mut bands, h);
            bands.push_str("\" fill=\"");
            bands.push_str(if style % 2.0 == 0.0 {
                "var(--_s)"
            } else {
                "none"
            });
            bands.push_str("\"/>");
        }
        if let Some(label) = section_labels.get(category_index) {
            let mut attrs = String::from(" class=\"ondine-section-label\" data-id=\"");
            attrs.push_str(&esc_text(section));
            attrs.push('"');
            section_out.push_str(&label_svg(
                label,
                pad + 8.0 + label.width / 2.0,
                y + h / 2.0,
                &attrs,
                None,
            ));
        }
        row = end;
    }

    // Excluded days, as Mermaid marks them: a day at a time from the first task's start, and not
    // at all when the chart spans more than five years.
    let mut excluded = String::new();
    if (!model.excludes.is_empty() || !model.includes.is_empty())
        && add(Date(start), 6.0, Unit::Year).time() > stop
    {
        let mut run: Option<(Date, Date)> = None;
        let flush = |run: &mut Option<(Date, Date)>, excluded: &mut String| {
            let Some((from, to)) = run.take() else {
                return;
            };
            let mut day_start = from;
            day_start.set_hours(0.0, 0.0, 0.0, 0.0);
            let mut day_end = to;
            day_end.set_hours(23.0, 59.0, 59.0, 999.0);
            let ex0 = js_max(left, x(day_start.time()));
            let ex1 = js_min(right, x(day_end.time()));
            if ex1 > ex0 {
                excluded.push_str("<rect x=\"");
                push_num(excluded, ex0);
                excluded.push_str("\" y=\"");
                push_num(excluded, plot_top);
                excluded.push_str("\" width=\"");
                push_num(excluded, ex1 - ex0);
                excluded.push_str("\" height=\"");
                push_num(excluded, plot_height);
                excluded.push_str("\"/>");
            }
        };
        let mut day = Date(start);
        while day.time() <= stop {
            if model.is_invalid_date(day)? {
                run = Some(run.map_or((day, day), |(from, _)| (from, day)));
            } else {
                flush(&mut run, &mut excluded);
            }
            day = add_days(day, 1.0);
        }
        flush(&mut run, &mut excluded);
    }

    let mut links: Vec<LinkInfo> = Vec::new();
    let mut tasks_out = String::new();
    for bar in &bars {
        let Some(task) = tasks.get(bar.task) else {
            continue;
        };
        #[expect(clippy::cast_precision_loss, reason = "row counts stay small")]
        let cy = plot_top + bar.row as f64 * row_height + row_height / 2.0;
        let state = if task.active {
            "active"
        } else if task.done {
            "done"
        } else {
            ""
        };
        let mut classes = String::from("ondine-task");
        if !state.is_empty() {
            classes.push_str(" ondine-");
            classes.push_str(state);
        }
        if task.crit {
            classes.push_str(" ondine-crit");
        }
        if task.milestone {
            classes.push_str(" ondine-milestone");
        }
        if task.classes.iter().any(|class| class == "clickable") {
            classes.push_str(" ondine-clickable");
        }

        let mut shape = String::new();
        let mut text_fill = "";
        if bar.x0.is_nan() {
            classes.push_str(" ondine-unscheduled");
            text_fill = " fill=\"var(--_m)\"";
        } else {
            // A critical task takes the critical color in place of the accent. Done, it keeps
            // the fill of finished work and only its border is in that color.
            let color = if task.crit { CRITICAL } else { "var(--_c)" };
            let mut paint = String::new();
            match state {
                "done" => paint.push_str("fill=\"var(--_a)\""),
                "active" => {
                    paint.push_str("fill=\"");
                    paint.push_str(color);
                    paint.push_str("\" fill-opacity=\"0.25\"");
                }
                _ => {
                    paint.push_str("fill=\"");
                    paint.push_str(color);
                    paint.push('"');
                }
            }
            let mut stroke = String::from("stroke=\"");
            stroke.push_str(if state == "done" && !task.crit {
                "var(--_b)"
            } else {
                color
            });
            stroke.push('"');
            if task.milestone {
                let cx = f64::midpoint(bar.x0, bar.x1);
                let r = bar_height / 2.0;
                shape.push_str("<path d=\"M");
                for (i, (px, py)) in [(cx, cy - r), (cx + r, cy), (cx, cy + r), (cx - r, cy)]
                    .into_iter()
                    .enumerate()
                {
                    if i > 0 {
                        shape.push('L');
                    }
                    push_num(&mut shape, px);
                    shape.push(',');
                    push_num(&mut shape, py);
                }
                shape.push_str("Z\" ");
                shape.push_str(&paint);
                shape.push(' ');
                shape.push_str(&stroke);
                shape.push_str(" stroke-linejoin=\"round\"/>");
            } else {
                shape.push_str("<rect x=\"");
                push_num(&mut shape, bar.x0);
                shape.push_str("\" y=\"");
                push_num(&mut shape, cy - bar_height / 2.0);
                shape.push_str("\" width=\"");
                push_num(&mut shape, bar.x1 - bar.x0);
                shape.push_str("\" height=\"");
                push_num(&mut shape, bar_height);
                shape.push_str("\" rx=\"");
                shape.push_str(RADIUS);
                shape.push_str("\" ");
                shape.push_str(&paint);
                shape.push(' ');
                shape.push_str(&stroke);
                shape.push_str("/>");
            }
            if bar.inside {
                if state.is_empty() {
                    text_fill = " fill=\"var(--_bg)\"";
                }
            } else if state == "done" {
                text_fill = " fill=\"var(--_m)\"";
            }
        }

        let mut attrs = String::from(" class=\"ondine-label\"");
        attrs.push_str(text_fill);
        let mut body = shape;
        body.push_str(&label_svg(&bar.label, bar.label_x, cy, &attrs, None));
        if let Some((_, href)) = model.links.iter().find(|(id, _)| *id == task.id) {
            body = linked(&body, href, &task.id, &mut links, url, "");
        }
        tasks_out.push_str("<g class=\"");
        tasks_out.push_str(&classes);
        tasks_out.push_str("\" data-id=\"");
        tasks_out.push_str(&esc_text(&task.id));
        tasks_out.push_str("\">");
        tasks_out.push_str(&body);
        tasks_out.push_str("</g>");
    }

    let mut today = String::new();
    if model.today_marker != "off" && now >= start && now <= stop && stop > start {
        let tx = x(now);
        let top = plot_top - grid_lead;
        let style = if model.today_marker.is_empty() {
            String::new()
        } else {
            let declarations: Vec<&str> = model.today_marker.split(',').collect();
            resolve_style(&declarations).line
        };
        today.push_str("<g class=\"ondine-today\"><path d=\"M");
        push_num(&mut today, tx);
        today.push(',');
        push_num(&mut today, top);
        today.push('V');
        push_num(&mut today, plot_bottom);
        today.push_str("\" fill=\"none\" stroke=\"var(--_c)\" stroke-width=\"2\"");
        today.push_str(&style);
        today.push_str("/>");
        if style.is_empty() {
            today.push_str("<path d=\"M");
            push_num(&mut today, tx - 4.0);
            today.push(',');
            push_num(&mut today, top - 5.0);
            today.push('H');
            push_num(&mut today, tx + 4.0);
            today.push('L');
            push_num(&mut today, tx);
            today.push(',');
            push_num(&mut today, top + 1.0);
            today.push_str("Z\" fill=\"var(--_c)\"/>");
        }
        today.push_str("</g>");
    }

    let mut axis_line = String::from("M");
    push_num(&mut axis_line, left);
    axis_line.push(',');
    push_num(&mut axis_line, plot_bottom);
    axis_line.push('H');
    push_num(&mut axis_line, right);
    if top_axis {
        axis_line.push('M');
        push_num(&mut axis_line, left);
        axis_line.push(',');
        push_num(&mut axis_line, plot_top - grid_lead);
        axis_line.push('H');
        push_num(&mut axis_line, right);
    }

    let mut content = label_svg(
        &title,
        width / 2.0,
        pad + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    if !bands.is_empty() {
        content.push_str("<g class=\"ondine-sections\">");
        content.push_str(&bands);
        content.push_str("</g>");
    }
    if !excluded.is_empty() {
        content.push_str("<g class=\"ondine-excluded\" fill=\"var(--_a)\">");
        content.push_str(&excluded);
        content.push_str("</g>");
    }
    content.push_str("<g class=\"ondine-grid\" fill=\"none\"><path d=\"");
    content.push_str(&grid);
    content.push_str("\" stroke=\"var(--_b)\" stroke-dasharray=\"1 3\"/><path d=\"");
    content.push_str(&axis_line);
    content.push_str("\" stroke=\"var(--_b)\"/></g>");
    if !vert_lines.is_empty() {
        content.push_str("<g class=\"ondine-verts\" fill=\"none\" stroke=\"var(--_l)\" stroke-width=\"1.5\" stroke-dasharray=\"4 3\">");
        content.push_str(&vert_lines);
        content.push_str("</g>");
    }
    content.push_str(&today);
    content.push_str("<g class=\"ondine-tasks\" font-size=\"");
    push_num(&mut content, label_size);
    content.push_str("\">");
    content.push_str(&tasks_out);
    content.push_str("</g><g class=\"ondine-section-labels\" font-size=\"");
    push_num(&mut content, section_size);
    content.push_str("\">");
    content.push_str(&section_out);
    content.push_str("</g><g class=\"ondine-axis\" font-size=\"");
    content.push_str(&int_string(tick_size));
    content.push_str("\" fill=\"var(--_m)\">");
    content.push_str(&axis_bottom);
    content.push_str(&axis_top);
    content.push_str("</g>");
    if !vert_labels.is_empty() {
        content.push_str("<g class=\"ondine-vert-labels\" font-size=\"");
        push_num(&mut content, label_size);
        content.push_str("\">");
        content.push_str(&vert_labels);
        content.push_str("</g>");
    }

    Ok(Rendered {
        height,
        links,
        svg: svg_document("gantt", width, height, size, options, acc, &content),
        width,
    })
}
