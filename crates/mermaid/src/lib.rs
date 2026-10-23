//! Compiles Mermaid source to SVG drawn with a [`Theme`].
//!
//! The pipeline follows Pele (MIT): preprocessing removes front matter, directives, and comments,
//! detection picks a diagram type, and each type parses into a model, lays it out, and emits SVG.
//!
//! The SVG is meant to be rasterized, not shown in a browser: the document always has a fixed
//! size, and there are no links (`click`, `href`, and `link` statements parse but are ignored),
//! tooltips, or accessibility markup (`accTitle` and `accDescr` parse but are not written). Ids
//! that the drawing references internally (markers, gradients, clip paths) use a fixed prefix.
//! Image nodes keep relative, `http`, and `https` addresses; what loads is up to the rasterizer.
#![expect(dead_code, reason = "Shared helpers are ported ahead of the diagram types that use them.")]

mod detect;
mod error;
mod preprocess;
mod yaml;

mod layout {
    pub(crate) mod compound;
    pub(crate) mod lanes;
    pub(crate) mod layered;
}

mod svg {
    pub(crate) mod builder;
    pub(crate) mod cluster;
    pub(crate) mod edges;
    pub(crate) mod root;
    pub(crate) mod shapes;
    pub(crate) mod theme;
    pub(crate) mod title;
}

mod text {
    pub(crate) mod entities;
    pub(crate) mod icons;
    pub(crate) mod label;
    pub(crate) mod measure;
}

mod util {
    pub(crate) mod chars;
    pub(crate) mod float;
    pub(crate) mod unicode;
    pub(crate) mod url;
}

mod diagrams;

pub use error::Code;
pub use error::Error;
pub use svg::theme::Color;
pub use svg::theme::Theme;

/// What the host sets for a render. There are no font options, since the font comes from the
/// [`Theme`] and every width is monospace.
#[derive(Clone, Debug)]
pub struct Options {
    /// Whether a diagram that runs across may be drawn running down when it does not fit
    /// `max_width`.
    pub auto_direction: bool,
    /// The width under which a diagram may turn to run down.
    pub direction_breakpoint: f64,
    /// Font size in pixels.
    pub font_size: f64,
    /// The most UTF-16 units of source to accept. Mermaid's default `maxTextSize`.
    pub limit: usize,
    /// The most edges a flowchart may have. `A & B --> C & D` multiplies edges, so a few
    /// kilobytes of source can ask for millions.
    pub max_edges: usize,
    /// The width the host has for the diagram, in pixels. A type that can draw itself narrower
    /// does, so its text is not shrunk with it. `None` leaves every diagram at its natural width.
    pub max_width: Option<f64>,
    /// The time a gantt chart schedules against, in milliseconds since the epoch. The compiler
    /// reads no clock, so this defaults to the epoch.
    pub now: f64,
    /// The most UTF-16 units of SVG to produce. A source within the limit can still describe
    /// megabytes of drawing, which would stall rasterizing.
    pub output_limit: usize,
    /// Space around a diagram. A treemap's `diagramPadding` config takes precedence.
    pub padding: f64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            auto_direction: true,
            direction_breakpoint: 640.0,
            font_size: 16.0,
            limit: 50_000,
            max_edges: 5_000,
            max_width: None,
            now: 0.0,
            output_limit: 4_000_000,
            padding: 8.0,
        }
    }
}

/// A rendered diagram.
#[derive(Clone, Debug, PartialEq)]
pub struct Rendered {
    /// Height in pixels.
    pub height: f64,
    /// The SVG document, sized `width` by `height`.
    pub svg: String,
    /// The detected diagram type, using Pele's names.
    pub r#type: &'static str,
    /// Width in pixels.
    pub width: f64,
}

/// Renders Mermaid `source` to SVG drawn with `theme`.
///
/// # Errors
///
/// Returns an [`Error`] for unsupported diagram types, syntax errors, and exceeded limits.
pub fn render(source: &str, options: &Options, theme: &Theme) -> Result<Rendered, Error> {
    if exceeds(source, options.limit) {
        return Err(Error::new(
            Code::Limit,
            format!("Diagram source is longer than the limit of {} characters.", options.limit),
        ));
    }

    let pre = preprocess::preprocess(source)?;
    let Some(r#type) = detect::detect(&pre.text) else {
        return Err(Error::new(Code::UnsupportedDiagram, "No diagram type detected for the given text."));
    };
    let config = yaml::Value::Object(pre.config);

    let Some(diagram) = diagrams::find(r#type) else {
        let mut error = Error::new(Code::UnsupportedDiagram, format!("Diagram type \"{type}\" is not supported yet."));
        error.r#type = Some(r#type);
        return Err(error);
    };

    let mut source =
        preprocess::encode_entities(if diagram.keep_comments { &pre.with_comments } else { &pre.text }).into_owned();
    source.push('\n');

    let input = diagrams::Input {
        config: &config,
        line_offset: pre.line_offset,
        max_edges: options.max_edges,
        source: &source,
        title: pre.title.as_deref(),
    };

    let drawn = (diagram.render)(&input, options)?;
    let rendered =
        Rendered { height: drawn.height, svg: svg::theme::resolve(&drawn.svg, theme), r#type, width: drawn.width };

    if exceeds(&rendered.svg, options.output_limit) {
        let mut error = Error::new(
            Code::Limit,
            format!("Diagram output is longer than the limit of {} characters.", options.output_limit),
        );
        error.r#type = Some(r#type);
        return Err(error);
    }

    Ok(rendered)
}

/// Whether `text` is longer than `limit` UTF-16 units.
fn exceeds(text: &str, limit: usize) -> bool {
    // No scalar takes more UTF-16 units than UTF-8 bytes, so text within the limit in bytes is
    // within it in units without a count.
    if text.len() <= limit {
        return false;
    }

    // Each scalar starts with one non-continuation byte, and only four-byte scalars need a second
    // UTF-16 unit. Counting bytes avoids decoding the whole SVG.
    let units = text.bytes().map(|byte| usize::from(byte & 0xc0 != 0x80) + usize::from(byte >= 0xf0)).sum::<usize>();
    units > limit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_has_no_browser_markup() {
        let theme = Theme::monochrome(Color::rgb(0xff, 0xff, 0xff), Color::rgb(0, 0, 0));
        let sources = [
            "flowchart LR\n  accTitle: Steps\n  accDescr: How it goes\n  A[Start] --> B[Done]\n  click A href \"https://example.com\" \"tip\"\n  click B call back()",
            "sequenceDiagram\n  accTitle: Greeting\n  Alice->>Bob: Hello\n  link Alice: Home @ https://example.com",
            "pie title Pets\n  accDescr: Pet counts\n  \"Dogs\" : 3\n  \"Cats\" : 2",
            "gantt\n  dateFormat YYYY-MM-DD\n  accTitle: Plan\n  section A\n  Task one :a1, 2024-01-01, 3d\n  click a1 href \"https://example.com\"",
            "stateDiagram-v2\n  [*] --> Still\n  click Still \"https://example.com\" \"tip\"",
            "classDiagram\n  class Duck\n  link Duck \"https://example.com\" \"tip\"",
        ];
        for source in sources {
            let rendered = render(source, &Options::default(), &theme).unwrap();
            for needle in ["<a ", "aria-", "role=", "<title", "<desc", "max-width:100%"] {
                assert!(!rendered.svg.contains(needle), "{needle} in {}", rendered.svg);
            }
            assert!(rendered.svg.contains(" width=\""), "{}", rendered.svg);
        }
    }

    fn svg(source: &str) -> String {
        let theme = Theme::monochrome(Color::rgb(0xff, 0xff, 0xff), Color::rgb(0, 0, 0));
        render(source, &Options::default(), &theme).unwrap().svg
    }

    #[test]
    fn decodes_entities_in_labels() {
        let out = svg("flowchart LR\n  A[\"GET /plans/&lcub;id&rcub;\"] --> B[\"items#lsqb;0#rsqb; #verbar; none\"]");
        assert!(out.contains("GET /plans/{id}"), "{out}");
        assert!(out.contains("items[0] | none"), "{out}");
        let out = svg("flowchart LR\n  A[\"x #amp;lt; y #38;gt; z\"]");
        assert!(out.contains("x &amp;lt; y &amp;gt; z"), "{out}");
    }

    #[test]
    fn styles_label_markup() {
        let out = svg("flowchart LR\n  A[\"H<sub>2</sub>O\"] --> B[\"x<sup>2</sup>\"]");
        assert!(out.contains("baseline-shift=\"sub\" font-size=\"smaller\">2</tspan>"), "{out}");
        assert!(out.contains("baseline-shift=\"super\" font-size=\"smaller\">2</tspan>"), "{out}");
        assert!(!out.contains("<sub>") && !out.contains("&lt;sub"), "{out}");

        let out = svg("flowchart LR\n  A[\"a <u>underlined</u> word\"]");
        assert!(out.contains("<tspan text-decoration=\"underline\">underlined</tspan>"), "{out}");
        let out = svg("kanban\n  todo[Todo]\n    t1[<u>card</u>]");
        assert!(out.contains("text-decoration=\"underline\">card</tspan>"), "{out}");

        let out = svg("flowchart LR\n  A[\"List<T>\"]");
        assert!(out.contains("List&lt;T&gt;"), "{out}");
        let out = svg("flowchart LR\n  A[\"<span title=x>red</span> and <small>print</small><img src=x>\"]");
        assert!(out.contains("red and print"), "{out}");
    }

    #[test]
    fn limits_reject_long_source() {
        let theme = Theme::monochrome(Color::rgb(0xff, 0xff, 0xff), Color::rgb(0, 0, 0));
        let options = Options { limit: 10, ..Options::default() };
        let error = render("flowchart LR\n  A --> B", &options, &theme).unwrap_err();
        assert_eq!(error.code, Code::Limit);
    }
}
