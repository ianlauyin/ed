use gpui::App;
use gpui::WindowBackgroundAppearance;

use crate::prelude::*;

/// Returns the [WindowBackgroundAppearance].
fn window_appearance(cx: &App) -> WindowBackgroundAppearance {
    cx.theme().styles.window_background_appearance
}

/// Returns if the window and it's surfaces are expected
/// to be transparent.
///
/// Helps determine if you need to take extra steps to prevent
/// transparent backgrounds.
pub fn theme_is_transparent(cx: &mut App) -> bool {
    matches!(
        window_appearance(cx),
        WindowBackgroundAppearance::Transparent
            | WindowBackgroundAppearance::Blurred
            | WindowBackgroundAppearance::MicaBackdrop
            | WindowBackgroundAppearance::MicaAltBackdrop
    )
}
