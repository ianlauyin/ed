use gpui::App;
use gpui::Pixels;
use gpui::Rems;
use gpui::px;
use gpui::rems;

const BASE_REM_SIZE_IN_PX: f32 = 16.0;

/// UI spacing scale. The number following "Base" is the pixel size at the
/// default rem size (16px); values scale with the user's UI font size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DynamicSpacing {
    Base00,
    Base01,
    Base02,
    Base03,
    Base04,
    Base06,
    Base08,
    Base12,
    Base16,
    Base20,
    Base24,
    Base32,
    Base40,
    Base48,
}

impl DynamicSpacing {
    /// Returns the spacing value in pixels at the default rem size.
    pub const fn base_px(self) -> f32 {
        match self {
            DynamicSpacing::Base00 => 0.0,
            DynamicSpacing::Base01 => 1.0,
            DynamicSpacing::Base02 => 2.0,
            DynamicSpacing::Base03 => 3.0,
            DynamicSpacing::Base04 => 4.0,
            DynamicSpacing::Base06 => 6.0,
            DynamicSpacing::Base08 => 8.0,
            DynamicSpacing::Base12 => 12.0,
            DynamicSpacing::Base16 => 16.0,
            DynamicSpacing::Base20 => 20.0,
            DynamicSpacing::Base24 => 24.0,
            DynamicSpacing::Base32 => 32.0,
            DynamicSpacing::Base40 => 40.0,
            DynamicSpacing::Base48 => 48.0,
        }
    }

    /// Returns the spacing value in rems.
    pub fn rems(self) -> Rems {
        rems(self.base_px() / BASE_REM_SIZE_IN_PX)
    }

    /// Returns the spacing value in pixels, scaled by the UI font size.
    pub fn px(self, cx: &App) -> Pixels {
        let ui_font_size: f32 = theme::theme_settings(cx).ui_font_size(cx).into();
        px(ui_font_size * self.base_px() / BASE_REM_SIZE_IN_PX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_px_matches_variant_name() {
        assert_eq!(DynamicSpacing::Base00.base_px(), 0.0);
        assert_eq!(DynamicSpacing::Base06.base_px(), 6.0);
        assert_eq!(DynamicSpacing::Base48.base_px(), 48.0);
    }

    #[test]
    fn test_rems() {
        assert_eq!(DynamicSpacing::Base16.rems(), rems(1.0));
        assert_eq!(DynamicSpacing::Base08.rems(), rems(0.5));
        assert_eq!(DynamicSpacing::Base00.rems(), rems(0.0));
    }
}
