//! The end markers of class relations. Port of Pele's `src/diagrams/class/markers.ts`.

use super::types::RelationEnd;
use crate::svg::builder::push_num;
use crate::svg::edges::{marker, marker_trim};

/// Length of the extension triangle.
const TRIANGLE: f64 = 12.0;
/// Length of the aggregation and composition diamonds.
const DIAMOND: f64 = 16.0;
/// Half the width of a triangle.
const HALF: f64 = 6.0;
/// Radius of the lollipop circle.
const RING: f64 = 5.0;

/// How far the line stops short of the node so that it meets the back of the marker.
pub(crate) fn class_marker_trim(r#type: RelationEnd) -> f64 {
    match r#type {
        RelationEnd::Type(1) => TRIANGLE,
        RelationEnd::Type(0 | 2) => DIAMOND,
        RelationEnd::Type(4) => 2.0 * RING,
        RelationEnd::Type(3) => marker_trim("arrow_point"),
        _ => 0.0,
    }
}

/// Appends `x,y`.
fn push_point(out: &mut String, x: f64, y: f64) {
    push_num(out, x);
    out.push(',');
    push_num(out, y);
}

/// Draws an end marker whose tip is at (`x`, `y`), pointing along (`dx`, `dy`).
pub(crate) fn class_marker(
    r#type: RelationEnd,
    x: f64,
    y: f64,
    dx: f64,
    dy: f64,
    color: &str,
) -> String {
    let nx = -dy;
    let ny = dx;
    let mut out = String::new();
    match r#type {
        RelationEnd::Type(1) => {
            let bx = x - dx * TRIANGLE;
            let by = y - dy * TRIANGLE;
            out.push_str("<path class=\"ondine-marker\" d=\"M");
            push_point(&mut out, x, y);
            out.push('L');
            push_point(&mut out, bx + nx * HALF, by + ny * HALF);
            out.push('L');
            push_point(&mut out, bx - nx * HALF, by - ny * HALF);
            out.push_str("Z\" stroke-linejoin=\"round\"/>");
        }
        RelationEnd::Type(kind @ (0 | 2)) => {
            let mx = x - (dx * DIAMOND) / 2.0;
            let my = y - (dy * DIAMOND) / 2.0;
            let half = HALF - 1.0;
            out.push_str("<path class=\"ondine-marker\" d=\"M");
            push_point(&mut out, x, y);
            out.push('L');
            push_point(&mut out, mx + nx * half, my + ny * half);
            out.push('L');
            push_point(&mut out, x - dx * DIAMOND, y - dy * DIAMOND);
            out.push('L');
            push_point(&mut out, mx - nx * half, my - ny * half);
            out.push_str("Z\" stroke-linejoin=\"round\"");
            if kind == 2 {
                out.push_str(" fill=\"");
                out.push_str(color);
                out.push('"');
            }
            out.push_str("/>");
        }
        RelationEnd::Type(4) => {
            out.push_str("<circle class=\"ondine-marker\" cx=\"");
            push_num(&mut out, x - dx * RING);
            out.push_str("\" cy=\"");
            push_num(&mut out, y - dy * RING);
            out.push_str("\" r=\"5\"/>");
        }
        RelationEnd::Type(3) => out = marker("arrow_point", x, y, dx, dy, color),
        _ => {}
    }
    out
}
