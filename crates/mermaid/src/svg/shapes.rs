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
fn size(w: f64, h: f64) -> ShapeSize {
    ShapeSize { dy: 0.0, h, w }
}

/// The span of a shape straight along every side.
fn flat(w: f64, h: f64, sideways: bool) -> f64 {
    (if sideways { h } else { w }) - 2.0 * CORNER
}

/// The span of a shape straight along the top and bottom except for `cut` at each end, and not
/// straight at the sides.
fn flat_across(cut: f64, w: f64, sideways: bool) -> f64 {
    if sideways {
        0.0
    } else {
        w - 2.0 * cut - 2.0 * CORNER
    }
}

/// How far an ellipse's outline has fallen away from its bounding box, `t` from the middle of a
/// side `half` long.
fn bulge(t: f64, half: f64, depth: f64) -> f64 {
    let k = (t.abs() / half).min(1.0);
    depth * (1.0 - (1.0 - k * k).sqrt())
}

/// The inset of a circle or ellipse.
fn round_inset(w: f64, h: f64, side: u8, t: f64) -> f64 {
    if side & 1 == 1 {
        bulge(t, h / 2.0, w / 2.0)
    } else {
        bulge(t, w / 2.0, h / 2.0)
    }
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
    if side & 1 == 1 {
        0.0
    } else {
        bulge(t, w / 2.0, cap(w))
    }
}

/// The size of a triangle around its label, pointing down when `flip`.
fn tri_size(tw: f64, th: f64, flip: bool) -> ShapeSize {
    let w = (1.6 * tw + 40.0).max(80.0);
    let h = (th + 10.0) / (1.0 - (tw + 8.0) / w);
    let dy = h / 2.0 - 10.0 - th / 2.0;
    ShapeSize {
        dy: if flip { -dy } else { dy },
        h,
        w,
    }
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
#[expect(
    clippy::many_single_char_names,
    reason = "mirrors Pele's geometry names"
)]
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
    [
        &rect(w, h, a, ROUNDED),
        "<path d=\"",
        d,
        "\" fill=\"none\"",
        line,
        "/>",
    ]
    .concat()
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
            ShapeSize {
                dy: ry / 2.0,
                h: th + 2.0 * PAD_Y + 2.0 * ry,
                w,
            }
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
        "div-rect" => ShapeSize {
            dy: 4.0,
            h: th + 2.0 * PAD_Y + 8.0,
            w: tw + 2.0 * PAD_X,
        },
        "win-pane" | "st-rect" => ShapeSize {
            dy: 4.0,
            h: th + 2.0 * PAD_Y + 8.0,
            w: tw + 2.0 * PAD_X + 8.0,
        },
        "tri" => tri_size(tw, th, false),
        "flip-tri" => tri_size(tw, th, true),
        "sl-rect" => ShapeSize {
            dy: 5.0,
            h: th + 2.0 * PAD_Y + 10.0,
            w: tw + 2.0 * PAD_X,
        },
        "notch-pent" => size(tw + 2.0 * PAD_X, th + 2.0 * PAD_Y),
        "hourglass" => size(32.0, 40.0),
        "h-cyl" => {
            let h = line_h(th);
            let rx = 12.0_f64.min(h / 4.0);
            size(tw + 2.0 * PAD_X + 2.0 * rx, h)
        }
        "cross-circ" => size(44.0, 44.0),
        "doc" | "tag-doc" => ShapeSize {
            dy: -5.0,
            h: th + 2.0 * PAD_Y + 10.0,
            w: tw + 2.0 * PAD_X,
        },
        "flag" => size(tw + 2.0 * PAD_X, th + 2.0 * PAD_Y + 16.0),
        "folder" => ShapeSize {
            dy: 4.0,
            h: th + 2.0 * PAD_Y + 8.0,
            w: (tw + 2.0 * PAD_X).max(64.0),
        },
        "bucket" => ShapeSize {
            dy: 3.0,
            h: th + 2.0 * PAD_Y + 10.0,
            w: tw + 2.0 * PAD_X + 20.0,
        },
        "console" | "browser" => ShapeSize {
            dy: 6.0,
            h: th + 2.0 * PAD_Y + 12.0,
            w: (tw + 2.0 * PAD_X).max(72.0),
        },
        "person" => ShapeSize {
            dy: 13.0,
            h: th + 2.0 * PAD_Y + 26.0,
            w: (tw + 2.0 * PAD_X).max(56.0),
        },
        "bang" => size(tw + 2.0 * PAD_X + 28.0, th + 2.0 * PAD_Y + 24.0),
        "cloud" => size(tw + 2.0 * PAD_X + 32.0, th + 2.0 * PAD_Y + 24.0),
        "brace" | "brace-r" => size(tw + 2.0 * PAD_X + 12.0, th + 2.0 * PAD_Y),
        "braces" => size(tw + 2.0 * PAD_X + 24.0, th + 2.0 * PAD_Y),
        "bolt" => size(36.0, 56.0),
        "lin-cyl" => {
            let w = (tw + 2.0 * PAD_X).max(56.0);
            let ry = cap(w);
            ShapeSize {
                dy: ry / 2.0 + 4.0,
                h: th + 2.0 * PAD_Y + 2.0 * ry + 8.0,
                w,
            }
        }
        "curv-trap" => {
            let h = line_h(th);
            size(tw + 2.0 * PAD_X + h * 0.6, h)
        }
        "docs" => ShapeSize {
            dy: -1.0,
            h: th + 2.0 * PAD_Y + 18.0,
            w: tw + 2.0 * PAD_X + 8.0,
        },
        "bow-rect" => {
            let h = line_h(th);
            size(tw + 2.0 * PAD_X + h * 0.4, h)
        }
        "lin-doc" => ShapeSize {
            dy: -5.0,
            h: th + 2.0 * PAD_Y + 10.0,
            w: tw + 2.0 * PAD_X + 8.0,
        },
        "collapsedGroup" => ShapeSize {
            dy: -2.0,
            h: th + 2.0 * PAD_Y + 8.0,
            w: tw + 2.0 * PAD_X + 8.0,
        },
        // `rect`, `rounded`, `notch-rect`, `datastore`, `tag-rect`, and unknown names.
        _ => r#box(tw, th),
    }
}

/// Draws `shape` at `w` by `h` centered on the origin, with outline attributes `a` and line
/// attributes `line` for the strokes drawn over it.
#[expect(
    clippy::many_single_char_names,
    reason = "mirrors Pele's geometry names"
)]
pub(crate) fn draw_shape(shape: &str, w: f64, h: f64, a: &str, line: &str) -> String {
    match shape {
        "rounded" => rect(
            w,
            h,
            a,
            &[" rx=\"", &num(12.0_f64.min(h / 2.0)), "\""].concat(),
        ),
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
        "dbl-circ" => [
            &circle(w / 2.0, a),
            "<circle r=\"",
            &num(w / 2.0 - 5.0),
            "\" fill=\"none\"",
            line,
            "/>",
        ]
        .concat(),
        "ellipse" => [
            "<ellipse rx=\"",
            &num(w / 2.0),
            "\" ry=\"",
            &num(h / 2.0),
            "\"",
            a,
            "/>",
        ]
        .concat(),
        "diam" => poly(
            &[0.0, -h / 2.0, w / 2.0, 0.0, 0.0, h / 2.0, -w / 2.0, 0.0],
            a,
        ),
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
                &[
                    -w / 2.0,
                    -h / 2.0,
                    w / 2.0,
                    -h / 2.0,
                    w / 2.0,
                    h / 2.0,
                    -w / 2.0,
                    h / 2.0,
                    -w / 2.0 + notch,
                    0.0,
                ],
                a,
            )
        }
        "lean-r" => {
            let s = h / 4.0;
            poly(
                &[
                    -w / 2.0 + s,
                    -h / 2.0,
                    w / 2.0,
                    -h / 2.0,
                    w / 2.0 - s,
                    h / 2.0,
                    -w / 2.0,
                    h / 2.0,
                ],
                a,
            )
        }
        "lean-l" => {
            let s = h / 4.0;
            poly(
                &[
                    -w / 2.0,
                    -h / 2.0,
                    w / 2.0 - s,
                    -h / 2.0,
                    w / 2.0,
                    h / 2.0,
                    -w / 2.0 + s,
                    h / 2.0,
                ],
                a,
            )
        }
        "trap-b" => {
            let s = h / 4.0;
            poly(
                &[
                    -w / 2.0 + s,
                    -h / 2.0,
                    w / 2.0 - s,
                    -h / 2.0,
                    w / 2.0,
                    h / 2.0,
                    -w / 2.0,
                    h / 2.0,
                ],
                a,
            )
        }
        "trap-t" => {
            let s = h / 4.0;
            poly(
                &[
                    -w / 2.0,
                    -h / 2.0,
                    w / 2.0,
                    -h / 2.0,
                    w / 2.0 - s,
                    h / 2.0,
                    -w / 2.0 + s,
                    h / 2.0,
                ],
                a,
            )
        }
        "text" => rect(w, h, " fill=\"none\" stroke=\"none\"", ""),
        "f-circ" => [
            "<circle r=\"",
            &num(w / 2.0),
            "\" fill=\"var(--_l)\"",
            line,
            "/>",
        ]
        .concat(),
        "fr-circ" => [
            &circle(w / 2.0, a),
            "<circle r=\"",
            &num(w / 2.0 - 4.0),
            "\" fill=\"var(--_l)\"",
            line,
            "/>",
        ]
        .concat(),
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
            &[
                "M",
                &num(-w / 2.0 + 8.0),
                ",",
                &num(-h / 2.0),
                "V",
                &num(h / 2.0),
            ]
            .concat(),
            line,
        ),
        "div-rect" => rect_and_lines(
            w,
            h,
            a,
            &[
                "M",
                &num(-w / 2.0),
                ",",
                &num(-h / 2.0 + 8.0),
                "H",
                &num(w / 2.0),
            ]
            .concat(),
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
        "sl-rect" => poly(
            &[
                -w / 2.0,
                -h / 2.0 + 10.0,
                w / 2.0,
                -h / 2.0,
                w / 2.0,
                h / 2.0,
                -w / 2.0,
                h / 2.0,
            ],
            a,
        ),
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
        "hourglass" => poly(
            &[
                -w / 2.0,
                -h / 2.0,
                w / 2.0,
                -h / 2.0,
                -w / 2.0,
                h / 2.0,
                w / 2.0,
                h / 2.0,
            ],
            a,
        ),
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
                &rect(
                    w,
                    h,
                    &a.replacen("stroke=\"var(--_b)\"", "stroke=\"none\"", 1),
                    "",
                ),
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
                &[
                    "M",
                    &num(-w / 2.0),
                    ",",
                    &num(-h / 2.0 + 12.0),
                    "H",
                    &num(w / 2.0),
                ]
                .concat(),
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
        "brace" => {
            rect(w, h, " fill=\"none\" stroke=\"none\"", "")
                + &brace(-w / 2.0 + 12.0, h, -1.0, line)
        }
        "brace-r" => {
            rect(w, h, " fill=\"none\" stroke=\"none\"", "") + &brace(w / 2.0 - 12.0, h, 1.0, line)
        }
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
                [
                    "M",
                    &num(-rx),
                    ",",
                    &num(y),
                    "A",
                    &num(rx),
                    ",",
                    &num(ry),
                    " 0 0 0 ",
                    &num(rx),
                    ",",
                    &num(y),
                ]
                .concat()
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
            [
                &path(&d, a),
                "<path d=\"",
                &arc(top),
                &arc(top + 8.0),
                "\" fill=\"none\"",
                line,
                "/>",
            ]
            .concat()
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
                    &[
                        "<g transform=\"translate(",
                        dx,
                        ",",
                        dy,
                        ")\">",
                        &wave(w - 8.0, h - 8.0, a),
                        "</g>",
                    ]
                    .concat(),
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
            &[
                "M",
                &num(w / 2.0 - 12.0),
                ",",
                &num(h / 2.0),
                "L",
                &num(w / 2.0),
                ",",
                &num(h / 2.0 - 12.0),
            ]
            .concat(),
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
        "rect" | "rounded" | "fr-rect" | "text" | "notch-rect" | "lin-rect" | "div-rect"
        | "win-pane" | "notch-pent" | "datastore" | "console" | "browser" | "tag-rect" => {
            flat(w, h, sideways)
        }
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
        "circle" | "dbl-circ" | "ellipse" | "diam" | "sm-circ" | "f-circ" | "fr-circ"
        | "cross-circ" => round_span(w, h, sideways),
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
    !matches!(
        shape,
        "sm-circ" | "f-circ" | "fr-circ" | "fork" | "hourglass" | "cross-circ" | "bolt"
    )
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

#[cfg(test)]
mod tests {
    //! Expectations come from Pele's shape table for every shape name, with Pele's radius variable
    //! replaced by Ondine's.

    use super::{
        draw_shape, end_side, inset_route, shape_has_label, shape_inset, shape_size, shape_span,
    };
    use crate::layout::compound::cnode;
    use crate::svg::builder::num;

    #[expect(clippy::type_complexity, reason = "a table of cases")]
    const CASES: &[(&str, f64, f64, &str, &str, &str, &str, bool)] = &[
        (
            "rect",
            0.0,
            0.0,
            "48,24,0",
            "<rect x=\"-24\" y=\"-12\" width=\"48\" height=\"24\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "24,0",
            true,
        ),
        (
            "rect",
            50.0,
            21.0,
            "86,45,0",
            "<rect x=\"-43\" y=\"-22.5\" width=\"86\" height=\"45\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "62,21",
            true,
        ),
        (
            "rect",
            123.4,
            42.0,
            "159.4,66,0",
            "<rect x=\"-79.7\" y=\"-33\" width=\"159.4\" height=\"66\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "135.4,42",
            true,
        ),
        (
            "rect",
            7.2,
            63.0,
            "48,87,0",
            "<rect x=\"-24\" y=\"-43.5\" width=\"48\" height=\"87\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "24,63",
            true,
        ),
        (
            "rounded",
            0.0,
            0.0,
            "48,24,0",
            "<rect x=\"-24\" y=\"-12\" width=\"48\" height=\"24\" rx=\"12\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "24,0",
            true,
        ),
        (
            "rounded",
            50.0,
            21.0,
            "86,45,0",
            "<rect x=\"-43\" y=\"-22.5\" width=\"86\" height=\"45\" rx=\"12\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "62,21",
            true,
        ),
        (
            "rounded",
            123.4,
            42.0,
            "159.4,66,0",
            "<rect x=\"-79.7\" y=\"-33\" width=\"159.4\" height=\"66\" rx=\"12\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "135.4,42",
            true,
        ),
        (
            "rounded",
            7.2,
            63.0,
            "48,87,0",
            "<rect x=\"-24\" y=\"-43.5\" width=\"48\" height=\"87\" rx=\"12\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "24,63",
            true,
        ),
        (
            "stadium",
            0.0,
            0.0,
            "40,24,0",
            "<rect x=\"-20\" y=\"-12\" width=\"40\" height=\"24\" rx=\"12\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,1.33,12,0,0,0,0,1.33,12",
            "16,14.4",
            true,
        ),
        (
            "stadium",
            50.0,
            21.0,
            "100.5,45,0",
            "<rect x=\"-50.25\" y=\"-22.5\" width=\"100.5\" height=\"45\" rx=\"22.5\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0.68,22.5,0,0,0,0,0.68,22.5",
            "55.5,27",
            true,
        ),
        (
            "stadium",
            123.4,
            42.0,
            "184.4,66,0",
            "<rect x=\"-92.2\" y=\"-33\" width=\"184.4\" height=\"66\" rx=\"33\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0.46,19.25,0,0,0,0,0.46,19.25",
            "118.4,39.6",
            true,
        ),
        (
            "stadium",
            7.2,
            63.0,
            "78.7,87,0",
            "<rect x=\"-39.35\" y=\"-43.5\" width=\"78.7\" height=\"87\" rx=\"43.5\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0.35,12,0,0,0,0,0.35,12",
            "0,52.2",
            true,
        ),
        (
            "fr-rect",
            0.0,
            0.0,
            "52,24,0",
            "<rect x=\"-26\" y=\"-12\" width=\"52\" height=\"24\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-18,-12V12M18,-12V12\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "28,0",
            true,
        ),
        (
            "fr-rect",
            50.0,
            21.0,
            "102,45,0",
            "<rect x=\"-51\" y=\"-22.5\" width=\"102\" height=\"45\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-43,-22.5V22.5M43,-22.5V22.5\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "78,21",
            true,
        ),
        (
            "fr-rect",
            123.4,
            42.0,
            "175.4,66,0",
            "<rect x=\"-87.7\" y=\"-33\" width=\"175.4\" height=\"66\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-79.7,-33V33M79.7,-33V33\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "151.4,42",
            true,
        ),
        (
            "fr-rect",
            7.2,
            63.0,
            "59.2,87,0",
            "<rect x=\"-29.6\" y=\"-43.5\" width=\"59.2\" height=\"87\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-21.6,-43.5V43.5M21.6,-43.5V43.5\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "35.2,63",
            true,
        ),
        (
            "cyl",
            0.0,
            0.0,
            "56,36,3",
            "<path d=\"M-28,-12A28,6 0 0 1 28,-12V12A28,6 0 0 1 -28,12Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-28,-12A28,6 0 0 0 28,-12\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0.12,6,0,0,0,0,0.12,6,0,0,0",
            "33.6,0",
            true,
        ),
        (
            "cyl",
            50.0,
            21.0,
            "86,57.29,3.07",
            "<path d=\"M-43,-22.5A43,6.14 0 0 1 43,-22.5V22.5A43,6.14 0 0 1 -43,22.5Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-43,-22.5A43,6.14 0 0 0 43,-22.5\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0.05,1.74,0,0,0,0,0.05,1.74,0,0,0",
            "51.6,21.29",
            true,
        ),
        (
            "cyl",
            123.4,
            42.0,
            "159.4,88.77,5.69",
            "<path d=\"M-79.7,-33A79.7,11.39 0 0 1 79.7,-33V33A79.7,11.39 0 0 1 -79.7,33Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-79.7,-33A79.7,11.39 0 0 0 79.7,-33\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0.03,0.84,0,0,0,0,0.03,0.84,0,0,0",
            "95.64,52.77",
            true,
        ),
        (
            "cyl",
            7.2,
            63.0,
            "56,99,3",
            "<path d=\"M-28,-43.5A28,6 0 0 1 28,-43.5V43.5A28,6 0 0 1 -28,43.5Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-28,-43.5A28,6 0 0 0 28,-43.5\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0.12,6,0,0,0,0,0.12,6,0,0,0",
            "33.6,63",
            true,
        ),
        (
            "circle",
            0.0,
            0.0,
            "44,44,0",
            "<circle r=\"22\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0.7,22,0,0.7,22,0,0.7,22,0,0.7,22",
            "26.4,26.4",
            true,
        ),
        (
            "circle",
            50.0,
            21.0,
            "70.23,70.23,0",
            "<circle r=\"35.12\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0.43,16.86,0,0.43,16.86,0,0.43,16.86,0,0.43,16.86",
            "42.14,42.14",
            true,
        ),
        (
            "circle",
            123.4,
            42.0,
            "146.35,146.35,0",
            "<circle r=\"73.18\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0.21,6.43,0,0.21,6.43,0,0.21,6.43,0,0.21,6.43",
            "87.81,87.81",
            true,
        ),
        (
            "circle",
            7.2,
            63.0,
            "79.41,79.41,0",
            "<circle r=\"39.71\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0.38,13.7,0,0.38,13.7,0,0.38,13.7,0,0.38,13.7",
            "47.65,47.65",
            true,
        ),
        (
            "dbl-circ",
            0.0,
            0.0,
            "54,54,0",
            "<circle r=\"27\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><circle r=\"22\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0.57,27,0,0.57,27,0,0.57,27,0,0.57,27",
            "32.4,32.4",
            true,
        ),
        (
            "dbl-circ",
            50.0,
            21.0,
            "80.23,80.23,0",
            "<circle r=\"40.12\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><circle r=\"35.12\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0.38,13.48,0,0.38,13.48,0,0.38,13.48,0,0.38,13.48",
            "48.14,48.14",
            true,
        ),
        (
            "dbl-circ",
            123.4,
            42.0,
            "156.35,156.35,0",
            "<circle r=\"78.18\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><circle r=\"73.18\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0.19,5.99,0,0.19,5.99,0,0.19,5.99,0,0.19,5.99",
            "93.81,93.81",
            true,
        ),
        (
            "dbl-circ",
            7.2,
            63.0,
            "89.41,89.41,0",
            "<circle r=\"44.71\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><circle r=\"39.71\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0.34,11.56,0,0.34,11.56,0,0.34,11.56,0,0.34,11.56",
            "53.65,53.65",
            true,
        ),
        (
            "ellipse",
            0.0,
            0.0,
            "36,24,0",
            "<ellipse rx=\"18\" ry=\"12\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0.57,12,0,2,18,0,0.57,12,0,2,18",
            "21.6,14.4",
            true,
        ),
        (
            "ellipse",
            50.0,
            21.0,
            "101,49.2,0",
            "<ellipse rx=\"50.5\" ry=\"24.6\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0.15,4.81,0,1.28,50.5,0,0.15,4.81,0,1.28,50.5",
            "60.6,29.52",
            true,
        ),
        (
            "ellipse",
            123.4,
            42.0,
            "196.42,74.4,0",
            "<ellipse rx=\"98.21\" ry=\"37.2\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0.06,1.78,0,1.08,40.14,0,0.06,1.78,0,1.08,40.14",
            "117.85,44.64",
            true,
        ),
        (
            "ellipse",
            7.2,
            63.0,
            "45.36,99.6,0",
            "<ellipse rx=\"22.68\" ry=\"49.8\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,1.49,49.8,0,0.14,4.58,0,1.49,49.8,0,0.14,4.58",
            "27.22,59.76",
            true,
        ),
        (
            "diam",
            0.0,
            0.0,
            "28.8,18,0",
            "<polygon points=\"0,-9 14.4,0 0,9 -14.4,0\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,3.44,18.75,0,8.8,48,0,3.44,18.75,0,8.8,48",
            "17.28,10.8",
            true,
        ),
        (
            "diam",
            50.0,
            21.0,
            "112.4,70.25,0",
            "<polygon points=\"0,-35.12 56.2,0 0,35.13 -56.2,0\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,3.44,18.75,0,8.8,48,0,3.44,18.75,0,8.8,48",
            "67.44,42.15",
            true,
        ),
        (
            "diam",
            123.4,
            42.0,
            "219.4,137.13,0",
            "<polygon points=\"0,-68.56 109.7,0 0,68.56 -109.7,0\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,3.44,18.75,0,8.8,48,0,3.44,18.75,0,8.8,48",
            "131.64,82.28",
            true,
        ),
        (
            "diam",
            7.2,
            63.0,
            "136.8,85.5,0",
            "<polygon points=\"0,-42.75 68.4,0 0,42.75 -68.4,0\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,3.44,18.75,0,8.8,48,0,3.44,18.75,0,8.8,48",
            "82.08,51.3",
            true,
        ),
        (
            "hex",
            0.0,
            0.0,
            "48,24,0",
            "<polygon points=\"-18,-12 18,-12 24,0 18,12 -18,12 -24,0\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,2.75,15,0,0,0,0,2.75,15",
            "24,12",
            true,
        ),
        (
            "hex",
            50.0,
            21.0,
            "108.5,45,0",
            "<polygon points=\"-43,-22.5 43,-22.5 54.25,0 43,22.5 -43,22.5 -54.25,0\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,2.75,15,0,0,0,0,2.75,15",
            "74,22.5",
            true,
        ),
        (
            "hex",
            123.4,
            42.0,
            "192.4,66,0",
            "<polygon points=\"-79.7,-33 79.7,-33 96.2,0 79.7,33 -79.7,33 -96.2,0\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,2.75,15,0,0,0,0,2.75,15",
            "147.4,33",
            true,
        ),
        (
            "hex",
            7.2,
            63.0,
            "86.7,87,0",
            "<polygon points=\"-21.6,-43.5 21.6,-43.5 43.35,0 21.6,43.5 -21.6,43.5 -43.35,0\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,2.75,15,0,0,0,0,2.75,15",
            "31.2,43.5",
            true,
        ),
        (
            "odd",
            0.0,
            0.0,
            "42,24,0",
            "<polygon points=\"-21,-12 21,-12 21,12 -21,12 -15,0\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,6,6,6",
            "18,0",
            true,
        ),
        (
            "odd",
            50.0,
            21.0,
            "97.25,45,0",
            "<polygon points=\"-48.62,-22.5 48.63,-22.5 48.63,22.5 -48.62,22.5 -37.37,0\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,11.25,11.25,11.25",
            "73.25,0",
            true,
        ),
        (
            "odd",
            123.4,
            42.0,
            "175.9,66,0",
            "<polygon points=\"-87.95,-33 87.95,-33 87.95,33 -87.95,33 -71.45,0\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,16.5,16.5,16.5",
            "151.9,0",
            true,
        ),
        (
            "odd",
            7.2,
            63.0,
            "64.95,87,0",
            "<polygon points=\"-32.47,-43.5 32.48,-43.5 32.48,43.5 -32.47,43.5 -10.73,0\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,21.75,21.75,21.75",
            "40.95,0",
            true,
        ),
        (
            "lean-r",
            0.0,
            0.0,
            "48,24,0",
            "<polygon points=\"-18,-12 24,-12 18,12 -24,12\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,3,3,3,0,0,0,3,3,3",
            "12,0",
            true,
        ),
        (
            "lean-r",
            50.0,
            21.0,
            "108.5,45,0",
            "<polygon points=\"-43,-22.5 54.25,-22.5 43,22.5 -54.25,22.5\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,5.63,5.63,5.63,0,0,0,5.63,5.63,5.63",
            "62,0",
            true,
        ),
        (
            "lean-r",
            123.4,
            42.0,
            "192.4,66,0",
            "<polygon points=\"-79.7,-33 96.2,-33 79.7,33 -96.2,33\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,8.25,8.25,8.25,0,0,0,8.25,8.25,8.25",
            "135.4,0",
            true,
        ),
        (
            "lean-r",
            7.2,
            63.0,
            "86.7,87,0",
            "<polygon points=\"-21.6,-43.5 43.35,-43.5 21.6,43.5 -43.35,43.5\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,10.88,10.88,10.88,0,0,0,10.88,10.88,10.88",
            "19.2,0",
            true,
        ),
        (
            "lean-l",
            0.0,
            0.0,
            "48,24,0",
            "<polygon points=\"-24,-12 18,-12 24,12 -18,12\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,3,3,3,0,0,0,3,3,3",
            "12,0",
            true,
        ),
        (
            "lean-l",
            50.0,
            21.0,
            "108.5,45,0",
            "<polygon points=\"-54.25,-22.5 43,-22.5 54.25,22.5 -43,22.5\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,5.63,5.63,5.63,0,0,0,5.63,5.63,5.63",
            "62,0",
            true,
        ),
        (
            "lean-l",
            123.4,
            42.0,
            "192.4,66,0",
            "<polygon points=\"-96.2,-33 79.7,-33 96.2,33 -79.7,33\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,8.25,8.25,8.25,0,0,0,8.25,8.25,8.25",
            "135.4,0",
            true,
        ),
        (
            "lean-l",
            7.2,
            63.0,
            "86.7,87,0",
            "<polygon points=\"-43.35,-43.5 21.6,-43.5 43.35,43.5 -21.6,43.5\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,10.88,10.88,10.88,0,0,0,10.88,10.88,10.88",
            "19.2,0",
            true,
        ),
        (
            "trap-b",
            0.0,
            0.0,
            "48,24,0",
            "<polygon points=\"-18,-12 18,-12 24,12 -24,12\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,3,3,3,0,0,0,3,3,3",
            "12,0",
            true,
        ),
        (
            "trap-b",
            50.0,
            21.0,
            "108.5,45,0",
            "<polygon points=\"-43,-22.5 43,-22.5 54.25,22.5 -54.25,22.5\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,5.63,5.63,5.63,0,0,0,5.63,5.63,5.63",
            "62,0",
            true,
        ),
        (
            "trap-b",
            123.4,
            42.0,
            "192.4,66,0",
            "<polygon points=\"-79.7,-33 79.7,-33 96.2,33 -96.2,33\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,8.25,8.25,8.25,0,0,0,8.25,8.25,8.25",
            "135.4,0",
            true,
        ),
        (
            "trap-b",
            7.2,
            63.0,
            "86.7,87,0",
            "<polygon points=\"-21.6,-43.5 21.6,-43.5 43.35,43.5 -43.35,43.5\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,10.88,10.88,10.88,0,0,0,10.88,10.88,10.88",
            "19.2,0",
            true,
        ),
        (
            "trap-t",
            0.0,
            0.0,
            "48,24,0",
            "<polygon points=\"-24,-12 24,-12 18,12 -18,12\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,3,3,3,0,0,0,3,3,3",
            "12,0",
            true,
        ),
        (
            "trap-t",
            50.0,
            21.0,
            "108.5,45,0",
            "<polygon points=\"-54.25,-22.5 54.25,-22.5 43,22.5 -43,22.5\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,5.63,5.63,5.63,0,0,0,5.63,5.63,5.63",
            "62,0",
            true,
        ),
        (
            "trap-t",
            123.4,
            42.0,
            "192.4,66,0",
            "<polygon points=\"-96.2,-33 96.2,-33 79.7,33 -79.7,33\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,8.25,8.25,8.25,0,0,0,8.25,8.25,8.25",
            "135.4,0",
            true,
        ),
        (
            "trap-t",
            7.2,
            63.0,
            "86.7,87,0",
            "<polygon points=\"-43.35,-43.5 43.35,-43.5 21.6,43.5 -21.6,43.5\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,10.88,10.88,10.88,0,0,0,10.88,10.88,10.88",
            "19.2,0",
            true,
        ),
        (
            "text",
            0.0,
            0.0,
            "8,8,0",
            "<rect x=\"-4\" y=\"-4\" width=\"8\" height=\"8\" fill=\"none\" stroke=\"none\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "text",
            50.0,
            21.0,
            "58,29,0",
            "<rect x=\"-29\" y=\"-14.5\" width=\"58\" height=\"29\" fill=\"none\" stroke=\"none\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "34,5",
            true,
        ),
        (
            "text",
            123.4,
            42.0,
            "131.4,50,0",
            "<rect x=\"-65.7\" y=\"-25\" width=\"131.4\" height=\"50\" fill=\"none\" stroke=\"none\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "107.4,26",
            true,
        ),
        (
            "text",
            7.2,
            63.0,
            "15.2,71,0",
            "<rect x=\"-7.6\" y=\"-35.5\" width=\"15.2\" height=\"71\" fill=\"none\" stroke=\"none\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,47",
            true,
        ),
        (
            "sm-circ",
            0.0,
            0.0,
            "14,14,0",
            "<circle r=\"7\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,2.67,7,0,2.67,7,0,2.67,7,0,2.67,7",
            "8.4,8.4",
            false,
        ),
        (
            "sm-circ",
            50.0,
            21.0,
            "14,14,0",
            "<circle r=\"7\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,2.67,7,0,2.67,7,0,2.67,7,0,2.67,7",
            "8.4,8.4",
            false,
        ),
        (
            "sm-circ",
            123.4,
            42.0,
            "14,14,0",
            "<circle r=\"7\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,2.67,7,0,2.67,7,0,2.67,7,0,2.67,7",
            "8.4,8.4",
            false,
        ),
        (
            "sm-circ",
            7.2,
            63.0,
            "14,14,0",
            "<circle r=\"7\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,2.67,7,0,2.67,7,0,2.67,7,0,2.67,7",
            "8.4,8.4",
            false,
        ),
        (
            "f-circ",
            0.0,
            0.0,
            "14,14,0",
            "<circle r=\"7\" fill=\"var(--_l)\" stroke=\"var(--_l)\"/>",
            "0,2.67,7,0,2.67,7,0,2.67,7,0,2.67,7",
            "8.4,8.4",
            false,
        ),
        (
            "f-circ",
            50.0,
            21.0,
            "14,14,0",
            "<circle r=\"7\" fill=\"var(--_l)\" stroke=\"var(--_l)\"/>",
            "0,2.67,7,0,2.67,7,0,2.67,7,0,2.67,7",
            "8.4,8.4",
            false,
        ),
        (
            "f-circ",
            123.4,
            42.0,
            "14,14,0",
            "<circle r=\"7\" fill=\"var(--_l)\" stroke=\"var(--_l)\"/>",
            "0,2.67,7,0,2.67,7,0,2.67,7,0,2.67,7",
            "8.4,8.4",
            false,
        ),
        (
            "f-circ",
            7.2,
            63.0,
            "14,14,0",
            "<circle r=\"7\" fill=\"var(--_l)\" stroke=\"var(--_l)\"/>",
            "0,2.67,7,0,2.67,7,0,2.67,7,0,2.67,7",
            "8.4,8.4",
            false,
        ),
        (
            "fr-circ",
            0.0,
            0.0,
            "18,18,0",
            "<circle r=\"9\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><circle r=\"5\" fill=\"var(--_l)\" stroke=\"var(--_l)\"/>",
            "0,1.88,9,0,1.88,9,0,1.88,9,0,1.88,9",
            "10.8,10.8",
            false,
        ),
        (
            "fr-circ",
            50.0,
            21.0,
            "18,18,0",
            "<circle r=\"9\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><circle r=\"5\" fill=\"var(--_l)\" stroke=\"var(--_l)\"/>",
            "0,1.88,9,0,1.88,9,0,1.88,9,0,1.88,9",
            "10.8,10.8",
            false,
        ),
        (
            "fr-circ",
            123.4,
            42.0,
            "18,18,0",
            "<circle r=\"9\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><circle r=\"5\" fill=\"var(--_l)\" stroke=\"var(--_l)\"/>",
            "0,1.88,9,0,1.88,9,0,1.88,9,0,1.88,9",
            "10.8,10.8",
            false,
        ),
        (
            "fr-circ",
            7.2,
            63.0,
            "18,18,0",
            "<circle r=\"9\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><circle r=\"5\" fill=\"var(--_l)\" stroke=\"var(--_l)\"/>",
            "0,1.88,9,0,1.88,9,0,1.88,9,0,1.88,9",
            "10.8,10.8",
            false,
        ),
        (
            "fork",
            0.0,
            0.0,
            "72,8,0",
            "<rect x=\"-36\" y=\"-4\" width=\"72\" height=\"8\" rx=\"2\" fill=\"var(--_l)\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "64,0",
            false,
        ),
        (
            "fork",
            50.0,
            21.0,
            "72,8,0",
            "<rect x=\"-36\" y=\"-4\" width=\"72\" height=\"8\" rx=\"2\" fill=\"var(--_l)\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "64,0",
            false,
        ),
        (
            "fork",
            123.4,
            42.0,
            "72,8,0",
            "<rect x=\"-36\" y=\"-4\" width=\"72\" height=\"8\" rx=\"2\" fill=\"var(--_l)\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "64,0",
            false,
        ),
        (
            "fork",
            7.2,
            63.0,
            "72,8,0",
            "<rect x=\"-36\" y=\"-4\" width=\"72\" height=\"8\" rx=\"2\" fill=\"var(--_l)\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "64,0",
            false,
        ),
        (
            "notch-rect",
            0.0,
            0.0,
            "48,24,0",
            "<polygon points=\"-14,-12 24,-12 24,12 -24,12 -24,-2\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "24,0",
            true,
        ),
        (
            "notch-rect",
            50.0,
            21.0,
            "86,45,0",
            "<polygon points=\"-33,-22.5 43,-22.5 43,22.5 -43,22.5 -43,-12.5\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "62,21",
            true,
        ),
        (
            "notch-rect",
            123.4,
            42.0,
            "159.4,66,0",
            "<polygon points=\"-69.7,-33 79.7,-33 79.7,33 -79.7,33 -79.7,-23\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "135.4,42",
            true,
        ),
        (
            "notch-rect",
            7.2,
            63.0,
            "48,87,0",
            "<polygon points=\"-14,-43.5 24,-43.5 24,43.5 -24,43.5 -24,-33.5\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "24,63",
            true,
        ),
        (
            "lin-rect",
            0.0,
            0.0,
            "44,24,0",
            "<rect x=\"-22\" y=\"-12\" width=\"44\" height=\"24\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-14,-12V12\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "20,0",
            true,
        ),
        (
            "lin-rect",
            50.0,
            21.0,
            "94,45,0",
            "<rect x=\"-47\" y=\"-22.5\" width=\"94\" height=\"45\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-39,-22.5V22.5\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "70,21",
            true,
        ),
        (
            "lin-rect",
            123.4,
            42.0,
            "167.4,66,0",
            "<rect x=\"-83.7\" y=\"-33\" width=\"167.4\" height=\"66\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-75.7,-33V33\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "143.4,42",
            true,
        ),
        (
            "lin-rect",
            7.2,
            63.0,
            "51.2,87,0",
            "<rect x=\"-25.6\" y=\"-43.5\" width=\"51.2\" height=\"87\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-17.6,-43.5V43.5\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "27.2,63",
            true,
        ),
        (
            "div-rect",
            0.0,
            0.0,
            "36,32,4",
            "<rect x=\"-18\" y=\"-16\" width=\"36\" height=\"32\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-18,-8H18\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "12,8",
            true,
        ),
        (
            "div-rect",
            50.0,
            21.0,
            "86,53,4",
            "<rect x=\"-43\" y=\"-26.5\" width=\"86\" height=\"53\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-43,-18.5H43\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "62,29",
            true,
        ),
        (
            "div-rect",
            123.4,
            42.0,
            "159.4,74,4",
            "<rect x=\"-79.7\" y=\"-37\" width=\"159.4\" height=\"74\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-79.7,-29H79.7\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "135.4,50",
            true,
        ),
        (
            "div-rect",
            7.2,
            63.0,
            "43.2,95,4",
            "<rect x=\"-21.6\" y=\"-47.5\" width=\"43.2\" height=\"95\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-21.6,-39.5H21.6\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "19.2,71",
            true,
        ),
        (
            "win-pane",
            0.0,
            0.0,
            "44,32,4",
            "<rect x=\"-22\" y=\"-16\" width=\"44\" height=\"32\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-14,-16V16M-22,-8H22\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "20,8",
            true,
        ),
        (
            "win-pane",
            50.0,
            21.0,
            "94,53,4",
            "<rect x=\"-47\" y=\"-26.5\" width=\"94\" height=\"53\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-39,-26.5V26.5M-47,-18.5H47\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "70,29",
            true,
        ),
        (
            "win-pane",
            123.4,
            42.0,
            "167.4,74,4",
            "<rect x=\"-83.7\" y=\"-37\" width=\"167.4\" height=\"74\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-75.7,-37V37M-83.7,-29H83.7\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "143.4,50",
            true,
        ),
        (
            "win-pane",
            7.2,
            63.0,
            "51.2,95,4",
            "<rect x=\"-25.6\" y=\"-47.5\" width=\"51.2\" height=\"95\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-17.6,-47.5V47.5M-25.6,-39.5H25.6\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "27.2,71",
            true,
        ),
        (
            "tri",
            0.0,
            0.0,
            "80,11.11,-4.44",
            "<polygon points=\"0,-5.56 40,5.56 -40,5.56\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,20,20,20,0,0,0,20,20,20",
            "0,0",
            true,
        ),
        (
            "tri",
            50.0,
            21.0,
            "120,60,9.5",
            "<polygon points=\"0,-30 60,30 -60,30\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,30,30,30,0,0,0,30,30,30",
            "0,0",
            true,
        ),
        (
            "tri",
            123.4,
            42.0,
            "237.44,116.44,27.22",
            "<polygon points=\"0,-58.22 118.72,58.22 -118.72,58.22\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,59.36,59.36,59.36,0,0,0,59.36,59.36,59.36",
            "0,0",
            true,
        ),
        (
            "tri",
            7.2,
            63.0,
            "80,90.12,3.56",
            "<polygon points=\"0,-45.06 40,45.06 -40,45.06\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,20,20,20,0,0,0,20,20,20",
            "0,0",
            true,
        ),
        (
            "flip-tri",
            0.0,
            0.0,
            "80,11.11,4.44",
            "<polygon points=\"-40,-5.56 40,-5.56 0,5.56\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,20,20,20,0,0,0,20,20,20",
            "0,0",
            true,
        ),
        (
            "flip-tri",
            50.0,
            21.0,
            "120,60,-9.5",
            "<polygon points=\"-60,-30 60,-30 0,30\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,30,30,30,0,0,0,30,30,30",
            "0,0",
            true,
        ),
        (
            "flip-tri",
            123.4,
            42.0,
            "237.44,116.44,-27.22",
            "<polygon points=\"-118.72,-58.22 118.72,-58.22 0,58.22\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,59.36,59.36,59.36,0,0,0,59.36,59.36,59.36",
            "0,0",
            true,
        ),
        (
            "flip-tri",
            7.2,
            63.0,
            "80,90.12,-3.56",
            "<polygon points=\"-40,-45.06 40,-45.06 0,45.06\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,20,20,20,0,0,0,20,20,20",
            "0,0",
            true,
        ),
        (
            "sl-rect",
            0.0,
            0.0,
            "36,34,5",
            "<polygon points=\"-18,-7 18,-17 18,17 -18,17\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "5,5,5,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "sl-rect",
            50.0,
            21.0,
            "86,55,5",
            "<polygon points=\"-43,-17.5 43,-27.5 43,27.5 -43,27.5\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "5,5,5,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "sl-rect",
            123.4,
            42.0,
            "159.4,76,5",
            "<polygon points=\"-79.7,-28 79.7,-38 79.7,38 -79.7,38\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "5,5,5,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "sl-rect",
            7.2,
            63.0,
            "43.2,97,5",
            "<polygon points=\"-21.6,-38.5 21.6,-48.5 21.6,48.5 -21.6,48.5\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "5,5,5,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "notch-pent",
            0.0,
            0.0,
            "36,24,0",
            "<polygon points=\"-8,-12 8,-12 18,-2 18,12 -18,12 -18,-2\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "12,0",
            true,
        ),
        (
            "notch-pent",
            50.0,
            21.0,
            "86,45,0",
            "<polygon points=\"-33,-22.5 33,-22.5 43,-12.5 43,22.5 -43,22.5 -43,-12.5\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "62,21",
            true,
        ),
        (
            "notch-pent",
            123.4,
            42.0,
            "159.4,66,0",
            "<polygon points=\"-69.7,-33 69.7,-33 79.7,-23 79.7,33 -79.7,33 -79.7,-23\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "135.4,42",
            true,
        ),
        (
            "notch-pent",
            7.2,
            63.0,
            "43.2,87,0",
            "<polygon points=\"-11.6,-43.5 11.6,-43.5 21.6,-33.5 21.6,43.5 -21.6,43.5 -21.6,-33.5\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "19.2,63",
            true,
        ),
        (
            "hourglass",
            0.0,
            0.0,
            "32,40,0",
            "<polygon points=\"-16,-20 16,-20 -16,20 16,20\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,16,16,16,0,0,0,16,16,16",
            "0,0",
            false,
        ),
        (
            "hourglass",
            50.0,
            21.0,
            "32,40,0",
            "<polygon points=\"-16,-20 16,-20 -16,20 16,20\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,16,16,16,0,0,0,16,16,16",
            "0,0",
            false,
        ),
        (
            "hourglass",
            123.4,
            42.0,
            "32,40,0",
            "<polygon points=\"-16,-20 16,-20 -16,20 16,20\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,16,16,16,0,0,0,16,16,16",
            "0,0",
            false,
        ),
        (
            "hourglass",
            7.2,
            63.0,
            "32,40,0",
            "<polygon points=\"-16,-20 16,-20 -16,20 16,20\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,16,16,16,0,0,0,16,16,16",
            "0,0",
            false,
        ),
        (
            "delay",
            0.0,
            0.0,
            "42,24,0",
            "<path d=\"M-21,-12H9A12,12 0 0 1 9,12H-21Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "delay",
            50.0,
            21.0,
            "97.25,45,0",
            "<path d=\"M-48.62,-22.5H26.13A22.5,22.5 0 0 1 26.13,22.5H-48.62Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "delay",
            123.4,
            42.0,
            "175.9,66,0",
            "<path d=\"M-87.95,-33H54.95A33,33 0 0 1 54.95,33H-87.95Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "delay",
            7.2,
            63.0,
            "64.95,87,0",
            "<path d=\"M-32.47,-43.5H-11.02A43.5,43.5 0 0 1 -11.02,43.5H-32.47Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "h-cyl",
            0.0,
            0.0,
            "48,24,0",
            "<path d=\"M-18,-12H18A6,12 0 0 1 18,12H-18A6,12 0 0 1 -18,-12Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M18,-12A6,12 0 0 0 18,12\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "h-cyl",
            50.0,
            21.0,
            "108.5,45,0",
            "<path d=\"M-43,-22.5H43A11.25,22.5 0 0 1 43,22.5H-43A11.25,22.5 0 0 1 -43,-22.5Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M43,-22.5A11.25,22.5 0 0 0 43,22.5\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "h-cyl",
            123.4,
            42.0,
            "183.4,66,0",
            "<path d=\"M-79.7,-33H79.7A12,33 0 0 1 79.7,33H-79.7A12,33 0 0 1 -79.7,-33Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M79.7,-33A12,33 0 0 0 79.7,33\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "h-cyl",
            7.2,
            63.0,
            "67.2,87,0",
            "<path d=\"M-21.6,-43.5H21.6A12,43.5 0 0 1 21.6,43.5H-21.6A12,43.5 0 0 1 -21.6,-43.5Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M21.6,-43.5A12,43.5 0 0 0 21.6,43.5\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "cross-circ",
            0.0,
            0.0,
            "44,44,0",
            "<circle r=\"22\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-15.56,-15.56L15.56,15.56M15.56,-15.56L-15.56,15.56\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0.7,22,0,0.7,22,0,0.7,22,0,0.7,22",
            "26.4,26.4",
            false,
        ),
        (
            "cross-circ",
            50.0,
            21.0,
            "44,44,0",
            "<circle r=\"22\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-15.56,-15.56L15.56,15.56M15.56,-15.56L-15.56,15.56\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0.7,22,0,0.7,22,0,0.7,22,0,0.7,22",
            "26.4,26.4",
            false,
        ),
        (
            "cross-circ",
            123.4,
            42.0,
            "44,44,0",
            "<circle r=\"22\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-15.56,-15.56L15.56,15.56M15.56,-15.56L-15.56,15.56\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0.7,22,0,0.7,22,0,0.7,22,0,0.7,22",
            "26.4,26.4",
            false,
        ),
        (
            "cross-circ",
            7.2,
            63.0,
            "44,44,0",
            "<circle r=\"22\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-15.56,-15.56L15.56,15.56M15.56,-15.56L-15.56,15.56\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0.7,22,0,0.7,22,0,0.7,22,0,0.7,22",
            "26.4,26.4",
            false,
        ),
        (
            "st-rect",
            0.0,
            0.0,
            "44,32,4",
            "<rect x=\"-14\" y=\"-16\" width=\"36\" height=\"24\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><rect x=\"-18\" y=\"-12\" width=\"36\" height=\"24\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><rect x=\"-22\" y=\"-8\" width=\"36\" height=\"24\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "12,0",
            true,
        ),
        (
            "st-rect",
            50.0,
            21.0,
            "94,53,4",
            "<rect x=\"-39\" y=\"-26.5\" width=\"86\" height=\"45\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><rect x=\"-43\" y=\"-22.5\" width=\"86\" height=\"45\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><rect x=\"-47\" y=\"-18.5\" width=\"86\" height=\"45\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "62,21",
            true,
        ),
        (
            "st-rect",
            123.4,
            42.0,
            "167.4,74,4",
            "<rect x=\"-75.7\" y=\"-37\" width=\"159.4\" height=\"66\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><rect x=\"-79.7\" y=\"-33\" width=\"159.4\" height=\"66\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><rect x=\"-83.7\" y=\"-29\" width=\"159.4\" height=\"66\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "135.4,42",
            true,
        ),
        (
            "st-rect",
            7.2,
            63.0,
            "51.2,95,4",
            "<rect x=\"-17.6\" y=\"-47.5\" width=\"43.2\" height=\"87\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><rect x=\"-21.6\" y=\"-43.5\" width=\"43.2\" height=\"87\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><rect x=\"-25.6\" y=\"-39.5\" width=\"43.2\" height=\"87\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "19.2,63",
            true,
        ),
        (
            "doc",
            0.0,
            0.0,
            "36,34,-5",
            "<path d=\"M-18,-17H18V7q-9,-12 -18,0t-18,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,10,10,10,0,0,0",
            "0,0",
            true,
        ),
        (
            "doc",
            50.0,
            21.0,
            "86,55,-5",
            "<path d=\"M-43,-27.5H43V17.5q-21.5,-12 -43,0t-43,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,10,10,10,0,0,0",
            "0,0",
            true,
        ),
        (
            "doc",
            123.4,
            42.0,
            "159.4,76,-5",
            "<path d=\"M-79.7,-38H79.7V28q-39.85,-12 -79.7,0t-79.7,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,10,10,10,0,0,0",
            "0,0",
            true,
        ),
        (
            "doc",
            7.2,
            63.0,
            "43.2,97,-5",
            "<path d=\"M-21.6,-48.5H21.6V38.5q-10.8,-12 -21.6,0t-21.6,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,10,10,10,0,0,0",
            "0,0",
            true,
        ),
        (
            "flag",
            0.0,
            0.0,
            "36,40,0",
            "<path d=\"M-18,-12q9,-10 18,0t18,0V12q-9,10 -18,0t-18,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "8,8,8,0,0,0,8,8,8,0,0,0",
            "0,0",
            true,
        ),
        (
            "flag",
            50.0,
            21.0,
            "86,61,0",
            "<path d=\"M-43,-22.5q21.5,-10 43,0t43,0V22.5q-21.5,10 -43,0t-43,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "8,8,8,0,0,0,8,8,8,0,0,0",
            "0,0",
            true,
        ),
        (
            "flag",
            123.4,
            42.0,
            "159.4,82,0",
            "<path d=\"M-79.7,-33q39.85,-10 79.7,0t79.7,0V33q-39.85,10 -79.7,0t-79.7,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "8,8,8,0,0,0,8,8,8,0,0,0",
            "0,0",
            true,
        ),
        (
            "flag",
            7.2,
            63.0,
            "43.2,103,0",
            "<path d=\"M-21.6,-43.5q10.8,-10 21.6,0t21.6,0V43.5q-10.8,10 -21.6,0t-21.6,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "8,8,8,0,0,0,8,8,8,0,0,0",
            "0,0",
            true,
        ),
        (
            "datastore",
            0.0,
            0.0,
            "48,24,0",
            "<rect x=\"-24\" y=\"-12\" width=\"48\" height=\"24\" fill=\"var(--_s)\" stroke=\"none\"/><path d=\"M-24,-12H24M-24,12H24\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "24,0",
            true,
        ),
        (
            "datastore",
            50.0,
            21.0,
            "86,45,0",
            "<rect x=\"-43\" y=\"-22.5\" width=\"86\" height=\"45\" fill=\"var(--_s)\" stroke=\"none\"/><path d=\"M-43,-22.5H43M-43,22.5H43\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "62,21",
            true,
        ),
        (
            "datastore",
            123.4,
            42.0,
            "159.4,66,0",
            "<rect x=\"-79.7\" y=\"-33\" width=\"159.4\" height=\"66\" fill=\"var(--_s)\" stroke=\"none\"/><path d=\"M-79.7,-33H79.7M-79.7,33H79.7\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "135.4,42",
            true,
        ),
        (
            "datastore",
            7.2,
            63.0,
            "48,87,0",
            "<rect x=\"-24\" y=\"-43.5\" width=\"48\" height=\"87\" fill=\"var(--_s)\" stroke=\"none\"/><path d=\"M-24,-43.5H24M-24,43.5H24\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "24,63",
            true,
        ),
        (
            "folder",
            0.0,
            0.0,
            "64,32,4",
            "<path d=\"M-32,-16H-6.4L1.6,-8H32V16H-32Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "folder",
            50.0,
            21.0,
            "86,53,4",
            "<path d=\"M-43,-26.5H-8.6L-0.6,-18.5H43V26.5H-43Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "folder",
            123.4,
            42.0,
            "159.4,74,4",
            "<path d=\"M-79.7,-37H-31.7L-23.7,-29H79.7V37H-79.7Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "folder",
            7.2,
            63.0,
            "64,95,4",
            "<path d=\"M-32,-47.5H-6.4L1.6,-39.5H32V47.5H-32Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "bucket",
            0.0,
            0.0,
            "56,34,3",
            "<path d=\"M-28,-12A28,5 0 0 1 28,-12L22.33,12A22.33,5 0 0 1 -22.33,12Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-28,-12A28,5 0 0 0 28,-12\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,2.83,2.83,2.83,0,0,0,2.83,2.83,2.83",
            "0,0",
            true,
        ),
        (
            "bucket",
            50.0,
            21.0,
            "106,55,3",
            "<path d=\"M-53,-22.5A53,5 0 0 1 53,-22.5L43.83,22.5A43.83,5 0 0 1 -43.83,22.5Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-53,-22.5A53,5 0 0 0 53,-22.5\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,4.58,4.58,4.58,0,0,0,4.58,4.58,4.58",
            "0,0",
            true,
        ),
        (
            "bucket",
            123.4,
            42.0,
            "179.4,76,3",
            "<path d=\"M-89.7,-33A89.7,5 0 0 1 89.7,-33L77.03,33A77.03,5 0 0 1 -77.03,33Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-89.7,-33A89.7,5 0 0 0 89.7,-33\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,6.33,6.33,6.33,0,0,0,6.33,6.33,6.33",
            "0,0",
            true,
        ),
        (
            "bucket",
            7.2,
            63.0,
            "63.2,97,3",
            "<path d=\"M-31.6,-43.5A31.6,5 0 0 1 31.6,-43.5L15.43,43.5A15.43,5 0 0 1 -15.43,43.5Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-31.6,-43.5A31.6,5 0 0 0 31.6,-43.5\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,8.08,8.08,8.08,0,0,0,8.08,8.08,8.08",
            "0,0",
            true,
        ),
        (
            "console",
            0.0,
            0.0,
            "72,36,6",
            "<rect x=\"-36\" y=\"-18\" width=\"72\" height=\"36\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-36,-6H36M-29,-14l3,2.5l-3,2.5M-23,-9h4\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "48,12",
            true,
        ),
        (
            "console",
            50.0,
            21.0,
            "86,57,6",
            "<rect x=\"-43\" y=\"-28.5\" width=\"86\" height=\"57\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-43,-16.5H43M-36,-24.5l3,2.5l-3,2.5M-30,-19.5h4\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "62,33",
            true,
        ),
        (
            "console",
            123.4,
            42.0,
            "159.4,78,6",
            "<rect x=\"-79.7\" y=\"-39\" width=\"159.4\" height=\"78\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-79.7,-27H79.7M-72.7,-35l3,2.5l-3,2.5M-66.7,-30h4\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "135.4,54",
            true,
        ),
        (
            "console",
            7.2,
            63.0,
            "72,99,6",
            "<rect x=\"-36\" y=\"-49.5\" width=\"72\" height=\"99\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-36,-37.5H36M-29,-45.5l3,2.5l-3,2.5M-23,-40.5h4\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "48,75",
            true,
        ),
        (
            "browser",
            0.0,
            0.0,
            "72,36,6",
            "<rect x=\"-36\" y=\"-18\" width=\"72\" height=\"36\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-36,-6H36\" fill=\"none\" stroke=\"var(--_l)\"/><circle cx=\"-28\" cy=\"-12\" r=\"1.5\" fill=\"none\" stroke=\"var(--_l)\"/><circle cx=\"-21\" cy=\"-12\" r=\"1.5\" fill=\"none\" stroke=\"var(--_l)\"/><circle cx=\"-14\" cy=\"-12\" r=\"1.5\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "48,12",
            true,
        ),
        (
            "browser",
            50.0,
            21.0,
            "86,57,6",
            "<rect x=\"-43\" y=\"-28.5\" width=\"86\" height=\"57\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-43,-16.5H43\" fill=\"none\" stroke=\"var(--_l)\"/><circle cx=\"-35\" cy=\"-22.5\" r=\"1.5\" fill=\"none\" stroke=\"var(--_l)\"/><circle cx=\"-28\" cy=\"-22.5\" r=\"1.5\" fill=\"none\" stroke=\"var(--_l)\"/><circle cx=\"-21\" cy=\"-22.5\" r=\"1.5\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "62,33",
            true,
        ),
        (
            "browser",
            123.4,
            42.0,
            "159.4,78,6",
            "<rect x=\"-79.7\" y=\"-39\" width=\"159.4\" height=\"78\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-79.7,-27H79.7\" fill=\"none\" stroke=\"var(--_l)\"/><circle cx=\"-71.7\" cy=\"-33\" r=\"1.5\" fill=\"none\" stroke=\"var(--_l)\"/><circle cx=\"-64.7\" cy=\"-33\" r=\"1.5\" fill=\"none\" stroke=\"var(--_l)\"/><circle cx=\"-57.7\" cy=\"-33\" r=\"1.5\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "135.4,54",
            true,
        ),
        (
            "browser",
            7.2,
            63.0,
            "72,99,6",
            "<rect x=\"-36\" y=\"-49.5\" width=\"72\" height=\"99\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-36,-37.5H36\" fill=\"none\" stroke=\"var(--_l)\"/><circle cx=\"-28\" cy=\"-43.5\" r=\"1.5\" fill=\"none\" stroke=\"var(--_l)\"/><circle cx=\"-21\" cy=\"-43.5\" r=\"1.5\" fill=\"none\" stroke=\"var(--_l)\"/><circle cx=\"-14\" cy=\"-43.5\" r=\"1.5\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "48,75",
            true,
        ),
        (
            "person",
            0.0,
            0.0,
            "56,50,13",
            "<rect x=\"-28\" y=\"1\" width=\"56\" height=\"24\" rx=\"12\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><circle cy=\"-11\" r=\"13\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "person",
            50.0,
            21.0,
            "86,71,13",
            "<rect x=\"-43\" y=\"-9.5\" width=\"86\" height=\"45\" rx=\"12\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><circle cy=\"-21.5\" r=\"13\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "person",
            123.4,
            42.0,
            "159.4,92,13",
            "<rect x=\"-79.7\" y=\"-20\" width=\"159.4\" height=\"66\" rx=\"12\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><circle cy=\"-32\" r=\"13\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "person",
            7.2,
            63.0,
            "56,113,13",
            "<rect x=\"-28\" y=\"-30.5\" width=\"56\" height=\"87\" rx=\"12\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><circle cy=\"-42.5\" r=\"13\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "bang",
            0.0,
            0.0,
            "64,48,0",
            "<polygon points=\"0,-24 5.84,-19.19 13.88,-21.62 16.36,-15.39 25.02,-14.96 23.64,-8.54 31.2,-5.34 26.24,0 31.2,5.34 23.64,8.54 25.02,14.96 16.36,15.39 13.88,21.62 5.84,19.19 0,24 -5.84,19.19 -13.88,21.62 -16.36,15.39 -25.02,14.96 -23.64,8.54 -31.2,5.34 -26.24,0 -31.2,-5.34 -23.64,-8.54 -25.02,-14.96 -16.36,-15.39 -13.88,-21.62 -5.84,-19.19\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "3,3,3,3,3,3,3,3,3,3,3,3",
            "0,0",
            true,
        ),
        (
            "bang",
            50.0,
            21.0,
            "114,69,0",
            "<polygon points=\"0,-34.5 10.4,-27.58 24.73,-31.08 29.14,-22.12 44.56,-21.51 42.11,-12.27 55.57,-7.68 46.74,0 55.57,7.68 42.11,12.27 44.56,21.51 29.14,22.12 24.73,31.08 10.4,27.58 0,34.5 -10.4,27.58 -24.73,31.08 -29.14,22.12 -44.56,21.51 -42.11,12.27 -55.57,7.68 -46.74,0 -55.57,-7.68 -42.11,-12.27 -44.56,-21.51 -29.14,-22.12 -24.73,-31.08 -10.4,-27.58\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "3,3,3,3,3,3,3,3,3,3,3,3",
            "0,0",
            true,
        ),
        (
            "bang",
            123.4,
            42.0,
            "187.4,90,0",
            "<polygon points=\"0,-45 17.1,-35.97 40.65,-40.54 47.91,-28.85 73.26,-28.06 69.23,-16.01 91.35,-10.01 76.83,0 91.35,10.01 69.23,16.01 73.26,28.06 47.91,28.85 40.65,40.54 17.1,35.97 0,45 -17.1,35.97 -40.65,40.54 -47.91,28.85 -73.26,28.06 -69.23,16.01 -91.35,10.01 -76.83,0 -91.35,-10.01 -69.23,-16.01 -73.26,-28.06 -47.91,-28.85 -40.65,-40.54 -17.1,-35.97\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "3,3,3,3,3,3,3,3,3,3,3,3",
            "0,0",
            true,
        ),
        (
            "bang",
            7.2,
            63.0,
            "71.2,111,0",
            "<polygon points=\"0,-55.5 6.5,-44.37 15.45,-50 18.2,-35.58 27.83,-34.6 26.3,-19.75 34.71,-12.35 29.19,0 34.71,12.35 26.3,19.75 27.83,34.6 18.2,35.58 15.45,50 6.5,44.37 0,55.5 -6.5,44.37 -15.45,50 -18.2,35.58 -27.83,34.6 -26.3,19.75 -34.71,12.35 -29.19,0 -34.71,-12.35 -26.3,-19.75 -27.83,-34.6 -18.2,-35.58 -15.45,-50 -6.5,-44.37\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "3,3,3,3,3,3,3,3,3,3,3,3",
            "0,0",
            true,
        ),
        (
            "cloud",
            0.0,
            0.0,
            "68,48,0",
            "<path d=\"M-20.8,24A13.2,13.2 0 0 1 -26.08,-2.4A14.28,16.8 0 0 1 -3.4,-17.28A13.6,14.88 0 0 1 21.08,-10.08A13.86,13.86 0 0 1 20.8,24Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "1,1,1,2,2,2,1,1,1,2,2,2",
            "0,0",
            true,
        ),
        (
            "cloud",
            50.0,
            21.0,
            "118,69,0",
            "<path d=\"M-40.02,34.5A18.98,18.98 0 0 1 -47.61,-3.45A24.78,24.15 0 0 1 -5.9,-24.84A23.6,21.39 0 0 1 36.58,-14.49A19.92,19.92 0 0 1 40.03,34.5Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "1,1,1,2,2,2,1,1,1,2,2,2",
            "0,0",
            true,
        ),
        (
            "cloud",
            123.4,
            42.0,
            "191.4,90,0",
            "<path d=\"M-70.95,45A24.75,24.75 0 0 1 -80.85,-4.5A40.19,31.5 0 0 1 -9.57,-32.4A38.28,27.9 0 0 1 59.33,-18.9A25.99,25.99 0 0 1 70.95,45Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "1,1,1,2,2,2,1,1,1,2,2,2",
            "0,0",
            true,
        ),
        (
            "cloud",
            7.2,
            63.0,
            "75.2,111,0",
            "<path d=\"M-7.07,55.5A30.53,30.53 0 0 1 -19.28,-5.55A15.79,38.85 0 0 1 -3.76,-39.96A15.04,34.41 0 0 1 23.31,-23.31A32.05,32.05 0 0 1 7.07,55.5Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "1,1,1,2,2,2,1,1,1,2,2,2",
            "0,0",
            true,
        ),
        (
            "brace",
            0.0,
            0.0,
            "48,24,0",
            "<rect x=\"-24\" y=\"-12\" width=\"48\" height=\"24\" fill=\"none\" stroke=\"none\"/><path d=\"M-12,-12q-6,0 -6,6V-6q0,6 -6,6q6,0 6,6V6q0,6 6,6\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "brace",
            50.0,
            21.0,
            "98,45,0",
            "<rect x=\"-49\" y=\"-22.5\" width=\"98\" height=\"45\" fill=\"none\" stroke=\"none\"/><path d=\"M-37,-22.5q-6,0 -6,6V-6q0,6 -6,6q6,0 6,6V16.5q0,6 6,6\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "brace",
            123.4,
            42.0,
            "171.4,66,0",
            "<rect x=\"-85.7\" y=\"-33\" width=\"171.4\" height=\"66\" fill=\"none\" stroke=\"none\"/><path d=\"M-73.7,-33q-6,0 -6,6V-6q0,6 -6,6q6,0 6,6V27q0,6 6,6\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "brace",
            7.2,
            63.0,
            "55.2,87,0",
            "<rect x=\"-27.6\" y=\"-43.5\" width=\"55.2\" height=\"87\" fill=\"none\" stroke=\"none\"/><path d=\"M-15.6,-43.5q-6,0 -6,6V-6q0,6 -6,6q6,0 6,6V37.5q0,6 6,6\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "brace-r",
            0.0,
            0.0,
            "48,24,0",
            "<rect x=\"-24\" y=\"-12\" width=\"48\" height=\"24\" fill=\"none\" stroke=\"none\"/><path d=\"M12,-12q6,0 6,6V-6q0,6 6,6q-6,0 -6,6V6q0,6 -6,6\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "brace-r",
            50.0,
            21.0,
            "98,45,0",
            "<rect x=\"-49\" y=\"-22.5\" width=\"98\" height=\"45\" fill=\"none\" stroke=\"none\"/><path d=\"M37,-22.5q6,0 6,6V-6q0,6 6,6q-6,0 -6,6V16.5q0,6 -6,6\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "brace-r",
            123.4,
            42.0,
            "171.4,66,0",
            "<rect x=\"-85.7\" y=\"-33\" width=\"171.4\" height=\"66\" fill=\"none\" stroke=\"none\"/><path d=\"M73.7,-33q6,0 6,6V-6q0,6 6,6q-6,0 -6,6V27q0,6 -6,6\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "brace-r",
            7.2,
            63.0,
            "55.2,87,0",
            "<rect x=\"-27.6\" y=\"-43.5\" width=\"55.2\" height=\"87\" fill=\"none\" stroke=\"none\"/><path d=\"M15.6,-43.5q6,0 6,6V-6q0,6 6,6q-6,0 -6,6V37.5q0,6 -6,6\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "braces",
            0.0,
            0.0,
            "60,24,0",
            "<rect x=\"-30\" y=\"-12\" width=\"60\" height=\"24\" fill=\"none\" stroke=\"none\"/><path d=\"M-18,-12q-6,0 -6,6V-6q0,6 -6,6q6,0 6,6V6q0,6 6,6\" fill=\"none\" stroke=\"var(--_l)\"/><path d=\"M18,-12q6,0 6,6V-6q0,6 6,6q-6,0 -6,6V6q0,6 -6,6\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "braces",
            50.0,
            21.0,
            "110,45,0",
            "<rect x=\"-55\" y=\"-22.5\" width=\"110\" height=\"45\" fill=\"none\" stroke=\"none\"/><path d=\"M-43,-22.5q-6,0 -6,6V-6q0,6 -6,6q6,0 6,6V16.5q0,6 6,6\" fill=\"none\" stroke=\"var(--_l)\"/><path d=\"M43,-22.5q6,0 6,6V-6q0,6 6,6q-6,0 -6,6V16.5q0,6 -6,6\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "braces",
            123.4,
            42.0,
            "183.4,66,0",
            "<rect x=\"-91.7\" y=\"-33\" width=\"183.4\" height=\"66\" fill=\"none\" stroke=\"none\"/><path d=\"M-79.7,-33q-6,0 -6,6V-6q0,6 -6,6q6,0 6,6V27q0,6 6,6\" fill=\"none\" stroke=\"var(--_l)\"/><path d=\"M79.7,-33q6,0 6,6V-6q0,6 6,6q-6,0 -6,6V27q0,6 -6,6\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "braces",
            7.2,
            63.0,
            "67.2,87,0",
            "<rect x=\"-33.6\" y=\"-43.5\" width=\"67.2\" height=\"87\" fill=\"none\" stroke=\"none\"/><path d=\"M-21.6,-43.5q-6,0 -6,6V-6q0,6 -6,6q6,0 6,6V37.5q0,6 6,6\" fill=\"none\" stroke=\"var(--_l)\"/><path d=\"M21.6,-43.5q6,0 6,6V-6q0,6 6,6q-6,0 -6,6V37.5q0,6 -6,6\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "bolt",
            0.0,
            0.0,
            "36,56,0",
            "<polygon points=\"5.4,-28 -18,4.48 -1.8,4.48 -5.4,28 18,-4.48 1.8,-4.48\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,9,9,9,0,0,0,9,9,9",
            "0,0",
            false,
        ),
        (
            "bolt",
            50.0,
            21.0,
            "36,56,0",
            "<polygon points=\"5.4,-28 -18,4.48 -1.8,4.48 -5.4,28 18,-4.48 1.8,-4.48\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,9,9,9,0,0,0,9,9,9",
            "0,0",
            false,
        ),
        (
            "bolt",
            123.4,
            42.0,
            "36,56,0",
            "<polygon points=\"5.4,-28 -18,4.48 -1.8,4.48 -5.4,28 18,-4.48 1.8,-4.48\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,9,9,9,0,0,0,9,9,9",
            "0,0",
            false,
        ),
        (
            "bolt",
            7.2,
            63.0,
            "36,56,0",
            "<polygon points=\"5.4,-28 -18,4.48 -1.8,4.48 -5.4,28 18,-4.48 1.8,-4.48\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,9,9,9,0,0,0,9,9,9",
            "0,0",
            false,
        ),
        (
            "lin-cyl",
            0.0,
            0.0,
            "56,44,7",
            "<path d=\"M-28,-16A28,6 0 0 1 28,-16V16A28,6 0 0 1 -28,16Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-28,-16A28,6 0 0 0 28,-16M-28,-8A28,6 0 0 0 28,-8\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0.12,6,0,0,0,0,0.12,6,0,0,0",
            "33.6,0",
            true,
        ),
        (
            "lin-cyl",
            50.0,
            21.0,
            "86,65.29,7.07",
            "<path d=\"M-43,-26.5A43,6.14 0 0 1 43,-26.5V26.5A43,6.14 0 0 1 -43,26.5Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-43,-26.5A43,6.14 0 0 0 43,-26.5M-43,-18.5A43,6.14 0 0 0 43,-18.5\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0.05,1.74,0,0,0,0,0.05,1.74,0,0,0",
            "51.6,21.29",
            true,
        ),
        (
            "lin-cyl",
            123.4,
            42.0,
            "159.4,96.77,9.69",
            "<path d=\"M-79.7,-37A79.7,11.39 0 0 1 79.7,-37V37A79.7,11.39 0 0 1 -79.7,37Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-79.7,-37A79.7,11.39 0 0 0 79.7,-37M-79.7,-29A79.7,11.39 0 0 0 79.7,-29\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0.03,0.84,0,0,0,0,0.03,0.84,0,0,0",
            "95.64,52.77",
            true,
        ),
        (
            "lin-cyl",
            7.2,
            63.0,
            "56,107,7",
            "<path d=\"M-28,-47.5A28,6 0 0 1 28,-47.5V47.5A28,6 0 0 1 -28,47.5Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-28,-47.5A28,6 0 0 0 28,-47.5M-28,-39.5A28,6 0 0 0 28,-39.5\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0.12,6,0,0,0,0,0.12,6,0,0,0",
            "33.6,63",
            true,
        ),
        (
            "curv-trap",
            0.0,
            0.0,
            "50.4,24,0",
            "<path d=\"M-18,-12H18A7.2,12 0 0 1 18,12H-18L-25.2,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "curv-trap",
            50.0,
            21.0,
            "113,45,0",
            "<path d=\"M-43,-22.5H43A13.5,22.5 0 0 1 43,22.5H-43L-56.5,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "curv-trap",
            123.4,
            42.0,
            "199,66,0",
            "<path d=\"M-79.7,-33H79.7A19.8,33 0 0 1 79.7,33H-79.7L-99.5,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "curv-trap",
            7.2,
            63.0,
            "95.4,87,0",
            "<path d=\"M-21.6,-43.5H21.6A26.1,43.5 0 0 1 21.6,43.5H-21.6L-47.7,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "docs",
            0.0,
            0.0,
            "44,42,-1",
            "<g transform=\"translate(4,-4)\"><path d=\"M-18,-17H18V7q-9,-12 -18,0t-18,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/></g><g transform=\"translate(0,0)\"><path d=\"M-18,-17H18V7q-9,-12 -18,0t-18,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/></g><g transform=\"translate(-4,4)\"><path d=\"M-18,-17H18V7q-9,-12 -18,0t-18,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/></g>",
            "0,0,0,0,0,0,10,10,10,0,0,0",
            "0,0",
            true,
        ),
        (
            "docs",
            50.0,
            21.0,
            "94,63,-1",
            "<g transform=\"translate(4,-4)\"><path d=\"M-43,-27.5H43V17.5q-21.5,-12 -43,0t-43,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/></g><g transform=\"translate(0,0)\"><path d=\"M-43,-27.5H43V17.5q-21.5,-12 -43,0t-43,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/></g><g transform=\"translate(-4,4)\"><path d=\"M-43,-27.5H43V17.5q-21.5,-12 -43,0t-43,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/></g>",
            "0,0,0,0,0,0,10,10,10,0,0,0",
            "0,0",
            true,
        ),
        (
            "docs",
            123.4,
            42.0,
            "167.4,84,-1",
            "<g transform=\"translate(4,-4)\"><path d=\"M-79.7,-38H79.7V28q-39.85,-12 -79.7,0t-79.7,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/></g><g transform=\"translate(0,0)\"><path d=\"M-79.7,-38H79.7V28q-39.85,-12 -79.7,0t-79.7,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/></g><g transform=\"translate(-4,4)\"><path d=\"M-79.7,-38H79.7V28q-39.85,-12 -79.7,0t-79.7,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/></g>",
            "0,0,0,0,0,0,10,10,10,0,0,0",
            "0,0",
            true,
        ),
        (
            "docs",
            7.2,
            63.0,
            "51.2,105,-1",
            "<g transform=\"translate(4,-4)\"><path d=\"M-21.6,-48.5H21.6V38.5q-10.8,-12 -21.6,0t-21.6,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/></g><g transform=\"translate(0,0)\"><path d=\"M-21.6,-48.5H21.6V38.5q-10.8,-12 -21.6,0t-21.6,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/></g><g transform=\"translate(-4,4)\"><path d=\"M-21.6,-48.5H21.6V38.5q-10.8,-12 -21.6,0t-21.6,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/></g>",
            "0,0,0,0,0,0,10,10,10,0,0,0",
            "0,0",
            true,
        ),
        (
            "bow-rect",
            0.0,
            0.0,
            "45.6,24,0",
            "<path d=\"M-18,-12H22.8A4.8,12 0 0 0 22.8,12H-18A4.8,12 0 0 1 -18,-12Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,4.8,4.8,4.8,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "bow-rect",
            50.0,
            21.0,
            "104,45,0",
            "<path d=\"M-43,-22.5H52A9,22.5 0 0 0 52,22.5H-43A9,22.5 0 0 1 -43,-22.5Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,9,9,9,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "bow-rect",
            123.4,
            42.0,
            "185.8,66,0",
            "<path d=\"M-79.7,-33H92.9A13.2,33 0 0 0 92.9,33H-79.7A13.2,33 0 0 1 -79.7,-33Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,13.2,13.2,13.2,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "bow-rect",
            7.2,
            63.0,
            "78,87,0",
            "<path d=\"M-21.6,-43.5H39A17.4,43.5 0 0 0 39,43.5H-21.6A17.4,43.5 0 0 1 -21.6,-43.5Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,17.4,17.4,17.4,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "tag-rect",
            0.0,
            0.0,
            "48,24,0",
            "<rect x=\"-24\" y=\"-12\" width=\"48\" height=\"24\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M12,12L24,0\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "24,0",
            true,
        ),
        (
            "tag-rect",
            50.0,
            21.0,
            "86,45,0",
            "<rect x=\"-43\" y=\"-22.5\" width=\"86\" height=\"45\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M31,22.5L43,10.5\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "62,21",
            true,
        ),
        (
            "tag-rect",
            123.4,
            42.0,
            "159.4,66,0",
            "<rect x=\"-79.7\" y=\"-33\" width=\"159.4\" height=\"66\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M67.7,33L79.7,21\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "135.4,42",
            true,
        ),
        (
            "tag-rect",
            7.2,
            63.0,
            "48,87,0",
            "<rect x=\"-24\" y=\"-43.5\" width=\"48\" height=\"87\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M12,43.5L24,31.5\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "24,63",
            true,
        ),
        (
            "tag-doc",
            0.0,
            0.0,
            "36,34,-5",
            "<path d=\"M-18,-17H18V7q-9,-12 -18,0t-18,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M6,3L18,-9\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,10,10,10,0,0,0",
            "0,0",
            true,
        ),
        (
            "tag-doc",
            50.0,
            21.0,
            "86,55,-5",
            "<path d=\"M-43,-27.5H43V17.5q-21.5,-12 -43,0t-43,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M31,13.5L43,1.5\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,10,10,10,0,0,0",
            "0,0",
            true,
        ),
        (
            "tag-doc",
            123.4,
            42.0,
            "159.4,76,-5",
            "<path d=\"M-79.7,-38H79.7V28q-39.85,-12 -79.7,0t-79.7,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M67.7,24L79.7,12\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,10,10,10,0,0,0",
            "0,0",
            true,
        ),
        (
            "tag-doc",
            7.2,
            63.0,
            "43.2,97,-5",
            "<path d=\"M-21.6,-48.5H21.6V38.5q-10.8,-12 -21.6,0t-21.6,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M9.6,34.5L21.6,22.5\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,10,10,10,0,0,0",
            "0,0",
            true,
        ),
        (
            "lin-doc",
            0.0,
            0.0,
            "44,34,-5",
            "<path d=\"M-22,-17H22V7q-11,-12 -22,0t-22,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-14,-17V11\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,10,10,10,0,0,0",
            "0,0",
            true,
        ),
        (
            "lin-doc",
            50.0,
            21.0,
            "94,55,-5",
            "<path d=\"M-47,-27.5H47V17.5q-23.5,-12 -47,0t-47,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-39,-27.5V21.5\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,10,10,10,0,0,0",
            "0,0",
            true,
        ),
        (
            "lin-doc",
            123.4,
            42.0,
            "167.4,76,-5",
            "<path d=\"M-83.7,-38H83.7V28q-41.85,-12 -83.7,0t-83.7,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-75.7,-38V32\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,10,10,10,0,0,0",
            "0,0",
            true,
        ),
        (
            "lin-doc",
            7.2,
            63.0,
            "51.2,97,-5",
            "<path d=\"M-25.6,-48.5H25.6V38.5q-12.8,-12 -25.6,0t-25.6,0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><path d=\"M-17.6,-48.5V42.5\" fill=\"none\" stroke=\"var(--_l)\"/>",
            "0,0,0,0,0,0,10,10,10,0,0,0",
            "0,0",
            true,
        ),
        (
            "collapsedGroup",
            0.0,
            0.0,
            "44,32,-2",
            "<rect x=\"-16\" y=\"-8\" width=\"32\" height=\"24\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><rect x=\"-22\" y=\"-16\" width=\"44\" height=\"28\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "collapsedGroup",
            50.0,
            21.0,
            "94,53,-2",
            "<rect x=\"-41\" y=\"-18.5\" width=\"82\" height=\"45\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><rect x=\"-47\" y=\"-26.5\" width=\"94\" height=\"49\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "collapsedGroup",
            123.4,
            42.0,
            "167.4,74,-2",
            "<rect x=\"-77.7\" y=\"-29\" width=\"155.4\" height=\"66\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><rect x=\"-83.7\" y=\"-37\" width=\"167.4\" height=\"70\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "collapsedGroup",
            7.2,
            63.0,
            "51.2,95,-2",
            "<rect x=\"-19.6\" y=\"-39.5\" width=\"39.2\" height=\"87\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/><rect x=\"-25.6\" y=\"-47.5\" width=\"51.2\" height=\"91\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "nope",
            0.0,
            0.0,
            "48,24,0",
            "<rect x=\"-24\" y=\"-12\" width=\"48\" height=\"24\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "nope",
            50.0,
            21.0,
            "86,45,0",
            "<rect x=\"-43\" y=\"-22.5\" width=\"86\" height=\"45\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "nope",
            123.4,
            42.0,
            "159.4,66,0",
            "<rect x=\"-79.7\" y=\"-33\" width=\"159.4\" height=\"66\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "nope",
            7.2,
            63.0,
            "48,87,0",
            "<rect x=\"-24\" y=\"-43.5\" width=\"48\" height=\"87\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "constructor",
            0.0,
            0.0,
            "48,24,0",
            "<rect x=\"-24\" y=\"-12\" width=\"48\" height=\"24\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "constructor",
            50.0,
            21.0,
            "86,45,0",
            "<rect x=\"-43\" y=\"-22.5\" width=\"86\" height=\"45\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "constructor",
            123.4,
            42.0,
            "159.4,66,0",
            "<rect x=\"-79.7\" y=\"-33\" width=\"159.4\" height=\"66\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "constructor",
            7.2,
            63.0,
            "48,87,0",
            "<rect x=\"-24\" y=\"-43.5\" width=\"48\" height=\"87\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "",
            0.0,
            0.0,
            "48,24,0",
            "<rect x=\"-24\" y=\"-12\" width=\"48\" height=\"24\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "",
            50.0,
            21.0,
            "86,45,0",
            "<rect x=\"-43\" y=\"-22.5\" width=\"86\" height=\"45\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "",
            123.4,
            42.0,
            "159.4,66,0",
            "<rect x=\"-79.7\" y=\"-33\" width=\"159.4\" height=\"66\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
        (
            "",
            7.2,
            63.0,
            "48,87,0",
            "<rect x=\"-24\" y=\"-43.5\" width=\"48\" height=\"87\" rx=\"var(--ondine-radius,0)\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
            "0,0,0,0,0,0,0,0,0,0,0,0",
            "0,0",
            true,
        ),
    ];

    #[test]
    fn shapes_match_pele() {
        for &(name, tw, th, size, drawn, insets, spans, has_label) in CASES {
            let s = shape_size(name, tw, th);
            assert_eq!(
                format!("{},{},{}", num(s.w), num(s.h), num(s.dy)),
                size,
                "{name} {tw} {th}"
            );
            assert_eq!(
                draw_shape(
                    name,
                    s.w,
                    s.h,
                    " fill=\"var(--_s)\" stroke=\"var(--_b)\"",
                    " stroke=\"var(--_l)\""
                ),
                drawn,
                "{name} {tw} {th}"
            );
            let got: Vec<String> = (0..4)
                .flat_map(|side| {
                    [0.0, 5.5, -30.0].map(|t| num(shape_inset(name, s.w, s.h, side, t)))
                })
                .collect();
            assert_eq!(got.join(","), insets, "{name}");
            assert_eq!(
                format!(
                    "{},{}",
                    num(shape_span(name, s.w, s.h, false)),
                    num(shape_span(name, s.w, s.h, true))
                ),
                spans,
                "{name}"
            );
            assert_eq!(shape_has_label(name), has_label, "{name}");
        }
    }

    #[expect(clippy::type_complexity, reason = "a table of cases")]
    const INSETS: &[(
        &[f64],
        usize,
        &str,
        f64,
        f64,
        (f64, f64, f64, f64, bool),
        i32,
        &str,
    )] = &[
        (
            &[100.0, 80.0, 0.0, 100.0, 200.0, 0.0],
            0,
            "circle",
            60.0,
            60.0,
            (100.0, 110.0, 60.0, 60.0, false),
            0,
            "100,80,0,100,200,0",
        ),
        (
            &[100.0, 80.0, 0.0, 100.0, 200.0, 0.0],
            3,
            "diam",
            80.0,
            40.0,
            (100.0, 220.0, 80.0, 40.0, false),
            0,
            "100,80,0,100,200,0",
        ),
        (
            &[130.0, 110.0, 1.0, 300.0, 110.0, 1.0],
            0,
            "stadium",
            60.0,
            40.0,
            (100.0, 110.0, 60.0, 40.0, false),
            1,
            "130,110,1,300,110,1",
        ),
        (
            &[70.0, 115.0, 1.0, 0.0, 0.0, 1.0],
            0,
            "hex",
            60.0,
            40.0,
            (100.0, 110.0, 60.0, 40.0, false),
            3,
            "72.5,115,1,0,0,1",
        ),
        (
            &[100.0, 80.0, 0.0],
            0,
            "rect",
            50.0,
            30.0,
            (100.0, 110.0, 60.0, 60.0, false),
            0,
            "100,95,0",
        ),
        (
            &[100.0, 80.0, 0.0],
            0,
            "cyl",
            60.0,
            60.0,
            (100.0, 110.0, 60.0, 60.0, true),
            0,
            "100,80,0",
        ),
        (
            &[105.0, 105.0, 0.0],
            0,
            "circle",
            60.0,
            60.0,
            (100.0, 110.0, 60.0, 60.0, false),
            -1,
            "105,105,0",
        ),
    ];

    #[test]
    fn inset_route_matches_pele() {
        for &(route, at, shape, w, h, (x, y, cw, ch, group), side, expected) in INSETS {
            let mut c = cnode(cw, ch, -1);
            c.x = x;
            c.y = y;
            c.is_group = group;
            assert_eq!(end_side(route, at, &c), side, "{shape}");
            let mut moved = route.to_vec();
            inset_route(&mut moved, at, shape, w, h, &c);
            let got: Vec<String> = moved.iter().map(|&v| num(v)).collect();
            assert_eq!(got.join(","), expected, "{shape}");
        }
    }
}
