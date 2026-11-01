use gpui::Hsla;

/// The colors of the local cursor and selections.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerColor {
    pub cursor: Hsla,
    pub selection: Hsla,
}

impl PlayerColor {
    /// The colors used when the editor is read-only.
    pub const fn read_only(&self) -> Self {
        Self { cursor: self.cursor.grayscale(), selection: self.selection.grayscale() }
    }
}
