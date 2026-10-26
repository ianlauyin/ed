use std::sync::Arc;

use anyhow::Result;
use gpui::App;
use gpui::AsyncApp;
use gpui::Entity;
use gpui::SharedString;
use gpui::Task;
use language::LanguageRegistry;
use language::language_settings::AllLanguageSettings;
use project::LspStore;
use settings::Settings as _;
use settings::SettingsLocation;

const SCHEMA_URI_PREFIX: &str = "ed://schemas/";

const TSCONFIG_SCHEMA: &str = include_str!("schemas/tsconfig.json");
const PACKAGE_JSON_SCHEMA: &str = include_str!("schemas/package.json");

const JSONC_SCHEMA: &str = r#"{"allowTrailingCommas":true}"#;

pub fn init(cx: &mut App) {
    project::lsp_store::json_language_server_ext::register_schema_handler(handle_schema_request, cx);
}

pub fn handle_schema_request(_lsp_store: Entity<LspStore>, uri: String, _cx: &mut AsyncApp) -> Task<Result<String>> {
    let path = match uri.strip_prefix(SCHEMA_URI_PREFIX) {
        Some(path) => path,
        None => return Task::ready(Err(anyhow::anyhow!("Invalid schema URI: {}", uri))),
    };

    if let Some(json) = resolve_static_schema(path) {
        return Task::ready(Ok(json));
    }

    Task::ready(Err(anyhow::anyhow!("Unrecognized schema: {path}")))
}

fn resolve_static_schema(path: &str) -> Option<String> {
    match path {
        "tsconfig" => Some(TSCONFIG_SCHEMA.to_string()),
        "package_json" => Some(PACKAGE_JSON_SCHEMA.to_string()),
        "jsonc" => Some(JSONC_SCHEMA.to_string()),
        _ => None,
    }
}

const JSONC_LANGUAGE_NAME: &str = "JSONC";

pub fn all_schema_file_associations(
    languages: &Arc<LanguageRegistry>,
    path: Option<SettingsLocation<'_>>,
    cx: &mut App,
) -> serde_json::Value {
    let extension_globs = languages
        .available_language_for_name(JSONC_LANGUAGE_NAME)
        .map(|language| language.matcher().path_suffixes.clone())
        .into_iter()
        .flatten()
        // Path suffixes can be entire file names or just their extensions.
        .flat_map(|path_suffix| [format!("*.{path_suffix}").into(), path_suffix]);
    let override_globs = AllLanguageSettings::get(path, cx)
        .file_types
        .get(JSONC_LANGUAGE_NAME)
        .into_iter()
        .flat_map(|(_, glob_strings)| glob_strings.iter().map(SharedString::from));
    let jsonc_globs = extension_globs.chain(override_globs).collect::<Vec<_>>();

    serde_json::json!([
        {
            "fileMatch": ["tsconfig.json"],
            "url": format!("{SCHEMA_URI_PREFIX}tsconfig")
        },
        {
            "fileMatch": ["package.json"],
            "url": format!("{SCHEMA_URI_PREFIX}package_json")
        },
        {
            "fileMatch": &jsonc_globs,
            "url": format!("{SCHEMA_URI_PREFIX}jsonc")
        },
    ])
}
