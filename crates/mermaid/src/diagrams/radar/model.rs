//! The radar model: axes, curves with a value per axis, and chart options. Port of Pele's
//! `src/diagrams/radar/model.ts`.

use std::collections::BTreeMap;

use super::parser::{OptionValue, RadarAst};
use crate::error::{Code, Error};
use crate::text::label::trim_start;
use crate::util::chars::is_space;

/// An axis: Pele's `RadarAxis`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RadarAxis {
    /// The label, or the name when none is given.
    pub(crate) label: String,
    /// The name.
    pub(crate) name: String,
}

/// A curve: Pele's `RadarCurve`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RadarCurve {
    /// Values in axis order, or as written when the curve does not name its axes.
    pub(crate) entries: Vec<f64>,
    /// The label, or the name when none is given.
    pub(crate) label: String,
    /// The name.
    pub(crate) name: String,
}

/// Chart options: Pele's `RadarOptions`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RadarOptions {
    /// `circle` or `polygon`.
    pub(crate) graticule: String,
    /// The value at the rim, or the largest value when `None`.
    pub(crate) max: Option<f64>,
    /// The value at the center.
    pub(crate) min: f64,
    /// Whether the legend is drawn.
    pub(crate) show_legend: bool,
    /// The number of graticule rings, at most [`MAX_TICKS`].
    pub(crate) ticks: f64,
}

/// A radar chart ready to draw: Pele's `RadarModel`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RadarModel {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    /// Axes in authored order.
    pub(crate) axes: Vec<RadarAxis>,
    /// Curves in authored order.
    pub(crate) curves: Vec<RadarCurve>,
    /// Chart options.
    pub(crate) options: RadarOptions,
    /// The title.
    pub(crate) title: Option<String>,
}

/// The most graticule rings.
const MAX_TICKS: f64 = 32.0;

/// `text.replace(/\n\s+/g, '\n')`.
fn dedent_lines(text: &str) -> String {
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

/// A semantic error with `message`.
#[cold]
fn invalid(message: String) -> Error {
    let mut error = Error::new(Code::Semantic, message);
    error.r#type = Some("radar");
    error
}

/// Builds the model from `ast`, as Pele's `buildRadar` does. `title` comes from front matter.
///
/// # Errors
///
/// Returns a semantic error for a curve that names axes before any axis exists, or misses one.
pub(crate) fn build_radar(ast: RadarAst, title: Option<&str>) -> Result<RadarModel, Error> {
    let axes: Vec<RadarAxis> = ast
        .axes
        .into_iter()
        .map(|axis| RadarAxis {
            label: axis.label.unwrap_or_else(|| axis.name.clone()),
            name: axis.name,
        })
        .collect();

    let mut curves = Vec::with_capacity(ast.curves.len());
    for curve in ast.curves {
        let detailed = curve
            .entries
            .first()
            .is_some_and(|entry| entry.axis.is_some());
        let entries = if detailed {
            if axes.is_empty() {
                return Err(invalid(String::from(
                    "Axes must be populated before curves for reference entries",
                )));
            }
            // The first value for each axis counts.
            let mut by_axis = BTreeMap::new();
            for entry in &curve.entries {
                if let Some(axis) = &entry.axis {
                    by_axis.entry(axis.as_str()).or_insert(entry.value);
                }
            }
            let mut entries = Vec::with_capacity(axes.len());
            for axis in &axes {
                let Some(&value) = by_axis.get(axis.name.as_str()) else {
                    let mut message = String::from("Missing entry for axis ");
                    message.push_str(&axis.label);
                    return Err(invalid(message));
                };
                entries.push(value);
            }
            entries
        } else {
            curve.entries.iter().map(|entry| entry.value).collect()
        };
        curves.push(RadarCurve {
            entries,
            label: curve.label.unwrap_or_else(|| curve.name.clone()),
            name: curve.name,
        });
    }

    let mut options = RadarOptions {
        graticule: String::from("circle"),
        max: None,
        min: 0.0,
        show_legend: true,
        ticks: 5.0,
    };
    for option in ast.options {
        match (option.name.as_str(), option.value) {
            ("showLegend", OptionValue::Bool(value)) => options.show_legend = value,
            ("graticule", OptionValue::Text(value)) => options.graticule = value,
            ("ticks", OptionValue::Number(value)) => options.ticks = value,
            ("max", OptionValue::Number(value)) => options.max = Some(value),
            ("min", OptionValue::Number(value)) => options.min = value,
            _ => {}
        }
    }
    if options.ticks > MAX_TICKS {
        options.ticks = MAX_TICKS;
    }

    Ok(RadarModel {
        acc_descr: ast
            .acc_descr
            .filter(|text| !text.is_empty())
            .map(|text| dedent_lines(&text)),
        acc_title: ast
            .acc_title
            .filter(|text| !text.is_empty())
            .map(|text| trim_start(&text).to_owned()),
        axes,
        curves,
        options,
        title: ast
            .title
            .filter(|text| !text.is_empty())
            .or_else(|| title.map(str::to_owned)),
    })
}
