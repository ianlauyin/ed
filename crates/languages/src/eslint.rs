use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use gpui::AsyncApp;
use language::LspAdapter;
use language::LspAdapterDelegate;
use lsp::CodeActionKind;
use lsp::LanguageServerBinary;
use lsp::LanguageServerName;
use lsp::Uri;
use project::lsp_store::language_server_settings;
use serde_json::Value;
use serde_json::json;
use util::merge_json_value_into;

/// ESLint's language server (`vscode-eslint-language-server --stdio`): lint
/// diagnostics, quick fixes and `source.fixAll.eslint`. It lints with the `eslint`
/// installed in the project and does nothing without one.
pub struct EsLintLspAdapter;

impl EsLintLspAdapter {
    const SERVER_NAME: LanguageServerName = LanguageServerName::new_static("eslint");
    const NPX_PACKAGE: &str = "vscode-langservers-extracted";
    const BIN: &str = "vscode-eslint-language-server";
    const ARGUMENTS: &[&str] = &["--stdio"];
}

/// The server asks for its options per document (section `""`) and needs the
/// workspace folder. User settings from `lsp.eslint.settings` are merged on top.
fn eslint_configuration(worktree_root: &Path, settings: Option<Value>) -> Value {
    let mut configuration = json!({
        "validate": "on",
        "run": "onType",
        "nodePath": null,
        "rulesCustomizations": [],
        "workingDirectory": { "mode": "auto" },
        "workspaceFolder": {
            "uri": Uri::from_file_path(worktree_root)
                .map(|uri| uri.as_str().to_owned())
                .unwrap_or_default(),
            "name": worktree_root
                .file_name()
                .unwrap_or(worktree_root.as_os_str())
                .to_string_lossy(),
        },
        "problems": {},
        "codeActionOnSave": { "enable": true },
        "codeAction": {
            "disableRuleComment": { "enable": true, "location": "separateLine" },
            "showDocumentation": { "enable": true },
        },
    });
    if let Some(settings) = settings {
        merge_json_value_into(settings, &mut configuration);
    }
    json!({ "": configuration })
}

#[async_trait(?Send)]
impl LspAdapter for EsLintLspAdapter {
    fn name(&self) -> LanguageServerName {
        EsLintLspAdapter::SERVER_NAME
    }

    fn binary_name(&self) -> String {
        EsLintLspAdapter::BIN.into()
    }

    fn code_action_kinds(&self) -> Option<Vec<CodeActionKind>> {
        Some(vec![CodeActionKind::QUICKFIX, CodeActionKind::new("source.fixAll.eslint")])
    }

    /// `vscode-eslint-language-server` on the shell `PATH`, then npx.
    async fn check_if_user_installed(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        crate::npx::node_server_binary(
            delegate,
            &[EsLintLspAdapter::NPX_PACKAGE],
            EsLintLspAdapter::BIN,
            EsLintLspAdapter::ARGUMENTS,
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
            language_server_settings(delegate.as_ref(), &EsLintLspAdapter::SERVER_NAME, cx)
                .and_then(|s| s.settings.clone())
        });
        Ok(eslint_configuration(delegate.worktree_root_path(), settings))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_name_the_workspace_folder() {
        let configuration = eslint_configuration(Path::new("/work/app"), None);
        let section = &configuration[""];
        assert_eq!(section["validate"], json!("on"));
        assert_eq!(section["workspaceFolder"]["name"], json!("app"));
        assert_eq!(section["workspaceFolder"]["uri"], json!("file:///work/app"));
    }

    #[test]
    fn user_settings_override_defaults() {
        let configuration =
            eslint_configuration(Path::new("/work/app"), Some(json!({ "run": "onSave", "useFlatConfig": true })));
        assert_eq!(configuration[""]["run"], json!("onSave"));
        assert_eq!(configuration[""]["useFlatConfig"], json!(true));
        assert_eq!(configuration[""]["validate"], json!("on"));
    }
}
