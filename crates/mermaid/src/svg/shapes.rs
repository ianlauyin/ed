//! Node shapes: their sizes around a label, their outlines, and where edges meet them. Port of
//! Pele's `src/svg/shapes.ts`. Unknown shape names draw and size as `rect`.

use crate::layout::compound::CNode;
use crate::svg::builder::num;
use crate::svg::theme::ROUNDED;

/// A shape's size around its label.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ShapeSize {
    /// Vertical shift of the label from the shape's center.
    pub(crate) dy: f64,
    /// Height.
    pub(crate) h: f64,
    /// Width.
    pub(crate) w: f64,
}

/// Space beside a label in a box.
pub(crate) const PAD_X: f64 = 18.0;
/// Space above and below a label in a box.
pub(crate) const PAD_Y: f64 = 12.0;

/// Kept clear at each end of a straight side, for the corner.
const CORNER: f64 = 12.0;

/// `(cos, sin)` of `PI * i / 14 - PI / 2` for the bang shape's 28 points, bit for bit as libm and
/// JavaScript's `Math` return them. The table keeps libm's trigonometry out of the module.
#[rustfmt::skip]
const STAR: [(f64, f64); 28] = [
    (f64::from_bits(0x3c91_a626_3314_5c07), f64::from_bits(0xbff0_0000_0000_0000)),
    (f64::from_bits(0x3fcc_7b90_e302_4584), f64::from_bits(0xbfef_329c_0558_e969)),
    (f64::from_bits(0x3fdb_c4c0_4d71_abc2), f64::from_bits(0xbfec_d4bc_a9cb_5c71)),
    (f64::from_bits(0x3fe3_f3a0_e28b_edd2), f64::from_bits(0xbfe9_04c3_7505_de4b)),
    (f64::from_bits(0x3fe9_04c3_7505_de4b), f64::from_bits(0xbfe3_f3a0_e28b_edd1)),
    (f64::from_bits(0x3fec_d4bc_a9cb_5c71), f64::from_bits(0xbfdb_c4c0_4d71_abc1)),
    (f64::from_bits(0x3fef_329c_0558_e969), f64::from_bits(0xbfcc_7b90_e302_4582)),
    (f64::from_bits(0x3ff0_0000_0000_0000), f64::from_bits(0x0000_0000_0000_0000)),
    (f64::from_bits(0x3fef_329c_0558_e969), f64::from_bits(0x3fcc_7b90_e302_4582)),
    (f64::from_bits(0x3fec_d4bc_a9cb_5c71), f64::from_bits(0x3fdb_c4c0_4d71_abc1)),
    (f64::from_bits(0x3fe9_04c3_7505_de4b), f64::from_bits(0x3fe3_f3a0_e28b_edd1)),
    (f64::from_bits(0x3fe3_f3a0_e28b_edd5), f64::from_bits(0x3fe9_04c3_7505_de48)),
    (f64::from_bits(0x3fdb_c4c0_4d71_abc2), f64::from_bits(0x3fec_d4bc_a9cb_5c71)),
    (f64::from_bits(0x3fcc_7b90_e302_4575), f64::from_bits(0x3fef_329c_0558_e96a)),
    (f64::from_bits(0x3c91_a626_3314_5c07), f64::from_bits(0x3ff0_0000_0000_0000)),
    (f64::from_bits(0xbfcc_7b90_e302_4570), f64::from_bits(0x3fef_329c_0558_e96a)),
    (f64::from_bits(0xbfdb_c4c0_4d71_abc0), f64::from_bits(0x3fec_d4bc_a9cb_5c71)),
    (f64::from_bits(0xbfe3_f3a0_e28b_edd4), f64::from_bits(0x3fe9_04c3_7505_de49)),
    (f64::from_bits(0xbfe9_04c3_7505_de4a), f64::from_bits(0x3fe3_f3a0_e28b_edd2)),
    (f64::from_bits(0xbfec_d4bc_a9cb_5c70), f64::from_bits(0x3fdb_c4c0_4d71_abc3)),
    (f64::from_bits(0xbfef_329c_0558_e969), f64::from_bits(0x3fcc_7b90_e302_4586)),
    (f64::from_bits(0xbff0_0000_0000_0000), f64::from_bits(0x3ca1_a626_3314_5c07)),
    (f64::from_bits(0xbfef_329c_0558_e96b), f64::from_bits(0xbfcc_7b90_e302_455f)),
    (f64::from_bits(0xbfec_d4bc_a9cb_5c71), f64::from_bits(0xbfdb_c4c0_4d71_abbf)),
    (f64::from_bits(0xbfe9_04c3_7505_de4c), f64::from_bits(0xbfe3_f3a0_e28b_edd0)),
    (f64::from_bits(0xbfe3_f3a0_e28b_edd3), f64::from_bits(0xbfe9_04c3_7505_de4a)),
    (f64::from_bits(0xbfdb_c4c0_4d71_abb5), f64::from_bits(0xbfec_d4bc_a9cb_5c74)),
    (f64::from_bits(0xbfcc_7b90_e302_4589), f64::from_bits(0xbfef_329c_0558_e969)),
];

/// A size with no label shift.
const fn size(w: f64, h: f64) -> ShapeSize {
    ShapeSize { dy: 0.0, h, w }
}

/// The span of a shape straight along every side.
fn flat(w: f64, h: f64, sideways: bool) -> f64 {
    (if sideways { h } else { w }) - 2.0 * CORNER
}

/// The span of a shape straight along the top and bottom except for `cut` at each end, and not
/// straight at the sides.
fn flat_across(cut: f64, w: f64, sideways: bool) -> f64 {
    if sideways { 0.0 } else { w - 2.0 * cut - 2.0 * CORNER }
}

/// How far an ellipse's outline has fallen away from its bounding box, `t` from the middle of a
/// side `half` long.
fn bulge(t: f64, half: f64, depth: f64) -> f64 {
    let k = (t.abs() / half).min(1.0);
    depth * (1.0 - (1.0 - k * k).sqrt())
}

/// The inset of a circle or ellipse.
fn round_inset(w: f64, h: f64, side: u8, t: f64) -> f64 {
    if side & 1 == 1 { bulge(t, h / 2.0, w / 2.0) } else { bulge(t, w / 2.0, h / 2.0) }
}

/// The span of a circle or ellipse.
fn round_span(w: f64, h: f64, sideways: bool) -> f64 {
    (if sideways { h } else { w }) * 0.6
}

/// The depth of a cylinder's cap.
fn cap(w: f64) -> f64 {
    12.0_f64.min(6.0_f64.max(w / 14.0))
}

/// The inset of a cylinder, whose caps bulge at the top and bottom.
fn cap_inset(w: f64, side: u8, t: f64) -> f64 {
    if side & 1 == 1 { 0.0 } else { bulge(t, w / 2.0, cap(w)) }
}

/// The size of a triangle around its label, pointing down when `flip`.
fn tri_size(tw: f64, th: f64, flip: bool) -> ShapeSize {
    let w = (1.6 * tw + 40.0).max(80.0);
    let h = (th + 10.0) / (1.0 - (tw + 8.0) / w);
    let dy = h / 2.0 - 10.0 - th / 2.0;
    ShapeSize { dy: if flip { -dy } else { dy }, h, w }
}

/// A path with attributes.
fn path(d: &str, a: &str) -> String {
    ["<path d=\"", d, "\"", a, "/>"].concat()
}

/// The inset of a parallelogram or trapezoid.
fn lean_inset(h: f64, side: u8) -> f64 {
    if side & 1 == 1 { h / 8.0 } else { 0.0 }
}

/// The size of a box around its label.
fn r#box(tw: f64, th: f64) -> ShapeSize {
    size((tw + 2.0 * PAD_X).max(2.0 * PAD_X + 12.0), th + 2.0 * PAD_Y)
}

/// The height of a one-line shape around a label `th` high.
fn line_h(th: f64) -> f64 {
    th + 2.0 * PAD_Y
}

/// A polygon through `points`, given as x, y pairs.
fn poly(points: &[f64], a: &str) -> String {
    let mut out = String::from("<polygon points=\"");
    for (i, pair) in points.chunks(2).enumerate() {
        if i > 0 {
            out.push(' ');
        }
        if let [x, y] = *pair {
            out.push_str(&num(x));
            out.push(',');
            out.push_str(&num(y));
        }
    }
    out.push('"');
    out.push_str(a);
    out.push_str("/>");
    out
}

/// A rectangle of `w` by `h` centered on the origin, with a corner attribute `rx`.
fn rect(w: f64, h: f64, a: &str, rx: &str) -> String {
    [
        "<rect x=\"",
        &num(-w / 2.0),
        "\" y=\"",
        &num(-h / 2.0),
        "\" width=\"",
        &num(w),
        "\" height=\"",
        &num(h),
        "\"",
        rx,
        a,
        "/>",
    ]
    .concat()
}

/// A circle of radius `r` centered on the origin.
fn circle(r: f64, a: &str) -> String {
    ["<circle r=\"", &num(r), "\"", a, "/>"].concat()
}

/// A brace at `x` the height of the shape, opening toward `dir`.
fn brace(x: f64, h: f64, dir: f64, line: &str) -> String {
    let y = h / 2.0;
    let k = 6.0 * dir;
    [
        "<path d=\"M",
        &num(x),
        ",",
        &num(-y),
        "q",
        &num(k),
        ",0 ",
        &num(k),
        ",6V-6q0,6 ",
        &num(k),
        ",6q",
        &num(-k),
        ",0 ",
        &num(-k),
        ",6V",
        &num(y - 6.0),
        "q0,6 ",
        &num(-k),
        ",6\" fill=\"none\"",
        line,
        "/>",
    ]
    .concat()
}

/// A document outline: a rectangle whose bottom edge waves.
#[expect(clippy::many_single_char_names, reason = "mirrors Pele's geometry names")]
fn wave(w: f64, h: f64, a: &str) -> String {
    let x = w / 2.0;
    let top = -h / 2.0;
    let base = h / 2.0 - 10.0;
    let q = w / 4.0;
    [
        "<path d=\"M",
        &num(-x),
        ",",
        &num(top),
        "H",
        &num(x),
        "V",
        &num(base),
        "q",
        &num(-q),
        ",-12 ",
        &num(-2.0 * q),
        ",0t",
        &num(-2.0 * q),
        ",0Z\"",
        a,
        "/>",
    ]
    .concat()
}

/// A rounded rectangle followed by lines drawn in the line style.
fn rect_and_lines(w: f64, h: f64, a: &str, d: &str, line: &str) -> String {
    [&rect(w, h, a, ROUNDED), "<path d=\"", d, "\" fill=\"none\"", line, "/>"].concat()
}

/// The size of `shape` around a label `tw` by `th`.
pub(crate) fn shape_size(shape: &str, tw: f64, th: f64) -> ShapeSize {
    match shape {
        "stadium" => {
            let h = line_h(th);
            size(tw + h * 0.5 + 2.0 * PAD_X - 8.0, h)
        }
        "fr-rect" => size(tw + 2.0 * PAD_X + 16.0, th + 2.0 * PAD_Y),
        "cyl" => {
            let w = (tw + 2.0 * PAD_X).max(56.0);
            let ry = cap(w);
            ShapeSize { dy: ry / 2.0, h: th + 2.0 * PAD_Y + 2.0 * ry, w }
        }
        "circle" => {
            let d = ((tw * tw + th * th).sqrt() + 16.0).max(44.0);
            size(d, d)
        }
        "dbl-circ" => {
            let d = ((tw * tw + th * th).sqrt() + 26.0).max(54.0);
            size(d, d)
        }
        "ellipse" => size(tw * 1.3 + 2.0 * PAD_X, th * 1.2 + 2.0 * PAD_Y),
        "diam" => {
            let a = tw + 16.0;
            let b = th + 8.0;
            size(a + 1.6 * b, a / 1.6 + b)
        }
        "hex" | "lean-r" | "lean-l" | "trap-b" | "trap-t" => {
            let h = line_h(th);
            size(tw + 2.0 * PAD_X + h / 2.0, h)
        }
        "odd" | "delay" => {
            let h = line_h(th);
            size(tw + 2.0 * PAD_X + h / 4.0, h)
        }
        "text" => size(tw + 8.0, th + 8.0),
        "sm-circ" | "f-circ" => size(14.0, 14.0),
        "fr-circ" => size(18.0, 18.0),
        "fork" => size(72.0, 8.0),
        "lin-rect" => size(tw + 2.0 * PAD_X + 8.0, th + 2.0 * PAD_Y),
        "div-rect" => ShapeSize { dy: 4.0, h: th + 2.0 * PAD_Y + 8.0, w: tw + 2.0 * PAD_X },
        "win-pane" | "st-rect" => ShapeSize { dy: 4.0, h: th + 2.0 * PAD_Y + 8.0, w: tw + 2.0 * PAD_X + 8.0 },
        "tri" => tri_size(tw, th, false),
        "flip-tri" => tri_size(tw, th, true),
        "sl-rect" => ShapeSize { dy: 5.0, h: th + 2.0 * PAD_Y + 10.0, w: tw + 2.0 * PAD_X },
        "notch-pent" => size(tw + 2.0 * PAD_X, th + 2.0 * PAD_Y),
        "hourglass" => size(32.0, 40.0),
        "h-cyl" => {
            let h = line_h(th);
            let rx = 12.0_f64.min(h / 4.0);
            size(tw + 2.0 * PAD_X + 2.0 * rx, h)
        }
        "cross-circ" => size(44.0, 44.0),
        "doc" | "tag-doc" => ShapeSize { dy: -5.0, h: th + 2.0 * PAD_Y + 10.0, w: tw + 2.0 * PAD_X },
        "flag" => size(tw + 2.0 * PAD_X, th + 2.0 * PAD_Y + 16.0),
        "folder" => ShapeSize { dy: 4.0, h: th + 2.0 * PAD_Y + 8.0, w: (tw + 2.0 * PAD_X).max(64.0) },
        "bucket" => ShapeSize { dy: 3.0, h: th + 2.0 * PAD_Y + 10.0, w: tw + 2.0 * PAD_X + 20.0 },
        "console" | "browser" => ShapeSize { dy: 6.0, h: th + 2.0 * PAD_Y + 12.0, w: (tw + 2.0 * PAD_X).max(72.0) },
        "person" => ShapeSize { dy: 13.0, h: th + 2.0 * PAD_Y + 26.0, w: (tw + 2.0 * PAD_X).max(56.0) },
        "bang" => size(tw + 2.0 * PAD_X + 28.0, th + 2.0 * PAD_Y + 24.0),
        "cloud" => size(tw + 2.0 * PAD_X + 32.0, th + 2.0 * PAD_Y + 24.0),
        "brace" | "brace-r" => size(tw + 2.0 * PAD_X + 12.0, th + 2.0 * PAD_Y),
        "braces" => size(tw + 2.0 * PAD_X + 24.0, th + 2.0 * PAD_Y),
        "bolt" => size(36.0, 56.0),
        "lin-cyl" => {
            let w = (tw + 2.0 * PAD_X).max(56.0);
            let ry = cap(w);
            ShapeSize { dy: ry / 2.0 + 4.0, h: th + 2.0 * PAD_Y + 2.0 * ry + 8.0, w }
        }
        "curv-trap" => {
            let h = line_h(th);
            size(tw + 2.0 * PAD_X + h * 0.6, h)
        }
        "docs" => ShapeSize { dy: -1.0, h: th + 2.0 * PAD_Y + 18.0, w: tw + 2.0 * PAD_X + 8.0 },
        "bow-rect" => {
            let h = line_h(th);
            size(tw + 2.0 * PAD_X + h * 0.4, h)
        }
        "lin-doc" => ShapeSize { dy: -5.0, h: th + 2.0 * PAD_Y + 10.0, w: tw + 2.0 * PAD_X + 8.0 },
        "collapsedGroup" => ShapeSize { dy: -2.0, h: th + 2.0 * PAD_Y + 8.0, w: tw + 2.0 * PAD_X + 8.0 },
        // `rect`, `rounded`, `notch-rect`, `datastore`, `tag-rect`, and unknown names.
        _ => r#box(tw, th),
    }
}

/// Draws `shape` at `w` by `h` centered on the origin, with outline attributes `a` and line
/// attributes `line` for the strokes drawn over it.
#[expect(clippy::many_single_char_names, reason = "mirrors Pele's geometry names")]
pub(crate) fn draw_shape(shape: &str, w: f64, h: f64, a: &str, line: &str) -> String {
    match shape {
        "rounded" => rect(w, h, a, &[" rx=\"", &num(12.0_f64.min(h / 2.0)), "\""].concat()),
        "stadium" => rect(w, h, a, &[" rx=\"", &num(h / 2.0), "\""].concat()),
        "fr-rect" => {
            let x = w / 2.0 - 8.0;
            let d = [
                "M",
                &num(-x),
                ",",
                &num(-h / 2.0),
                "V",
                &num(h / 2.0),
                "M",
                &num(x),
                ",",
                &num(-h / 2.0),
                "V",
                &num(h / 2.0),
            ]
            .concat();
            rect_and_lines(w, h, a, &d, line)
        }
        "cyl" => {
            let rx = w / 2.0;
            let ry = cap(w);
            let top = -h / 2.0 + ry;
            let bottom = h / 2.0 - ry;
            [
                "<path d=\"M",
                &num(-rx),
                ",",
                &num(top),
                "A",
                &num(rx),
                ",",
                &num(ry),
                " 0 0 1 ",
                &num(rx),
                ",",
                &num(top),
                "V",
                &num(bottom),
                "A",
                &num(rx),
                ",",
                &num(ry),
                " 0 0 1 ",
                &num(-rx),
                ",",
                &num(bottom),
                "Z\"",
                a,
                "/><path d=\"M",
                &num(-rx),
                ",",
                &num(top),
                "A",
                &num(rx),
                ",",
                &num(ry),
                " 0 0 0 ",
                &num(rx),
                ",",
                &num(top),
                "\" fill=\"none\"",
                line,
                "/>",
            ]
            .concat()
        }
        "circle" | "sm-circ" => circle(w / 2.0, a),
        "dbl-circ" => {
            [&circle(w / 2.0, a), "<circle r=\"", &num(w / 2.0 - 5.0), "\" fill=\"none\"", line, "/>"].concat()
        }
        "ellipse" => ["<ellipse rx=\"", &num(w / 2.0), "\" ry=\"", &num(h / 2.0), "\"", a, "/>"].concat(),
        "diam" => poly(&[0.0, -h / 2.0, w / 2.0, 0.0, 0.0, h / 2.0, -w / 2.0, 0.0], a),
        "hex" => {
            let m = h / 4.0;
            poly(
                &[
                    -w / 2.0 + m,
                    -h / 2.0,
                    w / 2.0 - m,
                    -h / 2.0,
                    w / 2.0,
                    0.0,
                    w / 2.0 - m,
                    h / 2.0,
                    -w / 2.0 + m,
                    h / 2.0,
                    -w / 2.0,
                    0.0,
                ],
                a,
            )
        }
        "odd" => {
            let notch = h / 4.0;
            poly(
                &[-w / 2.0, -h / 2.0, w / 2.0, -h / 2.0, w / 2.0, h / 2.0, -w / 2.0, h / 2.0, -w / 2.0 + notch, 0.0],
                a,
            )
        }
        "lean-r" => {
            let s = h / 4.0;
            poly(&[-w / 2.0 + s, -h / 2.0, w / 2.0, -h / 2.0, w / 2.0 - s, h / 2.0, -w / 2.0, h / 2.0], a)
        }
        "lean-l" => {
            let s = h / 4.0;
            poly(&[-w / 2.0, -h / 2.0, w / 2.0 - s, -h / 2.0, w / 2.0, h / 2.0, -w / 2.0 + s, h / 2.0], a)
        }
        "trap-b" => {
            let s = h / 4.0;
            poly(&[-w / 2.0 + s, -h / 2.0, w / 2.0 - s, -h / 2.0, w / 2.0, h / 2.0, -w / 2.0, h / 2.0], a)
        }
        "trap-t" => {
            let s = h / 4.0;
            poly(&[-w / 2.0, -h / 2.0, w / 2.0, -h / 2.0, w / 2.0 - s, h / 2.0, -w / 2.0 + s, h / 2.0], a)
        }
        "text" => rect(w, h, " fill=\"none\" stroke=\"none\"", ""),
        "f-circ" => ["<circle r=\"", &num(w / 2.0), "\" fill=\"var(--_l)\"", line, "/>"].concat(),
        "fr-circ" => {
            [&circle(w / 2.0, a), "<circle r=\"", &num(w / 2.0 - 4.0), "\" fill=\"var(--_l)\"", line, "/>"].concat()
        }
        "fork" => rect(w, h, &[" fill=\"var(--_l)\"", line].concat(), " rx=\"2\""),
        "notch-rect" => {
            let c = 10.0;
            poly(
                &[
                    -w / 2.0 + c,
                    -h / 2.0,
                    w / 2.0,
                    -h / 2.0,
                    w / 2.0,
                    h / 2.0,
                    -w / 2.0,
                    h / 2.0,
                    -w / 2.0,
                    -h / 2.0 + c,
                ],
                a,
            )
        }
        "lin-rect" => rect_and_lines(
            w,
            h,
            a,
            &["M", &num(-w / 2.0 + 8.0), ",", &num(-h / 2.0), "V", &num(h / 2.0)].concat(),
            line,
        ),
        "div-rect" => rect_and_lines(
            w,
            h,
            a,
            &["M", &num(-w / 2.0), ",", &num(-h / 2.0 + 8.0), "H", &num(w / 2.0)].concat(),
            line,
        ),
        "win-pane" => {
            let d = [
                "M",
                &num(-w / 2.0 + 8.0),
                ",",
                &num(-h / 2.0),
                "V",
                &num(h / 2.0),
                "M",
                &num(-w / 2.0),
                ",",
                &num(-h / 2.0 + 8.0),
                "H",
                &num(w / 2.0),
            ]
            .concat();
            rect_and_lines(w, h, a, &d, line)
        }
        "tri" => poly(&[0.0, -h / 2.0, w / 2.0, h / 2.0, -w / 2.0, h / 2.0], a),
        "flip-tri" => poly(&[-w / 2.0, -h / 2.0, w / 2.0, -h / 2.0, 0.0, h / 2.0], a),
        "sl-rect" => poly(&[-w / 2.0, -h / 2.0 + 10.0, w / 2.0, -h / 2.0, w / 2.0, h / 2.0, -w / 2.0, h / 2.0], a),
        "notch-pent" => {
            let c = 10.0;
            poly(
                &[
                    -w / 2.0 + c,
                    -h / 2.0,
                    w / 2.0 - c,
                    -h / 2.0,
                    w / 2.0,
                    -h / 2.0 + c,
                    w / 2.0,
                    h / 2.0,
                    -w / 2.0,
                    h / 2.0,
                    -w / 2.0,
                    -h / 2.0 + c,
                ],
                a,
            )
        }
        "hourglass" => poly(&[-w / 2.0, -h / 2.0, w / 2.0, -h / 2.0, -w / 2.0, h / 2.0, w / 2.0, h / 2.0], a),
        "delay" => {
            let r = h / 2.0;
            [
                "<path d=\"M",
                &num(-w / 2.0),
                ",",
                &num(-r),
                "H",
                &num(w / 2.0 - r),
                "A",
                &num(r),
                ",",
                &num(r),
                " 0 0 1 ",
                &num(w / 2.0 - r),
                ",",
                &num(r),
                "H",
                &num(-w / 2.0),
                "Z\"",
                a,
                "/>",
            ]
            .concat()
        }
        "h-cyl" => {
            let ry = h / 2.0;
            let rx = 12.0_f64.min(h / 4.0);
            let l = -w / 2.0 + rx;
            let r = w / 2.0 - rx;
            [
                "<path d=\"M",
                &num(l),
                ",",
                &num(-ry),
                "H",
                &num(r),
                "A",
                &num(rx),
                ",",
                &num(ry),
                " 0 0 1 ",
                &num(r),
                ",",
                &num(ry),
                "H",
                &num(l),
                "A",
                &num(rx),
                ",",
                &num(ry),
                " 0 0 1 ",
                &num(l),
                ",",
                &num(-ry),
                "Z\"",
                a,
                "/><path d=\"M",
                &num(r),
                ",",
                &num(-ry),
                "A",
                &num(rx),
                ",",
                &num(ry),
                " 0 0 0 ",
                &num(r),
                ",",
                &num(ry),
                "\" fill=\"none\"",
                line,
                "/>",
            ]
            .concat()
        }
        "cross-circ" => {
            let r = w / 2.0;
            let k = r * std::f64::consts::FRAC_1_SQRT_2;
            [
                &circle(r, a),
                "<path d=\"M",
                &num(-k),
                ",",
                &num(-k),
                "L",
                &num(k),
                ",",
                &num(k),
                "M",
                &num(k),
                ",",
                &num(-k),
                "L",
                &num(-k),
                ",",
                &num(k),
                "\" fill=\"none\"",
                line,
                "/>",
            ]
            .concat()
        }
        "st-rect" => {
            let bw = w - 8.0;
            let bh = h - 8.0;
            let mut out = String::new();
            for i in 0..3_u8 {
                let x = -w / 2.0 + f64::from(2 - i) * 4.0;
                let y = -h / 2.0 + f64::from(i) * 4.0;
                out.push_str(
                    &[
                        "<rect x=\"",
                        &num(x),
                        "\" y=\"",
                        &num(y),
                        "\" width=\"",
                        &num(bw),
                        "\" height=\"",
                        &num(bh),
                        "\"",
                        ROUNDED,
                        a,
                        "/>",
                    ]
                    .concat(),
                );
            }
            out
        }
        "doc" => wave(w, h, a),
        "flag" => {
            let x = w / 2.0;
            let y = h / 2.0 - 8.0;
            let q = w / 4.0;
            [
                "<path d=\"M",
                &num(-x),
                ",",
                &num(-y),
                "q",
                &num(q),
                ",-10 ",
                &num(2.0 * q),
                ",0t",
                &num(2.0 * q),
                ",0V",
                &num(y),
                "q",
                &num(-q),
                ",10 ",
                &num(-2.0 * q),
                ",0t",
                &num(-2.0 * q),
                ",0Z\"",
                a,
                "/>",
            ]
            .concat()
        }
        "datastore" => {
            let d = [
                "M",
                &num(-w / 2.0),
                ",",
                &num(-h / 2.0),
                "H",
                &num(w / 2.0),
                "M",
                &num(-w / 2.0),
                ",",
                &num(h / 2.0),
                "H",
                &num(w / 2.0),
            ]
            .concat();
            [
                &rect(w, h, &a.replacen("stroke=\"var(--_b)\"", "stroke=\"none\"", 1), ""),
                "<path d=\"",
                &d,
                "\" fill=\"none\"",
                line,
                "/>",
            ]
            .concat()
        }
        "folder" => {
            let x = w / 2.0;
            let y = h / 2.0;
            let tab = (w * 0.4).min(48.0);
            let d = [
                "M",
                &num(-x),
                ",",
                &num(-y),
                "H",
                &num(-x + tab),
                "L",
                &num(-x + tab + 8.0),
                ",",
                &num(-y + 8.0),
                "H",
                &num(x),
                "V",
                &num(y),
                "H",
                &num(-x),
                "Z",
            ]
            .concat();
            path(&d, a)
        }
        "bucket" => {
            let x = w / 2.0;
            let y = h / 2.0;
            let ry = 5.0;
            let taper = h / 6.0;
            let d = [
                "M",
                &num(-x),
                ",",
                &num(-y + ry),
                "A",
                &num(x),
                ",",
                &num(ry),
                " 0 0 1 ",
                &num(x),
                ",",
                &num(-y + ry),
                "L",
                &num(x - taper),
                ",",
                &num(y - ry),
                "A",
                &num(x - taper),
                ",",
                &num(ry),
                " 0 0 1 ",
                &num(-x + taper),
                ",",
                &num(y - ry),
                "Z",
            ]
            .concat();
            [
                &path(&d, a),
                "<path d=\"M",
                &num(-x),
                ",",
                &num(-y + ry),
                "A",
                &num(x),
                ",",
                &num(ry),
                " 0 0 0 ",
                &num(x),
                ",",
                &num(-y + ry),
                "\" fill=\"none\"",
                line,
                "/>",
            ]
            .concat()
        }
        "console" => {
            let d = [
                "M",
                &num(-w / 2.0),
                ",",
                &num(-h / 2.0 + 12.0),
                "H",
                &num(w / 2.0),
                "M",
                &num(-w / 2.0 + 7.0),
                ",",
                &num(-h / 2.0 + 4.0),
                "l3,2.5l-3,2.5M",
                &num(-w / 2.0 + 13.0),
                ",",
                &num(-h / 2.0 + 9.0),
                "h4",
            ]
            .concat();
            rect_and_lines(w, h, a, &d, line)
        }
        "browser" => {
            let mut out = rect_and_lines(
                w,
                h,
                a,
                &["M", &num(-w / 2.0), ",", &num(-h / 2.0 + 12.0), "H", &num(w / 2.0)].concat(),
                line,
            );
            for i in 0..3_u8 {
                out.push_str(
                    &[
                        "<circle cx=\"",
                        &num(-w / 2.0 + 8.0 + f64::from(i) * 7.0),
                        "\" cy=\"",
                        &num(-h / 2.0 + 6.0),
                        "\" r=\"1.5\" fill=\"none\"",
                        line,
                        "/>",
                    ]
                    .concat(),
                );
            }
            out
        }
        "person" => {
            let top = -h / 2.0;
            [
                "<rect x=\"",
                &num(-w / 2.0),
                "\" y=\"",
                &num(top + 26.0),
                "\" width=\"",
                &num(w),
                "\" height=\"",
                &num(h - 26.0),
                "\" rx=\"12\"",
                a,
                "/><circle cy=\"",
                &num(top + 14.0),
                "\" r=\"13\"",
                a,
                "/>",
            ]
            .concat()
        }
        "bang" => {
            let mut points = Vec::with_capacity(56);
            for (i, (cos, sin)) in STAR.into_iter().enumerate() {
                let k = if i % 2 == 0 { 1.0 } else { 0.82 };
                points.push(cos * (w / 2.0) * k);
                points.push(sin * (h / 2.0) * k);
            }
            poly(&points, a)
        }
        "cloud" => {
            let x = w / 2.0;
            let y = h / 2.0;
            let r = y * 0.55;
            let d = [
                "M",
                &num(-x + r),
                ",",
                &num(y),
                "A",
                &num(r),
                ",",
                &num(r),
                " 0 0 1 ",
                &num(-x + r * 0.6),
                ",",
                &num(-y * 0.1),
                "A",
                &num(x * 0.42),
                ",",
                &num(y * 0.7),
                " 0 0 1 ",
                &num(-x * 0.1),
                ",",
                &num(-y * 0.72),
                "A",
                &num(x * 0.4),
                ",",
                &num(y * 0.62),
                " 0 0 1 ",
                &num(x * 0.62),
                ",",
                &num(-y * 0.42),
                "A",
                &num(r * 1.05),
                ",",
                &num(r * 1.05),
                " 0 0 1 ",
                &num(x - r),
                ",",
                &num(y),
                "Z",
            ]
            .concat();
            path(&d, a)
        }
        "brace" => rect(w, h, " fill=\"none\" stroke=\"none\"", "") + &brace(-w / 2.0 + 12.0, h, -1.0, line),
        "brace-r" => rect(w, h, " fill=\"none\" stroke=\"none\"", "") + &brace(w / 2.0 - 12.0, h, 1.0, line),
        "braces" => {
            rect(w, h, " fill=\"none\" stroke=\"none\"", "")
                + &brace(-w / 2.0 + 12.0, h, -1.0, line)
                + &brace(w / 2.0 - 12.0, h, 1.0, line)
        }
        "bolt" => poly(
            &[
                w * 0.15,
                -h / 2.0,
                -w / 2.0,
                h * 0.08,
                -w * 0.05,
                h * 0.08,
                -w * 0.15,
                h / 2.0,
                w / 2.0,
                -h * 0.08,
                w * 0.05,
                -h * 0.08,
            ],
            a,
        ),
        "lin-cyl" => {
            let rx = w / 2.0;
            let ry = cap(w);
            let top = -h / 2.0 + ry;
            let bottom = h / 2.0 - ry;
            let arc = |y: f64| {
                ["M", &num(-rx), ",", &num(y), "A", &num(rx), ",", &num(ry), " 0 0 0 ", &num(rx), ",", &num(y)].concat()
            };
            let d = [
                "M",
                &num(-rx),
                ",",
                &num(top),
                "A",
                &num(rx),
                ",",
                &num(ry),
                " 0 0 1 ",
                &num(rx),
                ",",
                &num(top),
                "V",
                &num(bottom),
                "A",
                &num(rx),
                ",",
                &num(ry),
                " 0 0 1 ",
                &num(-rx),
                ",",
                &num(bottom),
                "Z",
            ]
            .concat();
            [&path(&d, a), "<path d=\"", &arc(top), &arc(top + 8.0), "\" fill=\"none\"", line, "/>"].concat()
        }
        "curv-trap" => {
            let x = w / 2.0;
            let y = h / 2.0;
            let k = h * 0.3;
            let d = [
                "M",
                &num(-x + k),
                ",",
                &num(-y),
                "H",
                &num(x - k),
                "A",
                &num(k),
                ",",
                &num(y),
                " 0 0 1 ",
                &num(x - k),
                ",",
                &num(y),
                "H",
                &num(-x + k),
                "L",
                &num(-x),
                ",0Z",
            ]
            .concat();
            path(&d, a)
        }
        "docs" => {
            let mut out = String::new();
            for (dx, dy) in [("4", "-4"), ("0", "0"), ("-4", "4")] {
                out.push_str(
                    &["<g transform=\"translate(", dx, ",", dy, ")\">", &wave(w - 8.0, h - 8.0, a), "</g>"].concat(),
                );
            }
            out
        }
        "bow-rect" => {
            let x = w / 2.0;
            let y = h / 2.0;
            let k = h * 0.2;
            let d = [
                "M",
                &num(-x + k),
                ",",
                &num(-y),
                "H",
                &num(x),
                "A",
                &num(k),
                ",",
                &num(y),
                " 0 0 0 ",
                &num(x),
                ",",
                &num(y),
                "H",
                &num(-x + k),
                "A",
                &num(k),
                ",",
                &num(y),
                " 0 0 1 ",
                &num(-x + k),
                ",",
                &num(-y),
                "Z",
            ]
            .concat();
            path(&d, a)
        }
        "tag-rect" => rect_and_lines(
            w,
            h,
            a,
            &["M", &num(w / 2.0 - 12.0), ",", &num(h / 2.0), "L", &num(w / 2.0), ",", &num(h / 2.0 - 12.0)].concat(),
            line,
        ),
        "tag-doc" => [
            &wave(w, h, a),
            "<path d=\"M",
            &num(w / 2.0 - 12.0),
            ",",
            &num(h / 2.0 - 14.0),
            "L",
            &num(w / 2.0),
            ",",
            &num(h / 2.0 - 26.0),
            "\" fill=\"none\"",
            line,
            "/>",
        ]
        .concat(),
        "lin-doc" => [
            &wave(w, h, a),
            "<path d=\"M",
            &num(-w / 2.0 + 8.0),
            ",",
            &num(-h / 2.0),
            "V",
            &num(h / 2.0 - 6.0),
            "\" fill=\"none\"",
            line,
            "/>",
        ]
        .concat(),
        "collapsedGroup" => [
            "<rect x=\"",
            &num(-w / 2.0 + 6.0),
            "\" y=\"",
            &num(-h / 2.0 + 8.0),
            "\" width=\"",
            &num(w - 12.0),
            "\" height=\"",
            &num(h - 8.0),
            "\"",
            ROUNDED,
            a,
            "/><rect x=\"",
            &num(-w / 2.0),
            "\" y=\"",
            &num(-h / 2.0),
            "\" width=\"",
            &num(w),
            "\" height=\"",
            &num(h - 4.0),
            "\"",
            ROUNDED,
            a,
            "/>",
        ]
        .concat(),
        // `rect` and unknown names.
        _ => rect(w, h, a, ROUNDED),
    }
}

/// Distance from the bounding box to the outline of `shape` on a side: 0 top, 1 right, 2 bottom,
/// 3 left. `t` is how far along the side from its middle, to the right or down.
pub(crate) fn shape_inset(shape: &str, w: f64, h: f64, side: u8, t: f64) -> f64 {
    let odd = side & 1 == 1;
    match shape {
        "stadium" if odd => bulge(t, h / 2.0, h / 2.0),
        "cyl" | "lin-cyl" => cap_inset(w, side, t),
        "circle" | "dbl-circ" | "ellipse" | "sm-circ" | "f-circ" | "fr-circ" | "cross-circ" => {
            round_inset(w, h, side, t)
        }
        "diam" if odd => (t.abs() * w) / h,
        "diam" => (t.abs() * h) / w,
        "hex" if odd => t.abs() / 2.0,
        "odd" if side == 3 => h / 4.0,
        "lean-r" | "lean-l" | "trap-b" | "trap-t" => lean_inset(h, side),
        "tri" | "flip-tri" | "bolt" if odd => w / 4.0,
        "sl-rect" if side == 0 => 5.0,
        "hourglass" if odd => w / 2.0,
        "doc" | "docs" | "tag-doc" | "lin-doc" if side == 2 => 10.0,
        "flag" if !odd => 8.0,
        "bucket" if odd => h / 12.0,
        "bang" => 3.0,
        "cloud" if odd => 2.0,
        "cloud" => 1.0,
        "bow-rect" if side == 1 => h * 0.2,
        _ => 0.0,
    }
}

/// How much of a side of `shape` edges may spread along, when the flow runs down or, if
/// `sideways`, across. A shape without one keeps its edges at the middle.
pub(crate) fn shape_span(shape: &str, w: f64, h: f64, sideways: bool) -> f64 {
    let span = match shape {
        "rect" | "rounded" | "fr-rect" | "text" | "notch-rect" | "lin-rect" | "div-rect" | "win-pane"
        | "notch-pent" | "datastore" | "console" | "browser" | "tag-rect" => flat(w, h, sideways),
        "stadium" => {
            if sideways {
                h * 0.6
            } else {
                w - h
            }
        }
        "cyl" => {
            if sideways {
                h - 2.0 * CORNER - 12.0
            } else {
                w * 0.6
            }
        }
        "lin-cyl" => {
            if sideways {
                h - 2.0 * CORNER - 20.0
            } else {
                w * 0.6
            }
        }
        "circle" | "dbl-circ" | "ellipse" | "diam" | "sm-circ" | "f-circ" | "fr-circ" | "cross-circ" => {
            round_span(w, h, sideways)
        }
        "hex" => {
            if sideways {
                h * 0.5
            } else {
                w - h / 2.0 - CORNER
            }
        }
        "odd" => flat_across(0.0, w, sideways),
        "lean-r" | "lean-l" | "trap-b" | "trap-t" => flat_across(h / 4.0, w, sideways),
        "fork" => (if sideways { h } else { w }) - 8.0,
        "st-rect" => (if sideways { h } else { w }) - 2.0 * CORNER - 8.0,
        _ => 0.0,
    };
    0.0_f64.max(span)
}

/// Whether `shape` holds a label.
pub(crate) fn shape_has_label(shape: &str) -> bool {
    !matches!(shape, "sm-circ" | "f-circ" | "fr-circ" | "fork" | "hourglass" | "cross-circ" | "bolt")
}

/// The side of a node's layout box that a route end at `at` lies on: 0 top, 1 right, 2 bottom,
/// 3 left, or -1 for none.
pub(crate) fn end_side(route: &[f64], at: usize, c: &CNode) -> i32 {
    let dx = route.get(at).copied().unwrap_or(f64::NAN) - c.x;
    let dy = route.get(at + 1).copied().unwrap_or(f64::NAN) - c.y;
    if (dy.abs() - c.h / 2.0).abs() < 0.5 {
        return if dy < 0.0 { 0 } else { 2 };
    }
    if (dx.abs() - c.w / 2.0).abs() < 0.5 {
        return if dx < 0.0 { 3 } else { 1 };
    }
    -1
}

/// Moves a route end at `at` from node `c`'s layout box onto the outline of `shape`, drawn `w` by
/// `h` in it. Groups have no outline to move onto.
pub(crate) fn inset_route(route: &mut [f64], at: usize, shape: &str, w: f64, h: f64, c: &CNode) {
    if c.is_group {
        return;
    }

    let side = end_side(route, at, c);
    if side == 0 || side == 2 {
        let t = route.get(at).copied().unwrap_or(f64::NAN) - c.x;
        let amount = shape_inset(shape, w, h, if side == 0 { 0 } else { 2 }, t) + (c.h - h) / 2.0;
        if let Some(y) = route.get_mut(at + 1) {
            *y += if side == 0 { amount } else { -amount };
        }
    } else if side >= 0 {
        let t = route.get(at + 1).copied().unwrap_or(f64::NAN) - c.y;
        let amount = shape_inset(shape, w, h, if side == 3 { 3 } else { 1 }, t) + (c.w - w) / 2.0;
        if let Some(x) = route.get_mut(at) {
            *x += if side == 3 { amount } else { -amount };
        }
    }
}
