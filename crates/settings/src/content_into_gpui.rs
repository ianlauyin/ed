use std::sync::Arc;

use gpui::FontFeatures;
use gpui::FontStyle;
use gpui::FontWeight;
use gpui::Modifiers;
use gpui::Pixels;
use gpui::SharedString;
use gpui::WindowBackgroundAppearance;
use gpui::px;
use settings_content::FontFamilyName;
use settings_content::FontFeaturesContent;
use settings_content::FontSize;
use settings_content::FontStyleContent;
use settings_content::FontWeightContent;
use settings_content::ModifiersContent;
use settings_content::PixelSetting;
use settings_content::WindowBackgroundContent;

/// A trait for converting settings content types into their GPUI equivalents.
pub trait IntoGpui {
    type Output;
    fn into_gpui(self) -> Self::Output;
}

impl IntoGpui for FontStyleContent {
    type Output = FontStyle;

    fn into_gpui(self) -> Self::Output {
        match self {
            FontStyleContent::Normal => FontStyle::Normal,
            FontStyleContent::Italic => FontStyle::Italic,
            FontStyleContent::Oblique => FontStyle::Oblique,
        }
    }
}

impl IntoGpui for FontWeightContent {
    type Output = FontWeight;

    fn into_gpui(self) -> Self::Output {
        FontWeight(self.0.clamp(100., 950.))
    }
}

impl IntoGpui for FontFeaturesContent {
    type Output = FontFeatures;

    fn into_gpui(self) -> Self::Output {
        FontFeatures(Arc::new(self.0.into_iter().collect()))
    }
}

impl IntoGpui for WindowBackgroundContent {
    type Output = WindowBackgroundAppearance;

    fn into_gpui(self) -> Self::Output {
        match self {
            WindowBackgroundContent::Opaque => WindowBackgroundAppearance::Opaque,
            WindowBackgroundContent::Transparent => WindowBackgroundAppearance::Transparent,
            WindowBackgroundContent::Blurred => WindowBackgroundAppearance::Blurred,
            WindowBackgroundContent::MicaBackdrop => WindowBackgroundAppearance::MicaBackdrop,
            WindowBackgroundContent::MicaAltBackdrop => WindowBackgroundAppearance::MicaAltBackdrop,
        }
    }
}

impl IntoGpui for ModifiersContent {
    type Output = Modifiers;

    fn into_gpui(self) -> Self::Output {
        Modifiers {
            control: self.control,
            alt: self.alt,
            shift: self.shift,
            platform: self.platform,
            function: self.function,
        }
    }
}

impl IntoGpui for FontSize {
    type Output = Pixels;

    fn into_gpui(self) -> Self::Output {
        px(self.0)
    }
}

impl IntoGpui for PixelSetting {
    type Output = Pixels;

    fn into_gpui(self) -> Self::Output {
        px(self.0)
    }
}

impl IntoGpui for FontFamilyName {
    type Output = SharedString;

    fn into_gpui(self) -> Self::Output {
        SharedString::from(self.0)
    }
}
