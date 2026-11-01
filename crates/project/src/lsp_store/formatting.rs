use std::collections::BTreeMap;
use std::ops::Range;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use std::vec;

use anyhow::Context as _;
use anyhow::Result;
use async_macos::command;
use futures::AsyncWriteExt as _;
use gpui::App;
use gpui::AppContext as _;
use gpui::AsyncApp;
use gpui::Context;
use gpui::Entity;
use gpui::Task;
use gpui::WeakEntity;
use itertools::Itertools as _;
use language::Buffer;
use language::CachedLspAdapter;
use language::Diff;
use language::LocalFile as _;
use language::ToOffset as _;
use language::ToPointUtf16;
use language::Transaction;
use language::language_settings::Formatter;
use language::language_settings::LanguageServerFormatterSpecifier;
use language::language_settings::LanguageSettings;
use language::point_to_lsp;
use language::range_to_lsp;
use lsp::CodeActionKind;
use lsp::Edit;
use lsp::LSP_REQUEST_TIMEOUT;
use lsp::LanguageServer;
use lsp::OneOf;
use lsp::TextEdit;
use lsp::request::ExecuteCommand;
use lsp::request::Formatting;
use lsp::request::RangeFormatting;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use text::Anchor;
use text::BufferId;
use text::LineEnding;
use text::OffsetRangeExt as _;
use text::ToPoint as _;
use util::defer;
use worktree::File;

use super::LanguageServerToQuery;
use super::LocalLspStore;
use super::LspStore;
use super::capabilities::lsp_ranges_conflict;
use super::capabilities::lsp_ranges_overlap;
use super::capabilities::text_document_capabilities_for_buffer;
use crate::CodeAction;
use crate::ProjectItem as _;
use crate::ProjectTransaction;
use crate::biome;
use crate::lsp_command;
use crate::lsp_command::GetCodeActions;
use crate::lsp_command::OnTypeFormatting;
use crate::lsp_command::file_path_to_lsp_url;

pub enum LspFormatTarget {
    Buffers,
    Ranges(BTreeMap<BufferId, Vec<Range<Anchor>>>),
}

#[derive(Debug)]
pub struct FormattableBuffer {
    handle: Entity<Buffer>,
    abs_path: Option<PathBuf>,
    env: Option<FxHashMap<String, String>>,
    ranges: Option<Vec<Range<Anchor>>>,
}

fn buffer_supports_lsp_formatting(
    local: &LocalLspStore,
    buffer: &Buffer,
    adapter: &CachedLspAdapter,
    server: &LanguageServer,
) -> bool {
    text_document_capabilities_for_buffer(local, "textDocument/formatting", buffer, adapter, server)
        .any(|capabilities| server_capabilities_support_formatting(capabilities.server_capabilities))
}

fn buffer_supports_lsp_range_formatting(
    local: &LocalLspStore,
    buffer: &Buffer,
    adapter: &CachedLspAdapter,
    server: &LanguageServer,
) -> bool {
    text_document_capabilities_for_buffer(local, "textDocument/rangeFormatting", buffer, adapter, server)
        .any(|capabilities| server_capabilities_support_range_formatting(capabilities.server_capabilities))
}

fn server_capabilities_support_formatting(capabilities: &lsp::ServerCapabilities) -> bool {
    matches!(
        capabilities.document_formatting_provider.as_ref(),
        Some(provider) if *provider != OneOf::Left(false)
    )
}

fn server_capabilities_support_range_formatting(capabilities: &lsp::ServerCapabilities) -> bool {
    matches!(
        capabilities.document_range_formatting_provider.as_ref(),
        Some(provider) if *provider != OneOf::Left(false)
    )
}

/// Apply edits to the buffer that will become part of the formatting transaction.
/// Fails if the buffer has been edited since the start of that transaction.
fn extend_formatting_transaction(
    buffer: &FormattableBuffer,
    formatting_transaction_id: text::TransactionId,
    cx: &mut AsyncApp,
    operation: impl FnOnce(&mut Buffer, &mut Context<Buffer>),
) -> anyhow::Result<()> {
    buffer.handle.update(cx, |buffer, cx| {
        let last_transaction_id = buffer.peek_undo_stack().map(text::HistoryEntry::transaction_id);
        if last_transaction_id != Some(formatting_transaction_id) {
            anyhow::bail!("Buffer edited while formatting. Aborting")
        }
        buffer.start_transaction();
        operation(buffer, cx);
        if let Some(transaction_id) = buffer.end_transaction(cx) {
            buffer.merge_transactions(transaction_id, formatting_transaction_id);
        }
        Ok(())
    })
}

impl LocalLspStore {
    async fn format_locally(
        lsp_store: WeakEntity<LspStore>,
        mut buffers: Vec<FormattableBuffer>,
        push_to_history: bool,
        cx: &mut AsyncApp,
    ) -> anyhow::Result<ProjectTransaction> {
        // Do not allow multiple concurrent formatting requests for the
        // same buffer.
        lsp_store.update(cx, |this, cx| {
            let this = &mut this.local;
            buffers.retain(|buffer| this.buffers_being_formatted.insert(buffer.handle.read(cx).buffer_id()));
        })?;

        let _cleanup = defer({
            let this = lsp_store.clone();
            let mut cx = cx.clone();
            let buffers = &buffers;
            move || {
                this.update(&mut cx, |this, cx| {
                    let this = &mut this.local;
                    for buffer in buffers {
                        this.buffers_being_formatted.remove(&buffer.handle.read(cx).buffer_id());
                    }
                })
                .ok();
            }
        });

        let mut project_transaction = ProjectTransaction::default();
        let mut format_error = None;

        for buffer in &buffers {
            // Create an empty transaction to hold all of the formatting edits.
            let formatting_transaction_id = buffer.handle.update(cx, |buffer, cx| {
                // ensure no transactions created while formatting are
                // grouped with the previous transaction in the history
                // based on the transaction group interval
                buffer.finalize_last_transaction();
                buffer.start_transaction().context("transaction already open")?;
                buffer.end_transaction(cx);
                let transaction_id = buffer.push_empty_transaction(cx.background_executor().now());
                buffer.finalize_last_transaction();
                anyhow::Ok(transaction_id)
            })?;

            let result = Self::format_buffer_locally(lsp_store.clone(), buffer, formatting_transaction_id, cx).await;

            buffer.handle.update(cx, |buffer, cx| {
                let Some(formatting_transaction) = buffer.get_transaction(formatting_transaction_id).cloned() else {
                    log::warn!("no formatting transaction");
                    return;
                };
                if formatting_transaction.edit_ids.is_empty() {
                    buffer.forget_transaction(formatting_transaction_id);
                    return;
                }
                if !push_to_history {
                    buffer.forget_transaction(formatting_transaction.id);
                }
                project_transaction.0.insert(cx.entity(), formatting_transaction);
            });

            // Keep formatting the remaining buffers on failure, surfacing the first error.
            if let Err(error) = result {
                format_error.get_or_insert(error);
            }
        }

        match format_error {
            Some(error) => Err(error),
            None => Ok(project_transaction),
        }
    }

    async fn format_buffer_locally(
        lsp_store: WeakEntity<LspStore>,
        buffer: &FormattableBuffer,
        formatting_transaction_id: clock::Lamport,
        cx: &mut AsyncApp,
    ) -> Result<()> {
        let (adapters_and_servers, settings, request_timeout) = lsp_store.update(cx, |lsp_store, cx| {
            buffer.handle.update(cx, |buffer, cx| {
                let adapters_and_servers =
                    if LocalLspStore::language_server_line_length_limit_exceeded(buffer).is_some() {
                        Vec::new()
                    } else {
                        lsp_store
                            .local
                            .language_servers_for_buffer(buffer, cx)
                            .map(|(adapter, lsp)| (Arc::clone(adapter), Arc::clone(lsp)))
                            .collect::<Vec<_>>()
                    };
                let settings = LanguageSettings::for_buffer(buffer, cx);
                let request_timeout = LSP_REQUEST_TIMEOUT;
                (adapters_and_servers, settings, request_timeout)
            })
        })?;

        // When formatting a selection, only the rows it spans may be touched.
        let selection_row_ranges = buffer.ranges.as_ref().map(|ranges| {
            buffer.handle.read_with(cx, |buffer, _cx| {
                let snapshot = buffer.snapshot();
                ranges
                    .iter()
                    .map(|range| {
                        let start = range.start.to_point(&snapshot);
                        let end = range.end.to_point(&snapshot);
                        // A selection ending at column 0 of a row only includes that row's
                        // preceding line break, not its content, so it shouldn't be trimmed.
                        let end_row = if end.column == 0 && end.row > start.row { end.row } else { end.row + 1 };
                        start.row..end_row
                    })
                    .collect::<Vec<_>>()
            })
        });

        // handle whitespace formatting
        if settings.remove_trailing_whitespace_on_save {
            let diff = buffer
                .handle
                .read_with(cx, |buffer, cx| buffer.remove_trailing_whitespace(selection_row_ranges.as_deref(), cx))
                .await;
            extend_formatting_transaction(buffer, formatting_transaction_id, cx, |buffer, cx| {
                buffer.apply_diff(diff, cx);
            })?;
        }

        if settings.ensure_final_newline_on_save {
            let diff =
                buffer.handle.read_with(cx, |buffer, _cx| buffer.ensure_final_newline(selection_row_ranges.as_deref()));
            extend_formatting_transaction(buffer, formatting_transaction_id, cx, |buffer, cx| {
                buffer.apply_diff(diff, cx);
            })?;
        }

        // Line endings are always normalized to LF.
        buffer.handle.update(cx, |buffer, cx| {
            if buffer.line_ending() != LineEnding::Unix {
                buffer.set_line_ending(LineEnding::Unix, cx);
            }
        });

        // Formatter for `code_actions_on_format` that runs before
        // the rest of the formatters
        let code_actions_on_format_formatters = settings
            .code_actions_on_format
            .iter()
            .filter(|(_, enabled)| **enabled)
            .map(|(action, _)| Formatter::CodeAction(action.clone()))
            .collect::<Vec<_>>();
        let formatters = code_actions_on_format_formatters.iter().chain([&settings.formatter]);

        // Only explicitly configured formatters surface their failures;
        // auto-resolved ones stay silent so a missing optional tool
        // (e.g. biome) does not warn on every save.
        let mut format_error = None;
        let auto_uses_biome =
            buffer.handle.read_with(cx, |buffer, _| biome::supports(buffer.language().map(AsRef::as_ref)));
        for formatter in formatters {
            let is_auto = formatter == &Formatter::Auto;
            let result = if is_auto && auto_uses_biome {
                Self::apply_biome_formatter(buffer, formatting_transaction_id, cx).await
            } else {
                let formatter = if is_auto {
                    &Formatter::LanguageServer(LanguageServerFormatterSpecifier::Current)
                } else {
                    formatter
                };
                Self::apply_formatter(
                    formatter,
                    &lsp_store,
                    buffer,
                    formatting_transaction_id,
                    &adapters_and_servers,
                    &settings,
                    request_timeout,
                    cx,
                )
                .await
            };
            if let Err(err) = result {
                log::error!("Formatter failed, skipping: {err:#}");
                if !is_auto {
                    format_error.get_or_insert(err);
                }
            }
        }

        match format_error {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    async fn apply_biome_formatter(
        buffer: &FormattableBuffer,
        formatting_transaction_id: clock::Lamport,
        cx: &mut AsyncApp,
    ) -> anyhow::Result<()> {
        // Biome formats whole files only. For selection formatting
        // (FormatSelections) we format the whole buffer and keep only the
        // edits that overlap the selected byte ranges.
        let byte_ranges = buffer.ranges.as_ref().map(|ranges| {
            buffer.handle.read_with(cx, |buffer, _| {
                let snapshot = buffer.snapshot();
                ranges
                    .iter()
                    .map(|range| range.start.to_offset(&snapshot)..range.end.to_offset(&snapshot))
                    .collect::<Vec<_>>()
            })
        });

        let diff = Self::format_via_biome(buffer, cx).await?;
        let Some(mut diff) = diff else {
            return Ok(());
        };

        if let Some(byte_ranges) = byte_ranges {
            diff.edits.retain(|(edit_range, _)| {
                byte_ranges.iter().any(|selection_range| {
                    edit_range.start < selection_range.end && edit_range.end > selection_range.start
                })
            });
            if diff.edits.is_empty() {
                return Ok(());
            }
        }

        extend_formatting_transaction(buffer, formatting_transaction_id, cx, |buffer, cx| {
            buffer.apply_diff(diff, cx);
        })?;
        Ok(())
    }

    async fn apply_formatter(
        formatter: &Formatter,
        lsp_store: &WeakEntity<LspStore>,
        buffer: &FormattableBuffer,
        formatting_transaction_id: clock::Lamport,
        adapters_and_servers: &[(Arc<CachedLspAdapter>, Arc<LanguageServer>)],
        settings: &LanguageSettings,
        request_timeout: Duration,
        cx: &mut AsyncApp,
    ) -> anyhow::Result<()> {
        match formatter {
            Formatter::Auto => {
                log::error!("Auto resolved above");
                return Ok(());
            }
            Formatter::LanguageServer(specifier) => {
                let Some(buffer_path_abs) = buffer.abs_path.as_ref() else {
                    log::warn!(
                        "Cannot format buffer that is not backed by a file on disk using language servers. Skipping"
                    );
                    return Ok(());
                };

                let adapter_and_server = match specifier {
                    LanguageServerFormatterSpecifier::Specific { name } => {
                        adapters_and_servers.iter().find(|(adapter, _)| adapter.name.0.as_ref() == name).cloned()
                    }
                    LanguageServerFormatterSpecifier::Current => lsp_store.read_with(cx, |lsp_store, cx| {
                        let local = &lsp_store.local;
                        let buffer_snapshot = buffer.handle.read(cx);
                        let adapter_and_server = if buffer.ranges.is_some() {
                            adapters_and_servers.iter().find(|(adapter, server)| {
                                buffer_supports_lsp_range_formatting(local, buffer_snapshot, adapter, server)
                            })
                        } else {
                            adapters_and_servers.iter().find(|(adapter, server)| {
                                buffer_supports_lsp_formatting(local, buffer_snapshot, adapter, server)
                                    || buffer_supports_lsp_range_formatting(local, buffer_snapshot, adapter, server)
                            })
                        };
                        adapter_and_server.cloned()
                    })?,
                };

                let Some((adapter, language_server)) = adapter_and_server else {
                    return Ok(());
                };

                let edits = if let Some(ranges) = buffer.ranges.as_ref() {
                    let range_edits = Self::format_ranges_via_lsp(
                        lsp_store,
                        &buffer.handle,
                        ranges,
                        buffer_path_abs,
                        &adapter,
                        &language_server,
                        settings,
                        cx,
                    )
                    .await
                    .context("Failed to format ranges via language server")?;

                    range_edits.unwrap_or_default()
                } else {
                    Self::format_via_lsp(
                        lsp_store,
                        &buffer.handle,
                        buffer_path_abs,
                        &adapter,
                        &language_server,
                        settings,
                        cx,
                    )
                    .await
                    .context("failed to format via language server")?
                };

                if edits.is_empty() {
                    return Ok(());
                }
                extend_formatting_transaction(buffer, formatting_transaction_id, cx, |buffer, cx| {
                    buffer.edit(edits, None, cx);
                })?;
            }
            Formatter::CodeAction(code_action_name) => {
                let Some(buffer_path_abs) = buffer.abs_path.as_ref() else {
                    log::warn!(
                        "Cannot format buffer that is not backed by a file on disk using code actions. Skipping"
                    );
                    return Ok(());
                };

                let code_action_kind: CodeActionKind = code_action_name.clone().into();

                let mut actions_and_servers = Vec::new();

                for (index, (_, language_server)) in adapters_and_servers.iter().enumerate() {
                    let actions_result = Self::get_server_code_actions_from_action_kinds(
                        lsp_store,
                        language_server.server_id(),
                        vec![code_action_kind.clone()],
                        &buffer.handle,
                        cx,
                    )
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to resolve code action {:?} with language server {}",
                            code_action_kind,
                            language_server.name()
                        )
                    });
                    let Ok(actions) = actions_result else {
                        // note: it may be better to set result to the error and break formatters here
                        // but for now we try to execute the actions that we can resolve and skip the rest
                        log::error!(
                            "Failed to resolve code action {:?} with language server {}",
                            code_action_kind,
                            language_server.name()
                        );
                        continue;
                    };
                    for action in actions {
                        actions_and_servers.push((action, index));
                    }
                }

                if actions_and_servers.is_empty() {
                    log::warn!("No code actions were resolved, continuing");
                    return Ok(());
                }

                'actions: for (mut action, server_index) in actions_and_servers {
                    let server = &adapters_and_servers[server_index].1;

                    let describe_code_action = |action: &CodeAction| {
                        format!(
                            "code action '{}' with title \"{}\" on server {}",
                            action.lsp_action.action_kind().unwrap_or_else(|| "unknown".into()).as_str(),
                            action.lsp_action.title(),
                            server.name(),
                        )
                    };

                    let can_resolve_actions = lsp_store.update(cx, |lsp_store, cx| {
                        lsp_store.text_document_capability_matches_for_server(
                            &buffer.handle,
                            server.server_id(),
                            "textDocument/codeAction",
                            |capabilities| GetCodeActions::can_resolve_actions(capabilities.server_capabilities),
                            cx,
                        )
                    })?;
                    if let Err(err) =
                        Self::try_resolve_code_action(server, &mut action, can_resolve_actions, request_timeout).await
                    {
                        log::error!("Failed to resolve {}. Error: {}", describe_code_action(&action), err);
                        continue;
                    }

                    if let Some(edit) = action.lsp_action.edit().cloned() {
                        // NOTE: code below duplicated from `Self::deserialize_workspace_edit`
                        // but filters out and logs warnings for code actions that require unreasonably
                        // difficult handling on our part, such as:
                        // - applying edits that call commands
                        //   which can result in arbitrary workspace edits being sent from the server that
                        //   have no way of being tied back to the command that initiated them (i.e. we
                        //   can't know which edits are part of the format request, or if the server is done sending
                        //   actions in response to the command)
                        // - actions that create/delete/modify/rename files other than the one we are formatting
                        //   as we then would need to handle such changes correctly in the local history as well
                        //   as the remote history through the ProjectTransaction
                        // - actions with snippet edits, as these simply don't make sense in the context of a format request
                        // Supporting these actions is not impossible, but not supported as of yet.
                        if edit.changes.is_none() && edit.document_changes.is_none() {
                            continue;
                        }

                        let mut operations = Vec::new();
                        if let Some(document_changes) = edit.document_changes {
                            match document_changes {
                                lsp::DocumentChanges::Edits(edits) => {
                                    operations.extend(edits.into_iter().map(lsp::DocumentChangeOperation::Edit));
                                }
                                lsp::DocumentChanges::Operations(ops) => operations = ops,
                            }
                        } else if let Some(changes) = edit.changes {
                            operations.extend(changes.into_iter().map(|(uri, edits)| {
                                lsp::DocumentChangeOperation::Edit(lsp::TextDocumentEdit {
                                    text_document: lsp::OptionalVersionedTextDocumentIdentifier { uri, version: None },
                                    edits: edits.into_iter().map(Edit::Plain).collect(),
                                })
                            }));
                        }

                        let mut edits = Vec::with_capacity(operations.len());

                        if operations.is_empty() {
                            continue;
                        }
                        for operation in operations {
                            let op = match operation {
                                lsp::DocumentChangeOperation::Edit(op) => op,
                                lsp::DocumentChangeOperation::Op(_) => {
                                    log::warn!(
                                        "Code actions which create, delete, or rename files are not supported on format. Skipping {}",
                                        describe_code_action(&action),
                                    );
                                    continue 'actions;
                                }
                            };
                            let Ok(file_path) = op.text_document.uri.to_file_path() else {
                                log::warn!(
                                    "Failed to convert URI '{:?}' to file path. Skipping {}",
                                    op.text_document.uri,
                                    describe_code_action(&action),
                                );
                                continue 'actions;
                            };
                            if &file_path != buffer_path_abs {
                                log::warn!(
                                    "File path '{:?}' does not match buffer path '{:?}'. Skipping {}",
                                    file_path,
                                    buffer_path_abs,
                                    describe_code_action(&action),
                                );
                                continue 'actions;
                            }

                            let mut lsp_edits = Vec::new();
                            for edit in op.edits {
                                match edit {
                                    Edit::Plain(edit) => {
                                        if !lsp_edits.contains(&edit) {
                                            lsp_edits.push(edit);
                                        }
                                    }
                                    Edit::Annotated(edit) => {
                                        if !lsp_edits.contains(&edit.text_edit) {
                                            lsp_edits.push(edit.text_edit);
                                        }
                                    }
                                    Edit::Snippet(_) => {
                                        log::warn!(
                                            "Code actions which produce snippet edits are not supported during formatting. Skipping {}",
                                            describe_code_action(&action),
                                        );
                                        continue 'actions;
                                    }
                                }
                            }
                            let edits_result = lsp_store
                                .update(cx, |lsp_store, cx| {
                                    lsp_store.local.edits_from_lsp(
                                        &buffer.handle,
                                        lsp_edits,
                                        server.server_id(),
                                        op.text_document.version,
                                        cx,
                                    )
                                })?
                                .await;
                            let Ok(resolved_edits) = edits_result else {
                                log::warn!(
                                    "Failed to resolve edits from LSP for buffer {:?} while handling {}",
                                    buffer_path_abs.as_path(),
                                    describe_code_action(&action),
                                );
                                continue 'actions;
                            };
                            edits.extend(resolved_edits);
                        }

                        if edits.is_empty() {
                            log::warn!("No edits resolved from LSP");
                            continue;
                        }

                        extend_formatting_transaction(buffer, formatting_transaction_id, cx, |buffer, cx| {
                            buffer.edit(edits, None, cx);
                        })?;
                    }

                    let Some(command) = action.lsp_action.command() else {
                        continue;
                    };

                    log::warn!(
                        "Executing code action command '{}'. This may cause formatting to abort unnecessarily as well as splitting formatting into two entries in the undo history",
                        command.command,
                    );

                    let server_capabilities = server.capabilities();
                    let available_commands = server_capabilities
                        .execute_command_provider
                        .as_ref()
                        .map_or_default(|options| options.commands.as_slice());
                    if !available_commands.contains(&command.command) {
                        log::warn!(
                            "Cannot execute a command {} not listed in the language server capabilities of server {}",
                            command.command,
                            server.name(),
                        );
                        continue;
                    }

                    extend_formatting_transaction(buffer, formatting_transaction_id, cx, |_, _| {})?;
                    log::info!("Executing command {}", command.command);

                    lsp_store.update(cx, |this, _| {
                        this.local.last_workspace_edits_by_language_server.remove(&server.server_id());
                    })?;

                    let execute_command_result = server
                        .request::<ExecuteCommand>(
                            lsp::ExecuteCommandParams {
                                command: command.command.clone(),
                                arguments: command.arguments.clone().unwrap_or_default(),
                                ..Default::default()
                            },
                            request_timeout,
                        )
                        .await
                        .into_response();

                    if execute_command_result.is_err() {
                        log::error!(
                            "Failed to execute command '{}' as part of {}",
                            command.command,
                            describe_code_action(&action),
                        );
                        continue 'actions;
                    }

                    let mut project_transaction_command = lsp_store.update(cx, |this, _| {
                        this.local
                            .last_workspace_edits_by_language_server
                            .remove(&server.server_id())
                            .unwrap_or_default()
                    })?;

                    if let Some(transaction) = project_transaction_command.0.remove(&buffer.handle) {
                        let transaction_id_project_transaction = transaction.id;
                        buffer.handle.update(cx, |buffer, _| {
                            // it may have been removed from history if push_to_history was
                            // false in deserialize_workspace_edit. If so push it so we
                            // can merge it with the format transaction
                            // and pop the combined transaction off the history stack
                            // later if push_to_history is false
                            if buffer.get_transaction(transaction.id).is_none() {
                                buffer.push_transaction(transaction, Instant::now());
                            }
                            buffer.merge_transactions(transaction_id_project_transaction, formatting_transaction_id);
                        });
                    }

                    if project_transaction_command.0.is_empty() {
                        continue;
                    }

                    let mut extra_buffers = String::new();
                    for buffer in project_transaction_command.0.keys() {
                        buffer.read_with(cx, |b, cx| {
                            let Some(path) = b.project_path(cx) else {
                                return;
                            };

                            if !extra_buffers.is_empty() {
                                extra_buffers.push_str(", ");
                            }
                            extra_buffers.push_str(path.path.as_unix_str());
                        });
                    }
                    log::warn!(
                        "Unexpected edits to buffers other than the buffer actively being formatted due to command {}. Impacted buffers: [{}].",
                        command.command,
                        extra_buffers,
                    );
                    // NOTE: if this case is hit, the proper thing to do is to for each buffer, merge the extra transaction
                    // into the existing transaction in project_transaction if there is one, and if there isn't one in project_transaction,
                    // add it so it's included, and merge it into the format transaction when its created later
                }
            }
        }

        Ok(())
    }

    pub async fn format_ranges_via_lsp(
        this: &WeakEntity<LspStore>,
        buffer_handle: &Entity<Buffer>,
        ranges: &[Range<Anchor>],
        abs_path: &Path,
        adapter: &Arc<CachedLspAdapter>,
        language_server: &Arc<LanguageServer>,
        settings: &LanguageSettings,
        cx: &mut AsyncApp,
    ) -> Result<Option<Vec<(Range<Anchor>, Arc<str>)>>> {
        let range_formatting_supported = this.read_with(cx, |this, cx| {
            let local = &this.local;
            buffer_supports_lsp_range_formatting(local, buffer_handle.read(cx), adapter, language_server)
        })?;
        if !range_formatting_supported {
            log::debug!(
                "Skipping range formatting: language server {} does not support range formatting",
                language_server.name()
            );
            return Ok(None);
        }

        let uri = file_path_to_lsp_url(abs_path)?;
        let text_document = lsp::TextDocumentIdentifier::new(uri);

        let request_timeout = LSP_REQUEST_TIMEOUT;
        let lsp_edits = {
            let mut lsp_ranges = Vec::new();
            this.update(cx, |_this, cx| {
                // TODO(#22930): In the case of formatting multibuffer selections, this buffer may
                // not have been sent to the language server. This seems like a fairly systemic
                // issue, though, the resolution probably is not specific to formatting.
                //
                // TODO: Instead of using current snapshot, should use the latest snapshot sent to
                // LSP.
                let snapshot = buffer_handle.read(cx).snapshot();
                for range in ranges {
                    lsp_ranges.push(range_to_lsp(range.to_point_utf16(&snapshot))?);
                }
                anyhow::Ok(())
            })??;

            let mut edits = None::<Vec<lsp::TextEdit>>;
            for range in lsp_ranges {
                if edits.as_ref().is_some_and(|edits| {
                    edits.iter().any(|kept| kept.range.start <= range.start && range.end <= kept.range.end)
                }) {
                    log::debug!(
                        "language server {}: skipping range formatting request for {range:?}: an earlier response already covers it",
                        language_server.name()
                    );
                    continue;
                }
                if let Some(range_edits) = language_server
                    .request::<RangeFormatting>(
                        lsp::DocumentRangeFormattingParams {
                            text_document: text_document.clone(),
                            range,
                            options: lsp_command::lsp_formatting_options(settings),
                            work_done_progress_params: lsp::WorkDoneProgressParams::default(),
                        },
                        request_timeout,
                    )
                    .await
                    .into_response()?
                {
                    let kept_edits = edits.get_or_insert_with(Vec::new);
                    let mut new_edits = Vec::new();
                    let mut overlaps_earlier_response = false;
                    for edit in range_edits {
                        let is_duplicate =
                            kept_edits.iter().any(|kept| kept.range == edit.range && kept.new_text == edit.new_text);
                        if is_duplicate {
                            continue;
                        }
                        if kept_edits.iter().any(|kept| lsp_ranges_conflict(&edit.range, &kept.range)) {
                            overlaps_earlier_response = true;
                            break;
                        }
                        new_edits.push(edit);
                    }
                    if overlaps_earlier_response {
                        log::debug!(
                            "language server {} returned range formatting edits for {range:?} that overlap an earlier response and differ; dropping the whole response",
                            language_server.name()
                        );
                        continue;
                    }
                    let response_start = kept_edits.len();
                    let mut dropped_within_response = 0_usize;
                    for edit in new_edits {
                        if kept_edits[response_start..].iter().any(|kept| lsp_ranges_overlap(&edit.range, &kept.range))
                        {
                            dropped_within_response += 1;
                            continue;
                        }
                        kept_edits.push(edit);
                    }
                    if dropped_within_response > 0 {
                        log::debug!(
                            "language server {} returned overlapping edits within one range formatting response for {range:?}; dropped {dropped_within_response} later edit(s)",
                            language_server.name()
                        );
                    }
                }
            }
            edits
        };

        if let Some(lsp_edits) = lsp_edits {
            this.update(cx, |this, cx| {
                this.local.edits_from_lsp(buffer_handle, lsp_edits, language_server.server_id(), None, cx)
            })?
            .await
            .map(Some)
        } else {
            Ok(Some(Vec::with_capacity(0)))
        }
    }

    async fn format_via_lsp(
        this: &WeakEntity<LspStore>,
        buffer: &Entity<Buffer>,
        abs_path: &Path,
        adapter: &Arc<CachedLspAdapter>,
        language_server: &Arc<LanguageServer>,
        settings: &LanguageSettings,
        cx: &mut AsyncApp,
    ) -> Result<Vec<(Range<Anchor>, Arc<str>)>> {
        let uri = file_path_to_lsp_url(abs_path)?;
        let text_document = lsp::TextDocumentIdentifier::new(uri);

        let (formatting_supported, range_formatting_supported) = this.read_with(cx, |this, cx| {
            let buffer = buffer.read(cx);
            (
                buffer_supports_lsp_formatting(&this.local, buffer, adapter, language_server),
                buffer_supports_lsp_range_formatting(&this.local, buffer, adapter, language_server),
            )
        })?;

        let request_timeout = LSP_REQUEST_TIMEOUT;

        let lsp_edits = if formatting_supported {
            let response = language_server
                .request::<Formatting>(
                    lsp::DocumentFormattingParams {
                        text_document,
                        options: lsp_command::lsp_formatting_options(settings),
                        work_done_progress_params: lsp::WorkDoneProgressParams::default(),
                    },
                    request_timeout,
                )
                .await
                .into_response()?;

            let Some(edits) = response else {
                return Ok(vec![]);
            };

            let buffer_end = buffer.read_with(cx, |buffer, _| point_to_lsp(buffer.max_point_utf16()));
            let should_apply_diff_based_edits = edits.len() == 1
                && edits
                    .first()
                    .is_some_and(|edit| edit.range == lsp::Range::new(lsp::Position::new(0, 0), buffer_end));

            if should_apply_diff_based_edits {
                let Some(text_edit) = edits.into_iter().next() else {
                    return Ok(vec![]);
                };
                let diff = buffer.update(cx, |buffer, cx| buffer.diff(text_edit.new_text, cx)).await;
                Some(buffer.read_with(cx, |buffer, _| {
                    let rope = buffer.as_rope();
                    diff.edits
                        .into_iter()
                        .map(|(range, text)| TextEdit {
                            range: lsp::Range::new(
                                point_to_lsp(rope.offset_to_point_utf16(range.start)),
                                point_to_lsp(rope.offset_to_point_utf16(range.end)),
                            ),
                            new_text: text.to_string(),
                        })
                        .collect()
                }))
            } else {
                Some(edits).filter(|edits| !edits.is_empty())
            }
        } else if range_formatting_supported {
            let buffer_start = lsp::Position::new(0, 0);
            let buffer_end = buffer.read_with(cx, |b, _| point_to_lsp(b.max_point_utf16()));
            language_server
                .request::<RangeFormatting>(
                    lsp::DocumentRangeFormattingParams {
                        text_document: text_document.clone(),
                        range: lsp::Range::new(buffer_start, buffer_end),
                        options: lsp_command::lsp_formatting_options(settings),
                        work_done_progress_params: lsp::WorkDoneProgressParams::default(),
                    },
                    request_timeout,
                )
                .await
                .into_response()?
        } else {
            None
        };

        if let Some(lsp_edits) = lsp_edits {
            this.update(cx, |this, cx| {
                this.local.edits_from_lsp(buffer, lsp_edits, language_server.server_id(), None, cx)
            })?
            .await
        } else {
            Ok(Vec::with_capacity(0))
        }
    }

    /// Formats the buffer with Biome, resolved per `biome::resolve_command`.
    async fn format_via_biome(buffer: &FormattableBuffer, cx: &mut AsyncApp) -> Result<Option<Diff>> {
        let language = buffer.handle.read_with(cx, |buffer, _| buffer.language().cloned());
        let stdin_file_path = biome::stdin_file_path(buffer.abs_path.as_deref(), language.as_deref());
        let start_dir = buffer.abs_path.as_deref().and_then(Path::parent).map(Path::to_path_buf);
        let path_env = buffer.env.as_ref().and_then(|env| env.get("PATH").cloned());
        let biome = cx
            .background_spawn({
                let start_dir = start_dir.clone();
                async move { biome::resolve_command(start_dir.as_deref(), path_env.as_deref()) }
            })
            .await;

        let mut child = command::new_command(&biome.program);
        if let Some(buffer_env) = buffer.env.as_ref() {
            child.envs(buffer_env);
        }
        // Biome discovers its config from the working directory, so run it from the
        // directory that holds `biome.json(c)`.
        if let Some(start_dir) = start_dir {
            child.current_dir(biome::working_dir(&start_dir));
        }
        child.args(biome.args).arg("format").arg("--stdin-file-path").arg(stdin_file_path);
        Self::run_formatter_command(buffer, child, "biome", cx).await
    }

    /// Pipes the buffer text into `child` and diffs the buffer against its stdout.
    async fn run_formatter_command(
        buffer: &FormattableBuffer,
        mut child: command::Command,
        name: &str,
        cx: &mut AsyncApp,
    ) -> Result<Option<Diff>> {
        use async_macos::command::Stdio;
        let mut child = child.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;

        let stdin = child.stdin.as_mut().context("failed to acquire stdin")?;
        let text = buffer.handle.read_with(cx, |buffer, _| buffer.as_rope().clone());
        for chunk in text.chunks() {
            stdin.write_all(chunk.as_bytes()).await?;
        }
        stdin.flush().await?;

        let output = child.output().await?;
        anyhow::ensure!(
            output.status.success(),
            "command failed with exit code {:?}:\nstdout: {}\nstderr: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );

        let stdout = String::from_utf8(output.stdout)?;
        if stdout.is_empty() && !text.is_empty() {
            // Some formatters (e.g. `cargo fmt`) rewrite files on disk instead of
            // printing the formatted contents to stdout. Treating empty stdout as the
            // new buffer contents would erase the buffer, so leave it untouched.
            log::warn!("External formatter `{name}` produced no output on stdout; leaving the buffer unchanged");
            return Ok(None);
        }

        Ok(Some(buffer.handle.update(cx, |buffer, cx| buffer.diff(stdout, cx)).await))
    }
}

impl LspStore {
    pub fn supports_range_formatting(&self, buffer: &Entity<Buffer>, cx: &App) -> bool {
        let settings = LanguageSettings::for_buffer(buffer.read(cx), cx);
        {
            match &settings.formatter {
                Formatter::Auto => {
                    biome::supports(buffer.read(cx).language().map(AsRef::as_ref))
                        || self.check_if_any_relevant_text_document_server_matches(
                            buffer,
                            "textDocument/rangeFormatting",
                            |_, capabilities| {
                                server_capabilities_support_range_formatting(capabilities.server_capabilities)
                            },
                            cx,
                        )
                }
                Formatter::LanguageServer(LanguageServerFormatterSpecifier::Current) => self
                    .check_if_any_relevant_text_document_server_matches(
                        buffer,
                        "textDocument/rangeFormatting",
                        |_, capabilities| {
                            server_capabilities_support_range_formatting(capabilities.server_capabilities)
                        },
                        cx,
                    ),
                Formatter::LanguageServer(LanguageServerFormatterSpecifier::Specific { name }) => self
                    .check_if_any_relevant_text_document_server_matches(
                        buffer,
                        "textDocument/rangeFormatting",
                        |server_name, capabilities| {
                            server_name.0.as_ref() == name
                                && server_capabilities_support_range_formatting(capabilities.server_capabilities)
                        },
                        cx,
                    ),
                // `FormatSelections` should only surface when a formatter can honor the
                // selected ranges. Code actions can still run as part of formatting, but
                // they operate on the whole buffer rather than the selected text.
                Formatter::CodeAction(_) => false,
            }
        }
    }

    pub fn on_type_format<T: ToPointUtf16>(
        &mut self,
        buffer: Entity<Buffer>,
        position: T,
        trigger: String,
        push_to_history: bool,
        cx: &mut Context<Self>,
    ) -> Option<Task<Result<Option<Transaction>>>> {
        if !self.check_if_any_relevant_text_document_server_matches(
            &buffer,
            "textDocument/onTypeFormatting",
            |_, capabilities| OnTypeFormatting::supports_on_type_formatting(&trigger, capabilities.server_capabilities),
            cx,
        ) {
            return None;
        }

        let position = {
            let buffer = buffer.read(cx);
            let position = position.to_point_utf16(buffer);
            buffer.anchor_before(position)
        };
        Some(self.on_type_format_impl(buffer, position, trigger, push_to_history, cx))
    }

    fn on_type_format_impl(
        &self,
        buffer: Entity<Buffer>,
        position: Anchor,
        trigger: String,
        push_to_history: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<Option<Transaction>>> {
        let autoindent_applied = buffer.update(cx, |buffer, _| buffer.wait_for_autoindent_applied());
        let options = buffer.update(cx, |buffer, cx| {
            lsp_command::lsp_formatting_options(LanguageSettings::for_buffer_at(buffer, position, cx).as_ref())
        });

        cx.spawn(async move |this, cx| {
            // The trigger position predates autoindent; if autoindent inserted whitespace exactly
            // there, the cursor ends up after it.
            let autoindent_edit_id = match autoindent_applied {
                Some(waiter) => waiter.await?,
                None => None,
            };
            let position = buffer.read_with(cx, |buffer, _| {
                let offset = position.to_offset(buffer);
                let offset = autoindent_edit_id
                    .and_then(|edit_id| {
                        buffer.edited_ranges_for_edit_ids::<usize>([&edit_id]).find(|range| range.start == offset)
                    })
                    .map_or(offset, |range| range.end);
                offset.to_point_utf16(buffer)
            });
            cx.update(|cx| {
                this.update(cx, |this, cx| {
                    this.request_lsp(
                        buffer.clone(),
                        LanguageServerToQuery::FirstCapable,
                        OnTypeFormatting { position, trigger, options, push_to_history },
                        cx,
                    )
                })
            })?
            .await
        })
    }

    pub fn last_formatting_failure(&self) -> Option<&str> {
        self.last_formatting_failure.as_deref()
    }

    pub fn reset_last_formatting_failure(&mut self) {
        self.last_formatting_failure = None;
    }

    pub fn format(
        &mut self,
        buffers: FxHashSet<Entity<Buffer>>,
        target: LspFormatTarget,
        push_to_history: bool,
        cx: &mut Context<Self>,
    ) -> Task<anyhow::Result<ProjectTransaction>> {
        {
            let buffers = buffers
                .into_iter()
                .map(|buffer_handle| {
                    let buffer = buffer_handle.read(cx);
                    let buffer_abs_path = File::from_dyn(buffer.file()).map(|file| file.abs_path(cx));

                    (buffer_handle, buffer_abs_path, buffer.buffer_id())
                })
                .collect::<Vec<_>>();

            cx.spawn(async move |lsp_store, cx| {
                let mut formattable_buffers = Vec::with_capacity(buffers.len());

                for (handle, abs_path, id) in buffers {
                    let env =
                        lsp_store.update(cx, |lsp_store, cx| lsp_store.environment_for_buffer(&handle, cx))?.await;

                    let ranges = match &target {
                        LspFormatTarget::Buffers => None,
                        LspFormatTarget::Ranges(ranges) => {
                            Some(ranges.get(&id).context("No format ranges provided for buffer")?.clone())
                        }
                    };

                    formattable_buffers.push(FormattableBuffer { handle, abs_path, env, ranges });
                }

                let result =
                    LocalLspStore::format_locally(lsp_store.clone(), formattable_buffers, push_to_history, cx).await;

                lsp_store.update(cx, |lsp_store, _| {
                    lsp_store.update_last_formatting_failure(&result);
                })?;

                result
            })
        }
    }

    pub(super) fn update_last_formatting_failure<T>(&mut self, formatting_result: &anyhow::Result<T>) {
        match &formatting_result {
            Ok(_) => self.last_formatting_failure = None,
            Err(error) => {
                let error_string = format!("{error:#}");
                log::error!("Formatting failed: {error_string}");
                self.last_formatting_failure.replace(error_string.lines().join(" "));
            }
        }
    }
}
