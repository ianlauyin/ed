use std::sync::Arc;

use async_trait::async_trait;
use gpui::AsyncApp;
use language::LanguageName;
use language::LspAdapter;
use language::LspAdapterDelegate;
use lsp::LanguageServerBinary;
use lsp::LanguageServerName;
use project::Fs;
use rustc_hash::FxHashMap;

use crate::npx;

/// Biome's language server (`biome lsp-proxy`): lint diagnostics and code actions
/// (quick fixes, `source.fixAll.biome`, `source.organizeImports.biome`).
/// Formatting stays on the Biome CLI (see `project::biome`).
pub(crate) struct BiomeLspAdapter {
    fs: Arc<dyn Fs>,
}

impl BiomeLspAdapter {
    const SERVER_NAME: LanguageServerName = LanguageServerName::new_static("biome");
    const NPX_PACKAGE: &str = "@biomejs/biome";
    const ARGUMENTS: &[&str] = &["lsp-proxy"];

    pub(crate) fn new(fs: Arc<dyn Fs>) -> Self {
        BiomeLspAdapter { fs }
    }
}

#[async_trait(?Send)]
impl LspAdapter for BiomeLspAdapter {
    fn name(&self) -> LanguageServerName {
        Self::SERVER_NAME
    }

    fn lint_switch(&self) -> Option<(&'static str, bool)> {
        Some(language::BIOME)
    }

    fn binary_name(&self) -> String {
        "biome".into()
    }

    /// Resolves Biome like the formatter does: the nearest `node_modules/.bin/biome`
    /// at or above the worktree root, `biome` on `PATH`, then npx.
    async fn check_if_user_installed(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        for dir in delegate.worktree_root_path().ancestors() {
            let path = dir.join("node_modules").join(".bin").join("biome");
            if self.fs.is_file(&path).await {
                return Some(LanguageServerBinary {
                    path,
                    env: Some(delegate.shell_env().await),
                    arguments: Self::ARGUMENTS.iter().map(Into::into).collect(),
                });
            }
        }
        npx::node_server_binary(delegate, &[Self::NPX_PACKAGE], "biome", Self::ARGUMENTS).await
    }

    fn language_ids(&self) -> FxHashMap<LanguageName, String> {
        FxHashMap::from_iter([
            (LanguageName::new_static("TypeScript"), "typescript".into()),
            (LanguageName::new_static("JavaScript"), "javascript".into()),
            (LanguageName::new_static("TSX"), "typescriptreact".into()),
            (LanguageName::new_static("JSON"), "json".into()),
            (LanguageName::new_static("JSONC"), "jsonc".into()),
            (LanguageName::new_static("CSS"), "css".into()),
        ])
    }
}
