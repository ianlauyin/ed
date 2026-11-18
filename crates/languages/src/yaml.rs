use std::num::NonZeroU32;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use gpui::AsyncApp;
use language::LanguageName;
use language::LspAdapter;
use language::LspAdapterDelegate;
use language::language_settings::LanguageSettings;
use language::language_settings::RewrapBehavior;
use language::language_settings::WordsCompletionMode;
use language::language_settings::all_language_settings;
use lsp::LanguageServerBinary;
use lsp::LanguageServerName;
use lsp::Uri;
use serde_json::Value;

use crate::LanguageDef;
use crate::npx;

pub(crate) struct YamlLspAdapter;

impl YamlLspAdapter {
    const SERVER_NAME: LanguageServerName = LanguageServerName::new_static("yaml-language-server");
}

#[async_trait(?Send)]
impl LspAdapter for YamlLspAdapter {
    fn name(&self) -> LanguageServerName {
        Self::SERVER_NAME
    }

    async fn check_if_user_installed(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        let bin = Self::SERVER_NAME;
        npx::node_server_binary(delegate, &[bin.as_ref()], bin.as_ref(), &["--stdio"]).await
    }

    async fn workspace_configuration(
        self: Arc<Self>,
        _: &Arc<dyn LspAdapterDelegate>,
        _: Option<Uri>,
        cx: &mut AsyncApp,
    ) -> Result<Value> {
        let tab_size =
            cx.update(|cx| all_language_settings(cx).language(Some(&LanguageName::new_static("YAML"))).tab_size);
        Ok(configuration(tab_size))
    }
}

fn configuration(tab_size: NonZeroU32) -> Value {
    serde_json::json!({
        "[yaml]": {"editor.tabSize": tab_size},
        "yaml": {
            "format": {"enable": true},
            // `kubernetes` is the server's built-in schema, resolved per document by `apiVersion`/`kind`.
            "schemas": {"kubernetes": ["**/kube/**/*.yaml", "**/kube/**/*.yml"]}
        }
    })
}

/// Settings for YAML.
pub(crate) fn settings() -> LanguageSettings {
    LanguageSettings {
        tab_size: NonZeroU32::new(2).expect("non-zero"),
        hard_tabs: false,
        soft_wrap: false,
        preferred_line_length: 120,
        wrap_guides: vec![120],
        lsp_document_symbols: true,
        allow_rewrap: RewrapBehavior::InComments,
        use_on_type_format: true,
        auto_indent_on_paste: false,
        word_completions: WordsCompletionMode::Fallback,
    }
}

pub(crate) fn languages() -> Vec<LanguageDef> {
    vec![LanguageDef {
        grammar: Some(tree_sitter_yaml::LANGUAGE.into()),
        adapters: vec![Arc::new(YamlLspAdapter)],
        ..LanguageDef::new("yaml", settings())
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_maps_kube_dirs_to_the_kubernetes_schema() {
        let config = configuration(NonZeroU32::new(2).unwrap());
        assert_eq!(config["[yaml]"]["editor.tabSize"], 2);
        assert_eq!(config["yaml"]["format"]["enable"], true);
        assert_eq!(
            config["yaml"]["schemas"]["kubernetes"],
            serde_json::json!(["**/kube/**/*.yaml", "**/kube/**/*.yml"])
        );
    }
}
