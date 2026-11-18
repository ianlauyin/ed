use std::any::Any;
use std::any::TypeId;
use std::rc::Rc;

use gpui::Action;
use gpui::App;
use gpui::Context;
use gpui::DispatchPhase;
use gpui::Entity;
use gpui::ModifiersChangedEvent;
use gpui::TaskExt as _;
use gpui::Window;
use util::ResultExt as _;
use workspace::item::Item as _;
use workspace::item::ItemBufferKind;

use super::EditorElement;
use super::EditorLayout;
use crate::Editor;
use crate::HalfPageDown;
use crate::HalfPageUp;
use crate::HandleInput;
use crate::LineDown;
use crate::LineUp;
use crate::PageDown;
use crate::PageUp;
use crate::hover_popover;
use crate::rust_analyzer_ext;
use crate::scroll::scroll_amount::ScrollAmount;

impl EditorElement {
    pub(super) fn register_actions(&self, window: &mut Window, cx: &mut App) {
        let editor = &self.editor;
        editor.update(cx, |editor, cx| {
            for action in editor.editor_actions.borrow().values() {
                (action)(editor, window, cx);
            }
        });

        rust_analyzer_ext::apply_related_actions(editor, window, cx);

        register_action(editor, window, Editor::open_context_menu);
        register_action(editor, window, Editor::move_left);
        register_action(editor, window, Editor::move_right);
        register_action(editor, window, Editor::move_down);
        register_action(editor, window, Editor::move_down_by_lines);
        register_action(editor, window, Editor::select_down_by_lines);
        register_action(editor, window, Editor::move_up);
        register_action(editor, window, Editor::move_up_by_lines);
        register_action(editor, window, Editor::select_up_by_lines);
        register_action(editor, window, Editor::select_page_down);
        register_action(editor, window, Editor::select_page_up);
        register_action(editor, window, Editor::cancel);
        register_action(editor, window, Editor::next_snippet_tabstop);
        register_action(editor, window, Editor::previous_snippet_tabstop);
        register_action(editor, window, Editor::copy);
        register_action(editor, window, Editor::diff_clipboard_with_selection);
        register_action(editor, window, Editor::move_page_up);
        register_action(editor, window, Editor::move_page_down);
        register_action(editor, window, Editor::next_screen);
        register_action(editor, window, Editor::scroll_cursor_top);
        register_action(editor, window, Editor::scroll_cursor_center);
        register_action(editor, window, Editor::scroll_cursor_bottom);
        register_action(editor, window, Editor::scroll_cursor_center_top_bottom);
        register_action(editor, window, |editor, _: &LineDown, window, cx| {
            editor.scroll_screen_with_cursor_margin(&ScrollAmount::Line(1.), window, cx);
        });
        register_action(editor, window, |editor, _: &LineUp, window, cx| {
            editor.scroll_screen_with_cursor_margin(&ScrollAmount::Line(-1.), window, cx);
        });
        register_action(editor, window, |editor, _: &HalfPageDown, window, cx| {
            editor.scroll_screen_with_cursor_margin(&ScrollAmount::Page(0.5), window, cx);
        });
        register_action(editor, window, |editor, _: &HalfPageUp, window, cx| {
            editor.scroll_screen_with_cursor_margin(&ScrollAmount::Page(-0.5), window, cx);
        });
        register_action(editor, window, |editor, _: &PageDown, window, cx| {
            editor.scroll_screen_with_cursor_margin(&ScrollAmount::Page(1.), window, cx);
        });
        register_action(editor, window, |editor, _: &PageUp, window, cx| {
            editor.scroll_screen_with_cursor_margin(&ScrollAmount::Page(-1.), window, cx);
        });
        register_action(editor, window, Editor::move_to_previous_word_start);
        register_action(editor, window, Editor::move_to_previous_subword_start);
        register_action(editor, window, Editor::move_to_next_word_end);
        register_action(editor, window, Editor::move_to_next_subword_end);
        register_action(editor, window, Editor::move_to_beginning_of_line);
        register_action(editor, window, Editor::move_to_end_of_line);
        register_action(editor, window, Editor::move_to_start_of_paragraph);
        register_action(editor, window, Editor::move_to_end_of_paragraph);
        register_action(editor, window, Editor::move_to_next_comment_paragraph);
        register_action(editor, window, Editor::move_to_previous_comment_paragraph);
        register_action(editor, window, Editor::move_to_beginning);
        register_action(editor, window, Editor::move_to_end);
        register_action(editor, window, Editor::move_to_start_of_excerpt);
        register_action(editor, window, Editor::move_to_start_of_next_excerpt);
        register_action(editor, window, Editor::move_to_end_of_excerpt);
        register_action(editor, window, Editor::move_to_end_of_previous_excerpt);
        register_action(editor, window, Editor::select_up);
        register_action(editor, window, Editor::select_down);
        register_action(editor, window, Editor::select_left);
        register_action(editor, window, Editor::select_right);
        register_action(editor, window, Editor::select_to_previous_word_start);
        register_action(editor, window, Editor::select_to_previous_subword_start);
        register_action(editor, window, Editor::select_to_next_word_end);
        register_action(editor, window, Editor::select_to_next_subword_end);
        register_action(editor, window, Editor::select_to_beginning_of_line);
        register_action(editor, window, Editor::select_to_end_of_line);
        register_action(editor, window, Editor::select_to_start_of_paragraph);
        register_action(editor, window, Editor::select_to_end_of_paragraph);
        register_action(editor, window, Editor::select_to_start_of_excerpt);
        register_action(editor, window, Editor::select_to_start_of_next_excerpt);
        register_action(editor, window, Editor::select_to_end_of_excerpt);
        register_action(editor, window, Editor::select_to_end_of_previous_excerpt);
        register_action(editor, window, Editor::select_to_beginning);
        register_action(editor, window, Editor::select_to_end);
        register_action(editor, window, Editor::select_all);
        register_action(editor, window, |editor, action, window, cx| {
            editor.select_all_matches(action, window, cx).log_err();
        });
        register_action(editor, window, Editor::select_line);
        register_action(editor, window, Editor::split_selection_into_lines);
        register_action(editor, window, Editor::add_selection_above);
        register_action(editor, window, Editor::add_selection_below);
        register_action(editor, window, Editor::insert_snippet_at_selections);
        register_action(editor, window, |editor, action, window, cx| {
            editor.select_next(action, window, cx).log_err();
        });
        register_action(editor, window, |editor, action, window, cx| {
            editor.select_previous(action, window, cx).log_err();
        });
        register_action(editor, window, |editor, action, window, cx| {
            editor.find_next_match(action, window, cx).log_err();
        });
        register_action(editor, window, |editor, action, window, cx| {
            editor.find_previous_match(action, window, cx).log_err();
        });
        register_action(editor, window, Editor::select_larger_syntax_node);
        register_action(editor, window, Editor::select_smaller_syntax_node);
        register_action(editor, window, Editor::select_next_syntax_node);
        register_action(editor, window, Editor::select_prev_syntax_node);
        register_action(editor, window, Editor::select_to_start_of_larger_syntax_node);
        register_action(editor, window, Editor::select_to_end_of_larger_syntax_node);
        register_action(editor, window, Editor::move_to_start_of_larger_syntax_node);
        register_action(editor, window, Editor::move_to_end_of_larger_syntax_node);
        register_action(editor, window, Editor::select_enclosing_symbol);
        register_action(editor, window, Editor::move_to_enclosing_bracket);
        register_action(editor, window, Editor::select_inside_delimiters);
        register_action(editor, window, Editor::select_around_delimiters);
        register_action(editor, window, Editor::undo_selection);
        register_action(editor, window, Editor::redo_selection);
        if editor.read(cx).buffer_kind(cx) == ItemBufferKind::Multibuffer {
            register_action(editor, window, Editor::expand_excerpts);
            register_action(editor, window, Editor::expand_excerpts_up);
            register_action(editor, window, Editor::expand_excerpts_down);
        }
        register_action(editor, window, Editor::go_to_diagnostic);
        register_action(editor, window, Editor::go_to_prev_diagnostic);
        register_action(editor, window, Editor::go_to_next_hunk);
        register_action(editor, window, Editor::go_to_prev_hunk);
        register_action(editor, window, Editor::go_to_next_document_highlight);
        register_action(editor, window, Editor::go_to_prev_document_highlight);
        register_action(editor, window, |editor, action, window, cx| {
            editor.go_to_definition(action, window, cx).detach_and_log_err(cx);
        });
        register_action(editor, window, |editor, action, window, cx| {
            editor.go_to_definition_split(action, window, cx).detach_and_log_err(cx);
        });
        register_action(editor, window, |editor, action, window, cx| {
            editor.go_to_declaration(action, window, cx).detach_and_log_err(cx);
        });
        register_action(editor, window, |editor, action, window, cx| {
            editor.go_to_declaration_split(action, window, cx).detach_and_log_err(cx);
        });
        register_action(editor, window, |editor, action, window, cx| {
            editor.go_to_implementation(action, window, cx).detach_and_log_err(cx);
        });
        register_action(editor, window, |editor, action, window, cx| {
            editor.go_to_implementation_split(action, window, cx).detach_and_log_err(cx);
        });
        register_action(editor, window, |editor, action, window, cx| {
            editor.go_to_type_definition(action, window, cx).detach_and_log_err(cx);
        });
        register_action(editor, window, |editor, action, window, cx| {
            editor.go_to_type_definition_split(action, window, cx).detach_and_log_err(cx);
        });
        register_action(editor, window, Editor::open_url);
        register_action(editor, window, Editor::open_selected_filename);
        register_action(editor, window, Editor::fold);
        register_action(editor, window, Editor::fold_at_level);
        register_action(editor, window, Editor::fold_at_level_1);
        register_action(editor, window, Editor::fold_at_level_2);
        register_action(editor, window, Editor::fold_at_level_3);
        register_action(editor, window, Editor::fold_at_level_4);
        register_action(editor, window, Editor::fold_at_level_5);
        register_action(editor, window, Editor::fold_at_level_6);
        register_action(editor, window, Editor::fold_at_level_7);
        register_action(editor, window, Editor::fold_at_level_8);
        register_action(editor, window, Editor::fold_at_level_9);
        register_action(editor, window, Editor::fold_all);
        register_action(editor, window, Editor::fold_function_bodies);
        register_action(editor, window, Editor::fold_recursive);
        register_action(editor, window, Editor::toggle_fold);
        register_action(editor, window, Editor::toggle_fold_recursive);
        register_action(editor, window, Editor::toggle_fold_all);
        register_action(editor, window, Editor::unfold_lines);
        register_action(editor, window, Editor::unfold_recursive);
        register_action(editor, window, Editor::unfold_all);
        register_action(editor, window, Editor::fold_selected_ranges);
        register_action(editor, window, Editor::set_mark);
        register_action(editor, window, Editor::save_location);
        register_action(editor, window, Editor::swap_selection_ends);
        register_action(editor, window, Editor::show_completions);
        register_action(editor, window, Editor::show_word_completions);
        register_action(editor, window, Editor::toggle_code_actions);
        register_action(editor, window, Editor::toggle_soft_wrap);
        register_action(editor, window, Editor::toggle_indent_guides);

        if editor.read(cx).lsp_data_enabled() {
            if editor.read(cx).can_accept_inlay_hint(cx) {
                register_action(editor, window, Editor::accept_inlay_hint);
            }
            register_action(editor, window, Editor::toggle_diagnostics);
        }
        if editor.read(cx).supports_minimap(cx) {
            register_action(editor, window, Editor::toggle_minimap);
        }
        register_action(editor, window, hover_popover::hover);
        register_action(editor, window, Editor::reveal_in_finder);
        register_action(editor, window, Editor::copy_path);
        register_action(editor, window, Editor::copy_relative_path);
        register_action(editor, window, Editor::copy_file_name);
        register_action(editor, window, Editor::copy_file_name_without_extension);
        register_action(editor, window, Editor::copy_highlight_json);
        register_action(editor, window, Editor::copy_file_location);
        register_action(editor, window, Editor::toggle_git_blame);
        if editor.read(cx).blame().is_some() {
            register_action(editor, window, Editor::open_git_blame_commit);
            if editor.update(cx, |editor, cx| editor.blame_revision_target(window, cx).is_some()) {
                register_action(editor, window, Editor::blame_revision);
            }
            if editor.update(cx, |editor, cx| editor.blame_previous_revision_target(window, cx).is_some()) {
                register_action(editor, window, Editor::blame_previous_revision);
            }
        }
        register_action(editor, window, Editor::toggle_selected_diff_hunks);
        register_action(editor, window, Editor::toggle_staged_selected_diff_hunks);
        register_action(editor, window, Editor::stage_and_next);
        register_action(editor, window, Editor::unstage_and_next);
        register_action(editor, window, Editor::expand_all_diff_hunks);
        register_action(editor, window, Editor::collapse_all_diff_hunks);
        register_action(editor, window, Editor::toggle_all_diff_hunks);
        register_action(editor, window, Editor::go_to_previous_change);
        register_action(editor, window, Editor::go_to_next_change);
        register_action(editor, window, Editor::go_to_prev_reference);
        register_action(editor, window, Editor::go_to_next_reference);
        register_action(editor, window, Editor::go_to_previous_symbol);
        register_action(editor, window, Editor::go_to_next_symbol);
        register_action(editor, window, Editor::restart_language_server);
        register_action(editor, window, Editor::stop_language_server);
        register_action(editor, window, Editor::show_character_palette);
        register_action(editor, window, |editor, action, window, cx| {
            if let Some(task) = editor.compose_completion(action, window, cx) {
                editor.detach_and_notify_err(task, window, cx);
            } else {
                cx.propagate();
            }
        });
        register_action(editor, window, |editor, action, window, cx| {
            if let Some(task) = editor.find_all_references(action, window, cx) {
                task.detach_and_log_err(cx);
            } else {
                cx.propagate();
            }
        });

        register_action(editor, window, Editor::context_menu_first);
        register_action(editor, window, Editor::context_menu_prev);
        register_action(editor, window, Editor::context_menu_next);
        register_action(editor, window, Editor::context_menu_last);
        register_action(editor, window, Editor::open_active_item_in_terminal);
        register_action(editor, window, Editor::open_selections_in_multibuffer);
        register_action(editor, window, Editor::reload_file);

        if !editor.read(cx).read_only(cx) {
            register_action(editor, window, Editor::newline);
            register_action(editor, window, Editor::newline_above);
            register_action(editor, window, Editor::newline_below);
            register_action(editor, window, Editor::backspace);
            register_action(editor, window, Editor::delete);
            register_action(editor, window, Editor::tab);
            register_action(editor, window, Editor::backtab);
            register_action(editor, window, Editor::indent);
            register_action(editor, window, Editor::outdent);
            register_action(editor, window, Editor::autoindent);
            register_action(editor, window, Editor::delete_line);
            register_action(editor, window, Editor::join_lines);
            register_action(editor, window, Editor::sort_lines_by_length);
            register_action(editor, window, Editor::sort_lines_case_sensitive);
            register_action(editor, window, Editor::sort_lines_case_insensitive);
            register_action(editor, window, Editor::unique_lines_case_insensitive);
            register_action(editor, window, Editor::unique_lines_case_sensitive);
            register_action(editor, window, Editor::reverse_lines);
            register_action(editor, window, Editor::shuffle_lines);
            register_action(editor, window, Editor::rotate_selections_forward);
            register_action(editor, window, Editor::rotate_selections_backward);
            register_action(editor, window, Editor::convert_indentation_to_spaces);
            register_action(editor, window, Editor::convert_indentation_to_tabs);
            register_action(editor, window, Editor::convert_to_upper_case);
            register_action(editor, window, Editor::convert_to_lower_case);
            register_action(editor, window, Editor::convert_to_title_case);
            register_action(editor, window, Editor::convert_to_snake_case);
            register_action(editor, window, Editor::convert_to_kebab_case);
            register_action(editor, window, Editor::convert_to_upper_camel_case);
            register_action(editor, window, Editor::convert_to_lower_camel_case);
            register_action(editor, window, Editor::convert_to_opposite_case);
            register_action(editor, window, Editor::convert_to_sentence_case);
            register_action(editor, window, Editor::toggle_case);
            register_action(editor, window, Editor::convert_to_rot13);
            register_action(editor, window, Editor::convert_to_rot47);
            register_action(editor, window, Editor::convert_to_base64);
            register_action(editor, window, Editor::convert_from_base64);
            register_action(editor, window, Editor::delete_to_previous_word_start);
            register_action(editor, window, Editor::delete_to_previous_subword_start);
            register_action(editor, window, Editor::delete_to_next_word_end);
            register_action(editor, window, Editor::delete_to_next_subword_end);
            register_action(editor, window, Editor::delete_to_beginning_of_line);
            register_action(editor, window, Editor::delete_to_end_of_line);
            register_action(editor, window, Editor::cut_to_end_of_line);
            register_action(editor, window, Editor::duplicate_line_up);
            register_action(editor, window, Editor::duplicate_line_down);
            register_action(editor, window, Editor::duplicate_selection);
            register_action(editor, window, Editor::move_line_up);
            register_action(editor, window, Editor::move_line_down);
            register_action(editor, window, Editor::transpose);
            register_action(editor, window, |editor, _: &crate::Rewrap, _, cx| {
                editor.rewrap(crate::RewrapOptions::default(), cx);
            });
            register_action(editor, window, Editor::cut);
            register_action(editor, window, Editor::kill_ring_cut);
            register_action(editor, window, Editor::kill_ring_yank);
            register_action(editor, window, Editor::paste);
            register_action(editor, window, Editor::undo);
            register_action(editor, window, Editor::redo);
            register_action(editor, window, Editor::toggle_comments);
            register_action(editor, window, Editor::toggle_block_comments);
            register_action(editor, window, Editor::toggle_markdown_block_quote);
            register_action(editor, window, Editor::unwrap_syntax_node);

            register_action(editor, window, Editor::restore_file);
            register_action(editor, window, Editor::git_restore);
            register_action(editor, window, Editor::restore_and_next);
            register_action(editor, window, Editor::insert_uuid_v4);
            register_action(editor, window, Editor::insert_uuid_v7);
            register_action(editor, window, Editor::align_selections);
            if editor.read(cx).enable_wrap_selections_in_tag(cx) {
                register_action(editor, window, Editor::wrap_selections_in_tag);
            }
            register_action(editor, window, |editor, HandleInput(text): &HandleInput, window, cx| {
                if text.is_empty() {
                    return;
                }
                editor.handle_input(text, window, cx);
            });
            register_action(editor, window, |editor, action, window, cx| {
                if let Some(task) = editor.organize_imports(action, window, cx) {
                    editor.detach_and_notify_err(task, window, cx);
                } else {
                    cx.propagate();
                }
            });
            register_action(editor, window, |editor, action, window, cx| {
                if let Some(task) = editor.confirm_completion(action, window, cx) {
                    editor.detach_and_notify_err(task, window, cx);
                } else {
                    cx.propagate();
                }
            });
            register_action(editor, window, |editor, action, window, cx| {
                if let Some(task) = editor.confirm_completion_replace(action, window, cx) {
                    editor.detach_and_notify_err(task, window, cx);
                } else {
                    cx.propagate();
                }
            });
            register_action(editor, window, |editor, action, window, cx| {
                if let Some(task) = editor.confirm_completion_insert(action, window, cx) {
                    editor.detach_and_notify_err(task, window, cx);
                } else {
                    cx.propagate();
                }
            });
            register_action(editor, window, |editor, action, window, cx| {
                if let Some(task) = editor.confirm_code_action(action, window, cx) {
                    editor.detach_and_notify_err(task, window, cx);
                } else {
                    cx.propagate();
                }
            });
            register_action(editor, window, |editor, action, window, cx| {
                if let Some(task) = editor.rename(action, window, cx) {
                    editor.detach_and_notify_err(task, window, cx);
                } else {
                    cx.propagate();
                }
            });
            register_action(editor, window, |editor, action, window, cx| {
                if let Some(task) = editor.confirm_rename(action, window, cx) {
                    editor.detach_and_notify_err(task, window, cx);
                } else {
                    cx.propagate();
                }
            });
        }
    }

    pub(super) fn register_key_listeners(&self, window: &mut Window, _: &mut App, layout: &EditorLayout) {
        let position_map = Rc::clone(&layout.position_map);
        window.on_key_event({
            let editor = self.editor.clone();
            move |event: &ModifiersChangedEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                editor.update(cx, |editor, cx| {
                    if editor.hover_state.focused(window, cx) {
                        return;
                    }

                    editor.handle_modifiers_changed(event.modifiers, &position_map, window, cx);
                });
            }
        });
    }
}

pub(crate) fn register_action<T: Action>(
    editor: &Entity<Editor>,
    window: &mut Window,
    listener: impl Fn(&mut Editor, &T, &mut Window, &mut Context<Editor>) + 'static,
) {
    register_action_erased(
        editor,
        window,
        TypeId::of::<T>(),
        Box::new(move |editor, action, window, cx| {
            listener(editor, action.downcast_ref().expect("action type matches listener"), window, cx);
        }),
    );
}

#[allow(clippy::type_complexity)]
pub(super) fn register_action_erased(
    editor: &Entity<Editor>,
    window: &mut Window,
    action_type: TypeId,
    listener: Box<dyn Fn(&mut Editor, &dyn Any, &mut Window, &mut Context<Editor>)>,
) {
    let editor = editor.clone();
    window.on_action(action_type, move |action, phase, window, cx| {
        if phase == DispatchPhase::Bubble {
            editor.update(cx, |editor, cx| {
                listener(editor, action, window, cx);
            });
        }
    });
}
