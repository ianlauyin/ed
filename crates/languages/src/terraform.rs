use std::num::NonZeroU32;
use std::sync::Arc;

use async_trait::async_trait;
use gpui::AsyncApp;
use language::LanguageName;
use language::LanguageServerName;
use language::LspAdapter;
use language::LspAdapterDelegate;
use language::language_settings::LanguageSettings;
use language::language_settings::RewrapBehavior;
use language::language_settings::WordsCompletionMode;
use lsp::LanguageServerBinary;
use rustc_hash::FxHashMap;

use crate::LanguageDef;

pub(crate) struct TerraformLspAdapter;

impl TerraformLspAdapter {
    const SERVER_NAME: &str = "terraform-ls";
}

#[async_trait(?Send)]
impl LspAdapter for TerraformLspAdapter {
    fn name(&self) -> LanguageServerName {
        LanguageServerName::new_static(Self::SERVER_NAME)
    }

    async fn check_if_user_installed(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        let path = delegate.which(Self::SERVER_NAME.as_ref()).await?;
        Some(LanguageServerBinary { path, arguments: vec!["serve".into()], env: None })
    }

    fn language_ids(&self) -> FxHashMap<LanguageName, String> {
        FxHashMap::from_iter([(LanguageName::new_static("Terraform Vars"), "terraform-vars".to_owned())])
    }
}

/// Settings for Terraform and Terraform Vars.
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

pub(crate) fn languages() -> Vec<LanguageDef> {
    let adapter: Arc<dyn LspAdapter> = Arc::new(TerraformLspAdapter);
    vec![
        LanguageDef { adapters: vec![Arc::clone(&adapter)], ..LanguageDef::new("terraform", settings()) },
        LanguageDef { adapters: vec![adapter], ..LanguageDef::new("terraform-vars", settings()) },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_ids_match_terraform_ls() {
        let adapter = TerraformLspAdapter;
        assert_eq!(adapter.name().0.as_ref(), "terraform-ls");
        assert_eq!(adapter.language_ids()[&LanguageName::new_static("Terraform Vars")], "terraform-vars");
        // `.tf` falls back to the lowercased language name.
        assert_eq!(LanguageName::new_static("Terraform").lsp_id(), "terraform");
    }

    #[test]
    fn languages_match_terraform_files_without_a_grammar() {
        for (name, suffix) in [("terraform", "tf"), ("terraform-vars", "tfvars")] {
            let config = crate::load_config(name);
            assert_eq!(config.matcher.path_suffixes, [suffix]);
        }
        assert!(languages().iter().all(|language| language.grammar.is_none() && language.adapters.len() == 1));
    }
}
