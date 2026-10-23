//! Crow's foot cardinality markers. Port of Pele's `src/diagrams/er/markers.ts`.

use crate::svg::builder::push_num;

/// How far a marker reaches back along its line from the box it touches.
pub(crate) const MARKER_LENGTH: f64 = 18.0;

/// Half the length of a bar.
const BAR: f64 = 5.0;
/// Half the spread of a crow's foot.
const FOOT: f64 = 6.0;
/// Length of a crow's foot.
const FOOT_LENGTH: f64 = 10.0;
/// Radius of a ring.
const RING: f64 = 3.5;

/// Draws a crow's foot cardinality marker over the end of a line that meets a box at
/// (`x`, `y`) while travelling along the unit vector (`dx`, `dy`). Types are Mermaid's
/// lower-cased cardinalities.
pub(crate) fn er_marker(r#type: &str, x: f64, y: f64, dx: f64, dy: f64) -> String {
    let at = |out: &mut String, back: f64, side: f64| {
        push_num(out, x - dx * back - dy * side);
        out.push(',');
        push_num(out, y - dy * back + dx * side);
    };
    let bar = |out: &mut String, back: f64| {
        out.push('M');
        at(out, back, -BAR);
        out.push('L');
        at(out, back, BAR);
    };
    let foot = |out: &mut String| {
        out.push('M');
        at(out, 0.0, -FOOT);
        out.push('L');
        at(out, FOOT_LENGTH, 0.0);
        out.push('L');
        at(out, 0.0, FOOT);
    };
    let ring = |out: &mut String, back: f64| {
        out.push_str("<circle class=\"ondine-marker\" cx=\"");
        push_num(out, x - dx * back);
        out.push_str("\" cy=\"");
        push_num(out, y - dy * back);
        out.push_str("\" r=\"3.5\" fill=\"var(--_bg)\"/>");
    };

    let mut out = String::new();
    match r#type {
        "only_one" => {
            out.push_str("<path class=\"ondine-marker\" d=\"");
            bar(&mut out, 6.0);
            bar(&mut out, 10.0);
            out.push_str("\"/>");
        }
        "zero_or_one" => {
            out.push_str("<path class=\"ondine-marker\" d=\"");
            bar(&mut out, 6.0);
            out.push_str("\"/>");
            ring(&mut out, 13.5);
        }
        "one_or_more" => {
            out.push_str("<path class=\"ondine-marker\" d=\"");
            foot(&mut out);
            bar(&mut out, FOOT_LENGTH + 3.5);
            out.push_str("\"/>");
        }
        "zero_or_more" => {
            out.push_str("<path class=\"ondine-marker\" d=\"");
            foot(&mut out);
            out.push_str("\"/>");
            ring(&mut out, FOOT_LENGTH + RING + 0.5);
        }
        "md_parent" => {
            out.push_str("<path class=\"ondine-marker\" d=\"M");
            at(&mut out, 0.0, 0.0);
            out.push('L');
            at(&mut out, 6.0, 4.0);
            out.push('L');
            at(&mut out, 12.0, 0.0);
            out.push('L');
            at(&mut out, 6.0, -4.0);
            out.push_str("Z\" fill=\"var(--_bg)\"/>");
        }
        _ => {}
    }
    out
}
