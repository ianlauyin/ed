use crate::PlayerColor;
use crate::StatusColorsRefinement;
use crate::ThemeColorsRefinement;

// If a theme customizes a foreground version of a status color, but does not
// customize the background color, then use a partly-transparent version of the
// foreground color for the background color.
/// Applies default status color backgrounds from their foreground counterparts.
pub fn apply_status_color_defaults(status: &mut StatusColorsRefinement) {
    for (fg_color, bg_color) in [
        (&status.deleted, &mut status.deleted_background),
        (&status.created, &mut status.created_background),
        (&status.modified, &mut status.modified_background),
        (&status.conflict, &mut status.conflict_background),
        (&status.error, &mut status.error_background),
        (&status.hidden, &mut status.hidden_background),
    ] {
        if bg_color.is_none()
            && let Some(fg_color) = fg_color
        {
            *bg_color = Some(fg_color.opacity(0.25));
        }
    }
}

/// Applies default theme color values derived from the player color.
pub fn apply_theme_color_defaults(theme_colors: &mut ThemeColorsRefinement, player: &PlayerColor) {
    if theme_colors.element_selection_background.is_none() {
        let mut selection = player.selection;
        if selection.a == 1.0 {
            selection.a = 0.25;
        }
        theme_colors.element_selection_background = Some(selection);
    }
}
