use std::path::PathBuf;

use anyhow::Context as _;
use anyhow::bail;
use collections::HashMap;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use util::ResultExt;
use util::truncate_and_remove_front;

use crate::ED_VARIABLE_NAME_PREFIX;
use crate::ResolvedTask;
use crate::SpawnInTerminal;
use crate::TaskContext;
use crate::TaskId;

/// A template definition of an ed task to run.
/// May use the [`VariableName`](crate::VariableName) to get the corresponding substitutions into its fields.
///
/// Template itself is not ready to spawn a task, it needs to be resolved with a [`TaskContext`] first, that
/// contains all relevant ed state in task variables.
/// A single template may produce different tasks (or none) for different contexts.
#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskTemplate {
    /// Human readable name of the task to display in the UI.
    pub label: String,
    /// Executable command to spawn.
    pub command: String,
    /// Arguments to the command.
    pub args: Vec<String>,
    /// Env overrides for the command, will be appended to the terminal's environment from the settings.
    pub env: HashMap<String, String>,
    /// Current working directory to spawn the command into, defaults to current project root.
    pub cwd: Option<String>,
    /// The runnable tags (from `runnables.scm`) this template attaches to.
    pub tags: Vec<String>,
}

/// What to do with the terminal pane and tab, after the command was started.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevealStrategy {
    /// Always show the task's pane, and focus the corresponding tab in it.
    #[default]
    Always,
    /// Always show the task's pane, add the task's tab in it, but don't focus it.
    NoFocus,
    /// Do not alter focus, but still add/reuse the task's tab in its pane.
    Never,
}

/// A group of task templates, provided by a language.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskTemplates(pub Vec<TaskTemplate>);

impl TaskTemplate {
    /// Replaces all `VariableName` task variables in the task template string fields.
    ///
    /// Every [`ResolvedTask`] gets a [`TaskId`], based on the `id_base` (to avoid collision with various task sources),
    /// and hashes of its template and [`TaskContext`].
    pub fn resolve_task(&self, id_base: &str, cx: &TaskContext) -> Option<ResolvedTask> {
        if self.label.trim().is_empty() || self.command.trim().is_empty() {
            return None;
        }

        let task_variables =
            cx.task_variables.0.iter().map(|(key, value)| (key.to_string(), value.as_str())).collect::<HashMap<_, _>>();
        let truncated_variables = truncate_variables(&task_variables);
        let cwd = match self.cwd.as_deref() {
            Some(cwd) => Some(PathBuf::from(substitute_all_template_variables_in_str(cwd, &task_variables)?)),
            None => None,
        }
        .or(cx.cwd.clone());
        let full_label = substitute_all_template_variables_in_str(&self.label, &task_variables)?;

        // Arbitrarily picked threshold below which we don't truncate any variables.
        const TRUNCATION_THRESHOLD: usize = 64;

        let human_readable_label = if full_label.len() > TRUNCATION_THRESHOLD {
            substitute_all_template_variables_in_str(&self.label, &truncated_variables)?
        } else {
            #[allow(
                clippy::redundant_clone,
                reason = "We want to clone the full_label to avoid borrowing it in the fold closure"
            )]
            full_label.clone()
        }
        .lines()
        .fold(String::new(), |mut string, line| {
            if string.is_empty() {
                string.push_str(line);
            } else {
                string.push_str("\\n");
                string.push_str(line);
            }
            string
        });

        let command = substitute_all_template_variables_in_str(&self.command, &task_variables)?;
        let args_with_substitutions = substitute_all_template_variables_in_vec(&self.args, &task_variables)?;

        let task_hash = to_hex_hash(self).context("hashing task template").log_err()?;
        let variables_hash = to_hex_hash(&task_variables).context("hashing task variables").log_err()?;
        let id = TaskId(format!("{id_base}_{task_hash}_{variables_hash}"));

        let env = {
            // Start with the project environment as the base.
            let mut env = cx.project_env.clone();

            // Extend that environment with what's defined in the TaskTemplate
            env.extend(self.env.clone());

            // Then we replace all task variables that could be set in environment variables
            let mut env = substitute_all_template_variables_in_map(&env, &task_variables)?;

            // Last step: set the task variables as environment variables too
            env.extend(task_variables.into_iter().map(|(k, v)| (k, v.to_owned())));
            env
        };

        Some(ResolvedTask {
            id: id.clone(),
            original_task: self.clone(),
            resolved_label: full_label.clone(),
            resolved: SpawnInTerminal {
                id,
                cwd,
                full_label,
                label: human_readable_label,
                command_label: args_with_substitutions.iter().fold(command.clone(), |mut command_label, arg| {
                    command_label.push(' ');
                    command_label.push_str(arg);
                    command_label
                }),
                command: Some(command),
                args: args_with_substitutions,
                env,
                ..SpawnInTerminal::default()
            },
        })
    }
}

const MAX_DISPLAY_VARIABLE_LENGTH: usize = 15;

fn truncate_variables(task_variables: &HashMap<String, &str>) -> HashMap<String, String> {
    task_variables
        .iter()
        .map(|(key, value)| (key.clone(), truncate_and_remove_front(value, MAX_DISPLAY_VARIABLE_LENGTH)))
        .collect()
}

fn to_hex_hash(object: impl Serialize) -> anyhow::Result<String> {
    let json = serde_json::to_string(&object).context("serializing the object")?;
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    Ok(hex::encode(hasher.finalize()))
}

fn substitute_all_template_variables_in_str<A: AsRef<str>>(
    template_str: &str,
    task_variables: &HashMap<String, A>,
) -> Option<String> {
    let substituted_string = shellexpand::env_with_context(template_str, |var| {
        // Colons denote a default value in case the variable is not set. We
        // want to preserve that default, as otherwise shellexpand will
        // substitute it for us.
        let colon_position = var.find(':').unwrap_or(var.len());
        let (variable_name, default) = var.split_at(colon_position);
        if let Some(name) = task_variables.get(variable_name) {
            // Got a task variable hit - use the variable value, ignore default
            return Ok(Some(name.as_ref().to_owned()));
        } else if variable_name.starts_with(ED_VARIABLE_NAME_PREFIX) {
            // Unknown ED variable - use default if available
            if !default.is_empty() {
                // Strip the colon and return the default value
                return Ok(Some(default[1..].to_owned()));
            } else {
                bail!("Unknown variable name: {variable_name}");
            }
        }
        // This is an unknown variable.
        // We should not error out, as they may come from user environment (e.g.
        // $PATH). That means that the variable substitution might not be
        // perfect. If there's a default, we need to return the string verbatim
        // as otherwise shellexpand will apply that default for us.
        if !default.is_empty() {
            return Ok(Some(format!("${{{var}}}")));
        }

        // Else we can just return None and that variable will be left as is.
        Ok(None)
    })
    .ok()?;

    Some(substituted_string.into_owned())
}

fn substitute_all_template_variables_in_vec(
    template_strs: &[String],
    task_variables: &HashMap<String, &str>,
) -> Option<Vec<String>> {
    let mut expanded = Vec::with_capacity(template_strs.len());
    for variable in template_strs {
        let new_value = substitute_all_template_variables_in_str(variable, task_variables)?;

        if !new_value.is_empty() || variable.is_empty() {
            expanded.push(new_value);
        }
    }

    Some(expanded)
}

fn substitute_all_template_variables_in_map(
    keys_and_values: &HashMap<String, String>,
    task_variables: &HashMap<String, &str>,
) -> Option<HashMap<String, String>> {
    let mut new_map: HashMap<String, String> = HashMap::default();
    for (key, value) in keys_and_values {
        let new_value = substitute_all_template_variables_in_str(value, task_variables)?;
        let new_key = substitute_all_template_variables_in_str(key, task_variables)?;
        new_map.insert(new_key, new_value);
    }

    Some(new_map)
}
