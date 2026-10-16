use anyhow::Result;
use async_trait::async_trait;
use collections::HashMap;
use gpui::AsyncApp;
use language::{LanguageName, LspAdapter, LspAdapterDelegate, Toolchain};
use lsp::{LanguageServerBinary, LanguageServerName, Uri};
use project::lsp_store::language_server_settings;
use serde_json::{Value, json};
use std::sync::Arc;

pub struct TailwindLspAdapter;

impl TailwindLspAdapter {
    const SERVER_NAME: LanguageServerName =
        LanguageServerName::new_static("tailwindcss-language-server");
}

#[async_trait(?Send)]
impl LspAdapter for TailwindLspAdapter {
    fn name(&self) -> LanguageServerName {
        Self::SERVER_NAME
    }

    async fn check_if_user_installed(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: Option<Toolchain>,
        _: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        let path = delegate.which(Self::SERVER_NAME.as_ref()).await?;
        let env = delegate.shell_env().await;

        Some(LanguageServerBinary {
            path,
            env: Some(env),
            arguments: vec!["--stdio".into()],
        })
    }

    async fn initialization_options(
        self: Arc<Self>,
        _: &Arc<dyn LspAdapterDelegate>,
        _: &mut AsyncApp,
    ) -> Result<Option<serde_json::Value>> {
        Ok(Some(json!({
            "provideFormatter": true,
        })))
    }

    async fn workspace_configuration(
        self: Arc<Self>,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: Option<Toolchain>,
        _: Option<Uri>,
        cx: &mut AsyncApp,
    ) -> Result<Value> {
        let mut tailwind_user_settings = cx.update(|cx| {
            language_server_settings(delegate.as_ref(), &Self::SERVER_NAME, cx)
                .and_then(|s| s.settings.clone())
                .unwrap_or_default()
        });

        if tailwind_user_settings.get("emmetCompletions").is_none() {
            tailwind_user_settings["emmetCompletions"] = Value::Bool(true);
        }

        if tailwind_user_settings.get("includeLanguages").is_none() {
            tailwind_user_settings["includeLanguages"] = json!({
                "html": "html",
                "css": "css",
                "javascript": "javascript",
                "typescript": "typescript",
                "typescriptreact": "typescriptreact",
            });
        }

        Ok(json!({
            "tailwindCSS": tailwind_user_settings
        }))
    }

    fn language_ids(&self) -> HashMap<LanguageName, String> {
        HashMap::from_iter([
            (LanguageName::new_static("Astro"), "astro".to_string()),
            (LanguageName::new_static("HTML"), "html".to_string()),
            (LanguageName::new_static("Gleam"), "html".to_string()),
            (LanguageName::new_static("CSS"), "css".to_string()),
            (
                LanguageName::new_static("JavaScript"),
                "javascript".to_string(),
            ),
            (
                LanguageName::new_static("TypeScript"),
                "typescript".to_string(),
            ),
            (
                LanguageName::new_static("TSX"),
                "typescriptreact".to_string(),
            ),
            (LanguageName::new_static("Svelte"), "svelte".to_string()),
            (LanguageName::new_static("Elixir"), "elixir".to_string()),
            (LanguageName::new_static("HEEx"), "heex".to_string()),
            (LanguageName::new_static("ERB"), "erb".to_string()),
            (LanguageName::new_static("HTML+ERB"), "erb".to_string()),
            (LanguageName::new_static("PHP"), "php".to_string()),
            (LanguageName::new_static("Vue.js"), "vue".to_string()),
        ])
    }
}
