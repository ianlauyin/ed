use anyhow::Result;
use async_trait::async_trait;
use gpui::AsyncApp;
use language::{LspAdapter, LspAdapterDelegate, language_settings::AllLanguageSettings};
use lsp::{LanguageServerBinary, LanguageServerName, Uri};
use project::lsp_store::language_server_settings;
use serde_json::Value;
use settings::{Settings, SettingsLocation};
use std::sync::Arc;
use util::{merge_json_value_into, rel_path::RelPath};

pub struct YamlLspAdapter;

impl YamlLspAdapter {
    const SERVER_NAME: LanguageServerName = LanguageServerName::new_static("yaml-language-server");
}

#[async_trait(?Send)]
impl LspAdapter for YamlLspAdapter {
    fn name(&self) -> LanguageServerName {
        Self::SERVER_NAME
    }

    async fn check_if_user_installed(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        let bin = Self::SERVER_NAME;
        crate::npx::node_server_binary(delegate, &[bin.as_ref()], bin.as_ref(), &["--stdio"]).await
    }

    async fn workspace_configuration(
        self: Arc<Self>,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: Option<Uri>,
        cx: &mut AsyncApp,
    ) -> Result<Value> {
        let location = SettingsLocation {
            worktree_id: delegate.worktree_id(),
            path: RelPath::empty(),
        };

        let tab_size = cx.update(|cx| {
            AllLanguageSettings::get(Some(location), cx)
                .language(Some(&"YAML".into()))
                .tab_size
        });

        let mut options = serde_json::json!({
            "[yaml]": {"editor.tabSize": tab_size},
            "yaml": {"format": {"enable": true}}
        });

        let project_options = cx.update(|cx| {
            language_server_settings(delegate.as_ref(), &Self::SERVER_NAME, cx)
                .and_then(|s| worktree_root(delegate, s.settings.clone()))
        });
        if let Some(override_options) = project_options {
            merge_json_value_into(override_options, &mut options);
        }
        Ok(options)
    }
}

fn worktree_root(delegate: &Arc<dyn LspAdapterDelegate>, settings: Option<Value>) -> Option<Value> {
    let Some(Value::Object(mut settings_map)) = settings else {
        return settings;
    };

    let Some(Value::Object(yaml_config)) = settings_map.get_mut("yaml") else {
        return Some(Value::Object(settings_map));
    };

    let Some(Value::Object(schemas)) = yaml_config.remove("schemas") else {
        return Some(Value::Object(settings_map));
    };

    let schemas = schemas
        .into_iter()
        .map(|(url, v)| {
            if !url.starts_with(".") && !url.starts_with("~") {
                (url, v)
            } else {
                let resolved_url = delegate
                    .resolve_relative_path(url.into())
                    .to_string_lossy()
                    .into_owned();
                (resolved_url, v)
            }
        })
        .collect::<serde_json::Map<String, Value>>();

    yaml_config.insert("schemas".into(), Value::Object(schemas));
    Some(Value::Object(settings_map))
}
