use anyhow::Result;
use async_trait::async_trait;
use collections::HashMap;
use gpui::{App, AsyncApp, Entity, Task};
use language::{
    Buffer, ContextProvider, LanguageName, LanguageRegistry, LocalFile as _, LspAdapter,
    LspAdapterDelegate,
};
use lsp::{LanguageServerBinary, LanguageServerName, Uri};
use project::lsp_store::language_server_settings;
use serde_json::{Value, json};
use settings::SettingsLocation;
use std::{borrow::Cow, str::FromStr, sync::Arc};
use task::{TaskTemplate, TaskTemplates, VariableName};
use util::{paths::PathStyle, rel_path::RelPath, union_json_value_into};

use crate::PackageJsonData;

pub(crate) struct JsonTaskProvider;

impl ContextProvider for JsonTaskProvider {
    fn associated_tasks(
        &self,
        buffer: Option<Entity<Buffer>>,
        cx: &App,
    ) -> gpui::Task<Option<TaskTemplates>> {
        let file = buffer.as_ref().and_then(|buf| buf.read(cx).file());
        let Some(file) = project::File::from_dyn(file).cloned() else {
            return Task::ready(None);
        };
        let is_package_json = file
            .path
            .ends_with(RelPath::from_unix_str("package.json").unwrap());
        let is_composer_json = file
            .path
            .ends_with(RelPath::from_unix_str("composer.json").unwrap());
        if !is_package_json && !is_composer_json {
            return Task::ready(None);
        }

        cx.spawn(async move |cx| {
            let contents = file
                .worktree
                .update(cx, |this, cx| this.load_file(&file.path, cx))
                .await
                .ok()?;
            let path = cx.update(|cx| file.abs_path(cx)).as_path().into();
            let contents_text = contents.text.to_string();

            let task_templates = if is_package_json {
                let package_json = serde_json_lenient::from_str::<
                    HashMap<String, serde_json_lenient::Value>,
                >(&contents_text)
                .ok()?;
                let package_json = PackageJsonData::new(path, package_json);
                let command = package_json.package_manager.unwrap_or("npm").to_owned();
                package_json
                    .scripts
                    .into_iter()
                    .map(|(_, key)| TaskTemplate {
                        label: format!("run {key}"),
                        command: command.clone(),
                        args: vec!["run".into(), key],
                        cwd: Some(VariableName::Dirname.template_value()),
                        ..TaskTemplate::default()
                    })
                    .chain([TaskTemplate {
                        label: "package script $ZED_CUSTOM_script".to_owned(),
                        command: command.clone(),
                        args: vec![
                            "run".into(),
                            VariableName::Custom("script".into()).template_value(),
                        ],
                        cwd: Some(VariableName::Dirname.template_value()),
                        tags: vec!["package-script".into()],
                        ..TaskTemplate::default()
                    }])
                    .collect()
            } else if is_composer_json {
                serde_json_lenient::Value::from_str(&contents_text)
                    .ok()?
                    .get("scripts")?
                    .as_object()?
                    .keys()
                    .map(|key| TaskTemplate {
                        label: format!("run {key}"),
                        command: "composer".to_owned(),
                        args: vec!["-d".into(), "$ZED_DIRNAME".into(), key.into()],
                        ..TaskTemplate::default()
                    })
                    .chain([TaskTemplate {
                        label: "composer script $ZED_CUSTOM_script".to_owned(),
                        command: "composer".to_owned(),
                        args: vec![
                            "-d".into(),
                            "$ZED_DIRNAME".into(),
                            VariableName::Custom("script".into()).template_value(),
                        ],
                        tags: vec!["composer-script".into()],
                        ..TaskTemplate::default()
                    }])
                    .collect()
            } else {
                vec![]
            };

            Some(TaskTemplates(task_templates))
        })
    }
}

pub struct JsonLspAdapter {
    languages: Arc<LanguageRegistry>,
}

impl JsonLspAdapter {
    pub fn new(languages: Arc<LanguageRegistry>) -> Self {
        Self { languages }
    }
}

#[async_trait(?Send)]
impl LspAdapter for JsonLspAdapter {
    fn name(&self) -> LanguageServerName {
        LanguageServerName("json-language-server".into())
    }

    fn binary_name(&self) -> String {
        "vscode-json-language-server".into()
    }

    async fn check_if_user_installed(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        crate::npx::node_server_binary(
            delegate,
            &["vscode-langservers-extracted"],
            "vscode-json-language-server",
            &["--stdio"],
        )
        .await
    }

    async fn initialization_options(
        self: Arc<Self>,
        _: &Arc<dyn LspAdapterDelegate>,
        _: &mut AsyncApp,
    ) -> Result<Option<serde_json::Value>> {
        Ok(Some(json!({
            "provideFormatter": true
        })))
    }

    async fn workspace_configuration(
        self: Arc<Self>,
        delegate: &Arc<dyn LspAdapterDelegate>,
        requested_uri: Option<Uri>,
        cx: &mut AsyncApp,
    ) -> Result<Value> {
        let requested_path = requested_uri.as_ref().and_then(|uri| {
            (uri.scheme() == "file")
                .then(|| uri.to_file_path().ok())
                .flatten()
        });
        let path_in_worktree = requested_path
            .as_ref()
            .and_then(|abs_path| {
                let rel_path = abs_path.strip_prefix(delegate.worktree_root_path()).ok()?;
                RelPath::new(rel_path, PathStyle::local()).ok()
            })
            .unwrap_or_else(|| Cow::Borrowed(RelPath::empty()));
        let mut config = cx.update(|cx| {
            let schemas = json_schema_store::all_schema_file_associations(
                &self.languages,
                Some(SettingsLocation {
                    worktree_id: delegate.worktree_id(),
                    path: path_in_worktree.as_ref(),
                }),
                cx,
            );

            // This can be viewed via `dev: open language server logs` -> `json-language-server` ->
            // `Server Info`
            serde_json::json!({
                "json": {
                    "format": {
                        "enable": true,
                    },
                    "validate": {
                        "enable": true,
                    },
                    "schemas": schemas
                }
            })
        });

        let project_options = cx.update(|cx| {
            language_server_settings(delegate.as_ref(), &self.name(), cx)
                .and_then(|s| worktree_root(delegate, s.settings.clone()))
        });

        if let Some(override_options) = project_options {
            union_json_value_into(override_options, &mut config);
        }

        Ok(config)
    }

    fn language_ids(&self) -> HashMap<LanguageName, String> {
        [
            (LanguageName::new_static("JSON"), "json".into()),
            (LanguageName::new_static("JSONC"), "jsonc".into()),
        ]
        .into_iter()
        .collect()
    }

    fn is_primary_zed_json_schema_adapter(&self) -> bool {
        true
    }

    async fn process_completions(&self, items: &mut [lsp::CompletionItem]) {
        unquote_filter_texts(items);
    }
}

/// json-language-server quotes `filterText` (`"status_bar"`), or the label of items
/// without one (`"dark"`), but the completion query never includes the opening
/// quote, which skews fuzzy match scores.
fn unquote_filter_texts(items: &mut [lsp::CompletionItem]) {
    for item in items {
        let filter_text = item.filter_text.as_deref().unwrap_or(&item.label);
        if filter_text.starts_with('"')
            && let Ok(unquoted) = serde_json::from_str::<String>(filter_text)
        {
            item.filter_text = Some(unquoted);
        }
    }
}

fn worktree_root(delegate: &Arc<dyn LspAdapterDelegate>, settings: Option<Value>) -> Option<Value> {
    let Some(Value::Object(mut settings_map)) = settings else {
        return settings;
    };

    let Some(Value::Object(json_config)) = settings_map.get_mut("json") else {
        return Some(Value::Object(settings_map));
    };

    let Some(Value::Array(schemas)) = json_config.get_mut("schemas") else {
        return Some(Value::Object(settings_map));
    };

    for schema in schemas.iter_mut() {
        let Value::Object(schema_map) = schema else {
            continue;
        };
        let Some(Value::String(url)) = schema_map.get_mut("url") else {
            continue;
        };

        if !url.starts_with(".") && !url.starts_with("~") {
            continue;
        }

        *url = delegate
            .resolve_relative_path(url.clone().into())
            .to_string_lossy()
            .into_owned();
    }

    Some(Value::Object(settings_map))
}
