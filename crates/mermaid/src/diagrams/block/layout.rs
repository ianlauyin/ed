//! The block grid: places each block in its container's columns and sizes the containers. Port of
//! Pele's `src/diagrams/block/layout.ts`.

use crate::error::{Code, Error};

/// A block in the grid. Cells live in one list and name their children by index.
#[derive(Clone, Debug, Default)]
pub(crate) struct Cell {
    /// Child cells, by index.
    pub(crate) children: Vec<usize>,
    /// Grid column.
    pub(crate) col: f64,
    /// Number of columns in use.
    pub(crate) cols: f64,
    /// Grid width of this container: a positive count, or -1 to put every child on one row.
    pub(crate) columns: f64,
    /// Height in pixels.
    pub(crate) h: f64,
    /// Height reserved above a container's grid for its title.
    pub(crate) head: f64,
    /// Height of each row.
    pub(crate) heights: Vec<f64>,
    /// Height the content needs. Set by the caller for leaves, computed here for containers.
    pub(crate) nh: f64,
    /// Width the content needs. Set by the caller for leaves, computed here for containers.
    pub(crate) nw: f64,
    /// Grid row.
    pub(crate) row: usize,
    /// Whether the cell is empty space.
    pub(crate) space: bool,
    /// Number of columns spanned.
    pub(crate) span: f64,
    /// Width in pixels.
    pub(crate) w: f64,
    /// Center x.
    pub(crate) x: f64,
    /// Center y.
    pub(crate) y: f64,
}

const MAX_SPAN: f64 = 1000.0;

/// Whether `columns` is a grid width Pele's `calculateBlockPosition` accepts.
#[expect(clippy::float_cmp, reason = "Number.isInteger")]
fn valid_columns(columns: f64) -> bool {
    columns != 0.0 && columns.is_finite() && columns.trunc() == columns
}

/// The error Pele's `calculateBlockPosition` throws for a grid width of 0.
///
/// # Errors
///
/// Returns a semantic error when a container with children has an invalid grid width.
pub(crate) fn check_columns(columns: f64) -> Result<(), Error> {
    if valid_columns(columns) {
        return Ok(());
    }
    let mut error = Error::new(Code::Semantic, "Columns must be an integer !== 0.");
    error.r#type = Some("block");
    Err(error)
}

/// The grid column and row of a position. `columns` has been checked.
fn block_position(columns: f64, position: f64) -> (f64, f64) {
    if columns < 0.0 {
        return (position, 0.0);
    }
    (position % columns, (position / columns).floor())
}

/// Puts each child in its grid cell and sizes the container. Every column is as wide as the widest
/// block needs. Every row that holds a plain block is as tall as the tallest block in the
/// container, as in Mermaid. A row of nothing but containers is only as tall as they need.
fn measure(cells: &mut [Cell], at: usize, gap: f64, inset: f64) {
    let Some(cell) = cells.get(at) else {
        return;
    };
    let columns = cell.columns;
    let children = cell.children.clone();
    let mut heights: Vec<f64> = Vec::new();
    let mut position = 0.0;
    let mut cols: f64 = 0.0;
    let mut cw: f64 = 0.0;
    let mut tallest: f64 = 0.0;
    for &c in &children {
        let Some(child) = cells.get_mut(c) else {
            continue;
        };
        let (px, py) = block_position(columns, position);
        // Mermaid lets a block wider than the rest of its row stick out of the grid. Here it
        // stops at the last column.
        let mut span = if child.span >= 1.0 {
            child.span.min(MAX_SPAN)
        } else {
            1.0
        };
        if columns > 0.0 && span > columns - px {
            span = columns - px;
        }
        child.span = span;
        child.col = px;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "rows count up from 0 one at a time"
        )]
        let py = py as usize;
        child.row = py;
        position += span;
        if px + span > cols {
            cols = px + span;
        }
        if py == heights.len() {
            heights.push(0.0);
        }
        if let Some(height) = heights.get_mut(py) {
            if child.children.is_empty() {
                *height = -1.0;
            } else if *height >= 0.0 && child.nh > *height {
                *height = child.nh;
            }
        }
        if child.space {
            continue;
        }
        let width = (child.nw - gap * (span - 1.0)) / span;
        if width > cw {
            cw = width;
        }
        if child.nh > tallest {
            tallest = child.nh;
        }
    }

    #[expect(clippy::cast_precision_loss, reason = "row counts stay far below 2^52")]
    let mut height = (heights.len() as f64 - 1.0) * gap;
    for row in &mut heights {
        if *row < 0.0 {
            *row = tallest;
        }
        height += *row;
    }
    if let Some(cell) = cells.get_mut(at) {
        cell.cols = cols;
        cell.heights = heights;
        cell.nw = cell.nw.max(cols * cw + (cols - 1.0) * gap + 2.0 * inset);
        cell.nh = height + 2.0 * inset + cell.head;
    }
}

/// Gives each child its cell, sharing out any room the container has beyond what it needs.
fn arrange(cells: &mut [Cell], at: usize, gap: f64, inset: f64) {
    let Some(cell) = cells.get(at) else {
        return;
    };
    let cols = cell.cols;
    let heights = cell.heights.clone();
    let children = cell.children.clone();
    #[expect(clippy::cast_precision_loss, reason = "row counts stay far below 2^52")]
    let rows = heights.len() as f64;
    let cw = (cell.w - 2.0 * inset - (cols - 1.0) * gap) / cols;
    let room = cell.h - 2.0 * inset - cell.head - (rows - 1.0) * gap;
    let needed = heights.iter().fold(0.0, |sum, height| sum + height);
    let scale = if needed > 0.0 { room / needed } else { 0.0 };
    let share = if needed > 0.0 { 0.0 } else { room / rows };
    let left = cell.x - cell.w / 2.0 + inset;
    let mut top = cell.y - cell.h / 2.0 + inset + cell.head;
    let mut row = 0;
    for &c in &children {
        let Some(child) = cells.get_mut(c) else {
            continue;
        };
        while row < child.row {
            top += heights.get(row).copied().unwrap_or(0.0) * scale + share + gap;
            row += 1;
        }
        child.w = cw * child.span + gap * (child.span - 1.0);
        child.h = heights.get(row).copied().unwrap_or(0.0) * scale + share;
        child.x = left + child.col * (cw + gap) + child.w / 2.0;
        child.y = top + child.h / 2.0;
    }
}

/// Lays out a tree of cells in place, `cells[0]` the root, listed so that every cell comes after
/// its parent in breadth-first order. The root has no border, so its grid starts at the origin.
/// Returns the root's width and height.
pub(crate) fn layout(cells: &mut [Cell], gap: f64) -> (f64, f64) {
    for at in (0..cells.len()).rev() {
        if cells.get(at).is_some_and(|cell| !cell.children.is_empty()) {
            measure(cells, at, gap, if at == 0 { 0.0 } else { gap });
        }
    }
    let Some(root) = cells.get_mut(0) else {
        return (0.0, 0.0);
    };
    root.w = root.nw;
    root.h = root.nh;
    root.x = root.w / 2.0;
    root.y = root.h / 2.0;
    let size = (root.w, root.h);
    for at in 0..cells.len() {
        if cells.get(at).is_some_and(|cell| !cell.children.is_empty()) {
            arrange(cells, at, gap, if at == 0 { 0.0 } else { gap });
        }
    }
    size
}
