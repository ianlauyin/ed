//! The complete set of snippets. Edit these and rebuild to change snippets.
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

/// Snippet with all of the metadata
#[derive(Debug)]
pub struct SnippetDefinition {
    pub prefix: Vec<String>,
    pub body: String,
    pub description: Option<String>,
    pub name: String,
}

/// Language snippet scope id (see `LanguageName::snippet_scope_id`), or `None` for global snippets.
type SnippetKind = Option<String>;

static SNIPPETS: LazyLock<HashMap<SnippetKind, Vec<Arc<SnippetDefinition>>>> =
    LazyLock::new(snippets);

/// Returns the built-in snippets for `language`, followed by the global ones.
pub fn snippets_for(language: SnippetKind) -> Vec<Arc<SnippetDefinition>> {
    let mut snippets = SNIPPETS.get(&language).cloned().unwrap_or_default();
    if language.is_some() {
        snippets.extend(SNIPPETS.get(&None).cloned().unwrap_or_default());
    }
    snippets
}

fn snippets() -> HashMap<SnippetKind, Vec<Arc<SnippetDefinition>>> {
    let mut snippets = HashMap::default();
    snippets.insert(
        Some("rust".to_string()),
        vec![
            snippet(
                "main function",
                "main",
                "Create main function",
                &["pub fn main() {", "    $0", "}"],
            ),
            snippet(
                "async main function",
                "async_main",
                "Create async main function",
                &["#[tokio::main]", "pub async fn main() {", "    $0", "}"],
            ),
            snippet(
                "tests mod",
                "tests",
                "Create tests mod",
                &["#[cfg(test)]", "mod tests {", "    $0", "}"],
            ),
            snippet(
                "test function",
                "test",
                "Create test function",
                &["#[test]", "fn ${1:test}() {", "    $0", "}"],
            ),
            snippet(
                "async test function",
                "async_test",
                "Create async test function",
                &["#[tokio::test]", "async fn ${1:test}() {", "    $0", "}"],
            ),
        ],
    );
    snippets
}

fn snippet(name: &str, prefix: &str, description: &str, body: &[&str]) -> Arc<SnippetDefinition> {
    Arc::new(SnippetDefinition {
        prefix: vec![prefix.to_string()],
        body: body.join("\n"),
        description: Some(description.to_string()),
        name: name.to_string(),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn built_in_snippets_parse() {
        for snippets in super::snippets().values() {
            for snippet in snippets {
                crate::Snippet::parse(&snippet.body)
                    .unwrap_or_else(|e| panic!("invalid snippet '{}': {e:#}", snippet.name));
            }
        }
    }
}
