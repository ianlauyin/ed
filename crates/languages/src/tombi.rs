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
