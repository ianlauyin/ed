use std::num::NonZeroU32;

use language::language_settings::LanguageSettings;
use language::language_settings::RewrapBehavior;
use language::language_settings::WordsCompletionMode;

use crate::LanguageDef;

/// Settings for Git Commit.
pub(crate) fn settings() -> LanguageSettings {
    LanguageSettings {
        tab_size: NonZeroU32::new(4).expect("non-zero"),
        hard_tabs: false,
        soft_wrap: true,
        preferred_line_length: 120,
        wrap_guides: vec![120],
        lsp_document_symbols: true,
        allow_rewrap: RewrapBehavior::Anywhere,
        use_on_type_format: true,
        auto_indent_on_paste: true,
        word_completions: WordsCompletionMode::Fallback,
    }
}

pub(crate) fn languages() -> Vec<LanguageDef> {
    vec![LanguageDef::new("gitcommit", settings())]
}
