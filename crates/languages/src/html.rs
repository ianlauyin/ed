use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use collections::HashMap;
use gpui::AsyncApp;
use language::LanguageName;
use language::LspAdapter;
use language::LspAdapterDelegate;
use lsp::LanguageServerBinary;
use lsp::LanguageServerName;
use lsp::Uri;
use project::lsp_store::language_server_settings;
use serde_json::json;

pub struct HtmlLspAdapter;

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
        crate::npx::node_server_binary(
            delegate,
            &["vscode-langservers-extracted"],
            "vscode-html-language-server",
            &["--stdio"],
        )
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
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: Option<Uri>,
        cx: &mut AsyncApp,
    ) -> Result<serde_json::Value> {
        let project_options = cx.update(|cx| {
            language_server_settings(delegate.as_ref(), &self.name(), cx).and_then(|s| s.settings.clone())
        });

        Ok(project_options.unwrap_or_else(|| json!({})))
    }

    fn language_ids(&self) -> HashMap<LanguageName, String> {
        [(LanguageName::new_static("HTML"), "html".into())].into_iter().collect()
    }
}
