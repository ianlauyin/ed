use std::num::NonZeroU32;
use std::str::FromStr as _;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use gpui::App;
use gpui::AsyncApp;
use gpui::Entity;
use gpui::SharedString;
use gpui::Task;
use language::Buffer;
use language::ContextProvider;
use language::LanguageName;
use language::LanguageRegistry;
use language::LocalFile as _;
use language::LspAdapter;
use language::LspAdapterDelegate;
use language::language_settings::AllLanguageSettings;
use language::language_settings::LanguageSettings;
use language::language_settings::RewrapBehavior;
use language::language_settings::WordsCompletionMode;
use lsp::LanguageServerBinary;
use lsp::LanguageServerName;
use lsp::Uri;
use rustc_hash::FxHashMap;
use serde_json::Value;
use serde_json::json;
use task::TaskTemplate;
use task::TaskTemplates;
use task::VariableName;
use util::rel_path::RelPath;

use crate::LanguageDef;
use crate::PackageJsonData;
use crate::npx;

pub(crate) struct JsonTaskProvider;

impl ContextProvider for JsonTaskProvider {
    fn associated_tasks(&self, buffer: Option<Entity<Buffer>>, cx: &App) -> gpui::Task<Option<TaskTemplates>> {
        let file = buffer.as_ref().and_then(|buf| buf.read(cx).file());
        let Some(file) = project::File::from_dyn(file).cloned() else {
            return Task::ready(None);
        };
        let is_package_json = file.path.ends_with(RelPath::from_unix_str("package.json").expect("valid relative path"));
        let is_composer_json =
            file.path.ends_with(RelPath::from_unix_str("composer.json").expect("valid relative path"));
        if !is_package_json && !is_composer_json {
            return Task::ready(None);
        }

        cx.spawn(async move |cx| {
            let contents = file.worktree.update(cx, |this, cx| this.load_file(&file.path, cx)).await.ok()?;
            let path = cx.update(|cx| file.abs_path(cx)).as_path().into();
            let contents_text = contents.text.to_string();

            let task_templates = if is_package_json {
                let package_json = serde_json::from_str::<FxHashMap<String, serde_json::Value>>(&contents_text).ok()?;
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
                        label: "package script $ED_CUSTOM_script".to_owned(),
                        command: command.clone(),
                        args: vec!["run".into(), VariableName::Custom("script".into()).template_value()],
                        cwd: Some(VariableName::Dirname.template_value()),
                        tags: vec!["package-script".into()],
                        ..TaskTemplate::default()
                    }])
                    .collect()
            } else if is_composer_json {
                serde_json::Value::from_str(&contents_text)
                    .ok()?
                    .get("scripts")?
                    .as_object()?
                    .keys()
                    .map(|key| TaskTemplate {
                        label: format!("run {key}"),
                        command: "composer".to_owned(),
                        args: vec!["-d".into(), "$ED_DIRNAME".into(), key.into()],
                        ..TaskTemplate::default()
                    })
                    .chain([TaskTemplate {
                        label: "composer script $ED_CUSTOM_script".to_owned(),
                        command: "composer".to_owned(),
                        args: vec![
                            "-d".into(),
                            "$ED_DIRNAME".into(),
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

pub(crate) struct JsonLspAdapter {
    languages: Arc<LanguageRegistry>,
}

impl JsonLspAdapter {
    pub(crate) const fn new(languages: Arc<LanguageRegistry>) -> Self {
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
        npx::node_server_binary(
            delegate,
            &["vscode-langservers-extracted"],
            "vscode-json-language-server",
            &["--stdio"],
        )
        .await
    }

    async fn workspace_configuration(
        self: Arc<Self>,
        _: &Arc<dyn LspAdapterDelegate>,
        _: Option<Uri>,
        cx: &mut AsyncApp,
    ) -> Result<Value> {
        Ok(cx.update(|cx| {
            let schemas = schema_associations(&jsonc_globs(&self.languages, cx));

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
        }))
    }

    fn language_ids(&self) -> FxHashMap<LanguageName, String> {
        [(LanguageName::new_static("JSON"), "json".into()), (LanguageName::new_static("JSONC"), "jsonc".into())]
            .into_iter()
            .collect()
    }

    async fn process_completions(&self, items: &mut [lsp::CompletionItem]) {
        unquote_filter_texts(items);
    }
}

/// Globs matching every JSONC file: the language's path suffixes plus `file_types` overrides.
fn jsonc_globs(languages: &Arc<LanguageRegistry>, cx: &App) -> Vec<SharedString> {
    let extension_globs = languages
        .available_language_for_name("JSONC")
        .map(|language| language.matcher().path_suffixes.clone())
        .into_iter()
        .flatten()
        // Path suffixes can be entire file names or just their extensions.
        .flat_map(|path_suffix| [format!("*.{path_suffix}").into(), path_suffix]);
    let override_globs = AllLanguageSettings::get_global(cx)
        .file_types
        .get("JSONC")
        .into_iter()
        .flat_map(|(_, glob_strings)| glob_strings.iter().map(SharedString::from));
    extension_globs.chain(override_globs).collect()
}

/// The server downloads the SchemaStore schemas itself; the JSONC schema is inline.
fn schema_associations(jsonc_globs: &[SharedString]) -> Value {
    json!([
        {
            "fileMatch": ["tsconfig*.json"],
            "url": "https://json.schemastore.org/tsconfig.json"
        },
        {
            "fileMatch": ["package.json"],
            "url": "https://json.schemastore.org/package.json"
        },
        {
            "fileMatch": jsonc_globs,
            "schema": { "allowTrailingCommas": true }
        },
    ])
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

/// Settings for JSON and JSONC.
pub(crate) fn settings() -> LanguageSettings {
    LanguageSettings {
        tab_size: NonZeroU32::new(2).expect("non-zero"),
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

pub(crate) fn languages(registry: Arc<LanguageRegistry>) -> Vec<LanguageDef> {
    let adapter: Arc<dyn LspAdapter> = Arc::new(JsonLspAdapter::new(registry));
    let context: Arc<dyn ContextProvider> = Arc::new(JsonTaskProvider);
    vec![
        LanguageDef {
            grammar: Some(tree_sitter_json::LANGUAGE.into()),
            adapters: vec![Arc::clone(&adapter)],
            context: Some(Arc::clone(&context)),
            ..LanguageDef::new("json", settings())
        },
        LanguageDef {
            grammar: Some(tree_sitter_json::LANGUAGE.into()),
            adapters: vec![adapter],
            context: Some(context),
            file_types: &["tsconfig*.json"],
            ..LanguageDef::new("jsonc", settings())
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_associations_use_schemastore_and_inline_jsonc() {
        let schemas = schema_associations(&["*.jsonc".into(), "jsonc".into()]);
        assert_eq!(
            schemas,
            json!([
                { "fileMatch": ["tsconfig*.json"], "url": "https://json.schemastore.org/tsconfig.json" },
                { "fileMatch": ["package.json"], "url": "https://json.schemastore.org/package.json" },
                { "fileMatch": ["*.jsonc", "jsonc"], "schema": { "allowTrailingCommas": true } },
            ])
        );
    }
}
