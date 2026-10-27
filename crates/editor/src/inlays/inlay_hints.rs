use std::collections::hash_map;
use std::ops::ControlFlow;
use std::ops::Range;
use std::time::Duration;

use clock::Global;
use collections::HashMap;
use collections::HashSet;
use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::Shared;
use futures::future::join_all;
use gpui::App;
use gpui::Entity;
use gpui::Pixels;
use gpui::Task;
use gpui::TaskExt;
use itertools::Itertools;
use language::BufferRow;
use language::BufferSnapshot;
use language::language_settings::InlayHintKind;
use language::language_settings::InlayHintSettings;
use lsp::LanguageServerId;
use multi_buffer::Anchor;
use multi_buffer::MultiBufferSnapshot;
use project::CodeAction;
use project::HoverBlock;
use project::HoverBlockKind;
use project::InlayHint;
use project::InlayHintLabel;
use project::InlayHintLabelPartTooltip;
use project::InlayHintTooltip;
use project::InvalidationStrategy;
use project::LspAction;
use project::ResolveState;
use project::lsp_store::CacheInlayHints;
use project::lsp_store::LspStore;
use project::lsp_store::ResolvedHint;
use text::Bias;
use text::BufferId;
use text::ToOffset;
use ui::Context;
use ui::Window;

use super::Inlay;
use super::InlayId;
use crate::AcceptInlayHint;
use crate::DisplayPoint;
use crate::Editor;
use crate::EditorSnapshot;
use crate::debounce_value;
use crate::display_map::DisplayMap;
use crate::display_map::DisplaySnapshot;
use crate::hover_links::InlayHighlight;
use crate::hover_links::TriggerPoint;
use crate::hover_links::show_link_definition;
use crate::hover_popover::InlayHover;
use crate::hover_popover::{self};
use crate::inlays::InlaySplice;

#[derive(Debug)]
pub(crate) struct HoveredInlayHintCommand {
    highlight: InlayHighlight,
    buffer_id: BufferId,
    action: CodeAction,
}

impl HoveredInlayHintCommand {
    pub(crate) fn contains_glyph(&self, snapshot: &EditorSnapshot, hint_glyph: Option<DisplayPoint>) -> bool {
        if !snapshot.can_resolve(&self.highlight.inlay_position) {
            return false;
        }
        let Some((hint, offset_in_hint)) = hint_glyph.and_then(|glyph| snapshot.inlay_hint_at(glyph)) else {
            return false;
        };
        hint.id == self.highlight.inlay && self.highlight.range.contains(&offset_in_hint)
    }
}

pub fn inlay_hint_settings(
    location: Anchor,
    snapshot: &MultiBufferSnapshot,
    cx: &Context<Editor>,
) -> InlayHintSettings {
    snapshot.language_settings_at(location, cx).inlay_hints
}

#[derive(Debug)]
pub struct LspInlayHintData {
    allowed_hint_kinds: HashSet<Option<InlayHintKind>>,
    invalidate_debounce: Option<Duration>,
    append_debounce: Option<Duration>,
    hint_refresh_tasks: HashMap<BufferId, Vec<HintRefreshTask>>,
    hint_chunk_fetching: HashMap<BufferId, (Global, HashSet<Range<BufferRow>>)>,
    invalidate_hints_for_buffers: HashSet<BufferId>,
    pub added_hints: HashMap<InlayId, Option<InlayHintKind>>,
    hovered_command: Option<HoveredInlayHintCommand>,
    pending_text_edit: Option<PendingHintTextEdit>,
    pending_refresh_acceptance: Option<(HashSet<BufferId>, Task<()>)>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HintTarget {
    pub buffer_id: BufferId,
    pub hint_id: InlayId,
    pub position: Anchor,
}

#[derive(Debug)]
struct HintRefreshTask {
    task: Task<()>,
    finished: Shared<oneshot::Receiver<()>>,
}

#[derive(Debug)]
struct PendingHintTextEdit {
    target: HintTarget,
    _apply_task: Task<()>,
}

impl LspInlayHintData {
    pub fn new(settings: InlayHintSettings) -> Self {
        Self {
            hint_refresh_tasks: HashMap::default(),
            added_hints: HashMap::default(),
            hint_chunk_fetching: HashMap::default(),
            invalidate_hints_for_buffers: HashSet::default(),
            invalidate_debounce: debounce_value(settings.edit_debounce_ms),
            append_debounce: debounce_value(settings.scroll_debounce_ms),
            allowed_hint_kinds: settings.enabled_inlay_hint_kinds(),
            hovered_command: None,
            pending_text_edit: None,
            pending_refresh_acceptance: None,
        }
    }

    pub fn clear(&mut self) {
        self.hint_refresh_tasks.clear();
        self.hint_chunk_fetching.clear();
        self.added_hints.clear();
        self.hovered_command = None;
        self.pending_text_edit = None;
        self.pending_refresh_acceptance = None;
    }

    /// Like `clear`, but only wipes tracking state for the given buffer IDs.
    /// Hints belonging to other buffers are left intact so they are neither
    /// re-fetched nor duplicated on the next `NewLinesShown`.
    pub fn clear_for_buffers(
        &mut self,
        buffer_ids: &HashSet<BufferId>,
        current_hints: impl IntoIterator<Item = Inlay>,
        snapshot: &MultiBufferSnapshot,
    ) {
        if self.hovered_command.as_ref().is_some_and(|command| buffer_ids.contains(&command.buffer_id)) {
            self.hovered_command = None;
        }
        self.remove_pending_text_edit(|target| buffer_ids.contains(&target.buffer_id));
        if self
            .pending_refresh_acceptance
            .as_ref()
            .is_some_and(|(pending_buffers, _)| !pending_buffers.is_disjoint(buffer_ids))
        {
            self.pending_refresh_acceptance = None;
        }
        for buffer_id in buffer_ids {
            self.hint_refresh_tasks.remove(buffer_id);
            self.hint_chunk_fetching.remove(buffer_id);
        }
        for hint in current_hints {
            if let Some((text_anchor, _)) = snapshot.anchor_to_buffer_anchor(hint.position)
                && buffer_ids.contains(&text_anchor.buffer_id)
            {
                self.added_hints.remove(&hint.id);
            }
        }
    }

    pub(crate) fn remove_inlay_chunk_data<'a>(
        &'a mut self,
        removed_buffer_ids: impl IntoIterator<Item = &'a BufferId> + 'a,
    ) {
        for buffer_id in removed_buffer_ids {
            if self
                .pending_refresh_acceptance
                .as_ref()
                .is_some_and(|(pending_buffers, _)| pending_buffers.contains(buffer_id))
            {
                self.pending_refresh_acceptance = None;
            }
            self.hint_refresh_tasks.remove(buffer_id);
            self.hint_chunk_fetching.remove(buffer_id);
        }
    }

    pub(super) fn remove_pending_text_edit(&mut self, remove: impl FnOnce(&HintTarget) -> bool) {
        if self.pending_text_edit.as_ref().is_some_and(|pending| remove(&pending.target)) {
            self.pending_text_edit = None;
        }
    }

    pub(super) fn remove_inlay(&mut self, inlay_id: &InlayId) {
        self.added_hints.remove(inlay_id);
        if self.hovered_command.as_ref().is_some_and(|command| command.highlight.inlay == *inlay_id) {
            self.hovered_command = None;
        }
    }

    /// Checks inlay hint settings for enabled hint kinds.
    /// Removes visible hints whose kind got disabled; re-enabling a kind forces new LSP queries.
    fn update_settings(
        &mut self,
        new_hint_settings: InlayHintSettings,
        visible_hints: impl IntoIterator<Item = Inlay>,
    ) -> ControlFlow<(), Option<InlaySplice>> {
        self.invalidate_debounce = debounce_value(new_hint_settings.edit_debounce_ms);
        self.append_debounce = debounce_value(new_hint_settings.scroll_debounce_ms);
        let new_allowed_hint_kinds = new_hint_settings.enabled_inlay_hint_kinds();
        if new_allowed_hint_kinds == self.allowed_hint_kinds {
            return ControlFlow::Break(());
        }
        self.allowed_hint_kinds = new_allowed_hint_kinds;
        ControlFlow::Continue(
            Some(InlaySplice {
                to_remove: visible_hints
                    .into_iter()
                    .filter_map(|inlay| {
                        let inlay_kind = self.added_hints.get(&inlay.id).copied()?;
                        if !self.allowed_hint_kinds.contains(&inlay_kind) { Some(inlay.id) } else { None }
                    })
                    .collect(),
                to_insert: Vec::new(),
            })
            .filter(|splice| !splice.is_empty()),
        )
    }

    fn pending_refreshes(&self, buffer_ids: &HashSet<BufferId>) -> Vec<Shared<oneshot::Receiver<()>>> {
        self.hint_refresh_tasks
            .iter()
            .filter(|(buffer_id, _)| buffer_ids.contains(buffer_id))
            .flat_map(|(_, refreshes)| refreshes)
            .filter(|refresh| !refresh.task.is_ready())
            .map(|refresh| refresh.finished.clone())
            .collect()
    }
}

#[derive(Debug, Clone)]
pub enum InlayHintRefreshReason {
    SettingsChange(InlayHintSettings),
    NewLinesShown,
    BufferEdited(BufferId),
    ServerRemoved,
    LanguageServerRegistered,
    RefreshRequested { server_id: LanguageServerId },
    BuffersRemoved(Vec<BufferId>),
}

impl Editor {
    pub fn supports_inlay_hints(&self, cx: &mut App) -> bool {
        let Some(provider) = self.semantics_provider.as_ref() else {
            return false;
        };

        let mut supports = false;
        self.buffer().update(cx, |this, cx| {
            this.for_each_buffer(&mut |buffer| {
                supports |= provider.supports_inlay_hints(buffer, cx);
            });
        });

        supports
    }

    pub fn accept_inlay_hint(&mut self, _: &AcceptInlayHint, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_accept_inlay_hint(cx) {
            return;
        }
        if let Some(inlay_hints) = self.inlay_hints.as_mut() {
            inlay_hints.pending_text_edit = None;
            inlay_hints.pending_refresh_acceptance = None;
        }
        self.refresh_inlay_hints(InlayHintRefreshReason::NewLinesShown, cx);
        if self.accept_inlay_hint_after_refresh(None, window, cx) {
            return;
        }
        let display_snapshot = self.display_snapshot(cx);
        let targets = self.inlay_hints_at_selections(&display_snapshot, cx).collect::<Vec<_>>();
        self.apply_inlay_hint_text_edits(targets.into_iter(), None, window, cx);
    }

    pub fn clear_inlay_hints(&mut self, cx: &mut Context<Self>) {
        let to_remove = Self::visible_inlay_hints(self.display_map.read(cx)).map(|inlay| inlay.id).collect::<Vec<_>>();
        self.splice_inlays(&to_remove, Vec::new(), cx);
    }

    pub fn update_inlay_link_and_hover_points(
        &mut self,
        snapshot: &EditorSnapshot,
        hint_glyph: Option<DisplayPoint>,
        mouse_position: Option<gpui::Point<Pixels>>,
        secondary_held: bool,
        shift_held: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hover_state.hint_hover_task = None;
        let hovered_hint = hint_glyph.and_then(|glyph| snapshot.inlay_hint_at(glyph));
        let resolved_hint = hovered_hint.and_then(|(hint, _)| {
            if !self.lsp_data_enabled() {
                return None;
            }
            let buffer_id = hint.position.raw_text_anchor()?.buffer_id;
            let lsp_store = self.project()?.read(cx).lsp_store();
            let resolved = lsp_store.update(cx, |lsp_store, cx| lsp_store.resolved_hint(buffer_id, hint.id, cx))?;
            Some((hint.id, buffer_id, resolved))
        });
        let Some((hint_id, buffer_id, resolved_hint)) = resolved_hint else {
            self.hide_hovered_link(cx);
            hover_popover::hover_at(self, None, mouse_position, window, cx);
            if let Some(inlay_hints) = self.inlay_hints.as_mut() {
                inlay_hints.hovered_command = None;
            }
            return;
        };
        if let ResolvedHint::Resolving(_) = &resolved_hint {
            self.hide_hovered_link(cx);
            hover_popover::hover_at(self, None, mouse_position, window, cx);
            if let Some(inlay_hints) = self.inlay_hints.as_mut() {
                inlay_hints.hovered_command = None;
            }
        }
        let scroll_position = snapshot.scroll_position();
        let update_hover = move |editor: &mut Editor,
                                 window: &mut Window,
                                 cx: &mut Context<Self>,
                                 cached_hint: InlayHint,
                                 secondary_held: bool,
                                 shift_held: bool|
              -> Option<Task<()>> {
            let snapshot = editor.snapshot(window, cx);
            if snapshot.scroll_position() != scroll_position
                || !editor.lsp_data_enabled()
                || !editor.inlay_hints.as_ref().is_some_and(|hints| hints.added_hints.contains_key(&hint_id))
            {
                return None;
            }
            let (hovered_hint, offset_in_hint) =
                hint_glyph.and_then(|glyph| snapshot.inlay_hint_at(glyph)).filter(|(hint, _)| hint.id == hint_id)?;
            let mut go_to_definition_updated = false;
            let mut hover_task = None;
            let mut inlay_command_updated = false;
            let previous_command_highlight = editor
                .inlay_hints
                .as_ref()
                .and_then(|inlay_hints| inlay_hints.hovered_command.as_ref())
                .map(|command| command.highlight.clone());
            let original_text = cached_hint.text();
            let actual_left_padding = if cached_hint.padding_left && !original_text.starts_with(" ") { 1 } else { 0 };
            let actual_right_padding = if cached_hint.padding_right && !original_text.ends_with(" ") { 1 } else { 0 };
            match cached_hint.label {
                InlayHintLabel::String(_) => {
                    if let Some(tooltip) = cached_hint.tooltip {
                        hover_task = hover_popover::hover_at_inlay(
                            editor,
                            InlayHover {
                                tooltip: match tooltip {
                                    InlayHintTooltip::String(text) => {
                                        HoverBlock { text, kind: HoverBlockKind::PlainText }
                                    }
                                    InlayHintTooltip::MarkupContent(content) => {
                                        HoverBlock { text: content.value, kind: content.kind }
                                    }
                                },
                                range: InlayHighlight {
                                    inlay: hovered_hint.id,
                                    inlay_position: hovered_hint.position,
                                    range: actual_left_padding..hovered_hint.text().len() - actual_right_padding,
                                },
                            },
                            window,
                            cx,
                        );
                    }
                }
                InlayHintLabel::LabelParts(label_parts) => {
                    if let Some(offset_in_label) = offset_in_hint.checked_sub(actual_left_padding)
                        && let Some((hovered_hint_part, part_range)) =
                            hover_popover::find_hovered_hint_part(label_parts, offset_in_label)
                    {
                        let highlight_start = actual_left_padding + part_range.start;
                        let highlight_end = actual_left_padding + part_range.end;
                        let highlight = InlayHighlight {
                            inlay: hovered_hint.id,
                            inlay_position: hovered_hint.position,
                            range: highlight_start..highlight_end,
                        };
                        if let Some((server_id, command)) = hovered_hint_part.command
                            && let Some(inlay_hints) = editor.inlay_hints.as_mut()
                        {
                            inlay_command_updated = true;
                            inlay_hints.hovered_command = Some(HoveredInlayHintCommand {
                                highlight: highlight.clone(),
                                buffer_id,
                                action: CodeAction {
                                    server_id,
                                    range: cached_hint.position..cached_hint.position,
                                    lsp_action: LspAction::Command(command),
                                    resolved: true,
                                },
                            });
                        }
                        if let Some(tooltip) = hovered_hint_part.tooltip {
                            hover_task = hover_popover::hover_at_inlay(
                                editor,
                                InlayHover {
                                    tooltip: match tooltip {
                                        InlayHintLabelPartTooltip::String(text) => {
                                            HoverBlock { text, kind: HoverBlockKind::PlainText }
                                        }
                                        InlayHintLabelPartTooltip::MarkupContent(content) => {
                                            HoverBlock { text: content.value, kind: content.kind }
                                        }
                                    },
                                    range: highlight.clone(),
                                },
                                window,
                                cx,
                            );
                        }
                        if let Some((language_server_id, location)) = hovered_hint_part.location
                            && secondary_held
                            && !editor.has_pending_nonempty_selection()
                        {
                            go_to_definition_updated = true;
                            show_link_definition(
                                shift_held,
                                editor,
                                TriggerPoint::InlayHint(highlight, location, language_server_id),
                                &snapshot,
                                window,
                                cx,
                            );
                        }
                    }
                }
            };

            if !go_to_definition_updated {
                editor.hide_hovered_link(cx);
            }
            if hover_task.is_none() {
                hover_popover::hover_at(editor, None, mouse_position, window, cx);
            }
            if !inlay_command_updated && let Some(inlay_hints) = editor.inlay_hints.as_mut() {
                inlay_hints.hovered_command = None;
            }
            let command_highlight = editor
                .inlay_hints
                .as_ref()
                .and_then(|inlay_hints| inlay_hints.hovered_command.as_ref())
                .map(|command| &command.highlight);
            if command_highlight != previous_command_highlight.as_ref() {
                cx.notify();
            }
            hover_task
        };
        self.hover_state.hint_hover_task = match resolved_hint {
            ResolvedHint::Resolved(hint) => update_hover(self, window, cx, hint, secondary_held, shift_held),
            ResolvedHint::Resolving(task) => Some(cx.spawn_in(window, async move |editor, cx| {
                let Some(hint) = task.await else {
                    return;
                };
                let Some(show_hover) = editor
                    .update_in(cx, |editor, window, cx| {
                        let modifiers = window.modifiers();
                        let secondary_held = Editor::is_cmd_or_ctrl_pressed(&modifiers, cx);
                        update_hover(editor, window, cx, hint, secondary_held, modifiers.shift)
                    })
                    .ok()
                    .flatten()
                else {
                    return;
                };
                show_hover.await;
            })),
        };
    }

    pub(crate) fn can_accept_inlay_hint(&self, cx: &App) -> bool {
        self.lsp_data_enabled()
            && !self.read_only(cx)
            && self.project().is_some()
            && self.inlay_hints.is_some()
    }

    pub(crate) fn inlay_hint_visibility_changed(&mut self, cx: &mut Context<Self>) {
        if self.inlay_hints.as_ref().is_none_or(|inlay_hints| inlay_hints.pending_text_edit.is_none()) {
            return;
        }
        let display_snapshot = self.display_snapshot(cx);
        if let Some(inlay_hints) = &mut self.inlay_hints {
            inlay_hints
                .remove_pending_text_edit(|target| !display_snapshot.is_inlay_visible(target.hint_id, target.position));
        }
    }

    pub(crate) fn is_inlay_hint_resolved(&self, buffer_id: BufferId, hint_id: InlayId, cx: &App) -> bool {
        self.project().is_some_and(|project| {
            project
                .read(cx)
                .lsp_store()
                .read(cx)
                .cached_inlay_hint(buffer_id, hint_id)
                .is_some_and(|hint| hint.resolve_state == ResolveState::Resolved)
        })
    }

    pub(crate) fn apply_inlay_hint_text_edits(
        &mut self,
        mut targets: impl Iterator<Item = HintTarget> + 'static,
        clicked_label_offset: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(inlay_hints) = self.inlay_hints.as_mut() else {
            return false;
        };
        inlay_hints.pending_text_edit = None;
        inlay_hints.pending_refresh_acceptance = None;
        if !self.lsp_data_enabled() || self.read_only(cx) {
            return false;
        }
        let Some(lsp_store) = self.project().map(|project| project.read(cx).lsp_store()) else {
            return false;
        };
        while let Some(target) = targets.next() {
            if self.is_buffer_read_only(target.buffer_id, cx) {
                continue;
            }
            let Some((has_text_edits, resolved)) = lsp_store
                .read(cx)
                .cached_inlay_hint(target.buffer_id, target.hint_id)
                .map(|hint| (hint.has_text_edits(), hint.resolve_state == ResolveState::Resolved))
            else {
                if self.accept_inlay_hint_after_refresh(clicked_label_offset.map(|offset| (target, offset)), window, cx)
                {
                    return true;
                }
                continue;
            };
            let needs_resolve = !has_text_edits || (clicked_label_offset.is_some() && !resolved);
            let resolve = if needs_resolve {
                match lsp_store
                    .update(cx, |lsp_store, cx| lsp_store.resolved_hint(target.buffer_id, target.hint_id, cx))
                {
                    Some(ResolvedHint::Resolving(resolve)) => Some(resolve),
                    Some(ResolvedHint::Resolved(_)) if has_text_edits => None,
                    Some(ResolvedHint::Resolved(_)) | None => continue,
                }
            } else {
                None
            };
            if let Some(resolve) = resolve {
                let Some(inlay_hints) = self.inlay_hints.as_mut() else {
                    return false;
                };
                inlay_hints.pending_text_edit = Some(PendingHintTextEdit {
                    target,
                    _apply_task: cx.spawn_in(window, async move |editor, cx| {
                        let resolved = resolve.await;
                        editor
                            .update_in(cx, |editor, window, cx| {
                                let Some(pending) = editor
                                    .inlay_hints
                                    .as_mut()
                                    .and_then(|inlay_hints| inlay_hints.pending_text_edit.take())
                                else {
                                    return;
                                };
                                if resolved.is_some()
                                    && !editor.apply_inlay_hint_text_edit(
                                        &lsp_store,
                                        pending.target,
                                        clicked_label_offset,
                                        window,
                                        cx,
                                    )
                                {
                                    editor.apply_inlay_hint_text_edits(targets, clicked_label_offset, window, cx);
                                }
                            })
                            .ok();
                    }),
                });
                return true;
            }
            if self.apply_inlay_hint_text_edit(&lsp_store, target, clicked_label_offset, window, cx) {
                return true;
            }
        }
        false
    }

    /// Updates inlay hints for the visible ranges of the singleton buffer(s).
    /// Based on its parameters, either invalidates the previous data, or appends to it.
    pub(crate) fn refresh_inlay_hints(&mut self, reason: InlayHintRefreshReason, cx: &mut Context<Self>) {
        if !self.lsp_data_enabled() || self.inlay_hints.is_none() {
            return;
        }
        let Some(semantics_provider) = self.semantics_provider() else {
            return;
        };
        let Some(invalidate_cache) = self.refresh_editor_data(&reason, cx) else {
            return;
        };

        let debounce = match &reason {
            InlayHintRefreshReason::SettingsChange(_) | InlayHintRefreshReason::BuffersRemoved(_) => None,
            _may_need_lsp_call => self.inlay_hints.as_ref().and_then(|inlay_hints| {
                if invalidate_cache.should_invalidate() {
                    inlay_hints.invalidate_debounce
                } else {
                    inlay_hints.append_debounce
                }
            }),
        };

        let mut visible_excerpts = self.visible_buffer_ranges(cx);
        visible_excerpts.retain(|(snapshot, _, _)| self.is_lsp_relevant(snapshot.file(), cx));

        let mut invalidate_hints_for_buffers = HashSet::default();
        let ignore_previous_fetches = match reason {
            InlayHintRefreshReason::SettingsChange(_)
            | InlayHintRefreshReason::ServerRemoved
            | InlayHintRefreshReason::LanguageServerRegistered
            | InlayHintRefreshReason::RefreshRequested { .. } => true,
            InlayHintRefreshReason::NewLinesShown | InlayHintRefreshReason::BuffersRemoved(_) => false,
            InlayHintRefreshReason::BufferEdited(buffer_id) => {
                let Some(affected_language) =
                    self.buffer().read(cx).buffer(buffer_id).and_then(|buffer| buffer.read(cx).language().cloned())
                else {
                    return;
                };

                invalidate_hints_for_buffers.extend(self.buffer().read(cx).all_buffers().into_iter().filter_map(
                    |buffer| {
                        let buffer = buffer.read(cx);
                        if buffer.language() == Some(&affected_language) { Some(buffer.remote_id()) } else { None }
                    },
                ));

                semantics_provider.invalidate_inlay_hints(&invalidate_hints_for_buffers, cx);
                visible_excerpts
                    .retain(|(buffer_snapshot, _, _)| buffer_snapshot.language() == Some(&affected_language));
                false
            }
        };

        let multi_buffer = self.buffer().clone();

        let Some(inlay_hints) = self.inlay_hints.as_mut() else {
            return;
        };

        if invalidate_cache.should_invalidate() && !matches!(reason, InlayHintRefreshReason::RefreshRequested { .. }) {
            if invalidate_hints_for_buffers.is_empty() {
                inlay_hints.clear();
            } else {
                inlay_hints.clear_for_buffers(
                    &invalidate_hints_for_buffers,
                    Self::visible_inlay_hints(self.display_map.read(cx)),
                    &multi_buffer.read(cx).snapshot(cx),
                );
            }
        }
        inlay_hints.invalidate_hints_for_buffers.extend(invalidate_hints_for_buffers);

        let mut buffers_to_query = HashMap::default();
        for (buffer_snapshot, visible_range, _) in visible_excerpts {
            let buffer_id = buffer_snapshot.remote_id();

            if !self.registered_buffers.contains_key(&buffer_id) {
                continue;
            }

            let Some(buffer) = multi_buffer.read(cx).buffer(buffer_id) else {
                continue;
            };

            let buffer_version = buffer_snapshot.version().clone();
            let buffer_anchor_range =
                buffer_snapshot.anchor_before(visible_range.start)..buffer_snapshot.anchor_after(visible_range.end);

            let visible_excerpts = buffers_to_query.entry(buffer_id).or_insert_with(|| VisibleExcerpts {
                ranges: Vec::new(),
                buffer_version: buffer_version.clone(),
                buffer: buffer.clone(),
            });
            visible_excerpts.buffer_version = buffer_version;
            visible_excerpts.ranges.push(buffer_anchor_range);
        }

        for (buffer_id, visible_excerpts) in buffers_to_query {
            let Some(buffer) = multi_buffer.read(cx).buffer(buffer_id) else {
                continue;
            };

            let (fetched_for_version, fetched_chunks) = inlay_hints.hint_chunk_fetching.entry(buffer_id).or_default();
            if visible_excerpts.buffer_version.changed_since(fetched_for_version) {
                fetched_for_version.clone_from(&visible_excerpts.buffer_version);
                fetched_chunks.clear();
                inlay_hints.hint_refresh_tasks.remove(&buffer_id);
            }

            let known_chunks = if ignore_previous_fetches {
                None
            } else {
                Some((fetched_for_version.clone(), fetched_chunks.clone()))
            };

            let mut applicable_chunks =
                semantics_provider.applicable_inlay_chunks(&buffer, &visible_excerpts.ranges, cx);
            applicable_chunks.retain(|chunk| fetched_chunks.insert(chunk.clone()));
            if applicable_chunks.is_empty() && !ignore_previous_fetches {
                continue;
            }
            inlay_hints.hint_refresh_tasks.entry(buffer_id).or_default().push(spawn_editor_hints_refresh(
                buffer_id,
                invalidate_cache,
                debounce,
                visible_excerpts,
                known_chunks,
                applicable_chunks,
                cx,
            ));
        }
    }

    pub(crate) fn hovered_inlay_hint_command(&self) -> Option<&HoveredInlayHintCommand> {
        self.inlay_hints.as_ref()?.hovered_command.as_ref()
    }

    pub(crate) fn activate_hovered_inlay_hint_command(
        &self,
        snapshot: &EditorSnapshot,
        down: Option<DisplayPoint>,
        up: Option<DisplayPoint>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(state) = self
            .hovered_inlay_hint_command()
            .filter(|state| state.contains_glyph(snapshot, down) && state.contains_glyph(snapshot, up))
        else {
            return false;
        };
        let Some(buffer) = self.buffer().read(cx).buffer(state.buffer_id) else {
            return false;
        };
        let Some(project) = self.project().cloned() else {
            return false;
        };
        let action = state.action.clone();
        project.update(cx, |project, cx| project.apply_code_action(buffer, action, true, cx)).detach_and_log_err(cx);
        true
    }

    fn is_buffer_read_only(&self, buffer_id: BufferId, cx: &App) -> bool {
        self.buffer.read(cx).buffer(buffer_id).is_none_or(|buffer| buffer.read(cx).read_only())
    }

    fn apply_inlay_hint_text_edit(
        &mut self,
        lsp_store: &Entity<LspStore>,
        target: HintTarget,
        clicked_label_offset: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.lsp_data_enabled() || self.read_only(cx) {
            return false;
        }
        let HintTarget { buffer_id, hint_id, position } = target;
        let display_snapshot = self.display_snapshot(cx);
        if !self.inlay_hints.as_ref().is_some_and(|inlay_hints| inlay_hints.added_hints.contains_key(&hint_id))
            || !display_snapshot.is_inlay_visible(hint_id, position)
        {
            return false;
        }
        let Some(buffer) = self.buffer.read(cx).buffer(buffer_id) else {
            return false;
        };
        let buffer_snapshot = buffer.read(cx);
        let Some(cached_hint) = lsp_store.read(cx).cached_inlay_hint(buffer_id, hint_id) else {
            return false;
        };
        if clicked_label_offset.is_some_and(|offset| {
            cached_hint.resolve_state != ResolveState::Resolved || label_part_has_command(cached_hint, offset)
        }) {
            return false;
        }
        let Some(text_edits) = cached_hint.text_edits.clone().filter(|text_edits| !text_edits.edits.is_empty()) else {
            return false;
        };
        if buffer_snapshot.read_only() || !buffer_snapshot.version().observed_all(&text_edits.buffer_version) {
            return false;
        }
        self.finalize_last_transaction(cx);
        self.transact(window, cx, |_, _, cx| {
            buffer.update(cx, |buffer, cx| buffer.edit(text_edits.edits, None, cx));
        });
        self.finalize_last_transaction(cx);
        self.splice_inlays(&[hint_id], Vec::new(), cx);
        true
    }

    fn accept_inlay_hint_after_refresh(
        &mut self,
        clicked_hint: Option<(HintTarget, usize)>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let snapshot = self.display_snapshot(cx);
        let clicked_hint = if let Some((target, offset)) = clicked_hint {
            if !snapshot.is_inlay_visible(target.hint_id, target.position) {
                return false;
            }
            let Some(text) = Self::visible_inlay_hints(self.display_map.read(cx))
                .find(|hint| hint.id == target.hint_id)
                .map(|hint| hint.text().clone())
            else {
                return false;
            };
            Some((target, offset, text))
        } else {
            None
        };
        let buffer_ids = if let Some((target, _, _)) = &clicked_hint {
            HashSet::from_iter([target.buffer_id])
        } else {
            self.selections
                .all_adjusted(&snapshot)
                .into_iter()
                .flat_map(|selection| snapshot.buffer_snapshot().range_to_buffer_ranges(selection.range()))
                .map(|(buffer, _, _)| buffer.remote_id())
                .collect::<HashSet<_>>()
        };
        let Some(inlay_hints) = self.inlay_hints.as_ref() else {
            return false;
        };
        let mut refreshes = inlay_hints.pending_refreshes(&buffer_ids);
        if refreshes.is_empty() {
            return false;
        }
        let edit_count = snapshot.buffer_snapshot().edit_count();
        let selections = self.selections.all_anchors(&snapshot);
        let line_mode = self.selections.line_mode();
        let task = cx.spawn_in(window, {
            let buffer_ids = buffer_ids.clone();
            async move |editor, cx| {
                let refreshed = loop {
                    if join_all(refreshes).await.iter().any(Result::is_err) {
                        break false;
                    }
                    let Ok(Some(pending)) = editor
                        .read_with(cx, |editor, _| Some(editor.inlay_hints.as_ref()?.pending_refreshes(&buffer_ids)))
                    else {
                        return;
                    };
                    if pending.is_empty() {
                        break true;
                    }
                    refreshes = pending;
                };
                editor
                    .update_in(cx, |editor, window, cx| {
                        editor.inlay_hints.as_mut()?.pending_refresh_acceptance = None;
                        if !refreshed || !editor.can_accept_inlay_hint(cx) {
                            return None;
                        }
                        let snapshot = editor.display_snapshot(cx);
                        if snapshot.buffer_snapshot().edit_count() != edit_count {
                            return None;
                        }
                        let targets = if let Some((target, _, text)) = &clicked_hint {
                            if !snapshot.buffer_snapshot().can_resolve(&target.position) {
                                return None;
                            }
                            let buffer_snapshot = snapshot.buffer_snapshot();
                            let position = target.position.bias_right(buffer_snapshot);
                            let hint = Self::visible_inlay_hints(editor.display_map.read(cx))
                                .filter(|hint| {
                                    hint.position.bias_right(buffer_snapshot).cmp(&position, buffer_snapshot).is_eq()
                                        && hint.text().chars().eq(text.chars())
                                        && snapshot.is_inlay_visible(hint.id, hint.position)
                                })
                                .exactly_one()
                                .ok()?;
                            vec![HintTarget { hint_id: hint.id, position: hint.position, ..*target }]
                        } else {
                            if editor.selections.all_anchors(&snapshot) != selections
                                || editor.selections.line_mode() != line_mode
                            {
                                return None;
                            }
                            editor.inlay_hints_at_selections(&snapshot, cx).collect::<Vec<_>>()
                        };
                        editor.apply_inlay_hint_text_edits(
                            targets.into_iter(),
                            clicked_hint.as_ref().map(|(_, offset, _)| *offset),
                            window,
                            cx,
                        );
                        Some(())
                    })
                    .ok();
            }
        });
        if let Some(inlay_hints) = &mut self.inlay_hints {
            inlay_hints.pending_refresh_acceptance = Some((buffer_ids, task));
        }
        true
    }

    fn inlay_hints_at_selections<'a>(
        &'a self,
        display_snapshot: &'a DisplaySnapshot,
        cx: &'a App,
    ) -> impl Iterator<Item = HintTarget> + 'a {
        let inlays = self.display_map.read(cx).current_inlays().as_slice();
        let buffer_snapshot = display_snapshot.buffer_snapshot();
        self.selections.all_adjusted(display_snapshot).into_iter().flat_map(move |selection| {
            let start = buffer_snapshot.anchor_before(selection.start);
            let end = buffer_snapshot.anchor_after(selection.end);
            let first_inlay = inlays.partition_point(|inlay| inlay.position.cmp(&start, buffer_snapshot).is_lt());
            inlays[first_inlay..]
                .iter()
                .take_while(move |inlay| inlay.position.cmp(&end, buffer_snapshot).is_le())
                .filter(|inlay| matches!(inlay.id, InlayId::Hint(_)))
                .filter(move |inlay| display_snapshot.is_inlay_visible(inlay.id, inlay.position))
                .filter_map(|inlay| {
                    Some(HintTarget {
                        buffer_id: inlay.position.raw_text_anchor()?.buffer_id,
                        hint_id: inlay.id,
                        position: inlay.position,
                    })
                })
        })
    }

    fn refresh_editor_data(
        &mut self,
        reason: &InlayHintRefreshReason,
        cx: &mut Context<'_, Editor>,
    ) -> Option<InvalidationStrategy> {
        let inlay_hints = self.inlay_hints.as_mut()?;

        let invalidate_cache = match reason {
            InlayHintRefreshReason::SettingsChange(new_settings) => {
                let visible_inlay_hints = Self::visible_inlay_hints(self.display_map.read(cx)).collect::<Vec<_>>();
                match inlay_hints.update_settings(*new_settings, visible_inlay_hints) {
                    ControlFlow::Break(()) => return None,
                    ControlFlow::Continue(splice) => {
                        if let Some(InlaySplice { to_remove, to_insert }) = splice {
                            self.splice_inlays(&to_remove, to_insert, cx);
                        }
                        InvalidationStrategy::None
                    }
                }
            }
            InlayHintRefreshReason::BuffersRemoved(buffers_removed) => {
                let to_remove = self
                    .display_map
                    .read(cx)
                    .current_inlays()
                    .filter_map(|inlay| {
                        let anchor = inlay.position.raw_text_anchor()?;
                        if buffers_removed.contains(&anchor.buffer_id) { Some(inlay.id) } else { None }
                    })
                    .collect::<Vec<_>>();
                self.splice_inlays(&to_remove, Vec::new(), cx);
                return None;
            }
            InlayHintRefreshReason::ServerRemoved => InvalidationStrategy::BufferEdited,
            InlayHintRefreshReason::NewLinesShown | InlayHintRefreshReason::LanguageServerRegistered => {
                InvalidationStrategy::None
            }
            InlayHintRefreshReason::BufferEdited(_) => InvalidationStrategy::BufferEdited,
            InlayHintRefreshReason::RefreshRequested { server_id } => {
                InvalidationStrategy::RefreshRequested { server_id: *server_id }
            }
        };

        Some(invalidate_cache)
    }

    fn visible_inlay_hints(display_map: &DisplayMap) -> impl Iterator<Item = Inlay> + use<'_> {
        display_map.current_inlays().filter(move |inlay| matches!(inlay.id, InlayId::Hint(_))).cloned()
    }

    #[allow(clippy::type_complexity)]
    fn inlay_hints_for_buffer(
        &self,
        invalidate_cache: InvalidationStrategy,
        buffer_excerpts: VisibleExcerpts,
        known_chunks: Option<(Global, HashSet<Range<BufferRow>>)>,
        cx: &mut Context<Self>,
    ) -> Option<Vec<Task<(Range<BufferRow>, anyhow::Result<CacheInlayHints>)>>> {
        let semantics_provider = self.semantics_provider()?;

        let new_hint_tasks = semantics_provider
            .inlay_hints(invalidate_cache, buffer_excerpts.buffer, buffer_excerpts.ranges, known_chunks, cx)
            .unwrap_or_default();

        let mut hint_tasks = None;
        for (row_range, new_hints_task) in new_hint_tasks {
            hint_tasks.get_or_insert_with(Vec::new).push(cx.spawn(async move |_, _| (row_range, new_hints_task.await)));
        }
        hint_tasks
    }

    fn apply_fetched_hints(
        &mut self,
        buffer_id: BufferId,
        query_version: Global,
        invalidate_cache: InvalidationStrategy,
        new_hints: Vec<(Range<BufferRow>, anyhow::Result<CacheInlayHints>)>,
        cx: &mut Context<Self>,
    ) {
        let multi_buffer_snapshot = self.buffer.read(cx).snapshot(cx);
        // Keep the visible inlays themselves (not only their ids): already displayed hints
        // take part in the bias normalization below.
        let visible_hints_for_buffer = Self::visible_inlay_hints(self.display_map.read(cx))
            .filter_map(|inlay| {
                let (anchor, _) = multi_buffer_snapshot.anchor_to_buffer_anchor(inlay.position)?;
                (anchor.buffer_id == buffer_id).then_some((inlay, anchor))
            })
            .collect::<Vec<_>>();
        let Some(inlay_hints) = &mut self.inlay_hints else {
            return;
        };
        let Some(buffer_snapshot) = self.buffer.read(cx).buffer(buffer_id).map(|buffer| buffer.read(cx).snapshot())
        else {
            return;
        };

        let mut hints_to_remove = Vec::new();

        // If we've received hints from the cache, it means `invalidate_cache` had invalidated whatever possible there,
        // and most probably there are no more hints with IDs from `visible_inlay_hint_ids` in the cache.
        // So, if we hover such hints, no resolve will happen.
        //
        // Another issue is in the fact that changing one buffer may lead to other buffers' hints changing, so more cache entries may be removed.
        // Hence, clear all excerpts' hints in the multi buffer: later, the invalidated ones will re-trigger the LSP query, the rest will be restored
        // from the cache.
        let invalidating = invalidate_cache.should_invalidate();
        if invalidating {
            let visible_inlay_hint_ids = visible_hints_for_buffer.iter().map(|(inlay, _)| inlay.id);
            for hint_id in visible_inlay_hint_ids.clone() {
                inlay_hints.added_hints.remove(&hint_id);
            }
            hints_to_remove.extend(visible_inlay_hint_ids);

            // When invalidating, this task removes ALL visible hints for the buffer
            // but only adds back hints for its own chunk ranges. Chunks fetched by
            // other concurrent tasks (e.g., a scroll task that completed before this
            // edit task) would have their hints removed but remain marked as "already
            // fetched" in hint_chunk_fetching, preventing re-fetch on the next
            // NewLinesShown. Fix: retain only chunks that this task has results for.
            let task_chunk_ranges: HashSet<&Range<BufferRow>> = new_hints.iter().map(|(range, _)| range).collect();
            if let Some((_, fetched_chunks)) = inlay_hints.hint_chunk_fetching.get_mut(&buffer_id) {
                fetched_chunks.retain(|chunk| task_chunk_ranges.contains(chunk));
            }
        }

        // Already displayed hints that survive this splice: `added_hints` keeps them from
        // being re-inserted, so they must take part in the shared-bias decision explicitly.
        let surviving_visible_hints = if invalidating || inlay_hints.invalidate_hints_for_buffers.contains(&buffer_id) {
            Vec::new()
        } else {
            visible_hints_for_buffer
                .into_iter()
                .map(|(inlay, anchor)| {
                    let offset = anchor.to_offset(&buffer_snapshot);
                    (inlay, anchor.bias, offset)
                })
                .collect::<Vec<_>>()
        };

        // Flatten all servers' hints of the fetched chunks. Each chunk result contains
        // every server's cached hints for it, and colocated hints always share a chunk.
        let mut new_hints = new_hints
            .into_iter()
            .filter_map(|(chunk_range, hints_result)| {
                let chunks_fetched = inlay_hints.hint_chunk_fetching.get_mut(&buffer_id);
                match hints_result {
                    Ok(new_hints) => {
                        if new_hints.is_empty()
                            && let Some((_, chunks_fetched)) = chunks_fetched
                        {
                            chunks_fetched.remove(&chunk_range);
                        }
                        Some(new_hints)
                    }
                    Err(e) => {
                        log::error!("Failed to query inlays for buffer row range {chunk_range:?}, {e:#}");
                        if let Some((for_version, chunks_fetched)) = chunks_fetched
                            && for_version == &query_version
                        {
                            chunks_fetched.remove(&chunk_range);
                        }
                        None
                    }
                }
            })
            .flat_map(|hints_by_server| {
                hints_by_server.into_iter().flat_map(|(server_id, hints)| {
                    let buffer_snapshot = &buffer_snapshot;
                    hints.into_iter().map(move |(hint_id, hint)| {
                        let offset = hint.position.to_offset(buffer_snapshot);
                        (server_id, hint_id, hint, offset)
                    })
                })
            })
            .collect::<Vec<_>>();

        let rebiased_inlays =
            normalize_hint_biases(&mut new_hints, surviving_visible_hints, &buffer_snapshot, &multi_buffer_snapshot);

        // `splice_inlays` drops removed ids from `added_hints`: remember the kinds to
        // restore, falling back to the fetched copy's kind if the id was not tracked.
        let rebiased_kinds = rebiased_inlays
            .iter()
            .filter_map(|inlay| {
                let kind = inlay_hints.added_hints.get(&inlay.id).copied().or_else(|| {
                    new_hints.iter().find(|(_, hint_id, _, _)| *hint_id == inlay.id).map(|(_, _, hint, _)| hint.kind)
                })?;
                Some((inlay.id, kind))
            })
            .collect::<Vec<_>>();
        hints_to_remove.extend(rebiased_inlays.iter().map(|inlay| inlay.id));

        // Ids that must not be inserted from `new_hints`: re-biased inlays are re-inserted
        // separately, and each id is inserted at most once per batch.
        let mut seen_ids = rebiased_inlays.iter().map(|inlay| inlay.id).collect::<HashSet<InlayId>>();

        // Already displayed hints go first, so they claim the (offset, text) dedup slot and
        // suppress identical copies from other servers, independently of the servers'
        // iteration order. The sort is stable: the relative order of the rest is preserved.
        new_hints.sort_by_key(|(_, hint_id, _, _)| {
            !(inlay_hints.added_hints.contains_key(hint_id) || seen_ids.contains(hint_id))
        });

        let mut inserted_hint_text: HashMap<usize, HashMap<String, LanguageServerId>> = HashMap::default();
        let new_hints = new_hints
            .into_iter()
            // Hidden kinds must not claim a dedup slot, or they could suppress a visible
            // identical hint from another server.
            .filter(|(_, _, hint, _)| inlay_hints.allowed_hint_kinds.contains(&hint.kind))
            // Deduplicate identical hints from different servers by (offset, text),
            // independently of the anchor bias.
            .filter(|(server_id, _, hint, offset)| {
                match inserted_hint_text.entry(*offset).or_default().entry(hint.text().to_string()) {
                    hash_map::Entry::Occupied(o) => o.get() == server_id,
                    hash_map::Entry::Vacant(v) => {
                        v.insert(*server_id);
                        true
                    }
                }
            })
            // Skip already shown hints; `added_hints` is updated after the splice.
            .filter(|(_, hint_id, _, _)| !inlay_hints.added_hints.contains_key(hint_id) && seen_ids.insert(*hint_id))
            .map(|(_, hint_id, hint, _)| (hint_id, hint))
            .sorted_by(|(_, a), (_, b)| a.position.cmp(&b.position, &buffer_snapshot))
            .collect::<Vec<_>>();

        // Re-biased displayed hints go first: at equal positions `InlayMap::splice` places
        // later insertions after earlier ones, so they keep preceding the new hints.
        let hints_to_insert: Vec<Inlay> = rebiased_inlays
            .into_iter()
            .chain(
                multi_buffer_snapshot
                    .text_anchors_to_visible_anchors(new_hints.iter().map(|(_, h)| h.position))
                    .into_iter()
                    .zip(&new_hints)
                    .filter_map(|(position, (hint_id, hint))| Some(Inlay::hint(*hint_id, position?, hint))),
            )
            .collect();

        let invalidate_hints_for_buffers = std::mem::take(&mut inlay_hints.invalidate_hints_for_buffers);
        if !invalidate_hints_for_buffers.is_empty() {
            hints_to_remove.extend(
                Self::visible_inlay_hints(self.display_map.read(cx))
                    .filter(|inlay| {
                        multi_buffer_snapshot
                            .anchor_to_buffer_anchor(inlay.position)
                            .is_none_or(|(anchor, _)| invalidate_hints_for_buffers.contains(&anchor.buffer_id))
                    })
                    .map(|inlay| inlay.id),
            );
        }

        self.splice_inlays(&hints_to_remove, hints_to_insert, cx);

        // Register insertions only after the splice, which drops every removed id from
        // `added_hints`, including re-biased hints re-inserted under the same id.
        if let Some(inlay_hints) = &mut self.inlay_hints {
            let fetched = new_hints.iter().map(|(id, hint)| (*id, hint.kind));
            inlay_hints.added_hints.extend(rebiased_kinds.into_iter().chain(fetched));
        }
    }
}

/// Makes all hints at the same buffer offset share one bias: `Right` on conflict,
/// otherwise their common bias. Fetched hints are re-biased in place.
///
/// Returns the displayed inlays at offsets that switched to `Right`, already re-biased
/// and in their current display order. They must be removed and re-inserted before
/// the fetched hints, so they keep preceding them at equal positions.
fn normalize_hint_biases(
    new_hints: &mut [(LanguageServerId, InlayId, InlayHint, usize)],
    displayed_hints: Vec<(Inlay, Bias, usize)>,
    buffer_snapshot: &BufferSnapshot,
    multi_buffer_snapshot: &MultiBufferSnapshot,
) -> Vec<Inlay> {
    let mut bias_by_offset: HashMap<usize, Bias> = HashMap::default();
    let fetched = new_hints.iter().map(|(_, _, hint, offset)| (*offset, hint.position.bias));
    let displayed = displayed_hints.iter().map(|(_, bias, offset)| (*offset, *bias));
    for (offset, bias) in fetched.chain(displayed) {
        bias_by_offset
            .entry(offset)
            .and_modify(|existing| {
                if *existing != bias {
                    *existing = Bias::Right;
                }
            })
            .or_insert(bias);
    }

    for (_, _, hint, offset) in new_hints.iter_mut() {
        if bias_by_offset.get(offset) == Some(&Bias::Right) {
            hint.position = hint.position.bias_right(buffer_snapshot);
        }
    }

    let offsets_to_rebias = displayed_hints
        .iter()
        .filter(|(_, bias, offset)| *bias == Bias::Left && bias_by_offset.get(offset) == Some(&Bias::Right))
        .map(|(_, _, offset)| *offset)
        .collect::<HashSet<_>>();
    displayed_hints
        .into_iter()
        .filter(|(_, _, offset)| offsets_to_rebias.contains(offset))
        .map(|(inlay, _, _)| Inlay { position: inlay.position.bias_right(multi_buffer_snapshot), ..inlay })
        .collect()
}

fn label_part_has_command(hint: &InlayHint, offset_in_hint: usize) -> bool {
    let InlayHintLabel::LabelParts(label_parts) = &hint.label else {
        return false;
    };
    let left_padding = usize::from(hint.padding_left && !hint.text().starts_with(" "));
    offset_in_hint
        .checked_sub(left_padding)
        .and_then(|offset_in_label| hover_popover::find_hovered_hint_part(label_parts.clone(), offset_in_label))
        .is_some_and(|(part, _)| part.command.is_some())
}

#[derive(Debug)]
struct VisibleExcerpts {
    ranges: Vec<Range<text::Anchor>>,
    buffer_version: Global,
    buffer: Entity<language::Buffer>,
}

fn spawn_editor_hints_refresh(
    buffer_id: BufferId,
    invalidate_cache: InvalidationStrategy,
    debounce: Option<Duration>,
    buffer_excerpts: VisibleExcerpts,
    known_chunks: Option<(Global, HashSet<Range<BufferRow>>)>,
    applicable_chunks: Vec<Range<BufferRow>>,
    cx: &Context<'_, Editor>,
) -> HintRefreshTask {
    let (finished_sender, finished) = oneshot::channel();
    let task = cx.spawn(async move |editor, cx| {
        if let Some(debounce) = debounce {
            cx.background_executor().timer(debounce).await;
        }

        let query_version = buffer_excerpts.buffer_version.clone();
        let Some(hint_tasks) = editor
            .update(cx, |editor, cx| editor.inlay_hints_for_buffer(invalidate_cache, buffer_excerpts, known_chunks, cx))
            .ok()
        else {
            return;
        };
        let hint_tasks = hint_tasks.unwrap_or_default();
        if hint_tasks.is_empty() {
            editor
                .update(cx, |editor, _| {
                    if let Some((_, hint_chunk_fetching)) = editor
                        .inlay_hints
                        .as_mut()
                        .and_then(|inlay_hints| inlay_hints.hint_chunk_fetching.get_mut(&buffer_id))
                    {
                        for applicable_chunks in &applicable_chunks {
                            hint_chunk_fetching.remove(applicable_chunks);
                        }
                    }
                })
                .ok();
            return;
        }
        let new_hints = join_all(hint_tasks).await;
        editor
            .update(cx, |editor, cx| {
                editor.apply_fetched_hints(buffer_id, query_version, invalidate_cache, new_hints, cx);
            })
            .ok();
        finished_sender.send(()).ok();
    });
    HintRefreshTask { task, finished: finished.shared() }
}
