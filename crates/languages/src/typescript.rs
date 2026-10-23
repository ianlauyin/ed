use std::borrow::Cow;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::LazyLock;

use anyhow::Context as _;
use anyhow::Result;
use collections::HashMap;
use futures::future::join_all;
use gpui::App;
use gpui::AppContext;
use gpui::Entity;
use gpui::Task;
use itertools::Itertools as _;
use language::Buffer;
use language::ContextLocation;
use language::ContextProvider;
use language::File;
use project::Fs;
use smol::lock::RwLock;
use task::TaskTemplate;
use task::TaskTemplates;
use task::VariableName;
use util::ResultExt;
use util::rel_path::RelPath;

use crate::PackageJson;
use crate::PackageJsonData;

pub(crate) struct TypeScriptContextProvider {
    fs: Arc<dyn Fs>,
    last_package_json: PackageJsonContents,
}

const TYPESCRIPT_RUNNER_VARIABLE: VariableName = VariableName::Custom(Cow::Borrowed("TYPESCRIPT_RUNNER"));

const TYPESCRIPT_JEST_TEST_NAME_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("TYPESCRIPT_JEST_TEST_NAME"));

const TYPESCRIPT_VITEST_TEST_NAME_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("TYPESCRIPT_VITEST_TEST_NAME"));

const TYPESCRIPT_JEST_PACKAGE_PATH_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("TYPESCRIPT_JEST_PACKAGE_PATH"));

const TYPESCRIPT_MOCHA_PACKAGE_PATH_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("TYPESCRIPT_MOCHA_PACKAGE_PATH"));

const TYPESCRIPT_VITEST_PACKAGE_PATH_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("TYPESCRIPT_VITEST_PACKAGE_PATH"));

const TYPESCRIPT_JASMINE_PACKAGE_PATH_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("TYPESCRIPT_JASMINE_PACKAGE_PATH"));

const TYPESCRIPT_BUN_PACKAGE_PATH_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("TYPESCRIPT_BUN_PACKAGE_PATH"));

const TYPESCRIPT_BUN_TEST_NAME_VARIABLE: VariableName = VariableName::Custom(Cow::Borrowed("TYPESCRIPT_BUN_TEST_NAME"));

const TYPESCRIPT_NODE_PACKAGE_PATH_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("TYPESCRIPT_NODE_PACKAGE_PATH"));

#[derive(Clone, Debug, Default)]
struct PackageJsonContents(Arc<RwLock<HashMap<PathBuf, PackageJson>>>);

impl PackageJsonData {
    fn fill_task_templates(&self, task_templates: &mut TaskTemplates) {
        if self.jest_package_path.is_some() {
            task_templates.0.push(TaskTemplate {
                label: "jest file test".to_owned(),
                command: TYPESCRIPT_RUNNER_VARIABLE.template_value(),
                args: vec![
                    "exec".to_owned(),
                    "--".to_owned(),
                    "jest".to_owned(),
                    "--runInBand".to_owned(),
                    VariableName::File.template_value(),
                ],
                cwd: Some(TYPESCRIPT_JEST_PACKAGE_PATH_VARIABLE.template_value()),
                ..TaskTemplate::default()
            });
            task_templates.0.push(TaskTemplate {
                label: format!("jest test {}", VariableName::Symbol.template_value()),
                command: TYPESCRIPT_RUNNER_VARIABLE.template_value(),
                args: vec![
                    "exec".to_owned(),
                    "--".to_owned(),
                    "jest".to_owned(),
                    "--runInBand".to_owned(),
                    "--testNamePattern".to_owned(),
                    format!("\"{}\"", TYPESCRIPT_JEST_TEST_NAME_VARIABLE.template_value()),
                    VariableName::File.template_value(),
                ],
                tags: vec!["ts-test".to_owned(), "js-test".to_owned(), "tsx-test".to_owned()],
                cwd: Some(TYPESCRIPT_JEST_PACKAGE_PATH_VARIABLE.template_value()),
                ..TaskTemplate::default()
            });
        }

        if self.vitest_package_path.is_some() {
            task_templates.0.push(TaskTemplate {
                label: format!("{} file test", "vitest".to_owned()),
                command: TYPESCRIPT_RUNNER_VARIABLE.template_value(),
                args: vec![
                    "exec".to_owned(),
                    "--".to_owned(),
                    "vitest".to_owned(),
                    "run".to_owned(),
                    "--no-file-parallelism".to_owned(),
                    VariableName::File.template_value(),
                ],
                cwd: Some(TYPESCRIPT_VITEST_PACKAGE_PATH_VARIABLE.template_value()),
                ..TaskTemplate::default()
            });
            task_templates.0.push(TaskTemplate {
                label: format!("{} test {}", "vitest".to_owned(), VariableName::Symbol.template_value(),),
                command: TYPESCRIPT_RUNNER_VARIABLE.template_value(),
                args: vec![
                    "exec".to_owned(),
                    "--".to_owned(),
                    "vitest".to_owned(),
                    "run".to_owned(),
                    "--no-file-parallelism".to_owned(),
                    "--testNamePattern".to_owned(),
                    format!("\"{}\"", TYPESCRIPT_VITEST_TEST_NAME_VARIABLE.template_value()),
                    VariableName::File.template_value(),
                ],
                tags: vec!["ts-test".to_owned(), "js-test".to_owned(), "tsx-test".to_owned()],
                cwd: Some(TYPESCRIPT_VITEST_PACKAGE_PATH_VARIABLE.template_value()),
                ..TaskTemplate::default()
            });
        }

        if self.mocha_package_path.is_some() {
            task_templates.0.push(TaskTemplate {
                label: format!("{} file test", "mocha".to_owned()),
                command: TYPESCRIPT_RUNNER_VARIABLE.template_value(),
                args: vec!["exec".to_owned(), "--".to_owned(), "mocha".to_owned(), VariableName::File.template_value()],
                cwd: Some(TYPESCRIPT_MOCHA_PACKAGE_PATH_VARIABLE.template_value()),
                ..TaskTemplate::default()
            });
            task_templates.0.push(TaskTemplate {
                label: format!("{} test {}", "mocha".to_owned(), VariableName::Symbol.template_value(),),
                command: TYPESCRIPT_RUNNER_VARIABLE.template_value(),
                args: vec![
                    "exec".to_owned(),
                    "--".to_owned(),
                    "mocha".to_owned(),
                    "--grep".to_owned(),
                    format!("\"{}\"", VariableName::Symbol.template_value()),
                    VariableName::File.template_value(),
                ],
                tags: vec!["ts-test".to_owned(), "js-test".to_owned(), "tsx-test".to_owned()],
                cwd: Some(TYPESCRIPT_MOCHA_PACKAGE_PATH_VARIABLE.template_value()),
                ..TaskTemplate::default()
            });
        }

        if self.jasmine_package_path.is_some() {
            task_templates.0.push(TaskTemplate {
                label: format!("{} file test", "jasmine".to_owned()),
                command: TYPESCRIPT_RUNNER_VARIABLE.template_value(),
                args: vec![
                    "exec".to_owned(),
                    "--".to_owned(),
                    "jasmine".to_owned(),
                    VariableName::File.template_value(),
                ],
                cwd: Some(TYPESCRIPT_JASMINE_PACKAGE_PATH_VARIABLE.template_value()),
                ..TaskTemplate::default()
            });
            task_templates.0.push(TaskTemplate {
                label: format!("{} test {}", "jasmine".to_owned(), VariableName::Symbol.template_value(),),
                command: TYPESCRIPT_RUNNER_VARIABLE.template_value(),
                args: vec![
                    "exec".to_owned(),
                    "--".to_owned(),
                    "jasmine".to_owned(),
                    format!("--filter={}", VariableName::Symbol.template_value()),
                    VariableName::File.template_value(),
                ],
                tags: vec!["ts-test".to_owned(), "js-test".to_owned(), "tsx-test".to_owned()],
                cwd: Some(TYPESCRIPT_JASMINE_PACKAGE_PATH_VARIABLE.template_value()),
                ..TaskTemplate::default()
            });
        }

        if self.bun_package_path.is_some() {
            task_templates.0.push(TaskTemplate {
                label: format!("{} file test", "bun test".to_owned()),
                command: "bun".to_owned(),
                args: vec!["test".to_owned(), VariableName::File.template_value()],
                cwd: Some(TYPESCRIPT_BUN_PACKAGE_PATH_VARIABLE.template_value()),
                ..TaskTemplate::default()
            });
            task_templates.0.push(TaskTemplate {
                label: format!("bun test {}", VariableName::Symbol.template_value(),),
                command: "bun".to_owned(),
                args: vec![
                    "test".to_owned(),
                    "--test-name-pattern".to_owned(),
                    format!("\"{}\"", TYPESCRIPT_BUN_TEST_NAME_VARIABLE.template_value()),
                    VariableName::File.template_value(),
                ],
                tags: vec!["ts-test".to_owned(), "js-test".to_owned(), "tsx-test".to_owned()],
                cwd: Some(TYPESCRIPT_BUN_PACKAGE_PATH_VARIABLE.template_value()),
                ..TaskTemplate::default()
            });
        }

        if self.node_package_path.is_some() {
            task_templates.0.push(TaskTemplate {
                label: format!("{} file test", "node test".to_owned()),
                command: "node".to_owned(),
                args: vec!["--test".to_owned(), VariableName::File.template_value()],
                tags: vec!["ts-test".to_owned(), "js-test".to_owned(), "tsx-test".to_owned()],
                cwd: Some(TYPESCRIPT_NODE_PACKAGE_PATH_VARIABLE.template_value()),
                ..TaskTemplate::default()
            });
            task_templates.0.push(TaskTemplate {
                label: format!("node test {}", VariableName::Symbol.template_value()),
                command: "node".to_owned(),
                args: vec![
                    "--test".to_owned(),
                    "--test-name-pattern".to_owned(),
                    format!("\"{}\"", VariableName::Symbol.template_value()),
                    VariableName::File.template_value(),
                ],
                tags: vec!["ts-test".to_owned(), "js-test".to_owned(), "tsx-test".to_owned()],
                cwd: Some(TYPESCRIPT_NODE_PACKAGE_PATH_VARIABLE.template_value()),
                ..TaskTemplate::default()
            });
        }

        let script_name_counts: HashMap<_, usize> =
            self.scripts.iter().fold(HashMap::default(), |mut acc, (_, script)| {
                *acc.entry(script).or_default() += 1;
                acc
            });
        for (path, script) in &self.scripts {
            let label = if script_name_counts.get(script).copied().unwrap_or_default() > 1
                && let Some(parent) = path.parent().and_then(|parent| parent.file_name())
            {
                let parent = parent.to_string_lossy();
                format!("{parent}/package.json > {script}")
            } else {
                format!("package.json > {script}")
            };
            task_templates.0.push(TaskTemplate {
                label,
                command: TYPESCRIPT_RUNNER_VARIABLE.template_value(),
                args: vec!["run".to_owned(), script.to_owned()],
                tags: vec!["package-script".into()],
                cwd: Some(path.parent().unwrap_or(Path::new("/")).to_string_lossy().to_string()),
                ..TaskTemplate::default()
            });
        }
    }
}

impl TypeScriptContextProvider {
    pub fn new(fs: Arc<dyn Fs>) -> Self {
        Self { fs, last_package_json: PackageJsonContents::default() }
    }

    fn combined_package_json_data(
        &self,
        fs: Arc<dyn Fs>,
        worktree_root: &Path,
        file_relative_path: &RelPath,
        cx: &App,
    ) -> Task<anyhow::Result<PackageJsonData>> {
        let new_json_data = file_relative_path
            .ancestors()
            .map(|path| worktree_root.join(path.as_std_path()))
            .map(|parent_path| self.package_json_data(&parent_path, self.last_package_json.clone(), fs.clone(), cx))
            .collect::<Vec<_>>();

        cx.background_spawn(async move {
            let mut package_json_data = PackageJsonData::default();
            for new_data in join_all(new_json_data).await.into_iter().flatten() {
                package_json_data.merge(new_data);
            }
            Ok(package_json_data)
        })
    }

    fn package_json_data(
        &self,
        directory_path: &Path,
        existing_package_json: PackageJsonContents,
        fs: Arc<dyn Fs>,
        cx: &App,
    ) -> Task<anyhow::Result<PackageJsonData>> {
        let package_json_path = directory_path.join("package.json");
        let metadata_check_fs = fs.clone();
        cx.background_spawn(async move {
            let metadata = metadata_check_fs
                .metadata(&package_json_path)
                .await
                .with_context(|| format!("getting metadata for {package_json_path:?}"))?
                .with_context(|| format!("missing FS metadata for {package_json_path:?}"))?;
            let mtime = metadata.mtime.timestamp_for_user();
            let existing_data = {
                let contents = existing_package_json.0.read().await;
                contents
                    .get(&package_json_path)
                    .filter(|package_json| package_json.mtime == mtime)
                    .map(|package_json| package_json.data.clone())
            };
            match existing_data {
                Some(existing_data) => Ok(existing_data),
                None => {
                    let package_json_string = fs
                        .load(&package_json_path)
                        .await
                        .with_context(|| format!("loading package.json from {package_json_path:?}"))?;
                    let package_json: HashMap<String, serde_json_lenient::Value> =
                        serde_json_lenient::from_str(&package_json_string)
                            .with_context(|| format!("parsing package.json from {package_json_path:?}"))?;
                    let new_data = PackageJsonData::new(package_json_path.as_path().into(), package_json);
                    {
                        let mut contents = existing_package_json.0.write().await;
                        contents.insert(package_json_path, PackageJson { mtime, data: new_data.clone() });
                    }
                    Ok(new_data)
                }
            }
        })
    }
}

async fn detect_package_manager(
    worktree_root: PathBuf,
    fs: Arc<dyn Fs>,
    package_json_data: Option<PackageJsonData>,
) -> &'static str {
    if let Some(package_json_data) = package_json_data
        && let Some(package_manager) = package_json_data.package_manager
    {
        return package_manager;
    }
    if fs.is_file(&worktree_root.join("pnpm-lock.yaml")).await {
        return "pnpm";
    }
    if fs.is_file(&worktree_root.join("yarn.lock")).await {
        return "yarn";
    }
    "npm"
}

impl ContextProvider for TypeScriptContextProvider {
    fn associated_tasks(&self, buffer: Option<Entity<Buffer>>, cx: &App) -> Task<Option<TaskTemplates>> {
        let file = buffer.and_then(|buffer| buffer.read(cx).file());
        let Some(file) = project::File::from_dyn(file).cloned() else {
            return Task::ready(None);
        };
        let Some(worktree_root) = file.worktree.read(cx).root_dir() else {
            return Task::ready(None);
        };
        let file_relative_path = file.path().clone();
        let package_json_data =
            self.combined_package_json_data(self.fs.clone(), &worktree_root, &file_relative_path, cx);

        cx.background_spawn(async move {
            let mut task_templates = TaskTemplates(Vec::new());
            task_templates.0.push(TaskTemplate {
                label: format!("execute selection {}", VariableName::SelectedText.template_value()),
                command: "node".to_owned(),
                args: vec!["-e".to_owned(), format!("\"{}\"", VariableName::SelectedText.template_value())],
                ..TaskTemplate::default()
            });

            match package_json_data.await {
                Ok(package_json) => {
                    package_json.fill_task_templates(&mut task_templates);
                }
                Err(e) => {
                    log::error!("Failed to read package.json for worktree {file_relative_path:?}: {e:#}");
                }
            }

            Some(task_templates)
        })
    }

    fn build_context(
        &self,
        current_vars: &task::TaskVariables,
        location: ContextLocation<'_>,
        _project_env: Option<HashMap<String, String>>,
        cx: &mut App,
    ) -> Task<Result<task::TaskVariables>> {
        let mut vars = task::TaskVariables::default();

        if let Some(symbol) = current_vars.get(&VariableName::Symbol) {
            vars.insert(TYPESCRIPT_JEST_TEST_NAME_VARIABLE, replace_test_name_parameters(symbol));
            vars.insert(TYPESCRIPT_VITEST_TEST_NAME_VARIABLE, replace_test_name_parameters(symbol));
            vars.insert(TYPESCRIPT_BUN_TEST_NAME_VARIABLE, replace_test_name_parameters(symbol));
        }
        let file_path = location.file_location.buffer.read(cx).file().map(|file| file.path());

        let args = location.worktree_root.zip(location.fs).zip(file_path).map(|((worktree_root, fs), file_path)| {
            (self.combined_package_json_data(fs.clone(), &worktree_root, file_path, cx), worktree_root, fs)
        });
        cx.background_spawn(async move {
            if let Some((task, worktree_root, fs)) = args {
                let package_json_data = task.await.log_err();
                vars.insert(
                    TYPESCRIPT_RUNNER_VARIABLE,
                    detect_package_manager(worktree_root, fs, package_json_data.clone()).await.to_owned(),
                );

                if let Some(package_json_data) = package_json_data {
                    if let Some(path) = package_json_data.jest_package_path {
                        vars.insert(
                            TYPESCRIPT_JEST_PACKAGE_PATH_VARIABLE,
                            path.parent().unwrap_or(Path::new("")).to_string_lossy().to_string(),
                        );
                    }

                    if let Some(path) = package_json_data.mocha_package_path {
                        vars.insert(
                            TYPESCRIPT_MOCHA_PACKAGE_PATH_VARIABLE,
                            path.parent().unwrap_or(Path::new("")).to_string_lossy().to_string(),
                        );
                    }

                    if let Some(path) = package_json_data.vitest_package_path {
                        vars.insert(
                            TYPESCRIPT_VITEST_PACKAGE_PATH_VARIABLE,
                            path.parent().unwrap_or(Path::new("")).to_string_lossy().to_string(),
                        );
                    }

                    if let Some(path) = package_json_data.jasmine_package_path {
                        vars.insert(
                            TYPESCRIPT_JASMINE_PACKAGE_PATH_VARIABLE,
                            path.parent().unwrap_or(Path::new("")).to_string_lossy().to_string(),
                        );
                    }

                    if let Some(path) = package_json_data.bun_package_path {
                        vars.insert(
                            TYPESCRIPT_BUN_PACKAGE_PATH_VARIABLE,
                            path.parent().unwrap_or(Path::new("")).to_string_lossy().to_string(),
                        );
                    }

                    if let Some(path) = package_json_data.node_package_path {
                        vars.insert(
                            TYPESCRIPT_NODE_PACKAGE_PATH_VARIABLE,
                            path.parent().unwrap_or(Path::new("")).to_string_lossy().to_string(),
                        );
                    }
                }
            }
            Ok(vars)
        })
    }
}

fn replace_test_name_parameters(test_name: &str) -> String {
    static PATTERN: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(\$([A-Za-z0-9_\.]+|[\#])|%[psdifjo#\$%])").unwrap());
    PATTERN.split(test_name).map(regex::escape).join("(.+?)")
}
