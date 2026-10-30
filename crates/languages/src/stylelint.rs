use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use gpui::AsyncApp;
use language::LspAdapter;
use language::LspAdapterDelegate;
use lsp::LanguageServerBinary;
use lsp::LanguageServerName;
use lsp::Uri;
use project::Fs;
use project::lsp_store::language_server_settings;
use serde_json::Value;
use serde_json::json;

/// Stylelint's language server (`stylelint-language-server --stdio`): lint
/// diagnostics, quick fixes and `source.fixAll.stylelint` for CSS. It lints with
/// the `stylelint` installed in the project and does nothing without one.
pub struct StylelintLspAdapter {
    fs: Arc<dyn Fs>,
}

impl StylelintLspAdapter {
    const SERVER_NAME: LanguageServerName = LanguageServerName::new_static("stylelint");
    const NPX_PACKAGE: &str = "@stylelint/language-server";
    const BIN: &str = "stylelint-language-server";
    const ARGUMENTS: &[&str] = &["--stdio"];

    pub fn new(fs: Arc<dyn Fs>) -> Self {
        StylelintLspAdapter { fs }
    }
}

/// The server reads its options from the `stylelint` configuration section.
fn stylelint_configuration(settings: Option<Value>) -> Value {
    json!({ "stylelint": settings.unwrap_or_else(|| json!({})) })
}

#[async_trait(?Send)]
impl LspAdapter for StylelintLspAdapter {
    fn name(&self) -> LanguageServerName {
        StylelintLspAdapter::SERVER_NAME
    }

    fn binary_name(&self) -> String {
        StylelintLspAdapter::BIN.into()
    }

    /// The nearest `node_modules/.bin/stylelint-language-server` at or above the
    /// worktree root, then `PATH`, then npx.
    async fn check_if_user_installed(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        for dir in delegate.worktree_root_path().ancestors() {
            let path = dir.join("node_modules").join(".bin").join(StylelintLspAdapter::BIN);
            if self.fs.is_file(&path).await {
                return Some(LanguageServerBinary {
                    path,
                    env: Some(delegate.shell_env().await),
                    arguments: StylelintLspAdapter::ARGUMENTS.iter().map(Into::into).collect(),
                });
            }
        }
        crate::npx::node_server_binary(
            delegate,
            &[StylelintLspAdapter::NPX_PACKAGE],
            StylelintLspAdapter::BIN,
            StylelintLspAdapter::ARGUMENTS,
        )
        .await
    }

    async fn workspace_configuration(
        self: Arc<Self>,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: Option<Uri>,
        cx: &mut AsyncApp,
    ) -> Result<Value> {
        let settings = cx.update(|cx| {
            language_server_settings(delegate.as_ref(), &StylelintLspAdapter::SERVER_NAME, cx)
                .and_then(|s| s.settings.clone())
        });
        Ok(stylelint_configuration(settings))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nests_settings_under_stylelint_section() {
        assert_eq!(stylelint_configuration(None), json!({ "stylelint": {} }));
        assert_eq!(
            stylelint_configuration(Some(json!({ "validate": ["css", "postcss"] }))),
            json!({ "stylelint": { "validate": ["css", "postcss"] } })
        );
    }
}
