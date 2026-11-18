use std::borrow::Cow;
use std::num::NonZeroU32;
use std::str;
use std::sync::Arc;

use anyhow::Result;
use gpui::App;
use gpui::Entity;
use gpui::SharedString;
use gpui::Task;
use language::Buffer;
use language::ContextLocation;
use language::ContextProvider;
use language::ManifestName;
use language::ManifestProvider;
use language::ManifestQuery;
use language::language_settings::LanguageSettings;
use language::language_settings::RewrapBehavior;
use language::language_settings::WordsCompletionMode;
use rustc_hash::FxHashMap;
use task::TaskTemplate;
use task::TaskTemplates;
use task::VariableName;
use util::rel_path::RelPath;

use crate::LanguageDef;

pub(crate) struct PyprojectTomlManifestProvider;

impl ManifestProvider for PyprojectTomlManifestProvider {
    fn name(&self) -> ManifestName {
        SharedString::new_static("pyproject.toml").into()
    }

    fn search(&self, ManifestQuery { path, depth, delegate }: ManifestQuery) -> Option<Arc<RelPath>> {
        let mut innermost_pyproject = None;
        let mut outermost_workspace_root = None;

        for path in path.ancestors().take(depth) {
            let pyproject_path = path.join(RelPath::from_unix_str("pyproject.toml").expect("valid relative path"));
            if delegate.exists(&pyproject_path, Some(false)) {
                if innermost_pyproject.is_none() {
                    innermost_pyproject = Some(Arc::from(path));
                }

                let lockfile_path = path.join(RelPath::from_unix_str("uv.lock").expect("valid relative path"));
                if delegate.exists(&lockfile_path, Some(false)) {
                    outermost_workspace_root = Some(Arc::from(path));
                }
            }
        }

        outermost_workspace_root.or(innermost_pyproject)
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
        _: ContextLocation<'_>,
        _: Option<FxHashMap<String, String>>,
        _: &mut gpui::App,
    ) -> Task<Result<task::TaskVariables>> {
        let test_target = self.build_pytest_target(variables);
        let module_target = self.build_module_target(variables);

        Task::ready(Ok(task::TaskVariables::from_iter(test_target.into_iter().chain([module_target]))))
    }

    fn associated_tasks(&self, _: Option<Entity<Buffer>>, _: &App) -> Task<Option<TaskTemplates>> {
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

        let runner = "pytest";
        let target_tags = ["python-pytest-class", "python-pytest-method"];
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
                    format!("{runner} $ED_CUSTOM_PYTHON_TEST_TARGET"),
                    ["-m".to_owned(), runner.to_owned(), test_target],
                )
            },
        ]);

        Task::ready(Some(TaskTemplates(tasks)))
    }
}

impl PythonContextProvider {
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
            (None, None) => file_path.to_owned(),
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
    let rel_path = RelPath::new(relative_path.as_ref()).ok()?;
    let path_with_dots = rel_path.display().replace('/', ".");
    Some(path_with_dots.strip_suffix(".py").map(ToOwned::to_owned).unwrap_or(path_with_dots))
}

/// Settings for Python.
pub(crate) fn settings() -> LanguageSettings {
    LanguageSettings {
        tab_size: NonZeroU32::new(4).expect("non-zero"),
        hard_tabs: false,
        soft_wrap: false,
        preferred_line_length: 120,
        wrap_guides: vec![120],
        lsp_document_symbols: true,
        allow_rewrap: RewrapBehavior::InComments,
        use_on_type_format: true,
        auto_indent_on_paste: true,
        word_completions: WordsCompletionMode::Fallback,
    }
}

pub(crate) fn languages() -> Vec<LanguageDef> {
    vec![LanguageDef {
        grammar: Some(tree_sitter_python::LANGUAGE.into()),
                context: Some(Arc::new(PythonContextProvider)),
        manifest_name: Some(SharedString::new_static("pyproject.toml").into()),
        ..LanguageDef::new("python", settings())
    }]
}
