//! The complete macOS keymap. Edit these bindings and rebuild to change shortcuts.
use std::rc::Rc;

use gpui::Action;
use gpui::App;
use gpui::DummyKeyboardMapper;
use gpui::KeyBinding;
use gpui::KeyBindingContextPredicate;
use serde_json::Value;
use serde_json::json;

type BuildAction<'a> = &'a dyn Fn(&str, Option<Value>) -> Box<dyn Action>;

pub(super) fn keymap(cx: &App) -> Vec<KeyBinding> {
    build(&|name, input| {
        cx.build_action(name, input).unwrap_or_else(|error| panic!("invalid built-in action {name}: {error}"))
    })
}

/// Emits bindings straight into gpui `KeyBinding`s. A binding in a deeper context wins; at the same
/// depth, the later binding wins. Only the US layout is supported, so keys are never remapped.
struct Keymap<'a> {
    build_action: BuildAction<'a>,
    bindings: Vec<KeyBinding>,
    context: Option<Rc<KeyBindingContextPredicate>>,
}

impl Keymap<'_> {
    fn group(&mut self, context: Option<&str>) {
        self.context = context.map(|context| {
            KeyBindingContextPredicate::parse(context)
                .unwrap_or_else(|error| panic!("invalid built-in context {context}: {error}"))
                .into()
        });
    }

    fn bind(&mut self, keystrokes: &str, action: &str) {
        self.push(keystrokes, action, None);
    }

    fn bind_input(&mut self, keystrokes: &str, action: &str, input: Value) {
        self.push(keystrokes, action, Some(input));
    }

    fn push(&mut self, keystrokes: &str, action: &str, input: Option<Value>) {
        let action_input = input.as_ref().map(|input| input.to_string().into());
        let binding = KeyBinding::load(
            keystrokes,
            (self.build_action)(action, input),
            self.context.clone(),
            false,
            action_input,
            &DummyKeyboardMapper,
        )
        .unwrap_or_else(|error| panic!("invalid built-in keystrokes {keystrokes}: {error}"));
        self.bindings.push(binding);
    }
}

#[expect(clippy::too_many_lines, reason = "the whole default keymap as one flat table")]
fn build(build_action: BuildAction<'_>) -> Vec<KeyBinding> {
    let mut k = Keymap { build_action, bindings: Vec::with_capacity(512), context: None };
    k.group(None);
    k.bind("home", "menu::SelectFirst");
    k.bind("shift-pageup", "menu::SelectFirst");
    k.bind("pageup", "menu::SelectFirst");
    k.bind("cmd-up", "menu::SelectFirst");
    k.bind("end", "menu::SelectLast");
    k.bind("shift-pagedown", "menu::SelectLast");
    k.bind("pagedown", "menu::SelectLast");
    k.bind("cmd-down", "menu::SelectLast");
    k.bind("tab", "menu::SelectNext");
    k.bind("ctrl-n", "menu::SelectNext");
    k.bind("down", "menu::SelectNext");
    k.bind("shift-tab", "menu::SelectPrevious");
    k.bind("ctrl-p", "menu::SelectPrevious");
    k.bind("up", "menu::SelectPrevious");
    k.bind("enter", "menu::Confirm");
    k.bind("ctrl-enter", "menu::SecondaryConfirm");
    k.bind("cmd-enter", "menu::SecondaryConfirm");
    k.bind("cmd-escape", "menu::Cancel");
    k.bind("ctrl-escape", "menu::Cancel");
    k.bind("ctrl-c", "menu::Cancel");
    k.bind("escape", "menu::Cancel");
    k.bind("alt-shift-enter", "menu::Restart");
    k.bind("cmd-shift-w", "workspace::CloseWindow");
    k.bind("shift-escape", "workspace::ToggleZoom");
    k.bind("cmd-o", "workspace::Open");
    k.bind("cmd-[", "workspace::GoBack");
    k.bind("cmd-]", "workspace::GoForward");
    k.bind("cmd-=", "ed::IncreaseBufferFontSize");
    k.bind("cmd-+", "ed::IncreaseBufferFontSize");
    k.bind("cmd--", "ed::DecreaseBufferFontSize");
    k.bind("cmd-0", "ed::ResetBufferFontSize");
    k.bind("cmd-q", "ed::Quit");
    k.bind("cmd-h", "ed::Hide");
    k.bind("alt-cmd-h", "ed::HideOthers");
    k.bind("cmd-m", "ed::Minimize");
    k.bind("ctrl-cmd-l", "lsp_tool::ToggleMenu");

    k.group(Some("menu"));
    k.bind("right", "menu::SelectChild");
    k.bind("left", "menu::SelectParent");

    k.group(Some("Editor"));
    k.bind("escape", "editor::Cancel");
    k.bind("shift-backspace", "editor::Backspace");
    k.bind("ctrl-h", "editor::Backspace");
    k.bind("backspace", "editor::Backspace");
    k.bind("ctrl-d", "editor::Delete");
    k.bind("delete", "editor::Delete");
    k.bind("tab", "editor::Tab");
    k.bind("shift-tab", "editor::Backtab");
    k.bind("cmd-backspace", "editor::DeleteToBeginningOfLine");
    k.bind("cmd-delete", "editor::DeleteToEndOfLine");
    k.bind_input(
        "alt-backspace",
        "editor::DeleteToPreviousWordStart",
        json!({"ignore_newlines": false, "ignore_brackets": false}),
    );
    k.bind_input(
        "ctrl-w",
        "editor::DeleteToPreviousWordStart",
        json!({"ignore_newlines": false, "ignore_brackets": false}),
    );
    k.bind_input(
        "alt-delete",
        "editor::DeleteToNextWordEnd",
        json!({"ignore_newlines": false, "ignore_brackets": false}),
    );
    k.bind("cmd-x", "editor::Cut");
    k.bind("cmd-c", "editor::Copy");
    k.bind("cmd-v", "editor::Paste");
    k.bind("cmd-z", "editor::Undo");
    k.bind("cmd-shift-z", "editor::Redo");
    k.bind("up", "editor::MoveUp");
    k.bind("ctrl-up", "editor::MoveToStartOfParagraph");
    k.bind("pageup", "editor::MovePageUp");
    k.bind("shift-pageup", "editor::SelectPageUp");
    k.bind("cmd-pageup", "editor::PageUp");
    k.bind("ctrl-pageup", "editor::LineUp");
    k.bind("down", "editor::MoveDown");
    k.bind("ctrl-down", "editor::MoveToEndOfParagraph");
    k.bind("pagedown", "editor::MovePageDown");
    k.bind("shift-pagedown", "editor::SelectPageDown");
    k.bind("cmd-pagedown", "editor::PageDown");
    k.bind("ctrl-pagedown", "editor::LineDown");
    k.bind("ctrl-p", "editor::MoveUp");
    k.bind("ctrl-n", "editor::MoveDown");
    k.bind("ctrl-b", "editor::MoveLeft");
    k.bind("left", "editor::MoveLeft");
    k.bind("ctrl-f", "editor::MoveRight");
    k.bind("right", "editor::MoveRight");
    k.bind("ctrl-l", "editor::ScrollCursorCenter");
    k.bind("alt-left", "editor::MoveToPreviousWordStart");
    k.bind("alt-right", "editor::MoveToNextWordEnd");
    k.bind_input(
        "cmd-left",
        "editor::MoveToBeginningOfLine",
        json!({"stop_at_soft_wraps": true, "stop_at_indent": true}),
    );
    k.bind_input(
        "ctrl-a",
        "editor::MoveToBeginningOfLine",
        json!({"stop_at_soft_wraps": false, "stop_at_indent": true}),
    );
    k.bind_input("home", "editor::MoveToBeginningOfLine", json!({"stop_at_soft_wraps": true, "stop_at_indent": true}));
    k.bind_input("cmd-right", "editor::MoveToEndOfLine", json!({"stop_at_soft_wraps": true}));
    k.bind_input("ctrl-e", "editor::MoveToEndOfLine", json!({"stop_at_soft_wraps": false}));
    k.bind_input("end", "editor::MoveToEndOfLine", json!({"stop_at_soft_wraps": true}));
    k.bind("cmd-up", "editor::MoveToBeginning");
    k.bind("cmd-down", "editor::MoveToEnd");
    k.bind("cmd-home", "editor::MoveToBeginning");
    k.bind("cmd-end", "editor::MoveToEnd");
    k.bind("shift-up", "editor::SelectUp");
    k.bind("ctrl-shift-p", "editor::SelectUp");
    k.bind("shift-down", "editor::SelectDown");
    k.bind("ctrl-shift-n", "editor::SelectDown");
    k.bind("shift-left", "editor::SelectLeft");
    k.bind("ctrl-shift-b", "editor::SelectLeft");
    k.bind("shift-right", "editor::SelectRight");
    k.bind("ctrl-shift-f", "editor::SelectRight");
    k.bind("alt-shift-left", "editor::SelectToPreviousWordStart");
    k.bind("alt-shift-right", "editor::SelectToNextWordEnd");
    k.bind("ctrl-shift-up", "editor::SelectToStartOfParagraph");
    k.bind("ctrl-shift-down", "editor::SelectToEndOfParagraph");
    k.bind("cmd-shift-up", "editor::SelectToBeginning");
    k.bind("cmd-shift-down", "editor::SelectToEnd");
    k.bind("cmd-a", "editor::SelectAll");
    k.bind("cmd-l", "editor::SelectLine");
    k.bind("alt-shift-o", "editor::OrganizeImports");
    k.bind_input(
        "cmd-shift-left",
        "editor::SelectToBeginningOfLine",
        json!({"stop_at_soft_wraps": true, "stop_at_indent": true}),
    );
    k.bind_input(
        "shift-home",
        "editor::SelectToBeginningOfLine",
        json!({"stop_at_soft_wraps": true, "stop_at_indent": true}),
    );
    k.bind_input(
        "ctrl-shift-a",
        "editor::SelectToBeginningOfLine",
        json!({"stop_at_soft_wraps": true, "stop_at_indent": true}),
    );
    k.bind_input("cmd-shift-right", "editor::SelectToEndOfLine", json!({"stop_at_soft_wraps": true}));
    k.bind_input("shift-end", "editor::SelectToEndOfLine", json!({"stop_at_soft_wraps": true}));
    k.bind_input("ctrl-shift-e", "editor::SelectToEndOfLine", json!({"stop_at_soft_wraps": true}));
    k.bind_input("ctrl-v", "editor::MovePageDown", json!({"center_cursor": true}));
    k.bind_input("ctrl-shift-v", "editor::MovePageUp", json!({"center_cursor": true}));
    k.bind("ctrl-cmd-space", "editor::ShowCharacterPalette");
    k.bind("cmd-'", "editor::ToggleSelectedDiffHunks");
    k.bind("cmd-\"", "editor::ExpandAllDiffHunks");
    k.bind("ctrl-f12", "editor::GoToDeclaration");
    k.bind("alt-ctrl-f12", "editor::GoToDeclarationSplit");

    k.group(Some("Editor && mode == full"));
    k.bind("shift-enter", "editor::Newline");
    k.bind("enter", "editor::Newline");
    k.bind("cmd-enter", "editor::NewlineBelow");
    k.bind("cmd-shift-enter", "editor::NewlineAbove");
    k.bind("cmd-f", "buffer_search::Deploy");
    k.bind_input("cmd-alt-l", "buffer_search::Deploy", json!({"selection_search_enabled": true}));
    k.bind("cmd-e", "buffer_search::UseSelectionForFind");
    k.bind("cmd-alt-e", "editor::SelectEnclosingSymbol");
    k.bind("alt-enter", "editor::OpenSelectionsInMultibuffer");
    k.bind("alt-cmd-f", "text_finder::Toggle");

    k.group(Some("Editor && multibuffer"));
    k.bind("cmd-up", "editor::MoveToStartOfExcerpt");
    k.bind("cmd-down", "editor::MoveToStartOfNextExcerpt");
    k.bind("cmd-shift-up", "editor::SelectToStartOfExcerpt");
    k.bind("cmd-shift-down", "editor::SelectToStartOfNextExcerpt");

    k.group(Some("Editor && mode == auto_height"));
    k.bind("ctrl-enter", "editor::Newline");
    k.bind("shift-enter", "editor::Newline");
    k.bind("ctrl-shift-enter", "editor::NewlineBelow");

    k.group(Some("Markdown"));
    k.bind("cmd-c", "markdown::Copy");

    k.group(Some("Editor"));
    k.bind("cmd-alt-z", "git::Restore");
    k.bind("cmd-alt-y", "git::ToggleStaged");
    k.bind("cmd-y", "git::StageAndNext");
    k.bind("cmd-shift-y", "git::UnstageAndNext");

    k.group(Some("BufferSearchBar"));
    k.bind("escape", "buffer_search::Dismiss");
    k.bind("tab", "buffer_search::FocusEditor");
    k.bind("enter", "search::SelectNextMatch");
    k.bind("shift-enter", "search::SelectPreviousMatch");
    k.bind("cmd-shift-enter", "editor::ToggleFoldAll");
    k.bind("alt-enter", "search::SelectAllMatches");
    k.bind("cmd-f", "search::FocusSearch");
    k.bind("cmd-alt-l", "search::ToggleSelection");
    k.bind("alt-cmd-f", "text_finder::Toggle");

    k.group(Some("BufferSearchBar && in_replace > Editor"));
    k.bind("enter", "search::ReplaceNext");
    k.bind("cmd-enter", "search::ReplaceAll");

    k.group(Some("BufferSearchBar && !in_replace > Editor"));
    k.bind("ctrl-enter", "editor::Newline");
    k.bind("shift-enter", "search::SelectPreviousMatch");

    k.group(Some("BufferSearchBar && !in_replace > Editor"));
    k.bind("up", "search::PreviousHistoryQuery");
    k.bind("down", "search::NextHistoryQuery");

    k.group(Some("BufferSearchBar || ProjectSearchBar"));
    k.bind("ctrl-enter", "editor::Newline");

    k.group(Some("ProjectSearchBar"));
    k.bind("escape", "project_search::ToggleFocus");
    k.bind("cmd-shift-j", "project_search::ToggleFilters");
    k.bind("cmd-shift-enter", "project_search::ToggleAllSearchResults");
    k.bind("cmd-shift-f", "search::FocusSearch");
    k.bind("cmd-shift-h", "search::ToggleReplace");
    k.bind("alt-cmd-g", "search::ToggleRegex");
    k.bind("alt-cmd-x", "search::ToggleRegex");
    k.bind("alt-cmd-f", "project_search::OpenTextFinder");

    k.group(Some("ProjectSearchBar > Editor"));
    k.bind("up", "search::PreviousHistoryQuery");
    k.bind("down", "search::NextHistoryQuery");

    k.group(Some("ProjectSearchBar && in_replace > Editor"));
    k.bind("enter", "search::ReplaceNext");
    k.bind("cmd-enter", "search::ReplaceAll");

    k.group(Some("ProjectSearchBar && !in_replace > Editor"));
    k.bind("ctrl-enter", "editor::Newline");

    k.group(Some("ProjectSearchView"));
    k.bind("escape", "project_search::ToggleFocus");
    k.bind("cmd-shift-j", "project_search::ToggleFilters");
    k.bind("cmd-shift-enter", "project_search::ToggleAllSearchResults");
    k.bind("cmd-shift-h", "search::ToggleReplace");
    k.bind("alt-cmd-g", "search::ToggleRegex");
    k.bind("alt-cmd-x", "search::ToggleRegex");
    k.bind("alt-cmd-f", "project_search::OpenTextFinder");

    k.group(Some("Pane"));
    k.bind("alt-cmd-left", "pane::ActivatePreviousItem");
    k.bind("cmd-{", "pane::ActivatePreviousItem");
    k.bind("alt-cmd-right", "pane::ActivateNextItem");
    k.bind("cmd-}", "pane::ActivateNextItem");
    k.bind("ctrl-shift-pageup", "pane::SwapItemLeft");
    k.bind("ctrl-shift-pagedown", "pane::SwapItemRight");
    k.bind_input("cmd-w", "pane::CloseActiveItem", json!({"close_pinned": false}));
    k.bind_input("alt-cmd-t", "pane::CloseOtherItems", json!({"close_pinned": false}));
    k.bind("ctrl-alt-cmd-w", "workspace::CloseInactiveTabsAndPanes");
    k.bind("cmd-f", "project_search::ToggleFocus");
    k.bind("cmd-g", "search::SelectNextMatch");
    k.bind("cmd-shift-g", "search::SelectPreviousMatch");
    k.bind("cmd-shift-h", "search::ToggleReplace");
    k.bind("cmd-alt-l", "search::ToggleSelection");
    k.bind("alt-enter", "search::SelectAllMatches");
    k.bind("alt-cmd-c", "search::ToggleCaseSensitive");
    k.bind("alt-cmd-w", "search::ToggleWholeWord");
    k.bind("alt-cmd-x", "search::ToggleRegex");

    k.group(Some("Editor"));
    k.bind("cmd-[", "editor::Outdent");
    k.bind("cmd-]", "editor::Indent");
    k.bind_input("cmd-ctrl-p", "editor::AddSelectionAbove", json!({"skip_soft_wrap": false}));
    k.bind_input("cmd-alt-up", "editor::AddSelectionAbove", json!({"skip_soft_wrap": true}));
    k.bind_input("cmd-ctrl-n", "editor::AddSelectionBelow", json!({"skip_soft_wrap": false}));
    k.bind_input("cmd-alt-down", "editor::AddSelectionBelow", json!({"skip_soft_wrap": true}));
    k.bind("cmd-shift-k", "editor::DeleteLine");
    k.bind("alt-up", "editor::MoveLineUp");
    k.bind("alt-down", "editor::MoveLineDown");
    k.bind("alt-shift-up", "editor::DuplicateLineUp");
    k.bind("alt-shift-down", "editor::DuplicateLineDown");
    k.bind("cmd-ctrl-left", "editor::SelectSmallerSyntaxNode");
    k.bind("cmd-ctrl-right", "editor::SelectLargerSyntaxNode");
    k.bind("cmd-ctrl-up", "editor::SelectPreviousSyntaxNode");
    k.bind("ctrl-shift-right", "editor::SelectLargerSyntaxNode");
    k.bind("ctrl-shift-left", "editor::SelectSmallerSyntaxNode");
    k.bind("cmd-ctrl-down", "editor::SelectNextSyntaxNode");
    k.bind_input("cmd-d", "editor::SelectNext", json!({"replace_newest": false}));
    k.bind("cmd-shift-l", "editor::SelectAllMatches");
    k.bind("cmd-f2", "editor::SelectAllMatches");
    k.bind_input("ctrl-cmd-d", "editor::SelectPrevious", json!({"replace_newest": false}));
    k.bind_input("cmd-/", "editor::ToggleComments", json!({"advance_downwards": false, "comment_empty_lines": true}));
    k.bind("shift-alt-a", "editor::ToggleBlockComments");
    k.bind_input("f8", "editor::GoToDiagnostic", json!({"severity": {"min": "hint", "max": "error"}}));
    k.bind_input("shift-f8", "editor::GoToPreviousDiagnostic", json!({"severity": {"min": "hint", "max": "error"}}));
    k.bind("f2", "editor::Rename");
    k.bind("f12", "editor::GoToDefinition");
    k.bind("alt-f12", "editor::GoToDefinitionSplit");
    k.bind("cmd-f12", "editor::GoToTypeDefinition");
    k.bind("shift-f12", "editor::GoToImplementation");
    k.bind("alt-cmd-f12", "editor::GoToTypeDefinitionSplit");
    k.bind("alt-shift-f12", "editor::FindAllReferences");
    k.bind("cmd-|", "editor::MoveToEnclosingBracket");
    k.bind("ctrl-m", "editor::MoveToEnclosingBracket");
    k.bind("alt-cmd-[", "editor::Fold");
    k.bind("alt-cmd-]", "editor::UnfoldLines");
    k.bind("ctrl-space", "editor::ShowCompletions");
    k.bind("ctrl-shift-space", "editor::ShowWordCompletions");
    k.bind("cmd-.", "editor::ToggleCodeActions");
    k.bind("cmd-\\", "pane::SplitRight");

    k.group(Some("Editor"));
    k.bind("cmd-shift-v", "preview::OpenPreview");

    k.group(Some("Editor && mode == full"));
    k.bind("ctrl-g", "go_to_line::Toggle");
    k.bind("cmd-shift-backspace", "editor::GoToPreviousChange");
    k.bind("cmd-shift-alt-backspace", "editor::GoToNextChange");

    k.group(Some("Pane"));
    k.bind_input("ctrl-1", "pane::ActivateItem", json!(0));
    k.bind_input("ctrl-2", "pane::ActivateItem", json!(1));
    k.bind_input("ctrl-3", "pane::ActivateItem", json!(2));
    k.bind_input("ctrl-4", "pane::ActivateItem", json!(3));
    k.bind_input("ctrl-5", "pane::ActivateItem", json!(4));
    k.bind_input("ctrl-6", "pane::ActivateItem", json!(5));
    k.bind_input("ctrl-7", "pane::ActivateItem", json!(6));
    k.bind_input("ctrl-8", "pane::ActivateItem", json!(7));
    k.bind_input("ctrl-9", "pane::ActivateItem", json!(8));
    k.bind("ctrl-0", "pane::ActivateLastItem");
    k.bind("cmd-shift-f", "pane::DeploySearch");

    k.group(Some("Workspace"));
    k.bind("f6", "workspace::FocusNextPart");
    k.bind("shift-f6", "workspace::FocusPreviousPart");
    k.bind("cmd-f6", "workspace::FocusNextPart");
    k.bind("fn-f", "ed::ToggleFullScreen");
    k.bind("ctrl-cmd-f", "ed::ToggleFullScreen");
    k.bind("alt-cmd-o", "projects::OpenRecent");
    k.bind("ctrl-r", "projects::OpenRecent");
    k.bind("cmd-ctrl-b", "git::Branch");
    k.bind("cmd-ctrl-w", "git::Worktree");
    k.bind("cmd-s", "workspace::Save");
    k.bind("alt-shift-enter", "toast::RunAction");
    k.bind("cmd-shift-s", "workspace::SaveAs");
    k.bind("cmd-shift-n", "workspace::NewWindow");
    k.bind("cmd-1", "mode::Editor");
    k.bind("cmd-2", "mode::GitHistory");
    k.bind("cmd-3", "mode::Terminal");
    k.bind("cmd-`", "workspace::ToggleZoom");
    k.bind("cmd-b", "workspace::CycleLeftDock");
    k.bind("cmd-j", "terminal_panel::ToggleShell");
    k.bind("ctrl-alt-0", "workspace::ResetActiveDockSize");
    k.bind_input("ctrl-alt--", "workspace::DecreaseActiveDockSize", json!({"px": 0}));
    k.bind_input("ctrl-alt-=", "workspace::IncreaseActiveDockSize", json!({"px": 0}));
    k.bind("ctrl-alt-)", "workspace::ResetOpenDocksSize");
    k.bind_input("ctrl-alt-_", "workspace::DecreaseOpenDocksSize", json!({"px": 0}));
    k.bind_input("ctrl-alt-+", "workspace::IncreaseOpenDocksSize", json!({"px": 0}));
    k.bind("cmd-shift-f", "pane::DeploySearch");
    k.bind_input("cmd-shift-h", "pane::DeploySearch", json!({"replace_enabled": true}));
    k.bind("cmd-shift-t", "pane::ReopenClosedItem");
    k.bind("cmd-t", "project_symbols::Toggle");
    k.bind("cmd-p", "file_finder::Toggle");
    k.bind_input("ctrl-shift-tab", "tab_switcher::Toggle", json!({"select_last": true}));
    k.bind("ctrl-tab", "tab_switcher::Toggle");
    k.bind("cmd-shift-p", "command_palette::Toggle");
    k.bind("cmd-shift-m", "diagnostics::Deploy");
    k.bind_input("cmd-w", "pane::CloseActiveItem", json!({"close_pinned": false}));
    k.bind("ctrl-alt-w", "workspace::CloseActiveDock");

    k.group(Some("Workspace && !Terminal"));
    k.bind("cmd-n", "workspace::NewFile");

    k.group(Some("Editor"));
    k.bind("cmd-u", "editor::UndoSelection");
    k.bind("cmd-shift-u", "editor::ToggleCase");
    k.bind("f1", "editor::OpenDocs");
    k.bind("ctrl-j", "editor::JoinLines");
    k.bind("ctrl-alt-backspace", "editor::DeleteToPreviousSubwordStart");
    k.bind("ctrl-alt-h", "editor::DeleteToPreviousSubwordStart");
    k.bind("ctrl-alt-delete", "editor::DeleteToNextSubwordEnd");
    k.bind("ctrl-alt-d", "editor::DeleteToNextSubwordEnd");
    k.bind("ctrl-alt-left", "editor::MoveToPreviousSubwordStart");
    k.bind("ctrl-alt-b", "editor::MoveToPreviousSubwordStart");
    k.bind("ctrl-alt-right", "editor::MoveToNextSubwordEnd");
    k.bind("ctrl-alt-f", "editor::MoveToNextSubwordEnd");
    k.bind("ctrl-alt-shift-left", "editor::SelectToPreviousSubwordStart");
    k.bind("ctrl-alt-shift-b", "editor::SelectToPreviousSubwordStart");
    k.bind("ctrl-alt-shift-right", "editor::SelectToNextSubwordEnd");
    k.bind("ctrl-alt-shift-f", "editor::SelectToNextSubwordEnd");

    k.group(Some("Editor && renaming"));
    k.bind("enter", "editor::ConfirmRename");

    k.group(Some("Editor && showing_completions"));
    k.bind("enter", "editor::ConfirmCompletion");
    k.bind("shift-enter", "editor::ConfirmCompletionReplace");
    k.bind("tab", "editor::ComposeCompletion");

    k.group(Some("Editor && in_snippet && has_next_tabstop && !showing_completions"));
    k.bind("tab", "editor::NextSnippetTabstop");

    k.group(Some("Editor && in_snippet && has_previous_tabstop && !showing_completions"));
    k.bind("shift-tab", "editor::PreviousSnippetTabstop");

    k.group(Some("Editor && showing_code_actions"));
    k.bind("enter", "editor::ConfirmCodeAction");

    k.group(Some("Editor && (showing_code_actions || showing_completions)"));
    k.bind("up", "editor::ContextMenuPrevious");
    k.bind("ctrl-p", "editor::ContextMenuPrevious");
    k.bind("down", "editor::ContextMenuNext");
    k.bind("ctrl-n", "editor::ContextMenuNext");
    k.bind("pageup", "editor::ContextMenuFirst");
    k.bind("pagedown", "editor::ContextMenuLast");

    k.group(Some("Editor && mode == full"));
    k.bind("shift-enter", "editor::ExpandExcerpts");
    k.bind("cmd-shift-e", "pane::RevealInProjectPanel");
    k.bind("cmd-f8", "editor::GoToHunk");
    k.bind("cmd-shift-f8", "editor::GoToPreviousHunk");

    k.group(Some("ProjectSearchBar && !in_replace"));
    k.bind("cmd-enter", "project_search::SearchInNew");

    k.group(Some("ProjectPanel"));
    k.bind("left", "project_panel::CollapseSelectedEntry");
    k.bind("cmd-left", "project_panel::CollapseAllEntries");
    k.bind("right", "project_panel::ExpandSelectedEntry");
    k.bind("cmd-right", "project_panel::ExpandAllEntries");
    k.bind("cmd-n", "project_panel::NewFile");
    k.bind("cmd-d", "project_panel::Duplicate");
    k.bind("alt-cmd-n", "project_panel::NewDirectory");
    k.bind("cmd-x", "project_panel::Cut");
    k.bind("cmd-c", "project_panel::Copy");
    k.bind("cmd-v", "project_panel::Paste");
    k.bind("cmd-alt-c", "workspace::CopyPath");
    k.bind("alt-cmd-shift-c", "workspace::CopyRelativePath");
    k.bind("cmd-z", "project_panel::Undo");
    k.bind("cmd-shift-z", "project_panel::Redo");
    k.bind("enter", "project_panel::Rename");
    k.bind("f2", "project_panel::Rename");
    k.bind_input("backspace", "project_panel::Delete", json!({"skip_prompt": false}));
    k.bind_input("delete", "project_panel::Delete", json!({"skip_prompt": false}));
    k.bind_input("cmd-backspace", "project_panel::Delete", json!({"skip_prompt": true}));
    k.bind_input("cmd-delete", "project_panel::Delete", json!({"skip_prompt": true}));
    k.bind("alt-cmd-r", "project_panel::RevealInFileManager");
    k.bind("ctrl-shift-enter", "workspace::OpenWithSystem");
    k.bind("alt-d", "project_panel::CompareMarkedFiles");
    k.bind("cmd-alt-shift-f", "project_panel::NewSearchInDirectory");
    k.bind("shift-f10", "project_panel::OpenContextMenu");
    k.bind("shift-down", "menu::SelectNext");
    k.bind("shift-up", "menu::SelectPrevious");
    k.bind("escape", "menu::Cancel");

    k.group(Some("ProjectPanel && not_editing"));
    k.bind("space", "project_panel::Open");

    k.group(Some("GitPanel && ChangesList && !GitBranchSelector"));
    k.bind("up", "menu::SelectPrevious");
    k.bind("down", "menu::SelectNext");
    k.bind("shift-up", "menu::SelectPrevious");
    k.bind("shift-down", "menu::SelectNext");
    k.bind("enter", "menu::Confirm");
    k.bind("escape", "menu::Cancel");

    k.group(Some("GitPanel && ChangesList && !GitBranchSelector"));
    k.bind("up", "git_panel::PreviousEntry");
    k.bind("down", "git_panel::NextEntry");
    k.bind("cmd-up", "git_panel::FirstEntry");
    k.bind("cmd-down", "git_panel::LastEntry");
    k.bind("left", "git_panel::CollapseSelectedEntry");
    k.bind("right", "git_panel::ExpandSelectedEntry");
    k.bind("cmd-alt-y", "git::ToggleStaged");
    k.bind("space", "git::ToggleStaged");
    k.bind("shift-space", "git::StageRange");
    k.bind("cmd-y", "git::StageFile");
    k.bind("cmd-shift-y", "git::UnstageFile");
    k.bind("cmd-alt-c", "workspace::CopyPath");
    k.bind("alt-cmd-shift-c", "workspace::CopyRelativePath");
    k.bind("alt-down", "git_panel::FocusEditor");
    k.bind("tab", "git_panel::FocusEditor");
    k.bind("shift-tab", "git_panel::FocusEditor");
    k.bind_input("backspace", "git::RestoreFile", json!({"skip_prompt": false}));
    k.bind_input("delete", "git::RestoreFile", json!({"skip_prompt": false}));
    k.bind_input("cmd-backspace", "git::RestoreFile", json!({"skip_prompt": true}));
    k.bind_input("cmd-delete", "git::RestoreFile", json!({"skip_prompt": true}));

    k.group(Some("GitPanel && CommitEditor"));
    k.bind("escape", "git::Cancel");

    k.group(Some("GitDiff > Editor"));
    k.bind("cmd-enter", "git::Commit");
    k.bind("cmd-shift-enter", "git::Amend");
    k.bind("cmd-ctrl-y", "git::StageAll");
    k.bind("cmd-ctrl-shift-y", "git::UnstageAll");
    k.bind("cmd-alt-z", "git::RestoreAndNext");

    k.group(Some("CommitEditor > Editor && !showing_code_actions && !showing_completions"));
    k.bind("enter", "editor::Newline");

    k.group(Some("CommitEditor > Editor"));
    k.bind("cmd-enter", "git::Commit");
    k.bind("cmd-shift-enter", "git::Amend");
    k.bind("tab", "git_panel::FocusChanges");
    k.bind("shift-tab", "git_panel::FocusChanges");
    k.bind("alt-up", "git_panel::FocusChanges");
    k.bind("alt-shift-escape", "git::ToggleFillCommitEditor");

    k.group(Some("GitPanel"));
    k.bind("cmd-ctrl-y", "git::StageAll");
    k.bind("cmd-ctrl-shift-y", "git::UnstageAll");
    k.bind("cmd-enter", "git::Commit");
    k.bind("cmd-shift-enter", "git::Amend");

    k.group(Some("GitCommit > Editor && mode == auto_height && !showing_code_actions && !showing_completions"));
    k.bind("enter", "editor::Newline");

    k.group(Some("GitCommit > Editor && mode == auto_height"));
    k.bind("escape", "menu::Cancel");
    k.bind("cmd-enter", "git::Commit");
    k.bind("cmd-shift-enter", "git::Amend");

    k.group(Some("Picker > Editor"));
    k.bind("escape", "menu::Cancel");
    k.bind("up", "menu::SelectPrevious");
    k.bind("down", "menu::SelectNext");
    k.bind("tab", "picker::ConfirmCompletion");
    k.bind_input("alt-enter", "picker::ConfirmInput", json!({"secondary": false}));
    k.bind_input("cmd-alt-enter", "picker::ConfirmInput", json!({"secondary": true}));

    k.group(Some("LspCommandSelector > Picker > Editor"));
    k.bind("tab", "lsp_command_selector::ToggleArgumentsFocus");

    k.group(Some("FileFinder || (FileFinder > Picker > Editor)"));
    k.bind("cmd-shift-i", "search::ToggleIncludeIgnored");

    k.group(Some("RecentProjects || (RecentProjects > Picker > Editor)"));
    k.bind("cmd-shift-a", "workspace::AddFolderToProject");
    k.bind("shift-backspace", "recent_projects::RemoveSelected");
    k.bind("cmd-shift-enter", "recent_projects::AddToWorkspace");

    k.group(Some("TabSwitcher"));
    k.bind("ctrl-shift-tab", "menu::SelectPrevious");
    k.bind("ctrl-up", "menu::SelectPrevious");
    k.bind("ctrl-down", "menu::SelectNext");
    k.bind("ctrl-backspace", "tab_switcher::CloseSelectedItem");

    k.group(Some("StashList || (StashList > Picker > Editor)"));
    k.bind("ctrl-shift-backspace", "stash_picker::DropStashItem");
    k.bind("ctrl-shift-v", "stash_picker::ShowStashItem");

    k.group(Some("Terminal"));
    k.bind_input("shift-enter", "terminal::SendText", json!("\u{001b}\r"));
    k.bind("ctrl-cmd-space", "terminal::ShowCharacterPalette");
    k.bind("cmd-c", "terminal::Copy");
    k.bind("cmd-v", "terminal::Paste");
    k.bind("ctrl-cmd-v", "terminal::PasteText");
    k.bind("cmd-f", "buffer_search::Deploy");
    k.bind("cmd-a", "editor::SelectAll");
    k.bind("cmd-k", "terminal::Clear");
    k.bind("cmd-n", "workspace::NewTerminal");
    k.bind_input("cmd-backspace", "terminal::SendKeystroke", json!("ctrl-u"));
    k.bind_input("alt-delete", "terminal::SendText", json!("\u{001b}d"));
    k.bind_input("cmd-delete", "terminal::SendKeystroke", json!("ctrl-k"));
    k.bind_input("cmd-right", "terminal::SendKeystroke", json!("ctrl-e"));
    k.bind_input("cmd-left", "terminal::SendKeystroke", json!("ctrl-a"));
    k.bind_input("alt-left", "terminal::SendText", json!("\u{001b}b"));
    k.bind_input("alt-right", "terminal::SendText", json!("\u{001b}f"));
    k.bind_input("alt-b", "terminal::SendText", json!("\u{001b}b"));
    k.bind_input("alt-f", "terminal::SendText", json!("\u{001b}f"));
    k.bind_input("ctrl-delete", "terminal::SendText", json!("\u{001b}[3;5~"));
    k.bind_input("up", "terminal::SendKeystroke", json!("up"));
    k.bind_input("pageup", "terminal::SendKeystroke", json!("pageup"));
    k.bind_input("down", "terminal::SendKeystroke", json!("down"));
    k.bind_input("pagedown", "terminal::SendKeystroke", json!("pagedown"));
    k.bind_input("escape", "terminal::SendKeystroke", json!("escape"));
    k.bind_input("enter", "terminal::SendKeystroke", json!("enter"));
    k.bind_input("ctrl-c", "terminal::SendKeystroke", json!("ctrl-c"));
    k.bind_input("ctrl-r", "terminal::SendKeystroke", json!("ctrl-r"));
    k.bind_input("ctrl-backspace", "terminal::SendKeystroke", json!("ctrl-w"));
    k.bind("shift-pageup", "terminal::ScrollPageUp");
    k.bind("cmd-up", "terminal::ScrollPageUp");
    k.bind("shift-pagedown", "terminal::ScrollPageDown");
    k.bind("cmd-down", "terminal::ScrollPageDown");
    k.bind("shift-up", "terminal::ScrollLineUp");
    k.bind("shift-down", "terminal::ScrollLineDown");
    k.bind("shift-home", "terminal::ScrollToTop");
    k.bind("cmd-home", "terminal::ScrollToTop");
    k.bind("shift-end", "terminal::ScrollToBottom");
    k.bind("cmd-end", "terminal::ScrollToBottom");
    k.bind("ctrl-shift-space", "terminal::ToggleViMode");
    k.bind("ctrl-alt-up", "pane::SplitUp");
    k.bind("ctrl-alt-down", "pane::SplitDown");
    k.bind("ctrl-alt-left", "pane::SplitLeft");
    k.bind("ctrl-alt-right", "pane::SplitRight");
    k.bind("cmd-d", "pane::SplitRight");
    k.bind("cmd-shift-d", "pane::SplitDown");
    k.bind("cmd-alt-r", "terminal::RerunTask");
    k.group(Some("ConfigureContextServerModal > Editor"));
    k.bind("escape", "menu::Cancel");
    k.bind("enter", "editor::Newline");
    k.bind("cmd-enter", "menu::Confirm");
    k.group(Some("MarkdownPreview"));
    k.bind("pageup", "markdown::ScrollPageUp");
    k.bind("pagedown", "markdown::ScrollPageDown");
    k.bind("up", "markdown::ScrollUp");
    k.bind("down", "markdown::ScrollDown");
    k.bind("alt-up", "markdown::ScrollUpByItem");
    k.bind("alt-down", "markdown::ScrollDownByItem");
    k.bind("cmd-up", "markdown::ScrollToTop");
    k.bind("cmd-down", "markdown::ScrollToBottom");
    k.bind("cmd-shift-v", "markdown::CloseAndReturnToEditor");
    k.bind("cmd-f", "buffer_search::Deploy");
    k.group(Some("InvalidBuffer"));
    k.bind("ctrl-shift-enter", "workspace::OpenWithSystem");
    k.group(Some("StashDiff > Editor"));
    k.bind("ctrl-space", "git::ApplyCurrentStash");
    k.bind("ctrl-shift-space", "git::PopCurrentStash");
    k.bind("ctrl-shift-backspace", "git::DropCurrentStash");
    k.group(Some("GitBranchSelector || (GitBranchSelector > Picker > Editor)"));
    k.bind("cmd-shift-backspace", "branch_picker::DeleteBranch");
    k.bind("cmd-alt-shift-backspace", "branch_picker::ForceDeleteBranch");
    k.bind("cmd-shift-i", "branch_picker::CycleBranchFilter");
    k.bind("cmd-k", "branch_picker::ToggleFilterMenu");
    k.group(Some("ImageViewer"));
    k.bind("cmd-=", "image_viewer::ZoomIn");
    k.bind("cmd-+", "image_viewer::ZoomIn");
    k.bind("cmd--", "image_viewer::ZoomOut");
    k.bind("cmd-0", "image_viewer::ResetZoom");
    // k.bind("cmd-k r", "editor::RevealInFileManager");
    k.bind("cmd-shift-0", "image_viewer::FitToView");
    k.group(Some("WorktreePicker || (WorktreePicker > Picker > Editor)"));
    k.bind("cmd-shift-backspace", "worktree_picker::DeleteWorktree");
    k.bind("cmd-alt-shift-backspace", "worktree_picker::ForceDeleteWorktree");
    k.group(Some("GitGraph"));
    k.bind("tab", "git_graph::FocusNextTabStop");
    k.bind("shift-tab", "git_graph::FocusPreviousTabStop");
    k.bind("cmd-f", "buffer_search::Deploy");
    k.bind("cmd-r", "git_graph::Refresh");
    k.group(Some("GitGraphSearchBar > Editor"));
    k.bind("tab", "git_graph::FocusNextTabStop");
    k.bind("shift-tab", "git_graph::FocusPreviousTabStop");
    k.group(Some("Picker > Editor"));
    k.bind("cmd-shift-s", "picker::ToggleMultiSelect");
    k.group(Some("FileFinder || (FileFinder > Picker > Editor) || (FileFinder > Picker > menu)"));
    k.bind("cmd-shift-p", "file_finder::SelectPrevious");
    k.bind("tab", "picker::MultiSelectNext");
    k.group(Some("TextFinder || (TextFinder > Picker > Editor) || (TextFinder > Picker > menu)"));
    k.bind("alt-cmd-f", "text_finder::ToProjectSearch");
    k.bind("tab", "picker::MultiSelectNext");
    k.bind("alt-cmd-[", "text_finder::Fold");
    k.bind("alt-cmd-]", "text_finder::Unfold");
    k.bind("cmd-shift-enter", "text_finder::ToggleFoldAll");
    k.bind("cmd-j", "pane::SplitDown");
    k.bind("cmd-k", "pane::SplitUp");
    k.bind("cmd-h", "pane::SplitLeft");
    k.bind("cmd-l", "pane::SplitRight");
    k.bindings
}

#[cfg(test)]
mod tests {
    use gpui::KeyBinding;
    use rustc_hash::FxHashMap;
    use serde_json::json;

    use super::build;

    fn test_keymap() -> Vec<KeyBinding> {
        let builders: FxHashMap<_, _> =
            gpui::generate_list_of_all_registered_actions().map(|action| (action.name, action.build)).collect();
        build(&|name, input| {
            let build = builders.get(name).unwrap_or_else(|| panic!("unknown action {name}"));
            build(input.unwrap_or_else(|| json!({}))).unwrap_or_else(|error| panic!("invalid action {name}: {error}"))
        })
    }

    fn keystrokes(binding: &KeyBinding) -> String {
        binding.keystrokes().iter().map(|keystroke| keystroke.inner().unparse()).collect::<Vec<_>>().join(" ")
    }

    fn context(binding: &KeyBinding) -> Option<String> {
        binding.predicate().map(|predicate| predicate.to_string())
    }

    #[test]
    fn all_bindings_build() {
        assert!(test_keymap().len() > 500);
    }

    #[test]
    fn terminal_split_down_binding() {
        let keymap = test_keymap();
        let split_down = keymap
            .iter()
            .find(|binding| context(binding).as_deref() == Some("Terminal") && keystrokes(binding) == "cmd-shift-d")
            .expect("terminal binds cmd-shift-d");
        assert_eq!(split_down.action().name(), "pane::SplitDown");
    }

    #[test]
    fn nav_history_has_one_binding_per_direction() {
        let keymap = test_keymap();
        for (action, expected) in [("workspace::GoBack", "cmd-["), ("workspace::GoForward", "cmd-]")] {
            let bound: Vec<_> =
                keymap.iter().filter(|binding| binding.action().name() == action).map(keystrokes).collect();
            assert_eq!(bound, [expected], "{action}");
        }
    }

    #[test]
    fn commit_editor_enter_yields_to_menus() {
        let keymap = test_keymap();
        let newlines: Vec<_> = keymap
            .iter()
            .filter(|binding| keystrokes(binding) == "enter" && binding.action().name() == "editor::Newline")
            .filter_map(context)
            .filter(|context| context.contains("CommitEditor") || context.contains("GitCommit"))
            .collect();
        assert_eq!(newlines.len(), 2);
        for context in newlines {
            assert!(context.contains("!showing_code_actions") && context.contains("!showing_completions"), "{context}");
        }
    }

    #[test]
    fn personal_bindings_live_in_their_context_groups() {
        let keymap = test_keymap();
        for (context_name, keys, action) in [
            (Some("Editor"), "cmd-shift-u", "editor::ToggleCase"),
            (Some("Editor"), "f1", "editor::OpenDocs"),
            (Some("Terminal"), "shift-enter", "terminal::SendText"),
        ] {
            let bound: Vec<_> = keymap
                .iter()
                .filter(|binding| context(binding).as_deref() == context_name && keystrokes(binding) == keys)
                .map(|binding| binding.action().name())
                .collect();
            assert_eq!(bound, [action], "{keys}");
        }
    }
}
