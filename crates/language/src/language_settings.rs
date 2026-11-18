//! Per-language settings and the editor-wide language constants.
//!
//! Each language's settings are hardcoded next to its language support (in the
//! `languages` crate) and registered with [`AllLanguageSettings::register`]. There is
//! no merging: a language's [`LanguageSettings`] is used as is.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::LazyLock;

use globset::Glob;
use globset::GlobSet;
use globset::GlobSetBuilder;
use gpui::App;
use gpui::Global;
use gpui::Rgba;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use serde::Deserialize;
use text::ToOffset;

use crate::Buffer;
use crate::BufferSnapshot;
use crate::LanguageName;

pub const INDENT_GUIDES: IndentGuideSettings =
    IndentGuideSettings { enabled: true, line_width: 1, active_line_width: 1 };
pub const WHITESPACE_SPACE: &str = "•";
pub const WHITESPACE_TAB: &str = "→";
pub const INLAY_HINTS: InlayHintSettings = InlayHintSettings {
    show_type_hints: true,
    show_parameter_hints: true,
    show_other_hints: true,
    show_background: false,
    edit_debounce_ms: 700,
    scroll_debounce_ms: 50,
};
pub const COMPLETIONS: CompletionSettings =
    CompletionSettings { words_min_length: 3, lsp: true, lsp_fetch_timeout_ms: 0 };

/// Returns the settings for all languages.
pub fn all_language_settings(cx: &App) -> &AllLanguageSettings {
    AllLanguageSettings::get_global(cx)
}

/// The settings registered for each language.
#[derive(Debug, Clone)]
pub struct AllLanguageSettings {
    /// Settings for buffers without a language or with an unregistered one.
    pub fallback: Arc<LanguageSettings>,
    languages: FxHashMap<LanguageName, Arc<LanguageSettings>>,
    /// Extra file globs per language name, checked before path suffixes.
    pub file_types: FxHashMap<Arc<str>, (GlobSet, Vec<String>)>,
    /// Semantic token rules applied above the defaults for each language.
    semantic_token_rules: FxHashMap<LanguageName, SemanticTokenRules>,
}

impl Global for AllLanguageSettings {}

impl Default for AllLanguageSettings {
    fn default() -> Self {
        Self {
            fallback: Arc::new(LanguageSettings::fallback()),
            languages: FxHashMap::default(),
            file_types: FxHashMap::default(),
            semantic_token_rules: FxHashMap::default(),
        }
    }
}

static FALLBACK: LazyLock<AllLanguageSettings> = LazyLock::new(AllLanguageSettings::default);

/// The settings for a particular language. Settings that never vary by language are
/// the constants in this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageSettings {
    /// How many columns a tab should occupy.
    pub tab_size: NonZeroU32,
    /// Whether to indent lines using tab characters, as opposed to multiple spaces.
    pub hard_tabs: bool,
    /// Whether to soft wrap at the editor width.
    pub soft_wrap: bool,
    /// The column at which to soft-wrap lines, for buffers where soft-wrap is enabled.
    pub preferred_line_length: u32,
    /// Columns at which to show wrap guides (vertical rulers).
    pub wrap_guides: Vec<usize>,
    /// Whether outlines and breadcrumbs use LSP document symbols instead of tree-sitter.
    pub lsp_document_symbols: bool,
    /// Where the `editor::Rewrap` action is allowed.
    pub allow_rewrap: RewrapBehavior,
    /// Whether to format on LSP on-type-format trigger characters.
    pub use_on_type_format: bool,
    /// Whether indentation of pasted content is adjusted to the context.
    pub auto_indent_on_paste: bool,
    /// How buffer words are offered as completions.
    pub word_completions: WordsCompletionMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionSettings {
    /// How many characters the query needs before word completions show automatically.
    pub words_min_length: usize,
    /// Whether to fetch LSP completions.
    pub lsp: bool,
    /// How long to wait for each language server's completions; 0 waits indefinitely.
    pub lsp_fetch_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndentGuideSettings {
    pub enabled: bool,
    /// Width of the indent guides in pixels, between 1 and 10.
    pub line_width: u32,
    /// Width of the active indent guide in pixels, between 1 and 10.
    pub active_line_width: u32,
}

impl IndentGuideSettings {
    /// Returns the clamped line width in pixels for an indent guide based on whether it is active.
    pub fn visible_line_width(&self, active: bool) -> u32 {
        let width = if active { self.active_line_width } else { self.line_width };
        width.clamp(1, 10)
    }
}

impl LanguageSettings {
    pub fn for_buffer(buffer: &Buffer, cx: &App) -> Arc<LanguageSettings> {
        Self::resolve(Some(buffer), None, cx)
    }

    pub fn for_buffer_at<D: ToOffset>(buffer: &Buffer, position: D, cx: &App) -> Arc<LanguageSettings> {
        let language = buffer.language_at(position);
        Self::resolve(Some(buffer), language.map(|l| l.name()).as_ref(), cx)
    }

    pub fn for_buffer_snapshot(buffer: &BufferSnapshot, offset: Option<usize>, cx: &App) -> Arc<LanguageSettings> {
        let language = if let Some(offset) = offset { buffer.language_at(offset) } else { buffer.language() };

        if let Some(resolved) = buffer.resolved_settings()
            && language == buffer.language()
        {
            return Arc::clone(resolved);
        }

        AllLanguageSettings::get_global(cx).language(language.map(|l| l.name()).as_ref())
    }

    pub fn resolve(
        buffer: Option<&Buffer>,
        override_language: Option<&LanguageName>,
        cx: &App,
    ) -> Arc<LanguageSettings> {
        let Some(buffer) = buffer else {
            return AllLanguageSettings::get_global(cx).language(override_language);
        };

        if let Some(resolved) = buffer.resolved_settings()
            && override_language.is_none_or(|language| Some(language) == buffer.language().map(|l| l.name()).as_ref())
        {
            return Arc::clone(resolved);
        }

        Self::resolve_uncached(buffer, override_language, cx)
    }

    pub(crate) fn resolve_uncached(
        buffer: &Buffer,
        override_language: Option<&LanguageName>,
        cx: &App,
    ) -> Arc<LanguageSettings> {
        let buffer_language = buffer.language().map(|l| l.name());
        let language_name = override_language.or(buffer_language.as_ref());
        AllLanguageSettings::get_global(cx).language(language_name)
    }

    /// Settings for buffers without a language or with an unregistered one.
    pub fn fallback() -> Self {
        Self {
            tab_size: NonZeroU32::new(4).expect("non-zero"),
            hard_tabs: false,
            soft_wrap: false,
            preferred_line_length: 120,
            wrap_guides: vec![120],
            lsp_document_symbols: true,
            allow_rewrap: RewrapBehavior::InComments,
            use_on_type_format: true,
            auto_indent_on_paste: true,
            word_completions: WordsCompletionMode::Fallback,
        }
    }
}

/// The settings for inlay hints.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct InlayHintSettings {
    /// Whether type hints should be shown.
    pub show_type_hints: bool,
    /// Whether parameter hints should be shown.
    pub show_parameter_hints: bool,
    /// Whether other hints should be shown.
    pub show_other_hints: bool,
    /// Whether to show a background for inlay hints.
    ///
    /// If set to `true`, the background will use the `hint.background` color
    /// from the current theme.
    pub show_background: bool,
    /// Whether or not to debounce inlay hints updates after buffer edits.
    /// 0 disables debouncing.
    pub edit_debounce_ms: u64,
    /// Whether or not to debounce inlay hints updates after buffer scrolls.
    /// 0 disables debouncing.
    pub scroll_debounce_ms: u64,
}

impl InlayHintSettings {
    /// Returns the kinds of inlay hints that are enabled based on the settings.
    pub fn enabled_inlay_hint_kinds(&self) -> FxHashSet<Option<InlayHintKind>> {
        let mut kinds = FxHashSet::default();
        if self.show_type_hints {
            kinds.insert(Some(InlayHintKind::Type));
        }
        if self.show_parameter_hints {
            kinds.insert(Some(InlayHintKind::Parameter));
        }
        if self.show_other_hints {
            kinds.insert(None);
        }
        kinds
    }
}

impl AllLanguageSettings {
    pub fn get_global(cx: &App) -> &Self {
        cx.try_global::<Self>().unwrap_or(&FALLBACK)
    }

    /// Registers a language's settings, plus file globs that select the language
    /// before path suffixes do.
    pub fn register(
        name: LanguageName,
        settings: LanguageSettings,
        file_types: &[&str],
        semantic_token_rules: Option<SemanticTokenRules>,
        cx: &mut App,
    ) {
        cx.default_global::<Self>().insert(name, settings, file_types, semantic_token_rules);
    }

    fn insert(
        &mut self,
        name: LanguageName,
        settings: LanguageSettings,
        file_types: &[&str],
        semantic_token_rules: Option<SemanticTokenRules>,
    ) {
        if let Some(rules) = semantic_token_rules {
            self.semantic_token_rules.insert(name.clone(), rules);
        }
        if !file_types.is_empty() {
            let mut builder = GlobSetBuilder::new();
            for pattern in file_types {
                builder.add(Glob::new(pattern).expect("valid built-in file type glob"));
            }
            let globs = builder.build().expect("valid built-in file type globs");
            self.file_types
                .insert(name.0.as_ref().into(), (globs, file_types.iter().map(ToString::to_string).collect()));
        }
        self.languages.insert(name, Arc::new(settings));
    }

    pub fn semantic_token_rules(&self, language_name: &LanguageName) -> Option<&SemanticTokenRules> {
        self.semantic_token_rules.get(language_name)
    }

    /// Returns the [`LanguageSettings`] for the language with the specified name.
    pub fn language(&self, language_name: Option<&LanguageName>) -> Arc<LanguageSettings> {
        let settings = language_name.and_then(|name| self.languages.get(name)).unwrap_or(&self.fallback);
        Arc::clone(settings)
    }
}

/// Where `editor::Rewrap` is allowed.
#[derive(Debug, PartialEq, Eq, Clone, Copy, Default)]
pub enum RewrapBehavior {
    #[default]
    InComments,
    Anywhere,
}

/// How buffer words are offered as completions.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WordsCompletionMode {
    /// Only when LSP completions fail or time out.
    Fallback,
    /// Never automatically; the word completion action still works.
    Disabled,
}

/// The kind of an inlay hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InlayHintKind {
    Type,
    Parameter,
}

impl InlayHintKind {
    /// Returns the [`InlayHintKind`] for `name`, or `None` for an unknown name.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "type" => Some(Self::Type),
            "parameter" => Some(Self::Parameter),
            _ => None,
        }
    }

    pub const fn name(&self) -> &'static str {
        match self {
            Self::Type => "type",
            Self::Parameter => "parameter",
        }
    }
}

/// Custom rules for rendering LSP semantic tokens, loaded from each language's JSON.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(transparent)]
pub struct SemanticTokenRules {
    pub rules: Vec<SemanticTokenRule>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct SemanticTokenRule {
    pub token_type: Option<String>,
    #[serde(default)]
    pub token_modifiers: Vec<String>,
    #[serde(default)]
    pub style: Vec<String>,
    pub foreground_color: Option<Rgba>,
    pub background_color: Option<Rgba>,
    pub underline: Option<SemanticTokenColorOverride>,
    pub strikethrough: Option<SemanticTokenColorOverride>,
    pub font_weight: Option<SemanticTokenFontWeight>,
    pub font_style: Option<SemanticTokenFontStyle>,
}

impl SemanticTokenRule {
    pub const fn no_style_defined(&self) -> bool {
        self.style.is_empty()
            && self.foreground_color.is_none()
            && self.background_color.is_none()
            && self.underline.is_none()
            && self.strikethrough.is_none()
            && self.font_weight.is_none()
            && self.font_style.is_none()
    }
}

#[derive(Copy, Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum SemanticTokenColorOverride {
    InheritForeground(bool),
    Replace(Rgba),
}

#[derive(Copy, Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemanticTokenFontWeight {
    #[default]
    Normal,
    Bold,
}

#[derive(Copy, Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemanticTokenFontStyle {
    #[default]
    Normal,
    Italic,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_falls_back_for_missing_or_unregistered_language() {
        let mut all = AllLanguageSettings::default();
        let rust = LanguageSettings { tab_size: NonZeroU32::new(2).unwrap(), ..LanguageSettings::fallback() };
        all.insert(LanguageName::new_static("Rust"), rust.clone(), &[], None);

        assert_eq!(*all.language(Some(&LanguageName::new_static("Rust"))), rust);
        assert_eq!(*all.language(Some(&LanguageName::new_static("Go"))), LanguageSettings::fallback());
        assert_eq!(*all.language(None), LanguageSettings::fallback());
    }

    #[test]
    fn insert_records_file_types_only_when_given() {
        let mut all = AllLanguageSettings::default();
        all.insert(LanguageName::new_static("JSONC"), LanguageSettings::fallback(), &["tsconfig*.json"], None);
        all.insert(LanguageName::new_static("JSON"), LanguageSettings::fallback(), &[], None);

        let (globs, patterns) = &all.file_types["JSONC"];
        assert!(globs.is_match("tsconfig.base.json"));
        assert!(!globs.is_match("package.json"));
        assert_eq!(patterns, &vec!["tsconfig*.json".to_owned()]);
        assert!(!all.file_types.contains_key("JSON"));
    }

    #[test]
    fn insert_records_semantic_token_rules_only_when_given() {
        let mut all = AllLanguageSettings::default();
        let rust = LanguageName::new_static("Rust");
        let go = LanguageName::new_static("Go");
        all.insert(rust.clone(), LanguageSettings::fallback(), &[], Some(SemanticTokenRules::default()));
        all.insert(go.clone(), LanguageSettings::fallback(), &[], None);

        assert_eq!(all.semantic_token_rules(&rust), Some(&SemanticTokenRules::default()));
        assert_eq!(all.semantic_token_rules(&go), None);
    }

    #[test]
    fn indent_guide_visible_line_width() {
        assert_eq!(INDENT_GUIDES.visible_line_width(false), 1);
        let wide = IndentGuideSettings { active_line_width: 20, ..INDENT_GUIDES };
        assert_eq!(wide.visible_line_width(true), 10);
    }
}
