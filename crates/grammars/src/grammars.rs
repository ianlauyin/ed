use std::borrow::Cow;

use anyhow::Context as _;
use language_core::LanguageConfig;
use language_core::LanguageQueries;
use language_core::QueryFile;
use language_core::QueryFileContents;

// Dev builds read the checkout's query files at runtime instead of embedding
// them; see the `assets` crate for the rationale.
util::fs_embed! {
    struct GrammarDir,
    crate_relative = "src/",
    root_relative = "crates/grammars/src",
    exclude = ["*.rs"],
}

/// Register all built-in native tree-sitter grammars with the provided registration function.
///
/// Each grammar is registered as a `(&str, tree_sitter_language::LanguageFn)` pair.
/// This must be called before loading language configs/queries.
#[cfg(feature = "load-grammars")]
pub fn native_grammars() -> Vec<(&'static str, tree_sitter::Language)> {
    vec![
        ("bash", tree_sitter_bash::LANGUAGE.into()),
        ("css", tree_sitter_css::LANGUAGE.into()),
        ("go", tree_sitter_go::LANGUAGE.into()),
        ("html", tree_sitter_html::LANGUAGE.into()),
        ("json", tree_sitter_json::LANGUAGE.into()),
        ("jsonc", tree_sitter_json::LANGUAGE.into()),
        ("markdown", tree_sitter_md::LANGUAGE.into()),
        ("markdown-inline", tree_sitter_md::INLINE_LANGUAGE.into()),
        ("python", tree_sitter_python::LANGUAGE.into()),
        ("rust", tree_sitter_rust::LANGUAGE.into()),
        ("tsx", tree_sitter_typescript::LANGUAGE_TSX.into()),
        ("typescript", tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        ("yaml", tree_sitter_yaml::LANGUAGE.into()),
    ]
}

/// Load and parse the `config.toml` for a given language name.
pub fn load_config(name: &str) -> LanguageConfig {
    let config_toml = String::from_utf8(
        GrammarDir::get(&format!("{}/config.toml", name))
            .unwrap_or_else(|| panic!("missing config for language {:?}", name))
            .data
            .to_vec(),
    )
    .unwrap();

    LanguageConfig::from_toml(&config_toml)
        .with_context(|| format!("failed to load config.toml for language {name:?}"))
        .unwrap()
}

/// Load and parse the `config.toml` for a given language name, stripping fields
/// that require grammar support when grammars are not loaded.
pub fn load_config_for_feature(name: &str, grammars_loaded: bool) -> LanguageConfig {
    let config = load_config(name);

    if grammars_loaded {
        config
    } else {
        LanguageConfig {
            name: config.name,
            matcher: config.matcher,
            jsx_tag_auto_close: config.jsx_tag_auto_close,
            ..Default::default()
        }
    }
}

/// Get a raw embedded file by path (relative to `src/`).
///
/// Returns the file data as bytes, or `None` if the file does not exist.
pub fn get_file(path: &str) -> Option<rust_embed::EmbeddedFile> {
    GrammarDir::get(path)
}

/// Load all Tree-sitter query files for a given language name.
pub fn load_queries(name: &str) -> LanguageQueries {
    LanguageQueries::from_files(GrammarDir::iter().filter_map(|path| {
        let file_name = path.strip_prefix(name)?.strip_prefix('/')?;
        let query_file = file_name.parse::<QueryFile>().ok()?;
        let contents = match GrammarDir::get(path.as_ref())?.data {
            Cow::Borrowed(bytes) => Cow::Borrowed(std::str::from_utf8(bytes).ok()?),
            Cow::Owned(bytes) => Cow::Owned(String::from_utf8(bytes).ok()?),
        };
        Some(QueryFileContents::new(query_file, contents))
    }))
}

#[cfg(all(test, feature = "load-grammars"))]
mod tests {
    use std::collections::BTreeMap;
    use std::ops::Range;

    use tree_sitter::Parser;
    use tree_sitter::Query;
    use tree_sitter::QueryCursor;
    use tree_sitter::StreamingIterator;

    use super::load_config;
    use super::load_queries;
    use super::native_grammars;

    /// The capture each node is highlighted with. As in the editor, when several
    /// patterns capture the same node the last one wins.
    fn highlights(language_name: &str, source: &str) -> Vec<(String, String)> {
        let grammar_name = load_config(language_name).grammar.expect("language has a grammar");
        let grammar = native_grammars()
            .into_iter()
            .find(|(name, _)| *name == grammar_name.as_ref())
            .map(|(_, grammar)| grammar)
            .expect("grammar is registered");
        let query_source = load_queries(language_name).highlights.expect("language has highlights");
        let query = Query::new(&grammar, &query_source).unwrap();
        let mut parser = Parser::new();
        parser.set_language(&grammar).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut by_range: BTreeMap<(usize, usize), String> = BTreeMap::new();
        let mut cursor = QueryCursor::new();
        let mut captures = cursor.captures(&query, tree.root_node(), source.as_bytes());
        while let Some((query_match, index)) = captures.next() {
            let capture = query_match.captures()[*index];
            let range: Range<usize> = capture.node.byte_range();
            by_range.insert((range.start, range.end), query.capture_names()[capture.index as usize].to_string());
        }
        by_range.into_iter().map(|((start, end), capture)| (source[start..end].to_string(), capture)).collect()
    }

    fn assert_highlight(highlights: &[(String, String)], text: &str, capture: &str) {
        assert!(
            highlights.contains(&(text.to_string(), capture.to_string())),
            "{text:?} not {capture:?}: {highlights:?}"
        );
    }

    #[test]
    fn js_family_separates_definitions_calls_and_keywords() {
        let source = "function make(size) {\n  const total = size;\n  if (total) { return helper(total); }\n}\nclass A { run() { this.go(); } }\nconst arrow = () => {};\n";
        for language_name in ["javascript", "typescript", "tsx"] {
            let highlights = highlights(language_name, source);
            assert_highlight(&highlights, "make", "function.definition");
            assert_highlight(&highlights, "run", "function.definition");
            assert_highlight(&highlights, "arrow", "function.definition");
            assert_highlight(&highlights, "helper", "function");
            assert_highlight(&highlights, "go", "function.method");
            assert_highlight(&highlights, "size", "variable.parameter");
            assert_highlight(&highlights, "total", "variable");
            assert_highlight(&highlights, "const", "keyword.declaration");
            assert_highlight(&highlights, "if", "keyword.control");
        }
    }

    #[test]
    fn css_separates_selectors_properties_variables_and_values() {
        let highlights = highlights("css", ".card { --gap: 4px; display: flex; width: calc(var(--gap) * 2); }\n");
        assert_highlight(&highlights, "card", "selector.class");
        assert_highlight(&highlights, "display", "property");
        assert_highlight(&highlights, "--gap", "variable");
        assert_highlight(&highlights, "flex", "constant.builtin");
        assert_highlight(&highlights, "calc", "function");
        assert_highlight(&highlights, "px", "type.unit");
    }

    #[test]
    fn json_separates_keys_and_value_kinds() {
        for language_name in ["json", "jsonc"] {
            let highlights = highlights(language_name, "{\"name\": \"ed\", \"size\": 1, \"ok\": true, \"none\": null}");
            assert_highlight(&highlights, "\"name\"", "property.json_key");
            assert_highlight(&highlights, "\"ed\"", "string");
            assert_highlight(&highlights, "1", "number");
            assert_highlight(&highlights, "true", "boolean");
            assert_highlight(&highlights, "null", "constant.builtin");
        }
    }
}
