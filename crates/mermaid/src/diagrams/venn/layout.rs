//! Places venn circles and finds room for labels in their regions. Port of Pele's
//! `src/diagrams/venn/layout.ts`.
//!
//! Output depends on the last bit of these computations, so `hypot` is V8's, and the
//! trigonometric functions are the fdlibm ones that both V8 and Rust's WebAssembly math use.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

use super::db::utf16_cmp;
use super::types::VennSubset;
use crate::svg::builder::push_num;

/// A circle for one set.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Circle {
    /// The set's id.
    pub(crate) id: String,
    /// Radius.
    pub(crate) r: f64,
    /// Center x.
    pub(crate) x: f64,
    /// Center y.
    pub(crate) y: f64,
}

/// Where the content of a region goes: the point inside all of `inside` and outside every other
/// circle that is farthest from their outlines, and how far that is. A margin of 0 or less means
/// the circles as placed leave the region empty.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Spot {
    /// Distance to the nearest outline.
    pub(crate) margin: f64,
    /// x.
    pub(crate) x: f64,
    /// y.
    pub(crate) y: f64,
}

/// V8's `Math.hypot` for two values, copied from `crate::layout::layered`: a compensated sum of
/// squares scaled by the larger value. It differs from `f64::hypot` in the last bit.
pub(crate) fn hypot(x: f64, y: f64) -> f64 {
    let x = x.abs();
    let y = y.abs();
    if x == f64::INFINITY || y == f64::INFINITY {
        return f64::INFINITY;
    }
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    let max = if y > x { y } else { x };
    if max == 0.0 {
        return 0.0;
    }

    let mut sum = 0.0;
    let mut compensation = 0.0;
    for value in [x, y] {
        let n = value / max;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * max
}

/// `Math.min` and `Math.max`, which return `NaN` when either value is.
fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if b < a {
        b
    } else {
        a
    }
}

fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if b > a {
        b
    } else {
        a
    }
}

/// For every overlap of three or more sets, adds the pairwise overlaps it implies and the diagram
/// does not state, a quarter of the smaller set each, so that the circles are drawn overlapping.
pub(crate) fn ensure_pairwise_subsets(subsets: &[VennSubset]) -> Vec<VennSubset> {
    let mut present: BTreeSet<Vec<String>> = BTreeSet::new();
    let mut sizes: BTreeMap<&str, f64> = BTreeMap::new();
    for subset in subsets {
        let mut sorted = subset.sets.clone();
        sorted.sort_by(|a, b| utf16_cmp(a, b));
        present.insert(sorted);
        if let [only] = subset.sets.as_slice() {
            sizes.insert(only, subset.size);
        }
    }
    let mut out = subsets.to_vec();
    for subset in subsets {
        if subset.sets.len() < 3 {
            continue;
        }
        let mut members = subset.sets.clone();
        members.sort_by(|a, b| utf16_cmp(a, b));
        for (i, first) in members.iter().enumerate() {
            for second in members.iter().skip(i + 1) {
                let pair = vec![first.clone(), second.clone()];
                if !present.insert(pair.clone()) {
                    continue;
                }
                let size = match (sizes.get(first.as_str()), sizes.get(second.as_str())) {
                    (Some(&a), Some(&b)) => js_min(a, b) / 4.0,
                    _ => 2.5,
                };
                out.push(VennSubset {
                    label: Some(String::new()),
                    sets: pair,
                    size,
                });
            }
        }
    }
    out
}

/// The area two circles share when their centers are `d` apart.
pub(crate) fn lens_area(r1: f64, r2: f64, d: f64) -> f64 {
    if d >= r1 + r2 {
        return 0.0;
    }
    let small = js_min(r1, r2);
    if d <= (r1 - r2).abs() {
        return PI * small * small;
    }
    let a1 = r1 * r1 * ((d * d + r1 * r1 - r2 * r2) / (2.0 * d * r1)).acos();
    let a2 = r2 * r2 * ((d * d + r2 * r2 - r1 * r1) / (2.0 * d * r2)).acos();
    let a3 = 0.5
        * js_max(
            (-d + r1 + r2) * (d + r1 - r2) * (d - r1 + r2) * (d + r1 + r2),
            0.0,
        )
        .sqrt();
    a1 + a2 - a3
}

/// How far apart two circles sit for their overlap to have the given area. The area falls as the
/// distance grows, so bisection finds it.
#[expect(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "NaN fails the test, as in Pele"
)]
pub(crate) fn distance_for_overlap(r1: f64, r2: f64, area: f64) -> f64 {
    let mut low = (r1 - r2).abs();
    let mut high = r1 + r2;
    if !(area > 0.0) {
        return high;
    }
    let small = js_min(r1, r2);
    if area >= PI * (small * small) {
        return low;
    }
    for _ in 0..48 {
        let middle = f64::midpoint(low, high);
        if lens_area(r1, r2, middle) > area {
            low = middle;
        } else {
            high = middle;
        }
    }
    f64::midpoint(low, high)
}

/// How far a point is from the nearest outline it must stay within or without. Gives up, with
/// some value below `floor`, as soon as the point cannot reach `floor`.
fn margin(
    circles: &[Circle],
    inside: &[usize],
    outside: &[usize],
    x: f64,
    y: f64,
    floor: f64,
) -> f64 {
    let mut best = f64::INFINITY;
    for c in inside.iter().filter_map(|&i| circles.get(i)) {
        let dx = x - c.x;
        let dy = y - c.y;
        let room = c.r - (dx * dx + dy * dy).sqrt();
        if room < best {
            if room < floor {
                return room;
            }
            best = room;
        }
    }
    for c in outside.iter().filter_map(|&i| circles.get(i)) {
        let dx = x - c.x;
        let dy = y - c.y;
        let room = (dx * dx + dy * dy).sqrt() - c.r;
        if room < best {
            if room < floor {
                return room;
            }
            best = room;
        }
    }
    best
}

/// Finds the roomiest point of a region by sampling the smallest of its circles on a grid and then
/// walking uphill from the best sample. Ties go to the point nearest the middle of the region, so
/// a symmetric arrangement gets symmetric labels.
#[expect(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "NaN fails the test, as in Pele"
)]
pub(crate) fn find_spot(circles: &[Circle], inside: &[usize], outside: &[usize]) -> Spot {
    const GRID: i32 = 10;

    let count = inside.iter().fold(0.0, |count, _| count + 1.0);
    let mut cx = 0.0;
    let mut cy = 0.0;
    let Some(mut smallest) = inside.first().and_then(|&i| circles.get(i)) else {
        return Spot::default();
    };
    for c in inside.iter().filter_map(|&i| circles.get(i)) {
        cx += c.x / count;
        cy += c.y / count;
        if c.r < smallest.r {
            smallest = c;
        }
    }
    let r = smallest.r;
    let mut x = cx;
    let mut y = cy;
    let mut best = margin(circles, inside, outside, x, y, f64::NEG_INFINITY);
    let mut off = 0.0;
    let mut consider = |px: f64, py: f64, best: &mut f64, x: &mut f64, y: &mut f64| -> bool {
        let m = margin(circles, inside, outside, px, py, *best - r * 1e-9);
        if !(m > *best - r * 1e-9) {
            return false;
        }
        let distance = hypot(px - cx, py - cy);
        if m > *best + r * 1e-9 || distance < off - r * 1e-9 {
            *best = m;
            off = distance;
            *x = px;
            *y = py;
            return true;
        }
        false
    };
    for gy in -GRID..=GRID {
        for gx in -GRID..=GRID {
            if gx * gx + gy * gy <= GRID * GRID {
                consider(
                    smallest.x + (f64::from(gx) * r) / f64::from(GRID),
                    smallest.y + (f64::from(gy) * r) / f64::from(GRID),
                    &mut best,
                    &mut x,
                    &mut y,
                );
            }
        }
    }
    let mut step = r / f64::from(GRID);
    let mut round = 0;
    while round < 40 && step > r * 1e-6 {
        let mut moved = false;
        for k in 0..8 {
            let angle = (f64::from(k) * TAU) / 8.0;
            let (px, py) = (x + angle.cos() * step, y + angle.sin() * step);
            if consider(px, py, &mut best, &mut x, &mut y) {
                moved = true;
            }
        }
        if !moved {
            step /= 2.0;
        }
        round += 1;
    }
    Spot { margin: best, x, y }
}

/// `value` as a count, for arithmetic.
fn count(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}

/// Places one circle per set, with areas in proportion to the sizes of the sets and each pair as
/// far apart as its overlap asks: one, two and three sets exactly, more by relaxing a ring of
/// circles toward those distances. Lengths are in units of the square root of a size.
#[expect(clippy::too_many_lines, reason = "mirrors Pele's layoutCircles")]
pub(crate) fn layout_circles(subsets: &[VennSubset]) -> Vec<Circle> {
    let mut sizes: Vec<(&str, f64)> = Vec::new();
    let mut size_index: BTreeMap<&str, usize> = BTreeMap::new();
    for subset in subsets {
        for id in &subset.sets {
            if !size_index.contains_key(id.as_str()) {
                size_index.insert(id, sizes.len());
                sizes.push((id, 10.0));
            }
        }
    }
    for subset in subsets {
        if let [only] = subset.sets.as_slice()
            && let Some(entry) = size_index
                .get(only.as_str())
                .and_then(|&at| sizes.get_mut(at))
        {
            entry.1 = subset.size;
        }
    }

    let mut largest = 0.0;
    for &(_, size) in &sizes {
        if size.is_finite() && size > largest {
            largest = size;
        }
    }
    if largest == 0.0 {
        largest = 10.0;
    }
    let widest = (largest / PI).sqrt();
    let mut circles: Vec<Circle> = sizes
        .iter()
        .map(|&(id, size)| {
            // A set too small to hold its name, or with no size at all, still gets a circle that
            // shows.
            let r = if size.is_finite() && size > 0.0 {
                (size / PI).sqrt()
            } else {
                0.0
            };
            Circle {
                id: id.to_owned(),
                r: js_max(r, widest * 0.3),
                x: 0.0,
                y: 0.0,
            }
        })
        .collect();
    let n = circles.len();
    if n < 2 {
        return circles;
    }

    // The overlap each pair was given, by the indexes of its circles.
    let mut overlaps: BTreeMap<usize, f64> = BTreeMap::new();
    for subset in ensure_pairwise_subsets(subsets) {
        let [a, b] = subset.sets.as_slice() else {
            continue;
        };
        let (Some(&a), Some(&b)) = (size_index.get(a.as_str()), size_index.get(b.as_str())) else {
            continue;
        };
        if a != b {
            overlaps.insert(a.min(b) * n + a.max(b), subset.size);
        }
    }
    // How far apart each overlapping pair should be. Pairs that do not overlap only have to keep a
    // gap.
    let radius = |circles: &[Circle], i: usize| circles.get(i).map_or(0.0, |c| c.r);
    let mut wanted: BTreeMap<usize, f64> = BTreeMap::new();
    for (&key, &area) in &overlaps {
        let d = distance_for_overlap(radius(&circles, key / n), radius(&circles, key % n), area);
        wanted.insert(key, d);
    }
    let gap = widest * 0.15;
    let distance = |circles: &[Circle], a: usize, b: usize| {
        wanted
            .get(&(a * n + b))
            .copied()
            .unwrap_or_else(|| radius(circles, a) + radius(circles, b) + gap)
    };

    if n == 2 {
        let d = distance(&circles, 0, 1);
        if let Some(c) = circles.get_mut(1) {
            c.x = d;
        }
        return circles;
    }

    if n == 3 {
        // Two sets side by side and the third below, the corners of a triangle whose sides are
        // the three distances. An overlap of all three has to exist, so the triangle shrinks until
        // it does.
        let mut ab = distance(&circles, 0, 1);
        let mut ac = distance(&circles, 0, 2);
        let mut bc = distance(&circles, 1, 2);
        let triple = subsets.iter().any(|subset| {
            let distinct: BTreeSet<&String> = subset.sets.iter().collect();
            distinct.len() == 3
        });
        for _ in 0..40 {
            let x = if ab > 0.0 {
                (ab * ab + ac * ac - bc * bc) / (2.0 * ab)
            } else {
                0.0
            };
            if let Some(c) = circles.get_mut(1) {
                c.x = ab;
            }
            if let Some(c) = circles.get_mut(2) {
                c.x = js_max(-ac, js_min(ac, x));
                c.y = js_max(ac * ac - x * x, 0.0).sqrt();
            }
            if !triple || find_spot(&circles, &[0, 1, 2], &[]).margin > widest * 0.12 {
                break;
            }
            ab *= 0.94;
            ac *= 0.94;
            bc *= 0.94;
        }
        return circles;
    }

    let mut total = 0.0;
    for c in &circles {
        total += 2.0 * c.r + gap;
    }
    let rounds = js_min(300.0, (3e6 / (count(n) * count(n))).floor());
    if n <= 64 {
        let ring = total / TAU;
        for (i, c) in circles.iter_mut().enumerate() {
            let angle = (count(i) * TAU) / count(n) - FRAC_PI_2;
            c.x = angle.cos() * ring;
            c.y = angle.sin() * ring;
        }
    } else {
        let columns = count(n).sqrt().ceil();
        let cell = 2.0 * widest + gap;
        for (i, c) in circles.iter_mut().enumerate() {
            c.x = (count(i) % columns) * cell;
            c.y = (count(i) / columns).floor() * cell;
        }
    }
    let mut dx = vec![0.0; n];
    let mut dy = vec![0.0; n];
    let mut pulls = vec![0_u32; n];
    let mut round = 0.0;
    while round < rounds {
        dx.fill(0.0);
        dy.fill(0.0);
        pulls.fill(0);
        for a in 0..n {
            for b in a + 1..n {
                let (Some(ca), Some(cb)) = (circles.get(a), circles.get(b)) else {
                    continue;
                };
                let overlap = wanted.get(&(a * n + b)).copied();
                let apart = ca.r + cb.r + gap;
                let mut ux = cb.x - ca.x;
                let mut uy = cb.y - ca.y;
                let mut length = hypot(ux, uy);
                if overlap.is_none() && length >= apart {
                    continue;
                }
                if length < 1e-9 {
                    ux = (count(a) + count(b)).cos();
                    uy = (count(a) + count(b)).sin();
                    length = 1.0;
                }
                let shift = (length - overlap.unwrap_or(apart)) / 2.0 / length;
                if let (Some(dxa), Some(dya)) = (dx.get_mut(a), dy.get_mut(a)) {
                    *dxa += ux * shift;
                    *dya += uy * shift;
                }
                if let (Some(dxb), Some(dyb)) = (dx.get_mut(b), dy.get_mut(b)) {
                    *dxb -= ux * shift;
                    *dyb -= uy * shift;
                }
                for at in [a, b] {
                    if let Some(pull) = pulls.get_mut(at) {
                        *pull += 1;
                    }
                }
            }
        }
        for (i, c) in circles.iter_mut().enumerate() {
            let pull = pulls.get(i).copied().unwrap_or(0);
            if pull == 0 {
                continue;
            }
            c.x += dx.get(i).copied().unwrap_or(0.0) / f64::from(pull);
            c.y += dy.get(i).copied().unwrap_or(0.0) / f64::from(pull);
        }
        round += 1.0;
    }
    circles
}

/// A point where two outlines cross, and the circles that cross there, by index.
struct Corner {
    a: usize,
    b: usize,
    x: f64,
    y: f64,
}

fn push_point(out: &mut String, x: f64, y: f64) {
    push_num(out, x);
    out.push(',');
    push_num(out, y);
}

/// The outline of a whole circle.
fn whole(c: &Circle) -> String {
    let mut d = String::from("M");
    push_point(&mut d, c.x - c.r, c.y);
    d.push('A');
    push_point(&mut d, c.r, c.r);
    d.push_str(" 0 1 1 ");
    push_point(&mut d, c.x + c.r, c.y);
    d.push('A');
    push_point(&mut d, c.r, c.r);
    d.push_str(" 0 1 1 ");
    push_point(&mut d, c.x - c.r, c.y);
    d.push('Z');
    d
}

/// The outline of the region all the given circles share, as an SVG path, or empty if they share
/// none.
#[expect(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "NaN passes the test, as in Pele"
)]
pub(crate) fn region_path(circles: &[Circle]) -> String {
    if let [only] = circles {
        return whole(only);
    }
    let within = |x: f64, y: f64, skip: usize, other: usize| {
        circles.iter().enumerate().all(|(i, c)| {
            i == skip || i == other || !(hypot(x - c.x, y - c.y) > c.r * (1.0 + 1e-9))
        })
    };
    // The corners of the region: where two outlines cross inside all the other circles.
    let mut corners: Vec<Corner> = Vec::new();
    for (i, a) in circles.iter().enumerate() {
        for (j, b) in circles.iter().enumerate().skip(i + 1) {
            let d = hypot(b.x - a.x, b.y - a.y);
            if d >= a.r + b.r || d <= (a.r - b.r).abs() || d == 0.0 {
                continue;
            }
            let along = (a.r * a.r - b.r * b.r + d * d) / (2.0 * d);
            let up = js_max(a.r * a.r - along * along, 0.0).sqrt();
            let mx = a.x + ((b.x - a.x) * along) / d;
            let my = a.y + ((b.y - a.y) * along) / d;
            for sign in [1.0, -1.0] {
                let x = mx + (sign * up * (b.y - a.y)) / d;
                let y = my - (sign * up * (b.x - a.x)) / d;
                if within(x, y, i, j) {
                    corners.push(Corner { a: i, b: j, x, y });
                }
            }
        }
    }
    if corners.len() < 2 {
        // No corners: either one circle lies inside all the others, or there is nothing shared.
        let Some(mut smallest) = circles.first() else {
            return String::new();
        };
        for c in circles {
            if c.r < smallest.r {
                smallest = c;
            }
        }
        for c in circles {
            if !std::ptr::eq(c, smallest)
                && hypot(c.x - smallest.x, c.y - smallest.y) + smallest.r > c.r * (1.0 + 1e-9)
            {
                return String::new();
            }
        }
        return whole(smallest);
    }
    let total = count(corners.len());
    let mut cx = 0.0;
    let mut cy = 0.0;
    for p in &corners {
        cx += p.x / total;
        cy += p.y / total;
    }
    corners.sort_by(|p, q| {
        let diff = (p.y - cy).atan2(p.x - cx) - (q.y - cy).atan2(q.x - cx);
        diff.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut d = String::new();
    for (i, from) in corners.iter().enumerate() {
        let Some(to) = corners.get((i + 1) % corners.len()) else {
            continue;
        };
        if i == 0 {
            d.push('M');
            push_point(&mut d, from.x, from.y);
        }
        // The side between two corners is an arc of a circle that passes through both. Of those,
        // the one whose arc stays inside the others.
        let mut arc: Option<&Circle> = None;
        let mut large = false;
        let mut narrowest = f64::INFINITY;
        for at in [from.a, from.b] {
            if at != to.a && at != to.b {
                continue;
            }
            let Some(c) = circles.get(at) else {
                continue;
            };
            let start = (from.y - c.y).atan2(from.x - c.x);
            let mut sweep = (to.y - c.y).atan2(to.x - c.x) - start;
            if sweep <= 0.0 {
                sweep += TAU;
            }
            let mx = c.x + c.r * (start + sweep / 2.0).cos();
            let my = c.y + c.r * (start + sweep / 2.0).sin();
            let width = hypot(
                mx - f64::midpoint(from.x, to.x),
                my - f64::midpoint(from.y, to.y),
            );
            if width < narrowest {
                narrowest = width;
                arc = Some(c);
                large = sweep > PI;
            }
        }
        match arc {
            None => {
                d.push('L');
                push_point(&mut d, to.x, to.y);
            }
            Some(arc) => {
                d.push('A');
                push_point(&mut d, arc.r, arc.r);
                d.push_str(if large { " 0 1 1 " } else { " 0 0 1 " });
                push_point(&mut d, to.x, to.y);
            }
        }
    }
    d.push('Z');
    d
}
