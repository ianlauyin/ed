//! Provides `language`-related settings.

use std::num::NonZeroU32;
use std::sync::Arc;

use collections::FxHashMap;
use collections::HashMap;
use collections::HashSet;
use globset::Glob;
use globset::GlobSet;
use globset::GlobSetBuilder;
use gpui::App;
use gpui::Modifiers;
use gpui::SharedString;
pub use settings::AutoIndentMode;
pub use settings::CompletionSettingsContent;
use settings::DocumentFoldingRanges;
use settings::DocumentSymbols;
pub use settings::FormatOnSave;
pub use settings::Formatter;
pub use settings::FormatterList;
pub use settings::InlayHintKind;
use settings::IntoGpui;
pub use settings::LanguageSettingsContent;
pub use settings::LineEndingSetting;
pub use settings::LspInsertMode;
pub use settings::RewrapBehavior;
use settings::SemanticTokens;
use settings::Settings;
use settings::SettingsLocation;
pub use settings::ShowWhitespaceSetting;
pub use settings::SoftWrap;
pub use settings::SoftWrapIndent;
pub use settings::WordsCompletionMode;
use text::ToOffset;

use crate::Buffer;
use crate::BufferSnapshot;
use crate::File;
use crate::LanguageName;
use crate::ModelineSettings;

/// Returns the settings for all languages from the provided file.
pub fn all_language_settings<'a>(file: Option<&'a Arc<dyn File>>, cx: &'a App) -> &'a AllLanguageSettings {
    let location = file.map(|f| SettingsLocation { worktree_id: f.worktree_id(cx), path: f.path().as_ref() });
    AllLanguageSettings::get(location, cx)
}

/// The settings for all languages.
#[derive(Debug, Clone)]
pub struct AllLanguageSettings {
    /// The default language settings.
    pub defaults: Arc<LanguageSettings>,
    languages: HashMap<LanguageName, Arc<LanguageSettings>>,
    pub file_types: FxHashMap<Arc<str>, (GlobSet, Vec<String>)>,
}

settings::register_setting!(AllLanguageSettings);

#[derive(Debug, Clone, PartialEq)]
pub struct WhitespaceMap {
    pub space: SharedString,
    pub tab: SharedString,
}

/// The settings for a particular language.
#[derive(Debug, Clone, PartialEq)]
pub struct LanguageSettings {
    /// How many columns a tab should occupy.
    pub tab_size: NonZeroU32,
    /// Whether to indent lines using tab characters, as opposed to multiple
    /// spaces.
    pub hard_tabs: bool,
    /// How to soft-wrap long lines of text.
    pub soft_wrap: settings::SoftWrap,
    /// How to indent soft-wrapped continuation lines.
    pub soft_wrap_indent: settings::SoftWrapIndent,
    /// The column at which to soft-wrap lines, for buffers where soft-wrap
    /// is enabled.
    pub preferred_line_length: u32,
    /// Whether to show wrap guides (vertical rulers) in the editor.
    /// Setting this to true will show a guide at the 'preferred_line_length' value
    /// if softwrap is set to 'preferred_line_length', and will show any
    /// additional guides as specified by the 'wrap_guides' setting.
    pub show_wrap_guides: bool,
    /// Character counts at which to show wrap guides (vertical rulers) in the editor.
    pub wrap_guides: Vec<usize>,
    /// Indent guide related settings.
    pub indent_guides: IndentGuideSettings,
    /// Whether or not to perform a buffer format before saving.
    pub format_on_save: FormatOnSave,
    /// Whether or not to remove any trailing whitespace from lines of a buffer
    /// before saving it.
    pub remove_trailing_whitespace_on_save: bool,
    /// Whether or not to ensure there's a single newline at the end of a buffer
    /// when saving it.
    pub ensure_final_newline_on_save: bool,
    /// How line endings are initialized for new files and normalized during
    /// format and save.
    pub line_ending: LineEndingSetting,
    /// How to perform a buffer format.
    pub formatter: settings::FormatterList,
    /// Whether to automatically close JSX tags.
    pub jsx_tag_auto_close: bool,
    /// Whether to use language servers to provide code intelligence.
    pub enable_language_server: bool,
    /// Controls how semantic tokens from language servers are used for syntax highlighting.
    pub semantic_tokens: SemanticTokens,
    /// Controls whether folding ranges from language servers are used instead of
    /// tree-sitter and indent-based folding.
    pub document_folding_ranges: DocumentFoldingRanges,
    /// Controls the source of document symbols used for outlines and breadcrumbs.
    pub document_symbols: DocumentSymbols,
    /// Controls where the `editor::Rewrap` action is allowed for this language.
    pub allow_rewrap: RewrapBehavior,
    pub show_whitespaces: settings::ShowWhitespaceSetting,
    /// Visible characters used to render whitespace when show_whitespaces is enabled.
    pub whitespace_map: WhitespaceMap,
    /// Whether to start a new line with a comment when a previous line is a comment as well.
    pub extend_comment_on_newline: bool,
    /// Whether to continue markdown lists when pressing enter.
    pub extend_list_on_newline: bool,
    /// Whether to indent list items when pressing tab after a list marker.
    pub indent_list_on_tab: bool,
    /// Inlay hint related settings.
    pub inlay_hints: InlayHintSettings,
    /// Whether to automatically close brackets.
    pub use_autoclose: bool,
    /// Whether to automatically surround text with brackets.
    pub use_auto_surround: bool,
    /// Whether to use additional LSP queries to format (and amend) the code after
    /// every "trigger" symbol input, defined by LSP server capabilities.
    pub use_on_type_format: bool,
    /// Controls automatic indentation behavior when typing.
    pub auto_indent: AutoIndentMode,
    /// Whether indentation of pasted content should be adjusted based on the context.
    pub auto_indent_on_paste: bool,
    /// Controls how the editor handles the autoclosed characters.
    pub always_treat_brackets_as_autoclosed: bool,
    /// Which code actions to run on save
    pub code_actions_on_format: HashMap<String, bool>,
    /// Whether to perform linked edits
    pub linked_edits: bool,
    /// Task configuration for this language.
    pub tasks: LanguageTaskSettings,
    /// Whether to pop the completions menu while typing in an editor without
    /// explicitly requesting it.
    pub show_completions_on_input: bool,
    /// Whether to display inline and alongside documentation for items in the
    /// completions menu.
    pub show_completion_documentation: bool,
    /// Completion settings for this language.
    pub completions: CompletionSettings,
    /// Whether to enable word diff highlighting in the editor.
    ///
    /// When enabled, changed words within modified lines are highlighted
    /// to show exactly what changed.
    ///
    /// Default: `true`
    pub word_diff_enabled: bool,
    /// Whether to use tree-sitter bracket queries to detect and colorize the brackets in the editor.
    pub colorize_brackets: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompletionSettings {
    /// Controls how words are completed.
    /// For large documents, not all words may be fetched for completion.
    ///
    /// Default: `fallback`
    pub words: WordsCompletionMode,
    /// How many characters has to be in the completions query to automatically show the words-based completions.
    /// Before that value, it's still possible to trigger the words-based completion manually with the corresponding editor command.
    ///
    /// Default: 3
    pub words_min_length: usize,
    /// Whether to fetch LSP completions or not.
    ///
    /// Default: true
    pub lsp: bool,
    /// When fetching LSP completions, determines how long to wait for a response of a particular server.
    /// When set to 0, waits indefinitely.
    ///
    /// Default: 0
    pub lsp_fetch_timeout_ms: u64,
    /// Controls how LSP completions are inserted.
    ///
    /// Default: "replace_suffix"
    pub lsp_insert_mode: LspInsertMode,
}

/// The settings for indent guides.
#[derive(Debug, Clone, PartialEq)]
pub struct IndentGuideSettings {
    /// Whether to display indent guides in the editor.
    ///
    /// Default: true
    pub enabled: bool,
    /// The width of the indent guides in pixels, between 1 and 10.
    ///
    /// Default: 1
    pub line_width: u32,
    /// The width of the active indent guide in pixels, between 1 and 10.
    ///
    /// Default: 1
    pub active_line_width: u32,
    /// Determines how indent guides are colored.
    ///
    /// Default: Fixed
    pub coloring: settings::IndentGuideColoring,
    /// Determines how indent guide backgrounds are colored.
    ///
    /// Default: Disabled
    pub background_coloring: settings::IndentGuideBackgroundColoring,
}

impl IndentGuideSettings {
    /// Returns the clamped line width in pixels for an indent guide based on
    /// whether it is active, or `None` when line coloring is disabled.
    pub fn visible_line_width(&self, active: bool) -> Option<u32> {
        if self.coloring == settings::IndentGuideColoring::Disabled {
            return None;
        }
        let width = if active { self.active_line_width } else { self.line_width };
        Some(width.clamp(1, 10))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LanguageTaskSettings {
    /// Extra task variables to set for a particular language.
    pub variables: HashMap<String, String>,
    pub enabled: bool,
    /// Use LSP tasks over built-in language tasks.
    /// If no LSP tasks are returned due to error/timeout or regular execution,
    /// Built-in language tasks will be used instead.
    ///
    /// Other Zed tasks will still be shown:
    /// * Zed task from either of the task config file
    /// * Zed task from history (e.g. one-off task was spawned before)
    pub prefer_lsp: bool,
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

        let location =
            buffer.file().map(|f| SettingsLocation { worktree_id: f.worktree_id(cx), path: f.path().as_ref() });

        let mut settings = AllLanguageSettings::get(location, cx).language(language.map(|l| l.name()).as_ref());

        if let Some(modeline) = buffer.modeline() {
            merge_with_modeline(Arc::make_mut(&mut settings), modeline);
        }

        settings
    }

    pub fn resolve(
        buffer: Option<&Buffer>,
        override_language: Option<&LanguageName>,
        cx: &App,
    ) -> Arc<LanguageSettings> {
        let Some(buffer) = buffer else {
            return AllLanguageSettings::get(None, cx).language(override_language);
        };

        if let Some(resolved) = buffer.resolved_settings()
            && override_language.is_none_or(|language| Some(language) == buffer.language().map(|l| l.name()).as_ref())
        {
            return resolved.clone();
        }

        Self::resolve_uncached(buffer, override_language, cx)
    }

    pub(crate) fn resolve_uncached(
        buffer: &Buffer,
        override_language: Option<&LanguageName>,
        cx: &App,
    ) -> Arc<LanguageSettings> {
        let location =
            buffer.file().map(|f| SettingsLocation { worktree_id: f.worktree_id(cx), path: f.path().as_ref() });
        let buffer_language = buffer.language().map(|l| l.name());
        let language_name = override_language.or(buffer_language.as_ref());
        let mut settings = AllLanguageSettings::get(location, cx).language(language_name);

        if let Some(modeline) = buffer.modeline() {
            merge_with_modeline(Arc::make_mut(&mut settings), modeline);
        }

        settings
    }
}

// The settings for inlay hints.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct InlayHintSettings {
    /// Global switch to toggle hints on and off.
    ///
    /// Default: false
    pub enabled: bool,
    /// Whether type hints should be shown.
    ///
    /// Default: true
    pub show_type_hints: bool,
    /// Whether parameter hints should be shown.
    ///
    /// Default: true
    pub show_parameter_hints: bool,
    /// Whether other hints should be shown.
    ///
    /// Default: true
    pub show_other_hints: bool,
    /// Whether to show a background for inlay hints.
    ///
    /// If set to `true`, the background will use the `hint.background` color
    /// from the current theme.
    ///
    /// Default: false
    pub show_background: bool,
    /// Whether or not to debounce inlay hints updates after buffer edits.
    ///
    /// Set to 0 to disable debouncing.
    ///
    /// Default: 700
    pub edit_debounce_ms: u64,
    /// Whether or not to debounce inlay hints updates after buffer scrolls.
    ///
    /// Set to 0 to disable debouncing.
    ///
    /// Default: 50
    pub scroll_debounce_ms: u64,
    /// Toggles inlay hints (hides or shows) when the user presses the modifiers specified.
    /// If only a subset of the modifiers specified is pressed, hints are not toggled.
    /// If no modifiers are specified, this is equivalent to `None`.
    ///
    /// Default: None
    pub toggle_on_modifiers_press: Option<Modifiers>,
}

impl InlayHintSettings {
    /// Returns the kinds of inlay hints that are enabled based on the settings.
    pub fn enabled_inlay_hint_kinds(&self) -> HashSet<Option<InlayHintKind>> {
        let mut kinds = HashSet::default();
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
    /// Returns the [`LanguageSettings`] for the language with the specified name.
    pub fn language(&self, language_name: Option<&LanguageName>) -> Arc<LanguageSettings> {
        let settings = language_name.and_then(|name| self.languages.get(name)).unwrap_or(&self.defaults);

        Arc::clone(settings)
    }
}

fn merge_with_modeline(settings: &mut LanguageSettings, modeline: &ModelineSettings) {
    if let Some(tab_size) = modeline.tab_size {
        settings.tab_size = tab_size;
    }
    if let Some(hard_tabs) = modeline.hard_tabs {
        settings.hard_tabs = hard_tabs;
    }
    if let Some(preferred_line_length) = modeline.preferred_line_length {
        settings.preferred_line_length = preferred_line_length.into();
    }
    if let Some(auto_indent) = modeline.auto_indent {
        settings.auto_indent = if auto_indent { AutoIndentMode::SyntaxAware } else { AutoIndentMode::None };
    }
    if modeline.show_trailing_whitespace == Some(true) {
        settings.show_whitespaces = ShowWhitespaceSetting::Trailing;
    }
    if let Some(ensure_final_newline) = modeline.ensure_final_newline {
        settings.ensure_final_newline_on_save = ensure_final_newline;
    }
}

impl settings::Settings for AllLanguageSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let all_languages = &content.project.all_languages;

        fn load_from_content(settings: LanguageSettingsContent) -> LanguageSettings {
            let inlay_hints = settings.inlay_hints.unwrap();
            let completions = settings.completions.unwrap();
            let indent_guides = settings.indent_guides.unwrap();
            let tasks = settings.tasks.unwrap();
            let whitespace_map = settings.whitespace_map.unwrap();

            LanguageSettings {
                tab_size: settings.tab_size.unwrap(),
                hard_tabs: settings.hard_tabs.unwrap(),
                soft_wrap: settings.soft_wrap.unwrap(),
                soft_wrap_indent: settings.soft_wrap_indent.unwrap(),
                preferred_line_length: settings.preferred_line_length.unwrap(),
                show_wrap_guides: settings.show_wrap_guides.unwrap(),
                wrap_guides: settings.wrap_guides.unwrap(),
                indent_guides: IndentGuideSettings {
                    enabled: indent_guides.enabled.unwrap(),
                    line_width: indent_guides.line_width.unwrap(),
                    active_line_width: indent_guides.active_line_width.unwrap(),
                    coloring: indent_guides.coloring.unwrap(),
                    background_coloring: indent_guides.background_coloring.unwrap(),
                },
                format_on_save: settings.format_on_save.unwrap(),
                remove_trailing_whitespace_on_save: settings.remove_trailing_whitespace_on_save.unwrap(),
                ensure_final_newline_on_save: settings.ensure_final_newline_on_save.unwrap(),
                line_ending: settings.line_ending.unwrap(),
                formatter: settings.formatter.unwrap(),
                jsx_tag_auto_close: settings.jsx_tag_auto_close.unwrap().enabled.unwrap(),
                enable_language_server: settings.enable_language_server.unwrap(),
                semantic_tokens: settings.semantic_tokens.unwrap(),
                document_folding_ranges: settings.document_folding_ranges.unwrap(),
                document_symbols: settings.document_symbols.unwrap(),
                allow_rewrap: settings.allow_rewrap.unwrap(),

                show_whitespaces: settings.show_whitespaces.unwrap(),
                whitespace_map: WhitespaceMap {
                    space: SharedString::new(whitespace_map.space.unwrap().to_string()),
                    tab: SharedString::new(whitespace_map.tab.unwrap().to_string()),
                },
                extend_comment_on_newline: settings.extend_comment_on_newline.unwrap(),
                extend_list_on_newline: settings.extend_list_on_newline.unwrap(),
                indent_list_on_tab: settings.indent_list_on_tab.unwrap(),
                inlay_hints: InlayHintSettings {
                    enabled: inlay_hints.enabled.unwrap(),
                    show_type_hints: inlay_hints.show_type_hints.unwrap(),
                    show_parameter_hints: inlay_hints.show_parameter_hints.unwrap(),
                    show_other_hints: inlay_hints.show_other_hints.unwrap(),
                    show_background: inlay_hints.show_background.unwrap(),
                    edit_debounce_ms: inlay_hints.edit_debounce_ms.unwrap(),
                    scroll_debounce_ms: inlay_hints.scroll_debounce_ms.unwrap(),
                    toggle_on_modifiers_press: inlay_hints.toggle_on_modifiers_press.map(|m| m.into_gpui()),
                },
                use_autoclose: settings.use_autoclose.unwrap(),
                use_auto_surround: settings.use_auto_surround.unwrap(),
                use_on_type_format: settings.use_on_type_format.unwrap(),
                auto_indent: settings.auto_indent.unwrap(),
                auto_indent_on_paste: settings.auto_indent_on_paste.unwrap(),
                always_treat_brackets_as_autoclosed: settings.always_treat_brackets_as_autoclosed.unwrap(),
                code_actions_on_format: settings.code_actions_on_format.unwrap(),
                linked_edits: settings.linked_edits.unwrap(),
                tasks: LanguageTaskSettings {
                    variables: tasks.variables.unwrap_or_default(),
                    enabled: tasks.enabled.unwrap(),
                    prefer_lsp: tasks.prefer_lsp.unwrap(),
                },
                show_completions_on_input: settings.show_completions_on_input.unwrap(),
                show_completion_documentation: settings.show_completion_documentation.unwrap(),
                colorize_brackets: settings.colorize_brackets.unwrap(),
                completions: CompletionSettings {
                    words: completions.words.unwrap(),
                    words_min_length: completions.words_min_length.unwrap() as usize,
                    lsp: completions.lsp.unwrap(),
                    lsp_fetch_timeout_ms: completions.lsp_fetch_timeout_ms.unwrap(),
                    lsp_insert_mode: completions.lsp_insert_mode.unwrap(),
                },
                word_diff_enabled: settings.word_diff_enabled.unwrap(),
            }
        }

        let default_language_settings = Arc::new(load_from_content(all_languages.defaults.clone()));

        let mut languages = HashMap::default();
        for (language_name, settings) in &all_languages.languages.0 {
            let mut language_settings = all_languages.defaults.clone();
            language_settings.merge_from(settings);
            languages
                .insert(LanguageName(language_name.clone().into()), Arc::new(load_from_content(language_settings)));
        }

        let mut file_types: FxHashMap<Arc<str>, (GlobSet, Vec<String>)> = FxHashMap::default();

        for (language, patterns) in all_languages.file_types.iter().flatten() {
            let mut builder = GlobSetBuilder::new();

            for pattern in patterns {
                match Glob::new(pattern) {
                    Ok(glob) => {
                        builder.add(glob);
                    }
                    Err(error) => {
                        log::error!(
                            "Failed to parse a `file_types` pattern for {language}. Skipping this pattern:\n\n{error}"
                        );
                    }
                }
            }

            let matcher = builder.build().unwrap_or_else(|error| {
                log::error!(
                    "Failed to compile `file_types` patterns for {language}. Using an empty matcher:\n\n{error}"
                );
                GlobSet::empty()
            });
            file_types.insert(language.clone(), (matcher, patterns.clone()));
        }

        Self { defaults: default_language_settings, languages, file_types }
    }
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct JsxTagAutoCloseSettings {
    /// Enables or disables auto-closing of JSX tags.
    pub enabled: bool,
}
