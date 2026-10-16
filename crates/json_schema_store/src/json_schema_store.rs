use std::sync::{Arc, LazyLock};

use anyhow::Result;
use collections::HashMap;
use gpui::{App, AsyncApp, Entity, SharedString, Task};
use language::{LanguageRegistry, language_settings::AllLanguageSettings};
use parking_lot::RwLock;
use project::LspStore;
use settings::{Settings as _, SettingsLocation};
use util::schemars::{AllowTrailingCommas, DefaultDenyUnknownFields};

const SCHEMA_URI_PREFIX: &str = "zed://schemas/";

const TSCONFIG_SCHEMA: &str = include_str!("schemas/tsconfig.json");
const PACKAGE_JSON_SCHEMA: &str = include_str!("schemas/package.json");

static TASKS_SCHEMA: LazyLock<String> = LazyLock::new(|| {
    serde_json::to_string(&task::TaskTemplates::generate_json_schema())
        .expect("TaskTemplates schema should serialize")
});

static SNIPPETS_SCHEMA: LazyLock<String> = LazyLock::new(|| {
    serde_json::to_string(&snippet_provider::format::VsSnippetsFile::generate_json_schema())
        .expect("VsSnippetsFile schema should serialize")
});

static JSONC_SCHEMA: LazyLock<String> = LazyLock::new(|| {
    serde_json::to_string(&generate_jsonc_schema()).expect("JSONC schema should serialize")
});

static ACTION_SCHEMA_CACHE: LazyLock<RwLock<HashMap<String, String>>> =
    LazyLock::new(|| RwLock::new(HashMap::default()));

pub fn init(cx: &mut App) {
    project::lsp_store::json_language_server_ext::register_schema_handler(
        handle_schema_request,
        cx,
    );
}

pub fn handle_schema_request(
    _lsp_store: Entity<LspStore>,
    uri: String,
    _cx: &mut AsyncApp,
) -> Task<Result<String>> {
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
    let (schema_name, rest) = path.split_once('/').unzip();
    let schema_name = schema_name.unwrap_or(path);

    match schema_name {
        "tsconfig" => Some(TSCONFIG_SCHEMA.to_string()),
        "package_json" => Some(PACKAGE_JSON_SCHEMA.to_string()),
        "tasks" => Some(TASKS_SCHEMA.clone()),
        "snippets" => Some(SNIPPETS_SCHEMA.clone()),
        "jsonc" => Some(JSONC_SCHEMA.clone()),
        "action" => {
            let normalized_action_name = match rest {
                Some(name) => name,
                None => return None,
            };
            let action_name = denormalize_action_name(normalized_action_name);

            if let Some(cached) = ACTION_SCHEMA_CACHE.read().get(&action_name).cloned() {
                return Some(cached);
            }

            let mut generator = settings::action_schema_generator();
            let schema = settings::get_action_schema_by_name(&action_name, &mut generator);
            let json = serde_json::to_string(
                &root_schema_from_action_schema(schema, &mut generator).to_value(),
            )
            .expect("Action schema should serialize");

            ACTION_SCHEMA_CACHE
                .write()
                .insert(action_name, json.clone());
            Some(json)
        }

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

    let mut file_associations = serde_json::json!([
        {
            "fileMatch": [
                schema_file_match(paths::tasks_file()),
                paths::local_tasks_file_relative_path()
            ],
            "url": format!("{SCHEMA_URI_PREFIX}tasks"),
        },
        {
            "fileMatch": [
                schema_file_match(
                    paths::snippets_dir()
                        .join("*.json")
                        .as_path()
                )
            ],
            "url": format!("{SCHEMA_URI_PREFIX}snippets"),
        },
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
    ]);

    file_associations
        .as_array_mut()
        .unwrap()
        .extend(cx.all_action_names().into_iter().map(|&name| {
            let normalized_name = normalize_action_name(name);
            let file_name = normalized_action_name_to_file_name(normalized_name.clone());
            serde_json::json!({
                "fileMatch": [file_name],
                "url": format!("{SCHEMA_URI_PREFIX}action/{normalized_name}")
            })
        }));

    file_associations
}

fn generate_jsonc_schema() -> serde_json::Value {
    let generator = schemars::generate::SchemaSettings::draft2019_09()
        .with_transform(DefaultDenyUnknownFields)
        .with_transform(AllowTrailingCommas)
        .into_generator();
    let meta_schema = generator
        .settings()
        .meta_schema
        .as_ref()
        .expect("meta_schema should be present in schemars settings")
        .to_string();
    let defs = generator.definitions();
    let schema = schemars::json_schema!({
        "$schema": meta_schema,
        "allowTrailingCommas": true,
        "$defs": defs,
    });
    serde_json::to_value(schema).unwrap()
}

pub fn normalize_action_name(action_name: &str) -> String {
    action_name.replace("::", "__")
}

pub fn denormalize_action_name(action_name: &str) -> String {
    action_name.replace("__", "::")
}

pub fn normalized_action_file_name(action_name: &str) -> String {
    normalized_action_name_to_file_name(normalize_action_name(action_name))
}

pub fn normalized_action_name_to_file_name(mut normalized_action_name: String) -> String {
    normalized_action_name.push_str(".json");
    normalized_action_name
}

fn root_schema_from_action_schema(
    action_schema: Option<schemars::Schema>,
    generator: &mut schemars::SchemaGenerator,
) -> schemars::Schema {
    let Some(mut action_schema) = action_schema else {
        return schemars::json_schema!(false);
    };
    let meta_schema = generator
        .settings()
        .meta_schema
        .as_ref()
        .expect("meta_schema should be present in schemars settings")
        .to_string();
    let defs = generator.definitions();
    let mut schema = schemars::json_schema!({
        "$schema": meta_schema,
        "allowTrailingCommas": true,
        "$defs": defs,
    });
    schema
        .ensure_object()
        .extend(std::mem::take(action_schema.ensure_object()));
    schema
}

#[inline]
fn schema_file_match(path: &std::path::Path) -> String {
    path.strip_prefix(path.parent().unwrap().parent().unwrap())
        .unwrap()
        .display()
        .to_string()
        .replace('\\', "/")
}
