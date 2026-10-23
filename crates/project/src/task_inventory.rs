//! Lookup of the task templates languages provide for runnables (the gutter run icon).

use std::sync::Arc;

use anyhow::Result;
use collections::HashMap;
use gpui::App;
use gpui::AppContext as _;
use gpui::Entity;
use gpui::SharedString;
use gpui::Task;
use language::Buffer;
use language::ContextLocation;
use language::ContextProvider;
use language::Language;
use language::language_settings::LanguageSettings;
use lsp::LanguageServerId;
use task::TaskTemplate;
use task::TaskTemplates;
use task::TaskVariables;
use task::VariableName;
use text::Point;
use text::ToPoint;

use crate::git_store::GitStore;
use crate::worktree_store::WorktreeStore;

/// Kind of a source the tasks are fetched from, used to build unique task ids.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TaskSourceKind {
    /// Language-specific tasks, matched against tree-sitter runnable tags.
    Language { name: SharedString },
    /// Language-specific tasks coming from LSP servers.
    Lsp { language_name: SharedString, server: LanguageServerId },
}

impl TaskSourceKind {
    pub fn to_id_base(&self) -> String {
        match self {
            Self::Language { name } => format!("language_{name}"),
            Self::Lsp { server, language_name } => format!("lsp_{language_name}_{server}"),
        }
    }
}

/// Returns the task templates the buffer's language provides, if tasks are enabled for it.
pub fn language_task_templates(
    buffer: Option<Entity<Buffer>>,
    language: Arc<Language>,
    cx: &App,
) -> Task<Vec<(TaskSourceKind, TaskTemplate)>> {
    let enabled =
        LanguageSettings::resolve(buffer.as_ref().map(|b| b.read(cx)), Some(&language.name()), cx).tasks.enabled;
    let Some(provider) = language.context_provider().filter(|_| enabled) else {
        return Task::ready(Vec::new());
    };
    let kind = TaskSourceKind::Language { name: language.name().into() };
    let tasks = provider.associated_tasks(buffer, cx);
    cx.background_spawn(async move {
        tasks.await.into_iter().flat_map(|templates| templates.0).map(|template| (kind.clone(), template)).collect()
    })
}

/// A context provided that tries to provide values for all non-custom [`VariableName`] variants for a currently opened file.
/// Applied as a base for every custom [`ContextProvider`] unless explicitly oped out.
pub struct BasicContextProvider {
    worktree_store: Entity<WorktreeStore>,
    git_store: Entity<GitStore>,
}

impl BasicContextProvider {
    pub const fn new(worktree_store: Entity<WorktreeStore>, git_store: Entity<GitStore>) -> Self {
        Self { worktree_store, git_store }
    }
}

impl ContextProvider for BasicContextProvider {
    fn build_context(
        &self,
        _: &TaskVariables,
        location: ContextLocation<'_>,
        _: Option<HashMap<String, String>>,
        cx: &mut App,
    ) -> Task<Result<TaskVariables>> {
        let location = location.file_location;
        let buffer = location.buffer.read(cx);
        let buffer_snapshot = buffer.snapshot();
        let symbols = buffer_snapshot.symbols_containing(location.range.start, None);
        let symbol = symbols.last().map(|symbol| {
            let range = symbol.name_ranges.last().cloned().unwrap_or(0..symbol.text.len());
            symbol.text[range].to_string()
        });

        let current_file = buffer.file().and_then(|file| file.as_local());
        let Point { row, column } = location.range.start.to_point(&buffer_snapshot);
        let row = row + 1;
        let column = column + 1;
        let selected_text = buffer.chars_for_range(location.range.clone()).collect::<String>();

        let mut task_variables = TaskVariables::from_iter([
            (VariableName::Row, row.to_string()),
            (VariableName::Column, column.to_string()),
        ]);

        if let Some(symbol) = symbol {
            task_variables.insert(VariableName::Symbol, symbol);
        }
        if !selected_text.trim().is_empty() {
            task_variables.insert(VariableName::SelectedText, selected_text);
        }
        let worktree = buffer
            .file()
            .map(|file| file.worktree_id(cx))
            .and_then(|worktree_id| self.worktree_store.read(cx).worktree_for_id(worktree_id, cx));

        if let Some(worktree) = worktree {
            let worktree = worktree.read(cx);
            let path_style = worktree.path_style();
            task_variables.insert(VariableName::WorktreeRoot, worktree.abs_path().to_string_lossy().into_owned());
            if let Some(current_file) = current_file.as_ref() {
                let relative_path = current_file.path();
                task_variables.insert(VariableName::RelativeFile, relative_path.display(path_style).to_string());
                if let Some(relative_dir) = relative_path.parent() {
                    task_variables.insert(
                        VariableName::RelativeDir,
                        if relative_dir.is_empty() {
                            String::from(".")
                        } else {
                            relative_dir.display(path_style).to_string()
                        },
                    );
                }
            }
        }

        if let Some(worktree_id) = location.buffer.read(cx).file().map(|f| f.worktree_id(cx))
            && let Some(path) = self.git_store.read(cx).original_repo_path_for_worktree(worktree_id, cx)
        {
            task_variables.insert(VariableName::MainGitWorktree, path.to_string_lossy().into_owned());
        }

        if let Some(current_file) = current_file {
            let path = current_file.abs_path(cx);
            if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
                task_variables.insert(VariableName::Filename, String::from(filename));
            }

            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                task_variables.insert(VariableName::Stem, stem.into());
            }

            if let Some(dirname) = path.parent().and_then(|s| s.to_str()) {
                task_variables.insert(VariableName::Dirname, dirname.into());
            }

            task_variables.insert(VariableName::File, path.to_string_lossy().into_owned());
        }

        if let Some(language) = buffer.language() {
            task_variables.insert(VariableName::Language, language.name().to_string());
        }

        Task::ready(Ok(task_variables))
    }
}

/// A ContextProvider that doesn't provide any task variables on it's own, though it has some associated tasks.
pub struct ContextProviderWithTasks {
    templates: TaskTemplates,
}

impl ContextProviderWithTasks {
    pub const fn new(definitions: TaskTemplates) -> Self {
        Self { templates: definitions }
    }
}

impl ContextProvider for ContextProviderWithTasks {
    fn associated_tasks(&self, _: Option<Entity<Buffer>>, _: &App) -> Task<Option<TaskTemplates>> {
        Task::ready(Some(self.templates.clone()))
    }
}
