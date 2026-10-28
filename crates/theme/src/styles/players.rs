use gpui::Hsla;
use serde::Deserialize;

use crate::blue;

/// The colors of the local cursor and selections.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
pub struct PlayerColor {
    pub cursor: Hsla,
    pub selection: Hsla,
}

impl Default for PlayerColor {
    /// Don't use this!
    /// We have to have a default to be `[refineable::Refinable]`.
    /// TODO "Find a way to not need this for Refinable"
    fn default() -> Self {
        Self::dark()
    }
}

impl PlayerColor {
    pub fn dark() -> Self {
        Self { cursor: blue().dark().step_9(), selection: blue().dark().step_3() }
    }

    pub fn light() -> Self {
        Self { cursor: blue().light().step_9(), selection: blue().light().step_3() }
    }

    /// The colors used when the editor is read-only.
    pub const fn read_only(&self) -> Self {
        Self { cursor: self.cursor.grayscale(), selection: self.selection.grayscale() }
    }
}
