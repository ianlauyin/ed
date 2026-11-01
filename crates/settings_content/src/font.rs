use std::fmt::Display;
use std::sync::Arc;

use collections::IndexMap;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;

use crate::serialize_f32_with_two_decimal_places;

/// OpenType font features as a map of feature tag to value.
/// This is a content type that mirrors `gpui::FontFeatures` but without the Arc wrapper.
/// Values can be specified as booleans (true=1, false=0) or integers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct FontFeaturesContent(pub IndexMap<String, u32>);

impl FontFeaturesContent {
    pub fn new() -> Self {
        Self(IndexMap::default())
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum FeatureValue {
    Bool(bool),
    Number(serde_json::Number),
}

fn is_valid_feature_tag(tag: &str) -> bool {
    tag.len() == 4 && tag.chars().all(|c| c.is_ascii_alphanumeric())
}

impl<'de> Deserialize<'de> for FontFeaturesContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use std::fmt;

        use serde::de::MapAccess;
        use serde::de::Visitor;

        struct FontFeaturesVisitor;

        impl<'de> Visitor<'de> for FontFeaturesVisitor {
            type Value = FontFeaturesContent;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a map of font features")
            }

            fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut feature_map = IndexMap::default();

                while let Some((key, value)) = access.next_entry::<String, Option<FeatureValue>>()? {
                    if !is_valid_feature_tag(&key) {
                        log::error!("Incorrect font feature tag: {}", key);
                        continue;
                    }
                    if let Some(value) = value {
                        match value {
                            FeatureValue::Bool(enable) => {
                                feature_map.insert(key, if enable { 1 } else { 0 });
                            }
                            FeatureValue::Number(value) => {
                                if value.is_u64() {
                                    feature_map.insert(key, value.as_u64().unwrap() as u32);
                                } else {
                                    log::error!("Incorrect font feature value {} for feature tag {}", value, key);
                                    continue;
                                }
                            }
                        }
                    }
                }

                Ok(FontFeaturesContent(feature_map))
            }
        }

        deserializer.deserialize_map(FontFeaturesVisitor)
    }
}

/// A font size value in pixels, wrapping around `f32` for custom settings UI rendering.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, PartialOrd, derive_more::FromStr)]
#[serde(transparent)]
pub struct FontSize(#[serde(serialize_with = "serialize_f32_with_two_decimal_places")] pub f32);

impl Display for FontSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.2}", self.0)
    }
}

impl From<f32> for FontSize {
    fn from(value: f32) -> Self {
        Self(value)
    }
}

/// Font family name.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct FontFamilyName(pub Arc<str>);

impl AsRef<str> for FontFamilyName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for FontFamilyName {
    fn from(value: String) -> Self {
        Self(Arc::from(value))
    }
}

impl From<FontFamilyName> for String {
    fn from(value: FontFamilyName) -> Self {
        value.0.to_string()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize, derive_more::FromStr)]
#[serde(transparent)]
pub struct FontWeightContent(pub f32);

impl Display for FontWeightContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<f32> for FontWeightContent {
    fn from(weight: f32) -> Self {
        FontWeightContent(weight)
    }
}

impl Default for FontWeightContent {
    fn default() -> Self {
        Self::NORMAL
    }
}

impl FontWeightContent {
    pub const THIN: FontWeightContent = FontWeightContent(100.0);
    pub const EXTRA_LIGHT: FontWeightContent = FontWeightContent(200.0);
    pub const LIGHT: FontWeightContent = FontWeightContent(300.0);
    pub const NORMAL: FontWeightContent = FontWeightContent(400.0);
    pub const MEDIUM: FontWeightContent = FontWeightContent(500.0);
    pub const SEMIBOLD: FontWeightContent = FontWeightContent(600.0);
    pub const BOLD: FontWeightContent = FontWeightContent(700.0);
    pub const EXTRA_BOLD: FontWeightContent = FontWeightContent(800.0);
    pub const BLACK: FontWeightContent = FontWeightContent(900.0);
}
