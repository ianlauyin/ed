use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use gpui::AsyncApp;
use language::LanguageServerName;
use language::LspAdapter;
use language::LspAdapterDelegate;
use lsp::LanguageServerBinary;
use lsp::Uri;
use project::lsp_store::language_server_settings;
use serde_json::Value;
use settings::SemanticTokenRules;

/// Styles for tombi's custom token types (`key`, `table`, `boolean`, dates),
/// matching the fallback TOML highlighting.
pub(crate) fn semantic_token_rules() -> SemanticTokenRules {
    let content = grammars::get_file("toml/semantic_token_rules.json").expect("missing toml/semantic_token_rules.json");
    let json = std::str::from_utf8(&content.data).expect("invalid utf-8 in semantic_token_rules");
    serde_json::from_str::<SemanticTokenRules>(json).expect("failed to parse toml semantic_token_rules.json")
}

pub struct TombiLspAdapter;

impl TombiLspAdapter {
    const PACKAGE_NAME: &str = "tombi";
}

#[async_trait(?Send)]
impl LspAdapter for TombiLspAdapter {
    fn name(&self) -> LanguageServerName {
        LanguageServerName::new_static(Self::PACKAGE_NAME)
    }

    async fn check_if_user_installed(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        crate::npx::node_server_binary(delegate, &[Self::PACKAGE_NAME], Self::PACKAGE_NAME, &["lsp"]).await
    }

    async fn workspace_configuration(
        self: Arc<Self>,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: Option<Uri>,
        cx: &mut AsyncApp,
    ) -> Result<Value> {
        let settings = cx.update(|cx| {
            language_server_settings(delegate.as_ref(), &self.name(), cx).and_then(|s| s.settings.clone())
        });

        Ok(settings.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::semantic_token_rules;

    #[test]
    fn styles_tombi_token_types() {
        let rules = semantic_token_rules().rules;
        let style = |token_type: &str| {
            rules
                .iter()
                .find(|rule| rule.token_type.as_deref() == Some(token_type))
                .and_then(|rule| rule.style.first().cloned())
        };
        assert_eq!(style("key").as_deref(), Some("property"));
        assert_eq!(style("table").as_deref(), Some("type"));
        assert_eq!(style("boolean").as_deref(), Some("boolean"));
        for date in ["offsetDateTime", "localDateTime", "localDate", "localTime"] {
            assert_eq!(style(date).as_deref(), Some("constant"), "{date}");
        }
    }
}
