use std::num::NonZeroU32;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use gpui::AsyncApp;
use language::LspAdapter;
use language::LspAdapterDelegate;
use language::language_settings::Formatter;
use language::language_settings::LanguageSettings;
use language::language_settings::RewrapBehavior;
use language::language_settings::WordsCompletionMode;
use lsp::LanguageServerBinary;
use lsp::LanguageServerName;
use lsp::Uri;
use project::Fs;
use rustc_hash::FxHashMap;
use serde_json::json;

use crate::LanguageDef;
use crate::npx;
use crate::stylelint::StylelintLspAdapter;

pub(crate) struct CssLspAdapter;

#[async_trait(?Send)]
impl LspAdapter for CssLspAdapter {
    fn name(&self) -> LanguageServerName {
        LanguageServerName("vscode-css-language-server".into())
    }

    async fn check_if_user_installed(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        npx::node_server_binary(delegate, &["vscode-langservers-extracted"], "vscode-css-language-server", &["--stdio"])
            .await
    }

    async fn initialization_options(
        self: Arc<Self>,
        _: &Arc<dyn LspAdapterDelegate>,
        _: &mut AsyncApp,
    ) -> Result<Option<serde_json::Value>> {
        Ok(Some(json!({
            "provideFormatter": true
        })))
    }

    async fn workspace_configuration(
        self: Arc<Self>,
        _: &Arc<dyn LspAdapterDelegate>,
        _: Option<Uri>,
        _: &mut AsyncApp,
    ) -> Result<serde_json::Value> {
        Ok(json!({
            "css": {
                "lint": {}
            },
            "less": {
                "lint": {}
            },
            "scss": {
                "lint": {}
            }
        }))
    }
}

/// Settings for CSS.
pub(crate) fn settings() -> LanguageSettings {
    LanguageSettings {
        tab_size: NonZeroU32::new(4).expect("non-zero"),
        hard_tabs: false,
        soft_wrap: false,
        preferred_line_length: 120,
        wrap_guides: vec![120],
        formatter: Formatter::Auto,
        code_actions_on_format: FxHashMap::default(),
        remove_trailing_whitespace_on_save: true,
        ensure_final_newline_on_save: true,
        lsp_document_symbols: true,
        allow_rewrap: RewrapBehavior::InComments,
        use_on_type_format: true,
        auto_indent_on_paste: true,
        word_completions: WordsCompletionMode::Fallback,
    }
}

pub(crate) fn languages(fs: Arc<dyn Fs>) -> Vec<LanguageDef> {
    vec![LanguageDef {
        grammar: Some(tree_sitter_css::LANGUAGE.into()),
        adapters: vec![Arc::new(CssLspAdapter), Arc::new(StylelintLspAdapter::new(fs))],
        ..LanguageDef::new("css", settings())
    }]
}
