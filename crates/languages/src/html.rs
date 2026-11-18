use std::num::NonZeroU32;
use std::sync::Arc;

use async_trait::async_trait;
use gpui::AsyncApp;
use language::LanguageName;
use language::LspAdapter;
use language::LspAdapterDelegate;
use language::language_settings::LanguageSettings;
use language::language_settings::RewrapBehavior;
use language::language_settings::WordsCompletionMode;
use lsp::LanguageServerBinary;
use lsp::LanguageServerName;
use rustc_hash::FxHashMap;

use crate::LanguageDef;
use crate::npx;

pub(crate) struct HtmlLspAdapter;

#[async_trait(?Send)]
impl LspAdapter for HtmlLspAdapter {
    fn name(&self) -> LanguageServerName {
        LanguageServerName("vscode-html-language-server".into())
    }

    async fn check_if_user_installed(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        npx::node_server_binary(
            delegate,
            &["vscode-langservers-extracted"],
            "vscode-html-language-server",
            &["--stdio"],
        )
        .await
    }

    fn language_ids(&self) -> FxHashMap<LanguageName, String> {
        std::iter::once((LanguageName::new_static("HTML"), "html".into())).collect()
    }
}

/// Settings for HTML.
pub(crate) fn settings() -> LanguageSettings {
    LanguageSettings {
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

pub(crate) fn languages() -> Vec<LanguageDef> {
    vec![LanguageDef {
        grammar: Some(tree_sitter_html::LANGUAGE.into()),
        adapters: vec![Arc::new(HtmlLspAdapter)],
        ..LanguageDef::new("html", settings())
    }]
}
