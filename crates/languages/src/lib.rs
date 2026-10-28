use std::sync::Arc;

use futures::FutureExt as _;
use gpui::App;
use gpui::SharedString;
use gpui::UpdateGlobal;
pub use language::*;
use project::Fs;
use python::PyprojectTomlManifestProvider;
use rust::CargoLibraryProvider;
use rust::CargoManifestProvider;
use settings::SemanticTokenRules;
use settings::SettingsStore;
use util::ResultExt;

use crate::json::JsonTaskProvider;

mod bash;
mod biome;
mod css;
mod go;
mod html;
mod json;
mod npx;
mod package_json;
mod python;
mod rust;
mod tombi;
mod tsgo;
mod typescript;
mod yaml;

pub(crate) use package_json::PackageJson;
pub(crate) use package_json::PackageJsonData;

pub fn init(languages: Arc<LanguageRegistry>, fs: Arc<dyn Fs>, cx: &mut App) {
    #[cfg(feature = "load-grammars")]
    languages.register_native_grammars(grammars::native_grammars());

    let bash_lsp_adapter = Arc::new(bash::BashLspAdapter);
    let css_lsp_adapter = Arc::new(css::CssLspAdapter);
    let biome_lsp_adapter = Arc::new(biome::BiomeLspAdapter::new(fs.clone()));
    let go_context_provider = Arc::new(go::GoContextProvider);
    let go_lsp_adapter = Arc::new(go::GoLspAdapter);
    let html_lsp_adapter = Arc::new(html::HtmlLspAdapter);
    let json_context_provider = Arc::new(JsonTaskProvider);
    let json_lsp_adapter = Arc::new(json::JsonLspAdapter::new(languages.clone()));
    let python_context_provider = Arc::new(python::PythonContextProvider);
    let rust_context_provider = Arc::new(rust::RustContextProvider);
    let rust_lsp_adapter = Arc::new(rust::RustLspAdapter);
    let typescript_context = Arc::new(typescript::TypeScriptContextProvider::new(fs.clone()));
    let tombi_lsp_adapter = Arc::new(tombi::TombiLspAdapter);
    let tsgo_lsp_adapter = Arc::new(tsgo::TsgoLspAdapter::new(fs));
    let yaml_lsp_adapter = Arc::new(yaml::YamlLspAdapter);

    let built_in_languages = [
        LanguageInfo {
            name: "bash",
            context: Some(Arc::new(bash::bash_task_context())),
            adapters: vec![bash_lsp_adapter],
            ..Default::default()
        },
        LanguageInfo { name: "c", adapters: vec![], ..Default::default() },
        LanguageInfo { name: "cpp", adapters: vec![], ..Default::default() },
        LanguageInfo { name: "css", adapters: vec![css_lsp_adapter, biome_lsp_adapter.clone()], ..Default::default() },
        LanguageInfo { name: "diff", adapters: vec![], ..Default::default() },
        LanguageInfo { name: "env", adapters: vec![], ..Default::default() },
        LanguageInfo {
            name: "go",
            adapters: vec![go_lsp_adapter.clone()],
            context: Some(go_context_provider.clone()),
            semantic_token_rules: Some(go::semantic_token_rules()),
            ..Default::default()
        },
        LanguageInfo {
            name: "gomod",
            adapters: vec![go_lsp_adapter.clone()],
            context: Some(go_context_provider.clone()),
            ..Default::default()
        },
        LanguageInfo {
            name: "gowork",
            adapters: vec![go_lsp_adapter],
            context: Some(go_context_provider),
            ..Default::default()
        },
        LanguageInfo { name: "html", adapters: vec![html_lsp_adapter], ..Default::default() },
        LanguageInfo {
            name: "json",
            adapters: vec![json_lsp_adapter.clone(), biome_lsp_adapter.clone()],
            context: Some(json_context_provider.clone()),
            ..Default::default()
        },
        LanguageInfo {
            name: "jsonc",
            adapters: vec![json_lsp_adapter, biome_lsp_adapter.clone()],
            context: Some(json_context_provider),
            ..Default::default()
        },
        LanguageInfo { name: "markdown", adapters: vec![], ..Default::default() },
        LanguageInfo { name: "markdown-inline", adapters: vec![], ..Default::default() },
        LanguageInfo {
            name: "python",
            adapters: vec![Arc::new(python::TyLspAdapter), Arc::new(python::RuffLspAdapter)],
            context: Some(python_context_provider),
            manifest_name: Some(SharedString::new_static("pyproject.toml").into()),
            semantic_token_rules: Some(python::semantic_token_rules()),
        },
        LanguageInfo {
            name: "rust",
            adapters: vec![rust_lsp_adapter],
            context: Some(rust_context_provider),
            manifest_name: Some(SharedString::new_static("Cargo.toml").into()),
            semantic_token_rules: Some(rust::semantic_token_rules()),
        },
        LanguageInfo { name: "toml", adapters: vec![tombi_lsp_adapter], ..Default::default() },
        LanguageInfo {
            name: "tsx",
            adapters: vec![tsgo_lsp_adapter.clone(), biome_lsp_adapter.clone()],
            context: Some(typescript_context.clone()),
            ..Default::default()
        },
        LanguageInfo {
            name: "typescript",
            adapters: vec![tsgo_lsp_adapter.clone(), biome_lsp_adapter.clone()],
            context: Some(typescript_context.clone()),
            ..Default::default()
        },
        LanguageInfo {
            name: "javascript",
            adapters: vec![tsgo_lsp_adapter, biome_lsp_adapter],
            context: Some(typescript_context),
            ..Default::default()
        },
        LanguageInfo { name: "yaml", adapters: vec![yaml_lsp_adapter], ..Default::default() },
        LanguageInfo { name: "gitcommit", ..Default::default() },
    ];

    for registration in built_in_languages {
        register_language(
            &languages,
            registration.name,
            registration.adapters,
            registration.context,
            registration.manifest_name,
            registration.semantic_token_rules,
            cx,
        );
    }

    let mut subscription = languages.subscribe();
    let mut prev_language_settings = languages.language_settings();

    cx.spawn(async move |cx| {
        while subscription.changed().await.is_ok() {
            let language_settings = languages.language_settings();
            if language_settings != prev_language_settings {
                cx.update(|cx| {
                    SettingsStore::update_global(cx, |settings, cx| {
                        settings
                            .set_language_defaults(
                                settings::LanguageDefaultsContent { all_languages: language_settings.clone() },
                                cx,
                            )
                            .log_err();
                    });
                });
                prev_language_settings = language_settings;
            }
        }
        anyhow::Ok(())
    })
    .detach();
    let manifest_providers: [Arc<dyn ManifestProvider>; 2] =
        [Arc::from(CargoManifestProvider), Arc::from(PyprojectTomlManifestProvider)];
    for provider in manifest_providers {
        project::ManifestProvidersStore::global(cx).register(provider);
    }
    project::library::LibraryProviders::register(Arc::new(CargoLibraryProvider), cx);
}

#[derive(Default)]
struct LanguageInfo {
    name: &'static str,
    adapters: Vec<Arc<dyn LspAdapter>>,
    context: Option<Arc<dyn ContextProvider>>,
    manifest_name: Option<ManifestName>,
    semantic_token_rules: Option<SemanticTokenRules>,
}

fn register_language(
    languages: &LanguageRegistry,
    name: &'static str,
    adapters: Vec<Arc<dyn LspAdapter>>,
    context: Option<Arc<dyn ContextProvider>>,
    manifest_name: Option<ManifestName>,
    semantic_token_rules: Option<SemanticTokenRules>,
    cx: &mut App,
) {
    let config = load_config(name);
    if let Some(rules) = &semantic_token_rules {
        SettingsStore::update_global(cx, |store, _| {
            store.set_language_semantic_token_rules(config.name.0.clone(), rules.clone());
        });
    }
    for adapter in adapters {
        languages.register_lsp_adapter(config.name.clone(), adapter);
    }
    languages.register_language(
        config.name.clone(),
        config.grammar.clone(),
        config.matcher.clone(),
        config.hidden,
        manifest_name.clone(),
        Arc::new(move || {
            let config = config.clone();
            let context = context.clone();
            let manifest_name = manifest_name.clone();
            async move {
                Ok(LoadedLanguage {
                    config,
                    queries: grammars::load_queries(name),
                    context_provider: context,
                    manifest_name,
                })
            }
            .boxed()
        }),
    );
}

fn load_config(name: &str) -> LanguageConfig {
    let grammars_loaded = cfg!(feature = "load-grammars");
    grammars::load_config_for_feature(name, grammars_loaded)
}
