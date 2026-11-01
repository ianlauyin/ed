mod buffer_line_height;
mod color_space;
mod default_colors;
mod fallback_themes;
mod scale;
mod schema;
mod styles;

use std::sync::Arc;

use gpui::App;
use gpui::Global;
use gpui::Hsla;
use gpui::Pixels;
use gpui::SharedString;
use gpui::Styled;
use gpui::Tiling;
use gpui::WindowBackgroundAppearance;
use gpui::px;
use serde::Deserialize;

pub use crate::buffer_line_height::*;
pub use crate::color_space::*;
pub(crate) use crate::default_colors::amber;
pub(crate) use crate::default_colors::blue;
pub(crate) use crate::default_colors::cyan;
pub(crate) use crate::default_colors::gold;
pub(crate) use crate::default_colors::grass;
pub(crate) use crate::default_colors::indigo;
pub(crate) use crate::default_colors::iris;
pub(crate) use crate::default_colors::jade;
pub(crate) use crate::default_colors::lime;
pub(crate) use crate::default_colors::neutral;
pub(crate) use crate::default_colors::orange;
pub(crate) use crate::default_colors::pink;
pub(crate) use crate::default_colors::purple;
pub(crate) use crate::default_colors::red;
pub(crate) use crate::default_colors::tomato;
pub(crate) use crate::default_colors::yellow;
pub use crate::fallback_themes::apply_status_color_defaults;
pub use crate::fallback_themes::apply_theme_color_defaults;
pub use crate::scale::*;
pub use crate::schema::*;
pub use crate::styles::*;

/// Defines window border radius for platforms that use client side decorations.
pub const CLIENT_SIDE_DECORATION_ROUNDING: Pixels = px(10.0);
/// Defines window shadow size for platforms that use client side decorations.
pub const CLIENT_SIDE_DECORATION_SHADOW: Pixels = px(10.0);
/// Defines window border width for platforms that use client side decorations.
pub const CLIENT_SIDE_DECORATION_BORDER: Pixels = px(1.0);

/// Styling helpers for elements that follow client-side window decorations.
pub trait ClientDecorationsExt: Styled {
    /// Rounds each corner whose two adjacent edges are both untiled.
    fn rounded_client_corners(mut self, tiling: Tiling) -> Self {
        if !tiling.top && !tiling.left {
            self = self.rounded_tl(CLIENT_SIDE_DECORATION_ROUNDING);
        }
        if !tiling.top && !tiling.right {
            self = self.rounded_tr(CLIENT_SIDE_DECORATION_ROUNDING);
        }
        if !tiling.bottom && !tiling.left {
            self = self.rounded_bl(CLIENT_SIDE_DECORATION_ROUNDING);
        }
        if !tiling.bottom && !tiling.right {
            self = self.rounded_br(CLIENT_SIDE_DECORATION_ROUNDING);
        }
        self
    }
}

impl<T: Styled> ClientDecorationsExt for T {}

/// The appearance of the theme.
#[derive(Debug, PartialEq, Clone, Copy, Deserialize)]
pub enum Appearance {
    /// A light appearance.
    Light,
    /// A dark appearance.
    Dark,
}

impl Appearance {
    /// Returns whether the appearance is light.
    pub const fn is_light(&self) -> bool {
        match self {
            Self::Light => true,
            Self::Dark => false,
        }
    }
}

/// Implementing this trait allows accessing the active theme.
pub trait ActiveTheme {
    /// Returns the active theme.
    fn theme(&self) -> &Arc<Theme>;
}

impl ActiveTheme for App {
    fn theme(&self) -> &Arc<Theme> {
        GlobalTheme::theme(self)
    }
}

/// A theme is the primary mechanism for defining the appearance of the UI.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// The unique identifier for the theme.
    pub id: String,
    /// The name of the theme.
    pub name: SharedString,
    /// The appearance of the theme (light or dark).
    pub appearance: Appearance,
    /// The colors and other styles for the theme.
    pub styles: ThemeStyles,
}

impl Theme {
    /// Returns the [`SystemColors`] for the theme.
    #[inline(always)]
    pub const fn system(&self) -> &SystemColors {
        &self.styles.system
    }

    /// Returns the [`AccentColors`] for the theme.
    #[inline(always)]
    pub const fn accents(&self) -> &AccentColors {
        &self.styles.accents
    }

    /// Returns the local [`PlayerColor`] (cursor and selection) for the theme.
    #[inline(always)]
    pub const fn player(&self) -> PlayerColor {
        self.styles.player
    }

    /// Returns the [`ThemeColors`] for the theme.
    #[inline(always)]
    pub const fn colors(&self) -> &ThemeColors {
        &self.styles.colors
    }

    /// Returns the [`SyntaxTheme`] for the theme.
    #[inline(always)]
    pub const fn syntax(&self) -> &Arc<SyntaxTheme> {
        &self.styles.syntax
    }

    /// Returns the [`StatusColors`] for the theme.
    #[inline(always)]
    pub const fn status(&self) -> &StatusColors {
        &self.styles.status
    }

    /// Returns the [`Appearance`] for the theme.
    #[inline(always)]
    pub const fn appearance(&self) -> Appearance {
        self.appearance
    }

    /// Returns the [`WindowBackgroundAppearance`] for the theme.
    #[inline(always)]
    pub const fn window_background_appearance(&self) -> WindowBackgroundAppearance {
        self.styles.window_background_appearance
    }

    /// Darkens the color by reducing its lightness.
    /// The resulting lightness is clamped to ensure it doesn't go below 0.0.
    ///
    /// The first value darkens light appearance mode, the second darkens appearance dark mode.
    ///
    /// Note: This is a tentative solution and may be replaced with a more robust color system.
    pub fn darken(&self, color: Hsla, light_amount: f32, dark_amount: f32) -> Hsla {
        let amount = match self.appearance {
            Appearance::Light => light_amount,
            Appearance::Dark => dark_amount,
        };
        let mut hsla = color;
        hsla.l = (hsla.l - amount).max(0.0);
        hsla
    }
}

/// The active theme.
pub struct GlobalTheme {
    theme: Arc<Theme>,
}
impl Global for GlobalTheme {}

impl GlobalTheme {
    /// Creates a new [`GlobalTheme`] with the given theme.
    pub const fn new(theme: Arc<Theme>) -> Self {
        Self { theme }
    }

    /// Returns the active theme.
    pub fn theme(cx: &App) -> &Arc<Theme> {
        &cx.global::<Self>().theme
    }
}
