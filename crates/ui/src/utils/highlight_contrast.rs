use std::ops::Range;

use gpui::{Hsla, TextRun};
use smallvec::SmallVec;

use super::ensure_minimum_contrast;

/// Merges overlapping highlight ranges by splitting at all range boundaries and blending
/// colors over `base_background` where ranges overlap; later ranges are blended on top.
/// The result contains non-overlapping ranges ordered from left to right.
pub fn merge_highlight_backgrounds<T: Ord + Copy>(
    ranges: impl IntoIterator<Item = (Range<T>, Hsla)>,
    base_background: Hsla,
) -> Vec<(Range<T>, Hsla)> {
    struct Boundary<T> {
        pos: T,
        is_start: bool,
        index: usize,
        color: Hsla,
    }

    let mut boundaries: SmallVec<[Boundary<T>; 16]> = SmallVec::new();
    for (index, (range, color)) in ranges.into_iter().enumerate() {
        if range.start < range.end {
            boundaries.push(Boundary {
                pos: range.start,
                is_start: true,
                index,
                color,
            });
            boundaries.push(Boundary {
                pos: range.end,
                is_start: false,
                index,
                color,
            });
        }
    }

    if boundaries.is_empty() {
        return Vec::new();
    }

    boundaries.sort_unstable_by(|a, b| a.pos.cmp(&b.pos).then_with(|| a.is_start.cmp(&b.is_start)));

    let mut processed_ranges: Vec<(Range<T>, Hsla)> = Vec::new();
    let mut active_ranges: SmallVec<[(usize, Hsla); 8]> = SmallVec::new();

    let mut i = 0;
    let mut start_pos = boundaries[0].pos;

    let boundaries_len = boundaries.len();
    while i < boundaries_len {
        let current_boundary_pos = boundaries[i].pos;
        if start_pos < current_boundary_pos && !active_ranges.is_empty() {
            let mut color = base_background;
            for &(_, c) in &active_ranges {
                color = Hsla::blend(color, c);
            }
            match processed_ranges.last_mut() {
                Some((last_range, last_color))
                    if *last_color == color && last_range.end == start_pos =>
                {
                    last_range.end = current_boundary_pos;
                }
                _ => processed_ranges.push((start_pos..current_boundary_pos, color)),
            }
        }
        while i < boundaries_len && boundaries[i].pos == current_boundary_pos {
            let boundary = &boundaries[i];
            let pos = active_ranges.binary_search_by_key(&boundary.index, |(ix, _)| *ix);
            if boundary.is_start {
                active_ranges.insert(pos.unwrap_or_else(|p| p), (boundary.index, boundary.color));
            } else if let Ok(pos) = pos {
                active_ranges.remove(pos);
            }
            i += 1;
        }
        start_pos = current_boundary_pos;
    }

    processed_ranges
}

/// Splits `text_runs` at highlight boundaries and adjusts the text color inside each
/// highlight to keep at least `minimum_apca_contrast` against the highlight background.
///
/// `highlights` must be non-overlapping and sorted (see [`merge_highlight_backgrounds`]),
/// with byte offsets relative to the text that `text_runs` start at `start_offset` of.
pub fn ensure_highlight_contrast(
    text_runs: &[TextRun],
    highlights: &[(Range<usize>, Hsla)],
    minimum_apca_contrast: f32,
    start_offset: usize,
) -> Vec<TextRun> {
    let mut output_runs: Vec<TextRun> = Vec::with_capacity(text_runs.len());
    let mut run_start = start_offset;
    let mut highlight_ix = 0;

    for text_run in text_runs {
        let run_end = run_start + text_run.len;
        while highlight_ix < highlights.len() && highlights[highlight_ix].0.end <= run_start {
            highlight_ix += 1;
        }
        let mut cursor = run_start;
        let mut local_highlight_ix = highlight_ix;
        while local_highlight_ix < highlights.len() {
            let (range, highlight_color) = &highlights[local_highlight_ix];
            if range.start >= run_end {
                break;
            }
            if range.start > cursor {
                output_runs.push(TextRun {
                    len: range.start - cursor,
                    ..text_run.clone()
                });
                cursor = range.start;
            }
            let slice_end = range.end.min(run_end);
            if slice_end > cursor {
                output_runs.push(TextRun {
                    len: slice_end - cursor,
                    color: ensure_minimum_contrast(
                        text_run.color,
                        *highlight_color,
                        minimum_apca_contrast,
                    ),
                    ..text_run.clone()
                });
                cursor = slice_end;
            }
            if range.end >= run_end {
                break;
            }
            local_highlight_ix += 1;
        }
        if cursor < run_end {
            output_runs.push(TextRun {
                len: run_end - cursor,
                ..text_run.clone()
            });
        }
        run_start = run_end;
        highlight_ix = local_highlight_ix;
    }
    output_runs
}

#[cfg(test)]
mod tests {
    use gpui::{font, hsla};

    use super::*;

    fn run(len: usize, color: Hsla) -> TextRun {
        TextRun {
            len,
            font: font("Helvetica"),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        }
    }

    #[test]
    fn test_merge_highlight_backgrounds() {
        let base = hsla(0., 0., 0., 1.);
        let red = hsla(0., 1., 0.5, 1.);
        let blue = hsla(0.66, 1., 0.5, 1.);
        let merged = merge_highlight_backgrounds([(2..6, red), (4..8, blue), (8..9, blue)], base);
        assert_eq!(merged, vec![(2..4, red), (4..9, blue)]);
    }

    #[test]
    fn test_ensure_highlight_contrast_splits_runs() {
        let text = hsla(0., 0., 0.5, 1.);
        let bg = hsla(0., 0., 0.55, 1.);
        let runs = ensure_highlight_contrast(&[run(4, text), run(4, text)], &[(3..5, bg)], 45., 0);
        assert_eq!(
            runs.iter().map(|run| run.len).collect::<Vec<_>>(),
            vec![3, 1, 1, 3]
        );
        assert_eq!(runs[0].color, text);
        assert_ne!(runs[1].color, text);
        assert_ne!(runs[2].color, text);
        assert_eq!(runs[3].color, text);
    }
}
