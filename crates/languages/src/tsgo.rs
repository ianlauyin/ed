use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
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
use project::Fs;
use project::lsp_store::language_server_settings;
use serde_json::Value;
use serde_json::json;

/// The TypeScript 7 native language server (`tsc --lsp --stdio`), for TypeScript,
/// TSX and JavaScript. Older TypeScript versions are not supported.
pub struct TsgoLspAdapter {
    fs: Arc<dyn Fs>,
}

impl TsgoLspAdapter {
    const SERVER_NAME: LanguageServerName = LanguageServerName::new_static("tsgo");
    const MIN_MAJOR_VERSION: u64 = 7;
    const NPX_PACKAGE: &str = "typescript@^7";
    const ARGUMENTS: &[&str] = &["--lsp", "--stdio"];

    pub fn new(fs: Arc<dyn Fs>) -> Self {
        TsgoLspAdapter { fs }
    }

    /// The nearest `node_modules/typescript` at or above the worktree root.
    async fn installed_typescript(&self, worktree_root: &Path) -> Option<InstalledTypeScript> {
        for dir in worktree_root.ancestors() {
            let package_dir = dir.join("node_modules").join("typescript");
            let Ok(package_json) = self.fs.load(&package_dir.join("package.json")).await else {
                continue;
            };
            let version = serde_json::from_str::<Value>(&package_json)
                .ok()
                .and_then(|json| json.get("version")?.as_str().map(str::to_owned))
                .unwrap_or_default();
            let bin_link = dir.join("node_modules").join(".bin").join("tsc");
            let bin = if self.fs.is_file(&bin_link).await { bin_link } else { package_dir.join("bin").join("tsc") };
            return Some(InstalledTypeScript { version, bin });
        }
        None
    }
}

struct InstalledTypeScript {
    version: String,
    bin: std::path::PathBuf,
}

fn major_version(version: &str) -> Option<u64> {
    version.split('.').next()?.parse().ok()
}

#[async_trait(?Send)]
impl LspAdapter for TsgoLspAdapter {
    fn name(&self) -> LanguageServerName {
        Self::SERVER_NAME
    }

    /// Uses the project's `typescript` when it is 7 or newer and refuses to start for
    /// older versions. Projects without `typescript` run the latest 7.x through npx.
    async fn find_binary(&self, delegate: &Arc<dyn LspAdapterDelegate>, _: &AsyncApp) -> Result<LanguageServerBinary> {
        if let Some(installed) = self.installed_typescript(delegate.worktree_root_path()).await {
            let supported = major_version(&installed.version).is_some_and(|major| major >= Self::MIN_MAJOR_VERSION);
            anyhow::ensure!(
                supported,
                "{} requires TypeScript {} or newer, but the project has typescript {} \
                (at {})",
                Self::SERVER_NAME,
                Self::MIN_MAJOR_VERSION,
                installed.version,
                installed.bin.display(),
            );
            return Ok(LanguageServerBinary {
                path: installed.bin,
                env: Some(delegate.shell_env().await),
                arguments: Self::ARGUMENTS.iter().map(Into::into).collect(),
            });
        }

        crate::npx::npx_binary(delegate, &[Self::NPX_PACKAGE], "tsc", Self::ARGUMENTS).await.with_context(|| {
            format!(
                "language server {} is not installed: `node_modules/.bin/tsc` was not \
                    found in the project, and `npx` was not found on your PATH",
                Self::SERVER_NAME,
            )
        })
    }

    async fn label_for_completion(
        &self,
        item: &lsp::CompletionItem,
        language: &Arc<language::Language>,
    ) -> Option<language::CodeLabel> {
        use lsp::CompletionItemKind as Kind;
        let label_len = item.label.len();
        let grammar = language.grammar()?;
        let highlight_id = match item.kind? {
            Kind::CLASS | Kind::INTERFACE | Kind::ENUM => grammar.highlight_id_for_name("type"),
            Kind::CONSTRUCTOR => grammar.highlight_id_for_name("type"),
            Kind::CONSTANT => grammar.highlight_id_for_name("constant"),
            Kind::FUNCTION | Kind::METHOD => grammar.highlight_id_for_name("function"),
            Kind::PROPERTY | Kind::FIELD => grammar.highlight_id_for_name("property"),
            Kind::VARIABLE => grammar.highlight_id_for_name("variable"),
            _ => None,
        }?;

        let text = if let Some(description) =
            item.label_details.as_ref().and_then(|label_details| label_details.description.as_ref())
        {
            format!("{} {}", item.label, description)
        } else if let Some(detail) = &item.detail {
            format!("{} {}", item.label, detail)
        } else {
            item.label.clone()
        };
        Some(language::CodeLabel::filtered(
            text,
            label_len,
            item.filter_text.as_deref(),
            vec![(0..label_len, highlight_id)],
        ))
    }

    async fn workspace_configuration(
        self: Arc<Self>,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: Option<Uri>,
        cx: &mut AsyncApp,
    ) -> Result<Value> {
        let settings = cx.update(|cx| {
            language_server_settings(delegate.as_ref(), &Self::SERVER_NAME, cx).and_then(|s| s.settings.clone())
        });
        Ok(settings.unwrap_or_else(|| json!({})))
    }

    fn language_ids(&self) -> HashMap<LanguageName, String> {
        HashMap::from_iter([
            (LanguageName::new_static("TypeScript"), "typescript".into()),
            (LanguageName::new_static("JavaScript"), "javascript".into()),
            (LanguageName::new_static("TSX"), "typescriptreact".into()),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_major_version() {
        assert_eq!(major_version("7.0.2"), Some(7));
        assert_eq!(major_version("7.1.0-dev.20261019.1"), Some(7));
        assert_eq!(major_version("5.9.3"), Some(5));
        assert_eq!(major_version(""), None);
    }
}
