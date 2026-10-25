use std::borrow::Cow;
use std::fmt::Write;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use collections::HashMap;
use gpui::App;
use gpui::AsyncApp;
use gpui::Entity;
use gpui::SharedString;
use gpui::Task;
use language::Buffer;
use language::ContextLocation;
use language::ContextProvider;
use language::LanguageName;
use language::LspAdapter;
use language::LspAdapterDelegate;
use language::ManifestName;
use language::ManifestProvider;
use language::ManifestQuery;
use language::Symbol;
use language::language_settings::LanguageSettings;
use lsp::CompletionItemKind;
use lsp::LanguageServerBinary;
use lsp::LanguageServerName;
use lsp::Uri;
use project::lsp_store::language_server_settings;
use serde_json::Value;
use serde_json::json;
use settings::SemanticTokenRules;
use task::TaskTemplate;
use task::TaskTemplates;
use task::VariableName;
use util::paths::PathStyle;
use util::rel_path::RelPath;

pub(crate) fn semantic_token_rules() -> SemanticTokenRules {
    let content =
        grammars::get_file("python/semantic_token_rules.json").expect("missing python/semantic_token_rules.json");
    let json = std::str::from_utf8(&content.data).expect("invalid utf-8 in semantic_token_rules");
    serde_json_lenient::from_str::<SemanticTokenRules>(json).expect("failed to parse python semantic_token_rules.json")
}

/// The project virtual environment, always `.venv` at the worktree root.
const VENV_DIR: &str = ".venv";

/// Directory inside a virtual environment that holds its executables.
const VENV_BIN_DIR: &str = if cfg!(windows) { "Scripts" } else { "bin" };

/// Resolves a Python tool run as a language server: the project's
/// `.venv` first, then the shell `PATH`, then `uvx <tool>`, which runs the
/// latest release from uv's cache, downloading it on first use.
async fn python_tool_binary(
    delegate: &Arc<dyn LspAdapterDelegate>,
    tool: &str,
    arguments: &[&str],
) -> Option<LanguageServerBinary> {
    let env = delegate.shell_env().await;
    let in_venv = delegate.worktree_root_path().join(VENV_DIR).join(VENV_BIN_DIR).join(tool);
    for candidate in [in_venv, PathBuf::from(tool)] {
        if let Some(path) = delegate.which(candidate.as_os_str()).await {
            return Some(LanguageServerBinary {
                path,
                env: Some(env),
                arguments: arguments.iter().map(Into::into).collect(),
            });
        }
    }

    let uvx = delegate.which("uvx".as_ref()).await?;
    Some(LanguageServerBinary {
        path: uvx,
        env: Some(env),
        arguments: std::iter::once(tool).chain(arguments.iter().copied()).map(Into::into).collect(),
    })
}

pub(crate) struct PyprojectTomlManifestProvider;

impl ManifestProvider for PyprojectTomlManifestProvider {
    fn name(&self) -> ManifestName {
        SharedString::new_static("pyproject.toml").into()
    }

    fn search(&self, ManifestQuery { path, depth, delegate }: ManifestQuery) -> Option<Arc<RelPath>> {
        let mut innermost_pyproject = None;
        let mut outermost_workspace_root = None;

        for path in path.ancestors().take(depth) {
            let pyproject_path = path.join(RelPath::from_unix_str("pyproject.toml").unwrap());
            if delegate.exists(&pyproject_path, Some(false)) {
                if innermost_pyproject.is_none() {
                    innermost_pyproject = Some(Arc::from(path));
                }

                let lockfile_path = path.join(RelPath::from_unix_str("uv.lock").unwrap());
                if delegate.exists(&lockfile_path, Some(false)) {
                    outermost_workspace_root = Some(Arc::from(path));
                }
            }
        }

        outermost_workspace_root.or(innermost_pyproject)
    }
}

enum TestRunner {
    Unittest,
    Pytest,
}

impl FromStr for TestRunner {
    type Err = ();

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "unittest" => Ok(Self::Unittest),
            "pytest" => Ok(Self::Pytest),
            _ => Err(()),
        }
    }
}

fn label_for_python_symbol(symbol: &Symbol, language: &Arc<language::Language>) -> Option<language::CodeLabel> {
    let name = &symbol.name;
    let (text, filter_range, display_range) = match symbol.kind {
        language::SymbolKind::Method | language::SymbolKind::Function => {
            let text = format!("def {}():\n", name);
            let filter_range = 4..4 + name.len();
            let display_range = 0..filter_range.end;
            (text, filter_range, display_range)
        }
        language::SymbolKind::Class => {
            let text = format!("class {}:", name);
            let filter_range = 6..6 + name.len();
            let display_range = 0..filter_range.end;
            (text, filter_range, display_range)
        }
        language::SymbolKind::Constant => {
            let text = format!("{} = 0", name);
            let filter_range = 0..name.len();
            let display_range = 0..filter_range.end;
            (text, filter_range, display_range)
        }
        _ => return None,
    };
    Some(language::CodeLabel::new(
        text[display_range.clone()].to_string(),
        filter_range,
        language.highlight_text(text.as_str(), display_range).collect(),
    ))
}

/// Returns the highlight ID for the given completion item kind, if it is supported.
///
/// The outer `Option` is `None` if the item kind returned by the language server is not covered.
/// The inner `Option` is `None` if the item kind is covered, but the highlight name is not present in the grammar.
fn highlight_id_for_completion(
    item_kind: CompletionItemKind,
    grammar: &Arc<language::Grammar>,
) -> Option<Option<language::HighlightId>> {
    match item_kind {
        CompletionItemKind::METHOD => Some(grammar.highlight_id_for_name("function.method.call")),
        CompletionItemKind::FUNCTION => Some(grammar.highlight_id_for_name("function.call")),
        CompletionItemKind::CLASS => Some(grammar.highlight_id_for_name("type")),
        CompletionItemKind::CONSTANT => Some(grammar.highlight_id_for_name("constant")),
        CompletionItemKind::VARIABLE => Some(grammar.highlight_id_for_name("variable")),
        _ => None,
    }
}

pub struct TyLspAdapter;

impl TyLspAdapter {
    const SERVER_NAME: LanguageServerName = LanguageServerName::new_static("ty");
}

#[async_trait(?Send)]
impl LspAdapter for TyLspAdapter {
    fn name(&self) -> LanguageServerName {
        Self::SERVER_NAME
    }

    async fn label_for_completion(
        &self,
        item: &lsp::CompletionItem,
        language: &Arc<language::Language>,
    ) -> Option<language::CodeLabel> {
        let label = &item.label;
        let label_len = label.len();
        let grammar = language.grammar()?;
        let highlight_id = highlight_id_for_completion(item.kind?, grammar)?;

        let mut text = label.clone();
        if let Some(completion_details) = item.label_details.as_ref().and_then(|details| details.detail.as_ref()) {
            write!(&mut text, " {}", completion_details).ok();
        }

        Some(language::CodeLabel::filtered(
            text,
            label_len,
            item.filter_text.as_deref(),
            highlight_id.map(|id| (0..label_len, id)).into_iter().collect(),
        ))
    }

    async fn label_for_symbol(
        &self,
        symbol: &language::Symbol,
        language: &Arc<language::Language>,
    ) -> Option<language::CodeLabel> {
        label_for_python_symbol(symbol, language)
    }

    async fn workspace_configuration(
        self: Arc<Self>,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: Option<Uri>,
        cx: &mut AsyncApp,
    ) -> Result<Value> {
        let settings = cx
            .update(|cx| language_server_settings(delegate.as_ref(), &self.name(), cx).and_then(|s| s.settings.clone()))
            .unwrap_or_else(|| json!({}));
        Ok(json!({ "ty": settings }))
    }

    async fn check_if_user_installed(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        python_tool_binary(delegate, "ty", &["server"]).await
    }
}

pub(crate) struct RuffLspAdapter;

impl RuffLspAdapter {
    const SERVER_NAME: LanguageServerName = LanguageServerName::new_static("ruff");
}

#[async_trait(?Send)]
impl LspAdapter for RuffLspAdapter {
    fn name(&self) -> LanguageServerName {
        Self::SERVER_NAME
    }

    async fn check_if_user_installed(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        python_tool_binary(delegate, "ruff", &["server"]).await
    }
}

pub(crate) struct PythonContextProvider;

const PYTHON_TEST_TARGET_TASK_VARIABLE: VariableName = VariableName::Custom(Cow::Borrowed("PYTHON_TEST_TARGET"));

const PYTHON_MODULE_NAME_TASK_VARIABLE: VariableName = VariableName::Custom(Cow::Borrowed("PYTHON_MODULE_NAME"));

/// Every Python task runs through `uv run python`, so uv picks (and syncs) the
/// project's `.venv`.
fn uv_python_task(label: String, args: impl IntoIterator<Item = String>) -> TaskTemplate {
    TaskTemplate {
        label,
        command: "uv".to_owned(),
        args: ["run".to_owned(), "python".to_owned()].into_iter().chain(args).collect(),
        cwd: Some(VariableName::WorktreeRoot.template_value()),
        ..TaskTemplate::default()
    }
}

impl ContextProvider for PythonContextProvider {
    fn build_context(
        &self,
        variables: &task::TaskVariables,
        location: ContextLocation<'_>,
        _: Option<HashMap<String, String>>,
        cx: &mut gpui::App,
    ) -> Task<Result<task::TaskVariables>> {
        let test_target = match selected_test_runner(Some(&location.file_location.buffer), cx) {
            TestRunner::Unittest => self.build_unittest_target(variables),
            TestRunner::Pytest => self.build_pytest_target(variables),
        };
        let module_target = self.build_module_target(variables);

        Task::ready(Ok(task::TaskVariables::from_iter(test_target.into_iter().chain([module_target]))))
    }

    fn associated_tasks(&self, buffer: Option<Entity<Buffer>>, cx: &App) -> Task<Option<TaskTemplates>> {
        let test_runner = selected_test_runner(buffer.as_ref(), cx);
        let file = VariableName::File.template_value_with_whitespace();
        let test_target = PYTHON_TEST_TARGET_TASK_VARIABLE.template_value_with_whitespace();

        let mut tasks = vec![
            uv_python_task(
                "execute selection".to_owned(),
                ["-c".to_owned(), VariableName::SelectedText.template_value_with_whitespace()],
            ),
            uv_python_task(format!("run '{}'", VariableName::File.template_value()), [file.clone()]),
            TaskTemplate {
                tags: vec!["python-module-main-method".to_owned()],
                ..uv_python_task(
                    format!("run module '{}'", VariableName::File.template_value()),
                    ["-m".to_owned(), PYTHON_MODULE_NAME_TASK_VARIABLE.template_value()],
                )
            },
        ];

        let (runner, target_tags) = match test_runner {
            TestRunner::Unittest => ("unittest", ["python-unittest-class", "python-unittest-method"]),
            TestRunner::Pytest => ("pytest", ["python-pytest-class", "python-pytest-method"]),
        };
        tasks.extend([
            // Run tests for an entire file
            uv_python_task(
                format!("{runner} '{}'", VariableName::File.template_value()),
                ["-m".to_owned(), runner.to_owned(), file],
            ),
            // Run test(s) for a specific target within a file
            TaskTemplate {
                tags: target_tags.map(ToOwned::to_owned).to_vec(),
                ..uv_python_task(
                    format!("{runner} $ZED_CUSTOM_PYTHON_TEST_TARGET"),
                    ["-m".to_owned(), runner.to_owned(), test_target],
                )
            },
        ]);

        Task::ready(Some(TaskTemplates(tasks)))
    }
}

fn selected_test_runner(location: Option<&Entity<Buffer>>, cx: &App) -> TestRunner {
    const TEST_RUNNER_VARIABLE: &str = "TEST_RUNNER";
    let language = LanguageName::new_static("Python");
    let settings = LanguageSettings::resolve(location.map(|b| b.read(cx)), Some(&language), cx);
    settings
        .tasks
        .variables
        .get(TEST_RUNNER_VARIABLE)
        .and_then(|val| TestRunner::from_str(val).ok())
        .unwrap_or(TestRunner::Pytest)
}

impl PythonContextProvider {
    fn build_unittest_target(&self, variables: &task::TaskVariables) -> Option<(VariableName, String)> {
        let python_module_name = python_module_name_from_relative_path(variables.get(&VariableName::RelativeFile)?)?;

        let unittest_class_name = variables.get(&VariableName::Custom(Cow::Borrowed("_unittest_class_name")));

        let unittest_method_name = variables.get(&VariableName::Custom(Cow::Borrowed("_unittest_method_name")));

        let unittest_target_str = match (unittest_class_name, unittest_method_name) {
            (Some(class_name), Some(method_name)) => {
                format!("{python_module_name}.{class_name}.{method_name}")
            }
            (Some(class_name), None) => format!("{python_module_name}.{class_name}"),
            (None, None) => python_module_name,
            // should never happen, a TestCase class is the unit of testing
            (None, Some(_)) => return None,
        };

        Some((PYTHON_TEST_TARGET_TASK_VARIABLE.clone(), unittest_target_str))
    }

    fn build_pytest_target(&self, variables: &task::TaskVariables) -> Option<(VariableName, String)> {
        let file_path = variables.get(&VariableName::RelativeFile)?;

        let pytest_class_name = variables.get(&VariableName::Custom(Cow::Borrowed("_pytest_class_name")));

        let pytest_method_name = variables.get(&VariableName::Custom(Cow::Borrowed("_pytest_method_name")));

        let pytest_target_str = match (pytest_class_name, pytest_method_name) {
            (Some(class_name), Some(method_name)) => {
                format!("{file_path}::{class_name}::{method_name}")
            }
            (Some(class_name), None) => {
                format!("{file_path}::{class_name}")
            }
            (None, Some(method_name)) => {
                format!("{file_path}::{method_name}")
            }
            (None, None) => file_path.to_string(),
        };

        Some((PYTHON_TEST_TARGET_TASK_VARIABLE.clone(), pytest_target_str))
    }

    fn build_module_target(&self, variables: &task::TaskVariables) -> (VariableName, String) {
        let python_module_name = variables
            .get(&VariableName::RelativeFile)
            .and_then(python_module_name_from_relative_path)
            .unwrap_or_default();

        (PYTHON_MODULE_NAME_TASK_VARIABLE.clone(), python_module_name)
    }
}

fn python_module_name_from_relative_path(relative_path: &str) -> Option<String> {
    let rel_path = RelPath::new(relative_path.as_ref(), PathStyle::local()).ok()?;
    let path_with_dots = rel_path.display(PathStyle::Unix).replace('/', ".");
    Some(path_with_dots.strip_suffix(".py").map(ToOwned::to_owned).unwrap_or(path_with_dots))
}
