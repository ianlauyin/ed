use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::ops::Range;
use std::sync::Arc;

use gpui::HighlightStyle;

#[derive(Debug, PartialEq, Eq, Clone, Default)]
pub struct SyntaxTheme {
    highlights: Vec<HighlightStyle>,
    capture_name_map: BTreeMap<String, usize>,
}

impl SyntaxTheme {
    pub fn new(highlights: impl IntoIterator<Item = (String, HighlightStyle)>) -> Self {
        let (capture_names, highlights) = highlights.into_iter().unzip();

        Self { capture_name_map: Self::create_capture_name_map(capture_names), highlights }
    }

    fn create_capture_name_map(highlights: Vec<String>) -> BTreeMap<String, usize> {
        highlights.into_iter().enumerate().map(|(i, key)| (key, i)).collect()
    }

    pub fn get(&self, highlight_index: impl Into<usize>) -> Option<&HighlightStyle> {
        self.highlights.get(highlight_index.into())
    }

    pub fn resolve_runs<'a, Id: Into<usize> + Copy + 'a>(
        &'a self,
        runs: impl IntoIterator<Item = &'a (Range<usize>, Id)> + 'a,
    ) -> impl Iterator<Item = (Range<usize>, HighlightStyle)> + 'a {
        runs.into_iter().filter_map(|(range, highlight_id)| Some((range.clone(), *self.get(*highlight_id)?)))
    }

    pub fn style_for_name(&self, name: &str) -> Option<HighlightStyle> {
        self.capture_name_map.get(name).map(|highlight_idx| self.highlights[*highlight_idx])
    }

    pub fn get_capture_name(&self, idx: impl Into<usize>) -> Option<&str> {
        let idx = idx.into();
        self.capture_name_map.iter().find(|(_, value)| **value == idx).map(|(key, _)| key.as_ref())
    }

    pub fn highlight_id(&self, capture_name: &str) -> Option<u32> {
        self.capture_name_map
            .range::<str, _>((
                capture_name
                    .split(".")
                    .next()
                    .map_or(std::ops::Bound::Included(capture_name), std::ops::Bound::Included),
                std::ops::Bound::Included(capture_name),
            ))
            .rfind(|(prefix, _)| {
                capture_name
                    .strip_prefix(*prefix)
                    .is_some_and(|remainder| remainder.is_empty() || remainder.starts_with('.'))
            })
            .map(|(_, index)| *index as u32)
    }

    /// Returns a new [`Arc<SyntaxTheme>`] with the given syntax styles merged in.
    pub fn merge(base: Arc<Self>, user_syntax_styles: Vec<(String, HighlightStyle)>) -> Arc<Self> {
        if user_syntax_styles.is_empty() {
            return base;
        }

        let mut base = Arc::try_unwrap(base).unwrap_or_else(|base| (*base).clone());

        for (name, highlight) in user_syntax_styles {
            match base.capture_name_map.entry(name) {
                Entry::Occupied(entry) => {
                    if let Some(existing_highlight) = base.highlights.get_mut(*entry.get()) {
                        existing_highlight.color = highlight.color.or(existing_highlight.color);
                        existing_highlight.font_weight = highlight.font_weight.or(existing_highlight.font_weight);
                        existing_highlight.font_style = highlight.font_style.or(existing_highlight.font_style);
                        existing_highlight.background_color =
                            highlight.background_color.or(existing_highlight.background_color);
                        existing_highlight.underline = highlight.underline.or(existing_highlight.underline);
                        existing_highlight.strikethrough = highlight.strikethrough.or(existing_highlight.strikethrough);
                        existing_highlight.fade_out = highlight.fade_out.or(existing_highlight.fade_out);
                    }
                }
                Entry::Vacant(vacant) => {
                    vacant.insert(base.highlights.len());
                    base.highlights.push(highlight);
                }
            }
        }

        Arc::new(base)
    }
}

#[cfg(feature = "bundled-themes")]
mod bundled_themes {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use gpui::FontStyle;
    use gpui::FontWeight;
    use gpui::HighlightStyle;
    use gpui::Hsla;
    use gpui::Rgba;
    use gpui::rgb;
    use serde::Deserialize;

    use super::SyntaxTheme;

    #[derive(Deserialize)]
    struct ThemeFile {
        themes: Vec<ThemeEntry>,
    }

    #[derive(Deserialize)]
    struct ThemeEntry {
        name: String,
        style: ThemeStyle,
    }

    #[derive(Deserialize)]
    struct ThemeStyle {
        syntax: BTreeMap<String, SyntaxStyleEntry>,
    }

    #[derive(Deserialize)]
    struct SyntaxStyleEntry {
        color: Option<String>,
        font_weight: Option<f32>,
        font_style: Option<String>,
    }

    impl SyntaxStyleEntry {
        fn to_highlight_style(&self) -> HighlightStyle {
            HighlightStyle {
                color: self.color.as_deref().map(hex_to_hsla),
                font_weight: self.font_weight.map(FontWeight),
                font_style: self.font_style.as_deref().and_then(|s| match s {
                    "italic" => Some(FontStyle::Italic),
                    "normal" => Some(FontStyle::Normal),
                    "oblique" => Some(FontStyle::Oblique),
                    _ => None,
                }),
                ..Default::default()
            }
        }
    }

    fn hex_to_hsla(hex: &str) -> Hsla {
        let hex = hex.trim_start_matches('#');
        let rgba: Rgba = match hex.len() {
            6 => rgb(u32::from_str_radix(hex, 16).unwrap_or(0)),
            8 => {
                let value = u32::from_str_radix(hex, 16).unwrap_or(0);
                Rgba {
                    r: ((value >> 24) & 0xff) as f32 / 255.0,
                    g: ((value >> 16) & 0xff) as f32 / 255.0,
                    b: ((value >> 8) & 0xff) as f32 / 255.0,
                    a: (value & 0xff) as f32 / 255.0,
                }
            }
            _ => rgb(0),
        };
        rgba.into()
    }

    fn load_theme(json: &str, theme_name: &str) -> Arc<SyntaxTheme> {
        let theme_file: ThemeFile = serde_json::from_str(json).expect("failed to parse theme JSON");
        let theme_entry = theme_file
            .themes
            .iter()
            .find(|entry| entry.name == theme_name)
            .unwrap_or_else(|| panic!("theme {theme_name:?} not found in theme JSON"));

        let highlights =
            theme_entry.style.syntax.iter().map(|(name, entry)| (name.clone(), entry.to_highlight_style()));

        Arc::new(SyntaxTheme::new(highlights))
    }

    impl SyntaxTheme {
        /// Load the "One Dark" syntax theme from the bundled theme JSON.
        pub fn one_dark() -> Arc<Self> {
            load_theme(include_str!("../../../assets/themes/one/one.json"), "One Dark")
        }
    }

    #[cfg(test)]
    mod tests {
        use std::sync::Arc;

        use gpui::HighlightStyle;

        use super::SyntaxTheme;
        use super::load_theme;

        fn style(theme: &SyntaxTheme, capture_name: &str) -> HighlightStyle {
            let highlight_id =
                theme.highlight_id(capture_name).unwrap_or_else(|| panic!("no style for {capture_name:?}"));
            *theme.get(highlight_id as usize).unwrap()
        }

        #[test]
        fn one_themes_style_each_code_kind_distinctly() {
            let json = include_str!("../../../assets/themes/one/one.json");
            let code = [
                "primary",
                "variable",
                "variable.parameter",
                "property",
                "function",
                "function.method",
                "function.definition",
                "function.special",
                "keyword",
                "keyword.control",
                "type",
                "operator",
                "string",
                "number",
                "boolean",
                "constant",
            ];
            let css = ["selector", "property", "variable", "function", "constant.builtin", "number", "type", "keyword"];
            let data = ["property", "type", "string", "number", "boolean", "constant"];
            for theme_name in ["One Dark", "One Light"] {
                let theme: Arc<SyntaxTheme> = load_theme(json, theme_name);
                for kinds in [&code[..], &css[..], &data[..]] {
                    for (i, a) in kinds.iter().enumerate() {
                        for b in &kinds[i + 1..] {
                            assert_ne!(
                                style(&theme, a),
                                style(&theme, b),
                                "{theme_name}: {a:?} and {b:?} share a style"
                            );
                        }
                    }
                }
            }
        }

        #[test]
        fn one_themes_keep_control_keywords_upright() {
            let json = include_str!("../../../assets/themes/one/one.json");
            for theme_name in ["One Dark", "One Light"] {
                let theme: Arc<SyntaxTheme> = load_theme(json, theme_name);
                assert_eq!(style(&theme, "keyword.control").font_style, Some(gpui::FontStyle::Normal), "{theme_name}");
            }
        }
    }
}
