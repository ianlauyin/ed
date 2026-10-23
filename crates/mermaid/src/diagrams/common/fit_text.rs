//! Fits one line of text into a width. Port of Pele's `src/diagrams/common/fit-text.ts`.

use crate::text::label::trim_end;
use crate::text::measure;

/// Text fitted into a width.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Fitted {
    /// Font size.
    pub(crate) size: f64,
    /// The text, cut and ended with an ellipsis when it did not fit whole.
    pub(crate) text: String,
    /// The measured width.
    pub(crate) width: f64,
}

/// Fits `text` into `max`: first by shrinking the font from `size` down to `min`, then by
/// cutting the text and adding an ellipsis. Returns `None` when not even one character fits.
pub(crate) fn fit_text(text: &str, max: f64, size: f64, min: f64, style: u8) -> Option<Fitted> {
    if text.is_empty() || max <= 0.0 {
        return None;
    }
    let mut width = measure::width(text, size, style);
    if width <= max {
        return Some(Fitted { size, text: text.to_owned(), width });
    }

    // Width grows with font size, so one division lands on the right size or just above it.
    let mut fitted = min.max((size * max / width).floor());
    while fitted >= min {
        width = measure::width(text, fitted, style);
        if width <= max {
            return Some(Fitted { size: fitted, text: text.to_owned(), width });
        }
        fitted -= 1.0;
    }

    // Byte offsets after each character, so a cut of `k` characters is `text[..ends[k - 1]]`.
    let ends: Vec<usize> = text.char_indices().map(|(at, c)| at + c.len_utf8()).collect();
    let cut = |count: usize| {
        let end = count.checked_sub(1).and_then(|k| ends.get(k)).copied().unwrap_or(0);
        let mut cut = String::from(trim_end(text.get(..end).unwrap_or("")));
        cut.push('…');
        cut
    };

    let mut low = 0;
    let mut high = ends.len().saturating_sub(1);
    while low < high {
        let mid = (low + high + 1) >> 1;
        if measure::width(&cut(mid), min, style) <= max {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    if low == 0 {
        return None;
    }

    let text = cut(low);
    width = measure::width(&text, min, style);
    (width <= max).then_some(Fitted { size: min, text, width })
}
