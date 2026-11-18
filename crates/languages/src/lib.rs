use std::borrow::Cow;
use std::str;
use std::sync::Arc;

use futures::FutureExt as _;
use gpui::App;
use language::language_settings::AllLanguageSettings;
use language::language_settings::LanguageSettings;
use language::language_settings::SemanticTokenRules;
#[allow(clippy::wildcard_imports)]
pub use language::*;
use language_core::QueryFile;
use language_core::QueryFileContents;
use project::Fs;
use project::library::LibraryProviders;
use python::PyprojectTomlManifestProvider;
use rust::CargoLibraryProvider;
use rust::CargoManifestProvider;
use rust_embed::RustEmbed;

mod bash;
mod biome;
mod css;
mod diff;
mod env;
mod git_commit;
mod go;
mod html;
mod json;
mod markdown;
mod npx;
mod package_json;
mod plain_text;
mod python;
mod rust;
mod stylelint;
mod terraform;
mod tombi;
mod tsgo;
mod typescript;
mod yaml;

pub(crate) use package_json::PackageJson;
pub(crate) use package_json::PackageJsonData;

pub fn init(languages: Arc<LanguageRegistry>, fs: Arc<dyn Fs>, cx: &mut App) {
    let biome: Arc<dyn LspAdapter> = Arc::new(biome::BiomeLspAdapter::new(Arc::clone(&fs)));
    let definitions = [
        bash::languages(),
        css::languages(Arc::clone(&biome), Arc::clone(&fs)),
        diff::languages(),
        env::languages(),
        git_commit::languages(),
        go::languages(),
        html::languages(),
        json::languages(Arc::clone(&languages), Arc::clone(&biome)),
        markdown::languages(),
        python::languages(),
        rust::languages(),
        terraform::languages(),
        tombi::languages(),
        typescript::languages(fs, biome),
        yaml::languages(),
    ];
    for definition in definitions.into_iter().flatten() {
        register(&languages, definition, cx);
    }
    AllLanguageSettings::register(PLAIN_TEXT.name(), plain_text::settings(), &[], None, cx);

    let manifest_providers: [Arc<dyn ManifestProvider>; 2] =
        [Arc::from(CargoManifestProvider), Arc::from(PyprojectTomlManifestProvider)];
    for provider in manifest_providers {
        project::ManifestProvidersStore::global(cx).register(provider);
    }
    LibraryProviders::register(Arc::new(CargoLibraryProvider), cx);
}

/// Everything that defines a language; each language module builds its own.
pub(crate) struct LanguageDef {
    /// Directory under `src/` holding the language's `config.toml` and queries.
    dir: &'static str,
    grammar: Option<tree_sitter::Language>,
    /// Language servers in start order; the first is the primary server.
    adapters: Vec<Arc<dyn LspAdapter>>,
    context: Option<Arc<dyn ContextProvider>>,
    manifest_name: Option<ManifestName>,
    semantic_token_rules: Option<SemanticTokenRules>,
    settings: LanguageSettings,
    /// File globs that select the language before path suffixes do.
    file_types: &'static [&'static str],
}

impl LanguageDef {
    /// A language with no grammar, servers or tasks.
    pub(crate) const fn new(dir: &'static str, settings: LanguageSettings) -> Self {
        Self {
            dir,
            grammar: None,
            adapters: Vec::new(),
            context: None,
            manifest_name: None,
            semantic_token_rules: None,
            settings,
            file_types: &[],
        }
    }
}

fn register(languages: &LanguageRegistry, definition: LanguageDef, cx: &mut App) {
    let LanguageDef { dir, grammar, adapters, context, manifest_name, semantic_token_rules, settings, file_types } =
        definition;
    let config = load_config(dir);
    AllLanguageSettings::register(config.name.clone(), settings, file_types, semantic_token_rules, cx);
    for adapter in adapters {
        languages.register_lsp_adapter(config.name.clone(), adapter);
    }
    languages.register_language(
        config.name.clone(),
        Arc::clone(&config.matcher),
        config.hidden,
        manifest_name.clone(),
        Arc::new(move || {
            let loaded = LoadedLanguage {
                config: config.clone(),
                grammar: grammar.clone(),
                queries: load_queries(dir),
                context_provider: context.clone(),
                manifest_name: manifest_name.clone(),
            };
            async move { Ok(loaded) }.boxed()
        }),
    );
}

/// Each language's `config.toml` and tree-sitter queries, by directory.
#[derive(RustEmbed)]
#[folder = "src/"]
#[exclude = "*.rs"]
struct LanguageDir;

fn load_config(dir: &str) -> LanguageConfig {
    let file = LanguageDir::get(&format!("{dir}/config.toml")).unwrap_or_else(|| panic!("missing {dir}/config.toml"));
    LanguageConfig::from_toml(str::from_utf8(&file.data).expect("bundled config is utf-8"))
        .unwrap_or_else(|error| panic!("invalid {dir}/config.toml: {error:#}"))
}

fn load_queries(dir: &str) -> LanguageQueries {
    LanguageQueries::from_files(LanguageDir::iter().filter_map(|path| {
        let query_file = path.strip_prefix(dir)?.strip_prefix('/')?.parse::<QueryFile>().ok()?;
        let contents = match LanguageDir::get(path.as_ref())?.data {
            Cow::Borrowed(bytes) => Cow::Borrowed(str::from_utf8(bytes).ok()?),
            Cow::Owned(bytes) => Cow::Owned(String::from_utf8(bytes).ok()?),
        };
        Some(QueryFileContents::new(query_file, contents))
    }))
}

/// Panics unless the queries in `dir` compile against `grammar`.
#[cfg(test)]
fn assert_queries_compile(dir: &str, grammar: tree_sitter::Language) {
    if let Err(error) = Language::new(load_config(dir), Some(grammar)).with_queries(load_queries(dir)) {
        panic!("{dir}: {error:#}");
    }
}

#[cfg(test)]
mod tests {
    use language::language_settings::RewrapBehavior;
    use language::language_settings::WordsCompletionMode;
    use rustc_hash::FxHashSet;

    use super::*;

    #[test]
    fn every_language_dir_has_a_loadable_config() {
        let dirs: FxHashSet<_> =
            LanguageDir::iter().filter_map(|path| Some(path.split_once('/')?.0.to_owned())).collect();
        assert!(dirs.len() > 20);
        for dir in dirs {
            let has_queries = load_queries(&dir).highlights.is_some();
            assert!(!load_config(&dir).name.as_ref().is_empty());
            // Only the grammar-less languages ship no queries.
            let grammarless =
                ["diff", "gitcommit", "gomod", "gowork", "terraform", "terraform-vars", "toml"].contains(&&*dir);
            assert_eq!(has_queries, !grammarless, "{dir}");
        }
    }

    #[test]
    fn queries_compile_against_each_language_grammar() {
        let definitions = [
            bash::languages(),
            env::languages(),
            go::languages(),
            html::languages(),
            markdown::languages(),
            python::languages(),
            rust::languages(),
            yaml::languages(),
        ];
        for definition in definitions.into_iter().flatten() {
            if let Some(grammar) = definition.grammar {
                assert_queries_compile(definition.dir, grammar);
            }
        }
        // These modules need adapters to build their definitions.
        assert_queries_compile("css", tree_sitter_css::LANGUAGE.into());
        assert_queries_compile("json", tree_sitter_json::LANGUAGE.into());
        assert_queries_compile("jsonc", tree_sitter_json::LANGUAGE.into());
        for (dir, grammar) in typescript::grammars() {
            assert_queries_compile(dir, grammar);
        }
    }

    #[test]
    fn language_settings_differ_where_languages_need_it() {
        let fallback = LanguageSettings::fallback();
        assert_eq!(rust::settings(), fallback);

        assert!(go::settings().hard_tabs && go::mod_settings().hard_tabs);

        for settings in
            [json::settings(), typescript::settings(), yaml::settings(), markdown::settings(), terraform::settings()]
        {
            assert_eq!(settings.tab_size.get(), 2);
        }
        assert!(!yaml::settings().auto_indent_on_paste);

        let markdown = markdown::settings();
        assert!(markdown.soft_wrap);
        assert_eq!(markdown.word_completions, WordsCompletionMode::Disabled);
        assert_eq!(markdown.allow_rewrap, RewrapBehavior::Anywhere);

        for settings in [plain_text::settings(), git_commit::settings()] {
            assert!(settings.soft_wrap);
            assert_eq!(settings.allow_rewrap, RewrapBehavior::Anywhere);
        }
    }
}
