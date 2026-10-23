//! The XY chart database. Port of Pele's `src/diagrams/xychart/db.ts`.

use crate::text::label::{trim, trim_start};
use crate::util::chars::is_space;
use crate::yaml::push_number;

/// A value of a series with its label, as written.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DataPoint {
    /// The label after the value, untrimmed, or empty.
    pub(crate) label: String,
    /// The value.
    pub(crate) value: f64,
}

/// The x axis: categories, or a numeric range.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum XAxis {
    /// Categories in order.
    Band {
        /// The categories, trimmed.
        categories: Vec<String>,
        /// The title.
        title: String,
    },
    /// A numeric range.
    Linear(LinearAxis),
}

impl XAxis {
    fn title_mut(&mut self) -> &mut String {
        match self {
            Self::Band { title, .. } | Self::Linear(LinearAxis { title, .. }) => title,
        }
    }

    /// The title.
    pub(crate) fn title(&self) -> &str {
        match self {
            Self::Band { title, .. } | Self::Linear(LinearAxis { title, .. }) => title,
        }
    }
}

/// A numeric axis.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LinearAxis {
    /// The largest value.
    pub(crate) max: f64,
    /// The smallest value.
    pub(crate) min: f64,
    /// The title.
    pub(crate) title: String,
}

/// A series: a line or bars.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Plot {
    /// Whether it is drawn as bars.
    pub(crate) bar: bool,
    /// Each value with its place on the x axis: a category, or a number written as text. A
    /// category with no value has `None`.
    pub(crate) data: Vec<(String, Option<f64>)>,
    /// The trimmed labels of the points of a line, when any is not empty.
    pub(crate) point_labels: Option<Vec<String>>,
    /// The title, trimmed.
    pub(crate) title: String,
}

/// The parsed XY chart: Pele's `XyChartDb`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct XyChartDb {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    has_set_x_axis: bool,
    /// False while the y range comes from the data and not from a `y-axis` statement.
    pub(crate) has_set_y_axis: bool,
    /// Whether the chart runs `horizontal`, when a statement says how it runs.
    pub(crate) horizontal: Option<bool>,
    /// Series, in order.
    pub(crate) plots: Vec<Plot>,
    /// The title.
    pub(crate) title: Option<String>,
    /// The x axis.
    pub(crate) x_axis: XAxis,
    /// The y axis.
    pub(crate) y_axis: LinearAxis,
}

impl Default for XyChartDb {
    fn default() -> Self {
        Self {
            acc_descr: None,
            acc_title: None,
            has_set_x_axis: false,
            has_set_y_axis: false,
            horizontal: None,
            plots: Vec::new(),
            title: None,
            x_axis: XAxis::Band {
                categories: Vec::new(),
                title: String::new(),
            },
            y_axis: LinearAxis {
                max: f64::NEG_INFINITY,
                min: f64::INFINITY,
                title: String::new(),
            },
        }
    }
}

/// `value` as JavaScript's template literals write a number.
fn number_text(value: f64) -> String {
    let mut out = String::new();
    push_number(&mut out, value);
    out
}

/// `text.replace(/\n\s+/g, '\n')`: whitespace after each line break removed, line breaks within
/// it included.
fn acc_description(text: &str) -> String {
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

impl XyChartDb {
    /// Sets how the chart runs: `horizontal`, or else vertical.
    pub(crate) fn set_orientation(&mut self, orientation: &str) {
        self.horizontal = Some(orientation == "horizontal");
    }

    /// Sets the title of the x axis.
    pub(crate) fn set_x_axis_title(&mut self, title: &str) {
        trim(title).clone_into(self.x_axis.title_mut());
    }

    /// Makes the x axis a numeric range.
    pub(crate) fn set_x_axis_range_data(&mut self, min: f64, max: f64) {
        let title = std::mem::take(self.x_axis.title_mut());
        self.x_axis = XAxis::Linear(LinearAxis { max, min, title });
        self.has_set_x_axis = true;
    }

    /// Makes the x axis categories.
    pub(crate) fn set_x_axis_band(&mut self, categories: &[String]) {
        let title = std::mem::take(self.x_axis.title_mut());
        self.x_axis = XAxis::Band {
            categories: categories.iter().map(|c| trim(c).to_owned()).collect(),
            title,
        };
        self.has_set_x_axis = true;
    }

    /// Sets the title of the y axis.
    pub(crate) fn set_y_axis_title(&mut self, title: &str) {
        trim(title).clone_into(&mut self.y_axis.title);
    }

    /// Sets the range of the y axis.
    pub(crate) fn set_y_axis_range_data(&mut self, min: f64, max: f64) {
        self.y_axis.min = min;
        self.y_axis.max = max;
        self.has_set_y_axis = true;
    }

    fn plot_data(&mut self, values: &[f64]) -> Vec<(String, Option<f64>)> {
        if values.is_empty() {
            return Vec::new();
        }
        // The first series fixes the x range when no x-axis statement did.
        if !self.has_set_x_axis {
            #[expect(
                clippy::cast_precision_loss,
                reason = "series lengths stay far below 2^52"
            )]
            let count = values.len() as f64;
            self.set_x_axis_range_data(1.0, count);
        }
        let values = match &self.x_axis {
            XAxis::Band { categories, .. } if values.len() > categories.len() => {
                values.get(..categories.len()).unwrap_or(values)
            }
            _ => values,
        };
        if !self.has_set_y_axis {
            for &value in values {
                if value < self.y_axis.min {
                    self.y_axis.min = value;
                }
                if value > self.y_axis.max {
                    self.y_axis.max = value;
                }
            }
        }
        match &self.x_axis {
            XAxis::Band { categories, .. } => categories
                .iter()
                .enumerate()
                .map(|(i, category)| (category.clone(), values.get(i).copied()))
                .collect(),
            XAxis::Linear(axis) => {
                if let [value] = values {
                    return vec![(number_text(axis.min), Some(*value))];
                }
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "series lengths stay far below 2^52"
                )]
                let step = (axis.max - axis.min) / (values.len() - 1) as f64;
                values
                    .iter()
                    .enumerate()
                    .map(|(i, &value)| {
                        #[expect(
                            clippy::cast_precision_loss,
                            reason = "series lengths stay far below 2^52"
                        )]
                        let at = axis.min + i as f64 * step;
                        (number_text(at), Some(value))
                    })
                    .collect()
            }
        }
    }

    fn add_plot(&mut self, bar: bool, title: &str, data: &[DataPoint]) {
        let values: Vec<f64> = data.iter().map(|d| d.value).collect();
        let point_labels = (!bar && data.iter().any(|d| !trim(&d.label).is_empty()))
            .then(|| data.iter().map(|d| trim(&d.label).to_owned()).collect());
        let plot = Plot {
            bar,
            data: self.plot_data(&values),
            // Mermaid accepts labels on the points of a bar series and ignores them.
            point_labels,
            title: trim(title).to_owned(),
        };
        self.plots.push(plot);
    }

    /// Adds a line series.
    pub(crate) fn set_line_data(&mut self, title: &str, data: &[DataPoint]) {
        self.add_plot(false, title, data);
    }

    /// Adds a bar series.
    pub(crate) fn set_bar_data(&mut self, title: &str, data: &[DataPoint]) {
        self.add_plot(true, title, data);
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
