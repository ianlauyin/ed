use anyhow::Result;
use async_trait::async_trait;
use collections::HashMap;
use gpui::AsyncApp;
use language::{LanguageName, LspAdapter, LspAdapterDelegate, PromptResponseContext, Toolchain};
use lsp::{CodeActionKind, LanguageServerBinary, LanguageServerName, Uri};
use project::{Fs, lsp_store::language_server_settings};
use regex::Regex;
use serde_json::Value;
use serde_json::json;
use settings::update_settings_file;
use std::sync::{Arc, LazyLock};
use util::merge_json_value_into;

const ACTION_ALWAYS: &str = "Always";
const ACTION_NEVER: &str = "Never";
const UPDATE_IMPORTS_MESSAGE_PATTERN: &str = "Update imports for";
const VTSLS_SERVER_NAME: &str = "vtsls";

pub struct VtslsLspAdapter {
    fs: Arc<dyn Fs>,
}

impl VtslsLspAdapter {
    const TYPESCRIPT_TSDK_PATH: &'static str = "node_modules/typescript/lib";

    pub fn new(fs: Arc<dyn Fs>) -> Self {
        VtslsLspAdapter { fs }
    }

    async fn tsdk_path(&self, adapter: &Arc<dyn LspAdapterDelegate>) -> Option<&'static str> {
        let tsdk_path = Self::TYPESCRIPT_TSDK_PATH;

        // vtsls doesn't support TypeScript 7+, which no longer ships `tsserver.js`.
        if self
            .fs
            .is_file(
                &adapter
                    .worktree_root_path()
                    .join(tsdk_path)
                    .join("tsserver.js"),
            )
            .await
        {
            Some(tsdk_path)
        } else {
            None
        }
    }

    pub fn enhance_diagnostic_message(message: &str) -> Option<String> {
        static SINGLE_WORD_REGEX: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"'([^\s']*)'").expect("Failed to create REGEX"));

        static MULTI_WORD_REGEX: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"'([^']+\s+[^']*)'").expect("Failed to create REGEX"));

        let first = SINGLE_WORD_REGEX.replace_all(message, "`$1`").to_string();
        let second = MULTI_WORD_REGEX
            .replace_all(&first, "\n```typescript\n$1\n```\n")
            .to_string();
        Some(second)
    }
}

const SERVER_NAME: LanguageServerName = LanguageServerName::new_static("vtsls");

#[async_trait(?Send)]
impl LspAdapter for VtslsLspAdapter {
    fn name(&self) -> LanguageServerName {
        SERVER_NAME
    }

    async fn check_if_user_installed(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: Option<Toolchain>,
        _: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        crate::npx::node_server_binary(
            delegate,
            &["@vtsls/language-server"],
            SERVER_NAME.as_ref(),
            &["--stdio"],
        )
        .await
    }

    fn code_action_kinds(&self) -> Option<Vec<CodeActionKind>> {
        Some(vec![
            CodeActionKind::QUICKFIX,
            CodeActionKind::REFACTOR,
            CodeActionKind::REFACTOR_EXTRACT,
            CodeActionKind::SOURCE,
        ])
    }

    async fn label_for_completion(
        &self,
        item: &lsp::CompletionItem,
        language: &Arc<language::Language>,
    ) -> Option<language::CodeLabel> {
        use lsp::CompletionItemKind as Kind;
        let label_len = item.label.len();
        let grammar = language.grammar()?;
        let highlight_id = match item.kind? {
            Kind::CLASS | Kind::INTERFACE | Kind::ENUM => grammar.highlight_id_for_name("type"),
            Kind::CONSTRUCTOR => grammar.highlight_id_for_name("type"),
            Kind::CONSTANT => grammar.highlight_id_for_name("constant"),
            Kind::FUNCTION | Kind::METHOD => grammar.highlight_id_for_name("function"),
            Kind::PROPERTY | Kind::FIELD => grammar.highlight_id_for_name("property"),
            Kind::VARIABLE => grammar.highlight_id_for_name("variable"),
            _ => None,
        }?;

        let text = if let Some(description) = item
            .label_details
            .as_ref()
            .and_then(|label_details| label_details.description.as_ref())
        {
            format!("{} {}", item.label, description)
        } else if let Some(detail) = &item.detail {
            format!("{} {}", item.label, detail)
        } else {
            item.label.clone()
        };
        Some(language::CodeLabel::filtered(
            text,
            label_len,
            item.filter_text.as_deref(),
            vec![(0..label_len, highlight_id)],
        ))
    }

    async fn workspace_configuration(
        self: Arc<Self>,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: Option<Toolchain>,
        _: Option<Uri>,
        cx: &mut AsyncApp,
    ) -> Result<Value> {
        let tsdk_path = self.tsdk_path(delegate).await;
        let config = serde_json::json!({
            "tsdk": tsdk_path,
            "suggest": {
                "completeFunctionCalls": true
            },
            "inlayHints": {
                "parameterNames": {
                    "enabled": "all",
                    "suppressWhenArgumentMatchesName": false
                },
                "parameterTypes": {
                    "enabled": true
                },
                "variableTypes": {
                    "enabled": true,
                    "suppressWhenTypeMatchesName": false
                },
                "propertyDeclarationTypes": {
                    "enabled": true
                },
                "functionLikeReturnTypes": {
                    "enabled": true
                },
                "enumMemberValues": {
                    "enabled": true
                }
            },
            "implementationsCodeLens": {
                "enabled": true,
                "showOnAllClassMethods": true,
                "showOnInterfaceMethods": true
            },
            "referencesCodeLens": {
                "enabled": true,
                "showOnAllFunctions": true
            },
            "tsserver": {
                "maxTsServerMemory": 8192
            },
        });

        let mut default_workspace_configuration = serde_json::json!({
            "typescript": config,
            "javascript": config,
            "vtsls": {
                "experimental": {
                    "completion": {
                        "enableServerSideFuzzyMatch": true,
                        "entriesLimit": 5000,
                    }
                },
               "autoUseWorkspaceTsdk": true
            }
        });

        let override_options = cx.update(|cx| {
            language_server_settings(delegate.as_ref(), &SERVER_NAME, cx)
                .and_then(|s| s.settings.clone())
        });

        if let Some(override_options) = override_options {
            merge_json_value_into(override_options, &mut default_workspace_configuration)
        }

        Ok(default_workspace_configuration)
    }

    fn diagnostic_message_to_markdown(&self, message: &str) -> Option<String> {
        VtslsLspAdapter::enhance_diagnostic_message(message)
    }

    fn language_ids(&self) -> HashMap<LanguageName, String> {
        HashMap::from_iter([
            (LanguageName::new_static("TypeScript"), "typescript".into()),
            (LanguageName::new_static("JavaScript"), "javascript".into()),
            (LanguageName::new_static("TSX"), "typescriptreact".into()),
        ])
    }

    fn process_prompt_response(&self, context: &PromptResponseContext, cx: &mut AsyncApp) {
        let selected_title = context.selected_action.title.as_str();
        let is_preference_response =
            selected_title == ACTION_ALWAYS || selected_title == ACTION_NEVER;
        if !is_preference_response {
            return;
        }

        if context.message.contains(UPDATE_IMPORTS_MESSAGE_PATTERN) {
            let setting_value = match selected_title {
                ACTION_ALWAYS => "always",
                ACTION_NEVER => "never",
                _ => return,
            };

            let settings = json!({
                "typescript": {
                    "updateImportsOnFileMove": {
                        "enabled": setting_value
                    }
                },
                "javascript": {
                    "updateImportsOnFileMove": {
                        "enabled": setting_value
                    }
                }
            });

            let _ = cx.update(|cx| {
                update_settings_file(self.fs.clone(), cx, move |content, _| {
                    let lsp_settings = content
                        .project
                        .lsp
                        .0
                        .entry(VTSLS_SERVER_NAME.into())
                        .or_default();

                    if let Some(existing) = &mut lsp_settings.settings {
                        merge_json_value_into(settings, existing);
                    } else {
                        lsp_settings.settings = Some(settings);
                    }
                });
            });
        }
    }
}
