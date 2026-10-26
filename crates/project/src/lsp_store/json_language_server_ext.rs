use anyhow::Context;
use anyhow::Result;
use gpui::App;
use gpui::AsyncApp;
use gpui::Entity;
use gpui::Global;
use gpui::Task;
use gpui::WeakEntity;
use lsp::LanguageServer;

use crate::LspStore;

const LOGGER: zlog::Logger = zlog::scoped!("json-schema");

/// https://github.com/Microsoft/vscode/blob/main/extensions/json-language-features/server/README.md#schema-content-request
///
/// Represents a "JSON language server-specific, non-standardized, extension to the LSP" with which the vscode-json-language-server
/// can request the contents of a schema that is associated with a uri scheme it does not support.
/// In our case, we provide the bundled schemas under the `ed://schemas/{name}` scheme and respond with their content on demand.
struct SchemaContentRequest {}

impl lsp::request::Request for SchemaContentRequest {
    type Params = Vec<String>;

    type Result = String;

    const METHOD: &'static str = "vscode/content";
}

type SchemaRequestHandler = fn(Entity<LspStore>, String, &mut AsyncApp) -> Task<Result<String>>;
pub struct SchemaHandlingImpl(SchemaRequestHandler);

impl Global for SchemaHandlingImpl {}

pub fn register_schema_handler(handler: SchemaRequestHandler, cx: &mut App) {
    debug_assert!(!cx.has_global::<SchemaHandlingImpl>(), "SchemaHandlingImpl already registered");
    cx.set_global(SchemaHandlingImpl(handler));
}

pub fn register_requests(lsp_store: WeakEntity<LspStore>, language_server: &LanguageServer) {
    language_server
        .on_request::<SchemaContentRequest, _, _>(move |params, cx| {
            let handler = cx.try_read_global::<SchemaHandlingImpl, _>(|handler, _| handler.0);
            let mut cx = cx.clone();
            let uri = params.clone().pop();
            let lsp_store = lsp_store.clone();
            let resolution = async move {
                let lsp_store = lsp_store.upgrade().context("LSP store has been dropped")?;
                let uri = uri.context("No URI")?;
                let handle_schema_request = handler.context("No schema handler registered")?;
                handle_schema_request(lsp_store, uri, &mut cx).await
            };
            async move {
                zlog::trace!(LOGGER => "Handling schema request for {:?}", &params);
                let result = resolution.await;
                match &result {
                    Ok(content) => {
                        zlog::trace!(LOGGER => "Schema request resolved with {}B schema", content.len());
                    }
                    Err(err) => {
                        zlog::warn!(LOGGER => "Schema request failed: {}", err);
                    }
                }
                result
            }
        })
        .detach();
}
