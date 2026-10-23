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
    pub(crate) mod components;
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
    fn sankey_reads_unicode_names() {
        for keyword in ["sankey", "sankey-beta"] {
            for (from, to) in [
                ("Доход", "Расходы"),
                ("Café", "München"),
                ("収入", "支出"),
                ("☀️", "🏠"),
                ("Cafe\u{301}", "Cafe\u{300}"),
            ] {
                for quoted in [false, true] {
                    let field = |name: &str| if quoted { format!("\"{name}\"") } else { name.to_owned() };
                    let out = svg(&format!("{keyword}\n{},{},100\n", field(from), field(to)));
                    assert!(out.contains(&format!("data-id=\"{from}\"")), "{out}");
                    assert!(out.contains(&format!(">{to} <tspan")), "{out}");
                }
            }
            let out = svg(&format!("{keyword}\n\"Доход #35; #amp; #34;чистый#34;\",Расходы,1\n"));
            assert!(out.contains(">Доход # &amp; &quot;чистый&quot; <tspan"), "{out}");

            let theme = Theme::default();
            for name in ["a#65534;", "\"a&#65535;\"", "a#1;", "a\tb", "a\u{7f}b"] {
                let source = format!("{keyword}\n{name},b,1\n");
                assert!(render(&source, &Options::default(), &theme).is_err(), "{source}");
            }
        }
    }

    #[test]
    fn sankey_fills_links_with_gradients() {
        let out = svg("sankey\nA,B,1\nA,C,2\n");
        assert_eq!(out.matches("<linearGradient").count(), 2, "{out}");
        assert!(out.contains("fill=\"url(#ondine-sankey-0)\""), "{out}");
        let out = svg("---\nconfig:\n  sankey:\n    linkColor: source\n---\nsankey\nA,B,1\n");
        assert!(!out.contains("<linearGradient"), "{out}");
    }

    #[test]
    fn sankey_keeps_adjacent_labels_apart() {
        let source = "sankey-beta\nVisitors,Signed up,3200\nVisitors,Bounced,9800\nSigned up,Opened a folder,2100\nSigned up,Never returned,1100\nOpened a folder,Ran Agency,1450\nOpened a folder,Write-only,650\nRan Agency,Upgraded to Pro,380\nRan Agency,Stayed free,1070\nUpgraded to Pro,Upgraded to Team,96\nUpgraded to Pro,Stayed on Pro,284";
        let theme = Theme::default();
        // At 16px the monospace labels are too wide to find a free place, unlike Pele's.
        {
            let font_size = 14.0;
            let options = Options { font_size, max_width: Some(664.0), ..Options::default() };
            let out = render(source, &options, &theme).unwrap().svg;
            let small = (font_size * 0.875).round();
            let bounds = |id: &str, value: u32| {
                let at = out.find(&format!("data-id=\"{id}\"><text x=\"")).unwrap();
                let rest = &out[at..];
                let attr = |name: &str| {
                    let from = rest.find(&format!(" {name}=\"")).unwrap() + name.len() + 3;
                    rest[from..from + rest[from..].find('"').unwrap()].to_owned()
                };
                let width = text::measure::width(&format!("{id} {value}"), small, 0);
                let x: f64 = attr("x").parse().unwrap();
                let left = if attr("text-anchor") == "end" { x - width } else { x };
                (left, left + width, attr("y").parse::<f64>().unwrap())
            };
            let a = bounds("Opened a folder", 2100);
            let b = bounds("Ran Agency", 1450);
            assert!(a.1 <= b.0 || b.1 <= a.0 || (a.2 - b.2).abs() >= small * 1.5, "{a:?} {b:?}");
        }
    }

    /// Every `name="number"` attribute value in `text`, in order.
    fn numbers(text: &str, name: &str) -> Vec<f64> {
        let needle = format!(" {name}=\"");
        text.match_indices(&needle)
            .filter_map(|(at, _)| {
                let rest = &text[at + needle.len()..];
                rest[..rest.find('"')?].parse().ok()
            })
            .collect()
    }

    #[test]
    fn sequence_starts_below_a_tall_self_message() {
        let out = svg("sequenceDiagram\n  A->>A: one<br/>two<br/>three\n  A->>B: next");
        let label = |id: &str| {
            let start = out.find(&format!("<g class=\"ondine-edge-label\" data-id=\"{id}\">")).unwrap();
            let end = start + out[start..].find("</g>").unwrap();
            numbers(&out[start..end], "y")
        };
        let own = label("i0");
        assert_eq!(own.len(), 3, "{out}");
        let step = label("i1")[0] - own[2];
        assert!(step >= own[1] - own[0], "{out}");
    }

    #[test]
    fn timeline_period_and_first_event_share_a_center() {
        let out = svg(
            "timeline TD\n  2002 : LinkedIn\n  2004 : Facebook : Google\n  2005<br>and on<br>and on : YouTube\n  2006 : first<br>second<br>third<br>fourth\n",
        );
        let rows: Vec<&str> = out.split("<g class=\"ondine-node ondine-period\"").skip(1).collect();
        assert_eq!(rows.len(), 4, "{out}");
        let mut above = 0.0;
        for row in rows {
            let rects: Vec<(f64, f64)> = row
                .split("<rect ")
                .skip(1)
                .map(|rect| {
                    let rect = &rect[..rect.find('>').unwrap()];
                    (numbers(rect, "y")[0], numbers(rect, "height")[0])
                })
                .collect();
            let middle = |(y, h): (f64, f64)| y + h / 2.0;
            assert!((middle(rects[0]) - middle(rects[1])).abs() < 0.01, "{row}");
            assert!(rects.iter().all(|&(y, _)| y > above), "{row}");
            above = rects.iter().fold(0.0, |most: f64, &(y, h)| most.max(y + h));
        }
    }

    /// The `translate(x,y)` of the element with `data-id="id"`.
    fn translate(svg: &str, id: &str) -> (f64, f64) {
        let at = svg.find(&format!("data-id=\"{id}\" transform=\"translate(")).unwrap();
        let rest = &svg[at..];
        let rest = &rest[rest.find('(').unwrap() + 1..rest.find(')').unwrap()];
        let (x, y) = rest.split_once(',').unwrap();
        (x.parse().unwrap(), y.parse().unwrap())
    }

    #[test]
    fn detects_c4_only_at_the_start() {
        let theme = Theme::default();
        for keyword in ["C4Context", "C4Container", "C4Component", "C4Dynamic", "C4Deployment"] {
            let flow =
                render(&format!("flowchart LR\n  a[Web app] --> b[{keyword} view]"), &Options::default(), &theme);
            assert_eq!(flow.unwrap().r#type, "flowchart");
            let sequence =
                render(&format!("sequenceDiagram\n  Alice->>Bob: {keyword} view"), &Options::default(), &theme);
            assert_eq!(sequence.unwrap().r#type, "sequence");
        }
    }

    #[test]
    fn c4_fits_a_relation_label_between_columns() {
        let out = svg(
            "C4Context\nSystem(a, \"A\")\nSystem(b, \"B\")\nRel(a, b, \"Sends a fairly long description of the relationship\")",
        );
        let (ax, _) = translate(&out, "a");
        let (bx, _) = translate(&out, "b");
        let start = out.find("<g class=\"ondine-edge-label\" data-id=\"a-b\"><rect").unwrap();
        let rect = &out[start..start + out[start..].find("/>").unwrap()];
        let (x, w) = (numbers(rect, "x")[0], numbers(rect, "width")[0]);
        assert!(x >= ax + 108.0 && x + w <= bx - 108.0, "{out}");
    }

    #[test]
    fn state_follows_the_flowchart_curve() {
        let body = "stateDiagram-v2\n direction TB\n state A {\n a --> b\n a --> c\n }\n out --> a\n A --> out";
        let smooth = svg(body);
        let paths = |out: &str| -> Vec<String> {
            out.split("<g class=\"ondine-edge ondine-transition\"")
                .skip(1)
                .map(|edge| {
                    let at = edge.find(" d=\"").unwrap() + 4;
                    edge[at..at + edge[at..].find('"').unwrap()].to_owned()
                })
                .collect()
        };
        assert!(paths(&smooth).iter().any(|d| d.contains('C')), "{smooth}");
        for curve in ["linear", "step", "stepBefore", "stepAfter"] {
            let out = svg(&format!("---\nconfig:\n  flowchart:\n    curve: {curve}\n---\n{body}"));
            assert_ne!(out, smooth, "{curve}");
            let paths = paths(&out);
            assert_eq!(paths.len(), 4, "{out}");
            assert!(paths.iter().all(|d| !d.contains('C')), "{curve}: {paths:?}");
        }
        let ignored = svg(&format!("---\nconfig:\n  state:\n    curve: step\n---\n{body}"));
        assert_eq!(ignored, smooth);
    }

    #[test]
    fn class_wraps_disconnected_groups() {
        let theme = Theme::default();
        let body = "classA <|-- classB\nclassC *-- classD\nclassE o-- classF\nclassG <-- classH\nclassI -- classJ\nclassK <.. classL\nclassM <|.. classN\nclassO .. classP";
        for dir in ["TB", "BT", "LR", "RL"] {
            let source = format!("classDiagram\ndirection {dir}\n{body}");
            let natural = render(&source, &Options { auto_direction: false, ..Options::default() }, &theme).unwrap();
            for max_width in [320.0, 600.0] {
                let options = Options { auto_direction: false, max_width: Some(max_width), ..Options::default() };
                let narrow = render(&source, &options, &theme).unwrap();
                if natural.width <= max_width {
                    assert_eq!(narrow.svg, natural.svg);
                    continue;
                }
                assert!(narrow.width <= max_width, "{dir} {max_width}: {}", narrow.width);
                assert!(narrow.height > natural.height);
                for (a, b) in
                    [("A", "B"), ("C", "D"), ("E", "F"), ("G", "H"), ("I", "J"), ("K", "L"), ("M", "N"), ("O", "P")]
                {
                    let (from, to) =
                        (translate(&narrow.svg, &format!("class{a}")), translate(&narrow.svg, &format!("class{b}")));
                    let (old_from, old_to) =
                        (translate(&natural.svg, &format!("class{a}")), translate(&natural.svg, &format!("class{b}")));
                    assert!(((to.0 - from.0) - (old_to.0 - old_from.0)).abs() < 0.01);
                    assert!(((to.1 - from.1) - (old_to.1 - old_from.1)).abs() < 0.01);
                }
            }
        }
        // A connected diagram stays as it is.
        let source = "classDiagram\nA --> B\nB --> C\nC --> A : uses";
        let natural = render(source, &Options::default(), &theme).unwrap();
        let narrow = render(source, &Options { max_width: Some(1.0), ..Options::default() }, &theme).unwrap();
        assert_eq!(narrow.svg, natural.svg);
    }

    #[test]
    fn quadrant_puts_a_crowded_label_below_its_title() {
        let out = svg(
            "quadrantChart\n  title Q4 candidates by effort and impact\n  x-axis Low effort --> High effort\n  y-axis Low impact --> High impact\n  quadrant-1 Plan carefully\n  quadrant-2 Do first\n  quadrant-3 Reconsider\n  quadrant-4 Quick wins\n  Chart gallery editor: [0.72, 0.85]\n  First-run onboarding: [0.35, 0.78]\n  Shared folders: [0.88, 0.70]\n  Export to PDF: [0.30, 0.55]\n  Custom themes: [0.45, 0.20]\n  Calendar sync: [0.65, 0.30]\n  Keyboard shortcut sheet: [0.12, 0.48]",
        );
        let start = out.find("<g class=\"ondine-quadrant\" data-id=\"3\">").unwrap();
        let quadrant = &out[start..start + out[start..].find("</g>").unwrap()];
        let (rect_x, rect_w) = (numbers(quadrant, "x")[0], numbers(quadrant, "width")[0]);
        let title_y = numbers(quadrant, "y")[1];
        let end = out.find(">Keyboard shortcut sheet</text>").unwrap();
        let label = &out[out[..end].rfind("<text").unwrap()..end];
        let (x, y) = (numbers(label, "x")[0], numbers(label, "y")[0]);
        let half = text::measure::width("Keyboard shortcut sheet", 12.0, 0) / 2.0;
        assert!(y > title_y + 12.0, "{out}");
        assert!(x - half > rect_x && x + half < rect_x + rect_w, "{out}");
    }

    #[test]
    fn limits_reject_long_source() {
        let theme = Theme::monochrome(Color::rgb(0xff, 0xff, 0xff), Color::rgb(0, 0, 0));
        let options = Options { limit: 10, ..Options::default() };
        let error = render("flowchart LR\n  A --> B", &options, &theme).unwrap_err();
        assert_eq!(error.code, Code::Limit);
    }
}
