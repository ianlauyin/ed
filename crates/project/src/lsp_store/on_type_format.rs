use anyhow::Result;
use gpui::Context;
use gpui::Entity;
use gpui::Task;
use language::Buffer;
use language::ToOffset as _;
use language::ToPointUtf16;
use language::Transaction;
use language::language_settings::LanguageSettings;
use text::Anchor;

use super::LanguageServerToQuery;
use super::LspStore;
use crate::lsp_command;
use crate::lsp_command::OnTypeFormatting;

impl LspStore {
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
}
