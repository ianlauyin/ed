use std::ops::Range;

use futures::FutureExt;
use gpui::App;
use gpui::Context;
use gpui::HighlightStyle;
use gpui::Task;
use itertools::Itertools as _;
use language::Buffer;
use language::LanguageAwareStyling;
use language::OutlineItem;
use language::highlight_ranges_from_text;
use language::language_settings::LanguageSettings;
use multi_buffer::AnchorRangeExt as _;
use multi_buffer::MultiBufferOffset;
use multi_buffer::MultiBufferRow;
use multi_buffer::ToOffset as _;
use text::BufferId;
use theme::ActiveTheme as _;
use theme::SyntaxTheme;
use unicode_segmentation::UnicodeSegmentation as _;

use crate::Editor;
use crate::EditorEvent;
use crate::LSP_REQUEST_DEBOUNCE_TIMEOUT;
use crate::display_map::DisplaySnapshot;

impl Editor {
    /// Returns all document outline items for a buffer, using LSP or
    /// tree-sitter based on the `document_symbols` setting.
    /// External consumers (outline panel) should use this.
    pub fn buffer_outline_items(
        &self,
        buffer_id: BufferId,
        cx: &mut Context<Self>,
    ) -> Task<Vec<OutlineItem<text::Anchor>>> {
        let Some(buffer) = self.buffer.read(cx).buffer(buffer_id) else {
            return Task::ready(Vec::new());
        };

        if lsp_symbols_enabled(buffer.read(cx), cx) {
            let refresh_task = self.refresh_document_symbols_tasks.get(&buffer_id).cloned();
            cx.spawn(async move |editor, cx| {
                if let Some(refresh_task) = refresh_task {
                    refresh_task.await;
                }
                editor
                    .read_with(cx, |editor, _| editor.lsp_document_symbols.get(&buffer_id).cloned().unwrap_or_default())
                    .ok()
                    .unwrap_or_default()
            })
        } else {
            let buffer_snapshot = buffer.read(cx).snapshot();
            let syntax = cx.theme().syntax().clone();
            cx.background_executor().spawn(async move { buffer_snapshot.outline(Some(&syntax)).items })
        }
    }

    /// Fetches document symbols from the LSP for buffers that have the setting
    /// enabled. Called from `update_lsp_data` on edits, server events, etc.
    /// When the fetch completes, stores results in `self.lsp_document_symbols`
    /// and emits `DocumentSymbolsChanged` so the outline panel picks up the new data.
    pub(super) fn refresh_document_symbols(&mut self, for_buffer: Option<BufferId>, cx: &mut Context<Self>) {
        if !self.lsp_data_enabled() {
            return;
        }
        let Some(project) = self.project.as_ref().map(|p| p.downgrade()) else {
            return;
        };

        let buffers_to_query = self
            .visible_buffers(cx)
            .into_iter()
            .filter(|buffer| self.is_lsp_relevant(buffer.read(cx).file(), cx))
            .filter_map(|buffer| {
                let id = buffer.read(cx).remote_id();
                if for_buffer.is_none_or(|target| target == id) && lsp_symbols_enabled(buffer.read(cx), cx) {
                    Some(buffer)
                } else {
                    None
                }
            })
            .unique_by(|buffer| buffer.read(cx).remote_id())
            .collect::<Vec<_>>();

        let mut symbols_altered = false;
        let multi_buffer = self.buffer().clone();
        self.lsp_document_symbols.retain(|buffer_id, _| {
            let Some(buffer) = multi_buffer.read(cx).buffer(*buffer_id) else {
                symbols_altered = true;
                return false;
            };
            let retain = lsp_symbols_enabled(buffer.read(cx), cx);
            symbols_altered |= !retain;
            retain
        });
        if symbols_altered {
            cx.emit(EditorEvent::DocumentSymbolsChanged);
        }

        self.refresh_document_symbols_tasks.retain(|buffer_id, _| {
            multi_buffer.read(cx).buffer(*buffer_id).is_some_and(|buffer| lsp_symbols_enabled(buffer.read(cx), cx))
        });

        for buffer in buffers_to_query {
            let buffer_id = buffer.read(cx).remote_id();
            let project = project.clone();
            let task = cx
                .spawn(async move |editor, cx| {
                    cx.background_executor().timer(LSP_REQUEST_DEBOUNCE_TIMEOUT).await;

                    let Some(task) = project
                        .update(cx, |project, cx| {
                            project
                                .lsp_store()
                                .update(cx, |lsp_store, cx| lsp_store.fetch_document_symbols(&buffer, cx))
                        })
                        .ok()
                    else {
                        return;
                    };
                    let mut items = task.await;

                    editor
                        .update(cx, |editor, cx| {
                            let syntax = cx.theme().syntax().clone();
                            let display_snapshot = editor.display_map.update(cx, |map, cx| map.snapshot(cx));
                            let language = buffer.read(cx).language().cloned();
                            for item in &mut items {
                                if let Some(highlights) = highlights_from_buffer(&display_snapshot, item, &syntax) {
                                    item.highlight_ranges = highlights;
                                } else if let Some(language) = &language {
                                    item.highlight_ranges = highlight_ranges_from_text(&item.text, language, &syntax);
                                }
                            }
                            editor.lsp_document_symbols.insert(buffer_id, items);
                            cx.emit(EditorEvent::DocumentSymbolsChanged);
                        })
                        .ok();
                })
                .shared();
            self.refresh_document_symbols_tasks.insert(buffer_id, task);
        }
    }
}

fn lsp_symbols_enabled(buffer: &Buffer, cx: &App) -> bool {
    LanguageSettings::for_buffer(buffer, cx).document_symbols.lsp_enabled()
}

/// Finds where the symbol name appears in the buffer and returns combined
/// (tree-sitter + semantic token) highlights for those positions.
///
/// First tries to find the name verbatim near the selection range so that
/// complex names (`impl Trait for Type`) get full highlighting. Falls back
/// to word-by-word matching for cases like `impl<T> Trait<T> for Type`
/// where the LSP name doesn't appear verbatim in the buffer.
fn highlights_from_buffer(
    display_snapshot: &DisplaySnapshot,
    item: &OutlineItem<text::Anchor>,
    syntax_theme: &SyntaxTheme,
) -> Option<Vec<(Range<usize>, HighlightStyle)>> {
    let outline_text = &item.text;
    if outline_text.is_empty() {
        return None;
    }

    let multi_buffer_snapshot = display_snapshot.buffer();
    let multi_buffer_source_range_anchors = multi_buffer_snapshot
        .text_anchors_to_visible_anchors([item.source_range_for_text.start, item.source_range_for_text.end]);
    let anchor_range =
        (|| Some((*multi_buffer_source_range_anchors.first()?)?..(*multi_buffer_source_range_anchors.get(1)?)?))();
    let buffer_snapshot = multi_buffer_snapshot.buffer_for_id(item.source_range_for_text.start.buffer_id)?;
    let can_reuse_buffer_highlight_offsets = buffer_snapshot
        .text_for_range(item.source_range_for_text.clone())
        .flat_map(str::bytes)
        .eq(outline_text.bytes());
    if can_reuse_buffer_highlight_offsets {
        let visible_range = anchor_range
            .as_ref()
            .and_then(|_| multi_buffer_snapshot.buffer_anchor_range_to_anchor_range(item.source_range_for_text.clone()))
            .map(|range| range.to_offset(multi_buffer_snapshot))
            .filter(|range| range.end.0.checked_sub(range.start.0) == Some(outline_text.len()));
        let highlights = if let Some(range) = visible_range {
            display_snapshot.combined_highlights(range, syntax_theme)
        } else {
            let mut offset = 0;
            buffer_snapshot
                .chunks(
                    item.source_range_for_text.clone(),
                    LanguageAwareStyling { tree_sitter: true, diagnostics: false },
                )
                .filter_map(|chunk| {
                    let start = offset;
                    offset += chunk.text.len();
                    chunk.syntax_highlight_id.and_then(|id| syntax_theme.get(id)).map(|style| (start..offset, *style))
                })
                .collect::<Vec<_>>()
        };
        return (!highlights.is_empty()).then_some(highlights);
    }

    let selection_point_range = anchor_range?.to_point(multi_buffer_snapshot);
    let mut search_start = selection_point_range.start;
    search_start.column = 0;
    let search_start_offset = search_start.to_offset(multi_buffer_snapshot);
    let mut search_end = selection_point_range.end;
    search_end.column = multi_buffer_snapshot.line_len(MultiBufferRow(search_end.row));

    let search_text = multi_buffer_snapshot.text_for_range(search_start..search_end).collect::<String>();

    let mut outline_text_highlights = Vec::new();
    match search_text.find(outline_text.as_str()) {
        Some(start_index) => {
            let multibuffer_start = search_start_offset + MultiBufferOffset(start_index);
            let multibuffer_end = multibuffer_start + MultiBufferOffset(outline_text.len());
            outline_text_highlights
                .extend(display_snapshot.combined_highlights(multibuffer_start..multibuffer_end, syntax_theme));
        }
        None => {
            for (outline_text_word_start, outline_word) in outline_text.split_word_bound_indices() {
                if let Some(start_index) = search_text.find(outline_word) {
                    let multibuffer_start = search_start_offset + MultiBufferOffset(start_index);
                    let multibuffer_end = multibuffer_start + MultiBufferOffset(outline_word.len());
                    outline_text_highlights.extend(
                        display_snapshot
                            .combined_highlights(multibuffer_start..multibuffer_end, syntax_theme)
                            .into_iter()
                            .map(|(range_in_word, style)| {
                                (
                                    outline_text_word_start + range_in_word.start
                                        ..outline_text_word_start + range_in_word.end,
                                    style,
                                )
                            }),
                    );
                }
            }
        }
    }

    if outline_text_highlights.is_empty() { None } else { Some(outline_text_highlights) }
}
