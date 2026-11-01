use std::path::Path;
use std::path::PathBuf;
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
use project::Fs;
use project::lsp_store::language_server_settings;
use serde_json::Value;
use serde_json::json;

const CONFIG_FILES: &[&str] = &["biome.json", "biome.jsonc"];
const SKIPPED_DIRS: &[&str] = &["node_modules", "target", "dist", "build"];
const MAX_CONFIG_SEARCH_DEPTH: usize = 3;

/// The directory to run Biome from when the worktree root has no config of its own:
/// the shallowest descendant holding `biome.json(c)` (skipping `node_modules`,
/// hidden and build directories). `None` when the root, or an ancestor of it, has a
/// config, or when there is none below.
fn find_config_dir(root: &Path) -> Option<PathBuf> {
    let has_config = |dir: &Path| CONFIG_FILES.iter().any(|name| dir.join(name).is_file());
    if root.ancestors().any(has_config) {
        return None;
    }
    let mut level = vec![root.to_path_buf()];
    for _ in 0..MAX_CONFIG_SEARCH_DEPTH {
        let mut next = Vec::new();
        for dir in &level {
            let Ok(entries) = std::fs::read_dir(dir) else { continue };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with('.') || SKIPPED_DIRS.contains(&name.as_ref()) {
                    continue;
                }
                let path = entry.path();
                if path.is_dir() {
                    next.push(path);
                }
            }
        }
        next.sort();
        if let Some(found) = next.iter().find(|dir| has_config(dir)) {
            return Some(found.clone());
        }
        level = next;
    }
    None
}

/// Biome's language server (`biome lsp-proxy`): lint diagnostics and code actions
/// (quick fixes, `source.fixAll.biome`, `source.organizeImports.biome`).
/// Formatting stays on the Biome CLI (see `project::biome`).
pub struct BiomeLspAdapter {
    fs: Arc<dyn Fs>,
}

impl BiomeLspAdapter {
    const SERVER_NAME: LanguageServerName = LanguageServerName::new_static("biome");
    const NPX_PACKAGE: &str = "@biomejs/biome";
    const ARGUMENTS: &[&str] = &["lsp-proxy"];

    pub fn new(fs: Arc<dyn Fs>) -> Self {
        BiomeLspAdapter { fs }
    }
}

#[async_trait(?Send)]
impl LspAdapter for BiomeLspAdapter {
    fn name(&self) -> LanguageServerName {
        Self::SERVER_NAME
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
        let worktree_root = delegate.worktree_root_path().to_path_buf();
        let config_dir = smol::unblock({
            let worktree_root = worktree_root.clone();
            move || find_config_dir(&worktree_root)
        })
        .await;
        let mut arguments: Vec<std::ffi::OsString> = Self::ARGUMENTS.iter().map(Into::into).collect();
        if let Some(config_dir) = &config_dir {
            arguments.push(format!("--config-path={}", config_dir.display()).into());
        }

        for dir in config_dir.as_deref().unwrap_or(&worktree_root).ancestors() {
            let path = dir.join("node_modules").join(".bin").join("biome");
            if self.fs.is_file(&path).await {
                return Some(LanguageServerBinary {
                    path,
                    env: Some(delegate.shell_env().await),
                    arguments,
                });
            }
        }
        let mut binary =
            crate::npx::node_server_binary(delegate, &[Self::NPX_PACKAGE], "biome", Self::ARGUMENTS).await?;
        binary.arguments.extend(arguments.into_iter().skip(Self::ARGUMENTS.len()));
        Some(binary)
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
            (LanguageName::new_static("JSON"), "json".into()),
            (LanguageName::new_static("JSONC"), "jsonc".into()),
            (LanguageName::new_static("CSS"), "css".into()),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("biome-adapter-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn finds_shallowest_nested_config() {
        let root = temp_dir("nested");
        std::fs::create_dir_all(root.join("framework_fe/node_modules/pkg")).unwrap();
        std::fs::create_dir_all(root.join("a/b/c")).unwrap();
        std::fs::write(root.join("framework_fe/biome.json"), "{}").unwrap();
        std::fs::write(root.join("framework_fe/node_modules/pkg/biome.json"), "{}").unwrap();
        std::fs::write(root.join("a/b/c/biome.jsonc"), "{}").unwrap();
        assert_eq!(find_config_dir(&root), Some(root.join("framework_fe")));
    }

    #[test]
    fn ignores_skipped_and_too_deep_configs() {
        let root = temp_dir("skipped");
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        std::fs::create_dir_all(root.join("a/b/c/d")).unwrap();
        std::fs::write(root.join("node_modules/pkg/biome.json"), "{}").unwrap();
        std::fs::write(root.join("a/b/c/d/biome.json"), "{}").unwrap();
        assert_eq!(find_config_dir(&root), None);
    }

    #[test]
    fn none_when_root_has_config() {
        let root = temp_dir("root");
        std::fs::create_dir_all(root.join("pkg")).unwrap();
        std::fs::write(root.join("biome.jsonc"), "{}").unwrap();
        std::fs::write(root.join("pkg/biome.json"), "{}").unwrap();
        assert_eq!(find_config_dir(&root), None);
    }
}
