use std::sync::Arc;

use gpui::Hsla;

/// A collection of colors that are used to color indent aware lines in the editor.
#[derive(Clone, Debug, PartialEq)]
pub struct AccentColors(pub Arc<[Hsla]>);

impl AccentColors {
    /// Returns the color for the given index.
    pub fn color_for_index(&self, index: u32) -> Hsla {
        self.0[index as usize % self.0.len()]
    }
}
