use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use gpui::AsyncApp;
use language::LanguageName;
use language::LspAdapter;
use language::LspAdapterDelegate;
use language::ServerNotInstalled;
use lsp::LanguageServerBinary;
use lsp::LanguageServerName;
use project::Fs;
use rustc_hash::FxHashMap;
use serde_json::Value;

use crate::npx;

/// The TypeScript 7 native language server (`tsc --lsp --stdio`), for TypeScript,
/// TSX and JavaScript. Older TypeScript versions are not supported.
pub(crate) struct TsgoLspAdapter {
    fs: Arc<dyn Fs>,
}

impl TsgoLspAdapter {
    const SERVER_NAME: LanguageServerName = LanguageServerName::new_static("tsgo");
    const MIN_MAJOR_VERSION: u64 = 7;
    const NPX_PACKAGE: &str = "typescript@^7";
    const ARGUMENTS: &[&str] = &["--lsp", "--stdio"];

    pub(crate) fn new(fs: Arc<dyn Fs>) -> Self {
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
    bin: PathBuf,
}

fn major_version(version: &str) -> Option<u64> {
    version.split('.').next()?.parse().ok()
}

fn is_supported_version(version: &str) -> bool {
    major_version(version).is_some_and(|major| major >= TsgoLspAdapter::MIN_MAJOR_VERSION)
}

#[async_trait(?Send)]
impl LspAdapter for TsgoLspAdapter {
    fn name(&self) -> LanguageServerName {
        Self::SERVER_NAME
    }

    // TODO: remove the npx fallback for projects with TypeScript < 7 once they upgrade.
    /// Uses the project's `typescript` when it is 7 or newer. Projects without
    /// `typescript`, or with an older one, run the latest 7.x through npx and leave
    /// the project untouched.
    async fn find_binary(&self, delegate: &Arc<dyn LspAdapterDelegate>, _: &AsyncApp) -> Result<LanguageServerBinary> {
        if let Some(installed) = self.installed_typescript(delegate.worktree_root_path()).await {
            if is_supported_version(&installed.version) {
                return Ok(LanguageServerBinary {
                    path: installed.bin,
                    env: Some(delegate.shell_env().await),
                    arguments: Self::ARGUMENTS.iter().map(Into::into).collect(),
                });
            }
        }

        npx::npx_binary(delegate, &[Self::NPX_PACKAGE], "tsc", Self::ARGUMENTS).await.ok_or_else(|| {
            ServerNotInstalled(format!(
                "language server {} needs TypeScript {} or newer: the project has none \
                    (or an older one) and `npx` was not found on your PATH",
                Self::SERVER_NAME,
                Self::MIN_MAJOR_VERSION,
            ))
            .into()
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

    fn language_ids(&self) -> FxHashMap<LanguageName, String> {
        FxHashMap::from_iter([
            (LanguageName::new_static("TypeScript"), "typescript".into()),
            (LanguageName::new_static("JavaScript"), "javascript".into()),
            (LanguageName::new_static("TSX"), "typescriptreact".into()),
        ])
    }
}

#[cfg(test)]
mod tests {

    use super::is_supported_version;
    use super::major_version;

    #[test]
    fn parses_major_version() {
        assert_eq!(major_version("7.0.2"), Some(7));
        assert_eq!(major_version("7.1.0-dev.20261019.1"), Some(7));
        assert_eq!(major_version("5.9.3"), Some(5));
        assert_eq!(major_version(""), None);
    }

    #[test]
    fn only_typescript_7_or_newer_is_supported() {
        assert!(is_supported_version("7.0.2"));
        assert!(is_supported_version("8.0.0"));
        assert!(!is_supported_version("6.0.3"));
        assert!(!is_supported_version(""));
    }
}
