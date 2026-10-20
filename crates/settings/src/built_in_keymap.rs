//! The complete macOS keymap. Edit these bindings and rebuild to change shortcuts.
use crate::KeyBindingSpec;
use serde_json::json;

pub fn keymap() -> Vec<KeyBindingSpec> {
    let mut keymap = Vec::new();
    add_bindings(
        &mut keymap,
        None,
        true,
        [
            KeyBindingSpec::new("home", "menu::SelectFirst"),
            KeyBindingSpec::new("shift-pageup", "menu::SelectFirst"),
            KeyBindingSpec::new("pageup", "menu::SelectFirst"),
            KeyBindingSpec::new("cmd-up", "menu::SelectFirst"),
            KeyBindingSpec::new("end", "menu::SelectLast"),
            KeyBindingSpec::new("shift-pagedown", "menu::SelectLast"),
            KeyBindingSpec::new("pagedown", "menu::SelectLast"),
            KeyBindingSpec::new("cmd-down", "menu::SelectLast"),
            KeyBindingSpec::new("tab", "menu::SelectNext"),
            KeyBindingSpec::new("ctrl-n", "menu::SelectNext"),
            KeyBindingSpec::new("down", "menu::SelectNext"),
            KeyBindingSpec::new("shift-tab", "menu::SelectPrevious"),
            KeyBindingSpec::new("ctrl-p", "menu::SelectPrevious"),
            KeyBindingSpec::new("up", "menu::SelectPrevious"),
            KeyBindingSpec::new("enter", "menu::Confirm"),
            KeyBindingSpec::new("ctrl-enter", "menu::SecondaryConfirm"),
            KeyBindingSpec::new("cmd-enter", "menu::SecondaryConfirm"),
            KeyBindingSpec::new("cmd-escape", "menu::Cancel"),
            KeyBindingSpec::new("ctrl-escape", "menu::Cancel"),
            KeyBindingSpec::new("ctrl-c", "menu::Cancel"),
            KeyBindingSpec::new("escape", "menu::Cancel"),
            KeyBindingSpec::new("alt-shift-enter", "menu::Restart"),
            KeyBindingSpec::new("cmd-shift-w", "workspace::CloseWindow"),
            KeyBindingSpec::new("shift-escape", "workspace::ToggleZoom"),
            KeyBindingSpec::new("cmd-o", "workspace::Open"),
            KeyBindingSpec::new("cmd-=", "zed::IncreaseBufferFontSize")
                .with_input(json!({"persist": false})),
            KeyBindingSpec::new("cmd-+", "zed::IncreaseBufferFontSize")
                .with_input(json!({"persist": false})),
            KeyBindingSpec::new("cmd--", "zed::DecreaseBufferFontSize")
                .with_input(json!({"persist": false})),
            KeyBindingSpec::new("cmd-0", "zed::ResetBufferFontSize")
                .with_input(json!({"persist": false})),
            KeyBindingSpec::new("cmd-q", "zed::Quit"),
            KeyBindingSpec::new("cmd-h", "zed::Hide"),
            KeyBindingSpec::new("alt-cmd-h", "zed::HideOthers"),
            KeyBindingSpec::new("cmd-m", "zed::Minimize"),
            KeyBindingSpec::new("ctrl-cmd-l", "lsp_tool::ToggleMenu"),
            KeyBindingSpec::new("ctrl-cmd-s", "workspace::ToggleWorktreeSecurity"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("menu"),
        false,
        [
            KeyBindingSpec::new("right", "menu::SelectChild"),
            KeyBindingSpec::new("left", "menu::SelectParent"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Editor"),
        true,
        [
            KeyBindingSpec::new("escape", "editor::Cancel"),
            KeyBindingSpec::new("shift-backspace", "editor::Backspace"),
            KeyBindingSpec::new("ctrl-h", "editor::Backspace"),
            KeyBindingSpec::new("backspace", "editor::Backspace"),
            KeyBindingSpec::new("ctrl-d", "editor::Delete"),
            KeyBindingSpec::new("delete", "editor::Delete"),
            KeyBindingSpec::new("tab", "editor::Tab"),
            KeyBindingSpec::new("shift-tab", "editor::Backtab"),
            KeyBindingSpec::new("ctrl-t", "editor::Transpose"),
            KeyBindingSpec::new("ctrl-k", "editor::KillRingCut"),
            KeyBindingSpec::new("ctrl-y", "editor::KillRingYank"),
            KeyBindingSpec::new("cmd-k cmd-q", "editor::Rewrap"),
            KeyBindingSpec::new("cmd-k q", "editor::Rewrap"),
            KeyBindingSpec::new("cmd-backspace", "editor::DeleteToBeginningOfLine"),
            KeyBindingSpec::new("cmd-delete", "editor::DeleteToEndOfLine"),
            KeyBindingSpec::new("alt-backspace", "editor::DeleteToPreviousWordStart")
                .with_input(json!({"ignore_newlines": false, "ignore_brackets": false})),
            KeyBindingSpec::new("ctrl-w", "editor::DeleteToPreviousWordStart")
                .with_input(json!({"ignore_newlines": false, "ignore_brackets": false})),
            KeyBindingSpec::new("alt-delete", "editor::DeleteToNextWordEnd")
                .with_input(json!({"ignore_newlines": false, "ignore_brackets": false})),
            KeyBindingSpec::new("cmd-x", "editor::Cut"),
            KeyBindingSpec::new("cmd-c", "editor::Copy"),
            KeyBindingSpec::new("cmd-v", "editor::Paste"),
            KeyBindingSpec::new("cmd-z", "editor::Undo"),
            KeyBindingSpec::new("cmd-shift-z", "editor::Redo"),
            KeyBindingSpec::new("up", "editor::MoveUp"),
            KeyBindingSpec::new("ctrl-up", "editor::MoveToStartOfParagraph"),
            KeyBindingSpec::new("pageup", "editor::MovePageUp"),
            KeyBindingSpec::new("shift-pageup", "editor::SelectPageUp"),
            KeyBindingSpec::new("cmd-pageup", "editor::PageUp"),
            KeyBindingSpec::new("ctrl-pageup", "editor::LineUp"),
            KeyBindingSpec::new("down", "editor::MoveDown"),
            KeyBindingSpec::new("ctrl-down", "editor::MoveToEndOfParagraph"),
            KeyBindingSpec::new("pagedown", "editor::MovePageDown"),
            KeyBindingSpec::new("shift-pagedown", "editor::SelectPageDown"),
            KeyBindingSpec::new("cmd-pagedown", "editor::PageDown"),
            KeyBindingSpec::new("ctrl-pagedown", "editor::LineDown"),
            KeyBindingSpec::new("ctrl-p", "editor::MoveUp"),
            KeyBindingSpec::new("ctrl-n", "editor::MoveDown"),
            KeyBindingSpec::new("ctrl-b", "editor::MoveLeft"),
            KeyBindingSpec::new("left", "editor::MoveLeft"),
            KeyBindingSpec::new("ctrl-f", "editor::MoveRight"),
            KeyBindingSpec::new("right", "editor::MoveRight"),
            KeyBindingSpec::new("ctrl-l", "editor::ScrollCursorCenter"),
            KeyBindingSpec::new("alt-left", "editor::MoveToPreviousWordStart"),
            KeyBindingSpec::new("alt-right", "editor::MoveToNextWordEnd"),
            KeyBindingSpec::new("cmd-left", "editor::MoveToBeginningOfLine")
                .with_input(json!({"stop_at_soft_wraps": true, "stop_at_indent": true})),
            KeyBindingSpec::new("ctrl-a", "editor::MoveToBeginningOfLine")
                .with_input(json!({"stop_at_soft_wraps": false, "stop_at_indent": true})),
            KeyBindingSpec::new("home", "editor::MoveToBeginningOfLine")
                .with_input(json!({"stop_at_soft_wraps": true, "stop_at_indent": true})),
            KeyBindingSpec::new("cmd-right", "editor::MoveToEndOfLine")
                .with_input(json!({"stop_at_soft_wraps": true})),
            KeyBindingSpec::new("ctrl-e", "editor::MoveToEndOfLine")
                .with_input(json!({"stop_at_soft_wraps": false})),
            KeyBindingSpec::new("end", "editor::MoveToEndOfLine")
                .with_input(json!({"stop_at_soft_wraps": true})),
            KeyBindingSpec::new("cmd-up", "editor::MoveToBeginning"),
            KeyBindingSpec::new("cmd-down", "editor::MoveToEnd"),
            KeyBindingSpec::new("cmd-home", "editor::MoveToBeginning"),
            KeyBindingSpec::new("cmd-end", "editor::MoveToEnd"),
            KeyBindingSpec::new("shift-up", "editor::SelectUp"),
            KeyBindingSpec::new("ctrl-shift-p", "editor::SelectUp"),
            KeyBindingSpec::new("shift-down", "editor::SelectDown"),
            KeyBindingSpec::new("ctrl-shift-n", "editor::SelectDown"),
            KeyBindingSpec::new("shift-left", "editor::SelectLeft"),
            KeyBindingSpec::new("ctrl-shift-b", "editor::SelectLeft"),
            KeyBindingSpec::new("shift-right", "editor::SelectRight"),
            KeyBindingSpec::new("ctrl-shift-f", "editor::SelectRight"),
            KeyBindingSpec::new("alt-shift-left", "editor::SelectToPreviousWordStart"),
            KeyBindingSpec::new("alt-shift-right", "editor::SelectToNextWordEnd"),
            KeyBindingSpec::new("ctrl-shift-up", "editor::SelectToStartOfParagraph"),
            KeyBindingSpec::new("ctrl-shift-down", "editor::SelectToEndOfParagraph"),
            KeyBindingSpec::new("cmd-shift-up", "editor::SelectToBeginning"),
            KeyBindingSpec::new("cmd-shift-down", "editor::SelectToEnd"),
            KeyBindingSpec::new("cmd-a", "editor::SelectAll"),
            KeyBindingSpec::new("cmd-l", "editor::SelectLine"),
            KeyBindingSpec::new("cmd-shift-i", "editor::Format"),
            KeyBindingSpec::new("alt-shift-o", "editor::OrganizeImports"),
            KeyBindingSpec::new("cmd-shift-left", "editor::SelectToBeginningOfLine")
                .with_input(json!({"stop_at_soft_wraps": true, "stop_at_indent": true})),
            KeyBindingSpec::new("shift-home", "editor::SelectToBeginningOfLine")
                .with_input(json!({"stop_at_soft_wraps": true, "stop_at_indent": true})),
            KeyBindingSpec::new("ctrl-shift-a", "editor::SelectToBeginningOfLine")
                .with_input(json!({"stop_at_soft_wraps": true, "stop_at_indent": true})),
            KeyBindingSpec::new("cmd-shift-right", "editor::SelectToEndOfLine")
                .with_input(json!({"stop_at_soft_wraps": true})),
            KeyBindingSpec::new("shift-end", "editor::SelectToEndOfLine")
                .with_input(json!({"stop_at_soft_wraps": true})),
            KeyBindingSpec::new("ctrl-shift-e", "editor::SelectToEndOfLine")
                .with_input(json!({"stop_at_soft_wraps": true})),
            KeyBindingSpec::new("ctrl-v", "editor::MovePageDown")
                .with_input(json!({"center_cursor": true})),
            KeyBindingSpec::new("ctrl-shift-v", "editor::MovePageUp")
                .with_input(json!({"center_cursor": true})),
            KeyBindingSpec::new("ctrl-cmd-space", "editor::ShowCharacterPalette"),
            KeyBindingSpec::new("cmd-;", "editor::ToggleLineNumbers"),
            KeyBindingSpec::new("cmd-'", "editor::ToggleSelectedDiffHunks"),
            KeyBindingSpec::new("cmd-\"", "editor::ExpandAllDiffHunks"),
            KeyBindingSpec::new("cmd-alt-g b", "git::Blame"),
            KeyBindingSpec::new("cmd-alt-g m", "git::OpenModifiedFiles"),
            KeyBindingSpec::new("cmd-i", "editor::ShowSignatureHelp"),
            KeyBindingSpec::new("ctrl-f12", "editor::GoToDeclaration"),
            KeyBindingSpec::new("alt-ctrl-f12", "editor::GoToDeclarationSplit"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Editor && mode == full"),
        true,
        [
            KeyBindingSpec::new("shift-enter", "editor::Newline"),
            KeyBindingSpec::new("enter", "editor::Newline"),
            KeyBindingSpec::new("cmd-enter", "editor::NewlineBelow"),
            KeyBindingSpec::new("cmd-shift-enter", "editor::NewlineAbove"),
            KeyBindingSpec::new("cmd-k z", "editor::ToggleSoftWrap"),
            KeyBindingSpec::new("cmd-f", "buffer_search::Deploy"),
            KeyBindingSpec::new("cmd-alt-l", "buffer_search::Deploy")
                .with_input(json!({"selection_search_enabled": true})),
            KeyBindingSpec::new("cmd-e", "buffer_search::UseSelectionForFind"),
            KeyBindingSpec::new("cmd-alt-e", "editor::SelectEnclosingSymbol"),
            KeyBindingSpec::new("alt-enter", "editor::OpenSelectionsInMultibuffer"),
            KeyBindingSpec::new("alt-cmd-f", "text_finder::Toggle"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Editor && multibuffer"),
        true,
        [
            KeyBindingSpec::new("cmd-up", "editor::MoveToStartOfExcerpt"),
            KeyBindingSpec::new("cmd-down", "editor::MoveToStartOfNextExcerpt"),
            KeyBindingSpec::new("cmd-shift-up", "editor::SelectToStartOfExcerpt"),
            KeyBindingSpec::new("cmd-shift-down", "editor::SelectToStartOfNextExcerpt"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Editor && mode == auto_height"),
        true,
        [
            KeyBindingSpec::new("ctrl-enter", "editor::Newline"),
            KeyBindingSpec::new("shift-enter", "editor::Newline"),
            KeyBindingSpec::new("ctrl-shift-enter", "editor::NewlineBelow"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Markdown"),
        true,
        [KeyBindingSpec::new("cmd-c", "markdown::Copy")],
    );
    add_bindings(
        &mut keymap,
        Some("Editor"),
        true,
        [
            KeyBindingSpec::new("cmd-alt-z", "git::Restore"),
            KeyBindingSpec::new("cmd-alt-y", "git::ToggleStaged"),
            KeyBindingSpec::new("cmd-y", "git::StageAndNext"),
            KeyBindingSpec::new("cmd-shift-y", "git::UnstageAndNext"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("BufferSearchBar"),
        true,
        [
            KeyBindingSpec::new("escape", "buffer_search::Dismiss"),
            KeyBindingSpec::new("tab", "buffer_search::FocusEditor"),
            KeyBindingSpec::new("enter", "search::SelectNextMatch"),
            KeyBindingSpec::new("shift-enter", "search::SelectPreviousMatch"),
            KeyBindingSpec::new("cmd-shift-enter", "editor::ToggleFoldAll"),
            KeyBindingSpec::new("alt-enter", "search::SelectAllMatches"),
            KeyBindingSpec::new("cmd-f", "search::FocusSearch"),
            KeyBindingSpec::new("cmd-alt-l", "search::ToggleSelection"),
            KeyBindingSpec::new("cmd-shift-o", "outline::Toggle"),
            KeyBindingSpec::new("alt-cmd-f", "text_finder::Toggle"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("BufferSearchBar && in_replace > Editor"),
        true,
        [
            KeyBindingSpec::new("enter", "search::ReplaceNext"),
            KeyBindingSpec::new("cmd-enter", "search::ReplaceAll"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("BufferSearchBar && !in_replace > Editor"),
        true,
        [
            KeyBindingSpec::new("ctrl-enter", "editor::Newline"),
            KeyBindingSpec::new("shift-enter", "search::SelectPreviousMatch"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("BufferSearchBar && !in_replace > Editor"),
        false,
        [
            KeyBindingSpec::new("up", "search::PreviousHistoryQuery"),
            KeyBindingSpec::new("down", "search::NextHistoryQuery"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("BufferSearchBar || ProjectSearchBar"),
        true,
        [KeyBindingSpec::new("ctrl-enter", "editor::Newline")],
    );
    add_bindings(
        &mut keymap,
        Some("ProjectSearchBar"),
        true,
        [
            KeyBindingSpec::new("escape", "project_search::ToggleFocus"),
            KeyBindingSpec::new("cmd-shift-j", "project_search::ToggleFilters"),
            KeyBindingSpec::new("cmd-shift-enter", "project_search::ToggleAllSearchResults"),
            KeyBindingSpec::new("cmd-shift-f", "search::FocusSearch"),
            KeyBindingSpec::new("cmd-shift-h", "search::ToggleReplace"),
            KeyBindingSpec::new("alt-cmd-g", "search::ToggleRegex"),
            KeyBindingSpec::new("alt-cmd-x", "search::ToggleRegex"),
            KeyBindingSpec::new("alt-cmd-f", "project_search::OpenTextFinder"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("ProjectSearchBar > Editor"),
        false,
        [
            KeyBindingSpec::new("up", "search::PreviousHistoryQuery"),
            KeyBindingSpec::new("down", "search::NextHistoryQuery"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("ProjectSearchBar && in_replace > Editor"),
        true,
        [
            KeyBindingSpec::new("enter", "search::ReplaceNext"),
            KeyBindingSpec::new("cmd-enter", "search::ReplaceAll"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("ProjectSearchBar && !in_replace > Editor"),
        false,
        [KeyBindingSpec::new("ctrl-enter", "editor::Newline")],
    );
    add_bindings(
        &mut keymap,
        Some("ProjectSearchView"),
        true,
        [
            KeyBindingSpec::new("escape", "project_search::ToggleFocus"),
            KeyBindingSpec::new("cmd-shift-j", "project_search::ToggleFilters"),
            KeyBindingSpec::new("cmd-shift-enter", "project_search::ToggleAllSearchResults"),
            KeyBindingSpec::new("cmd-shift-h", "search::ToggleReplace"),
            KeyBindingSpec::new("alt-cmd-g", "search::ToggleRegex"),
            KeyBindingSpec::new("alt-cmd-x", "search::ToggleRegex"),
            KeyBindingSpec::new("alt-cmd-f", "project_search::OpenTextFinder"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Pane"),
        true,
        [
            KeyBindingSpec::new("alt-cmd-left", "pane::ActivatePreviousItem"),
            KeyBindingSpec::new("cmd-{", "pane::ActivatePreviousItem"),
            KeyBindingSpec::new("alt-cmd-right", "pane::ActivateNextItem"),
            KeyBindingSpec::new("cmd-}", "pane::ActivateNextItem"),
            KeyBindingSpec::new("ctrl-shift-pageup", "pane::SwapItemLeft"),
            KeyBindingSpec::new("ctrl-shift-pagedown", "pane::SwapItemRight"),
            KeyBindingSpec::new("cmd-w", "pane::CloseActiveItem")
                .with_input(json!({"close_pinned": false})),
            KeyBindingSpec::new("alt-cmd-t", "pane::CloseOtherItems")
                .with_input(json!({"close_pinned": false})),
            KeyBindingSpec::new("ctrl-alt-cmd-w", "workspace::CloseInactiveTabsAndPanes"),
            KeyBindingSpec::new("cmd-k e", "pane::CloseItemsToTheLeft")
                .with_input(json!({"close_pinned": false})),
            KeyBindingSpec::new("cmd-k t", "pane::CloseItemsToTheRight")
                .with_input(json!({"close_pinned": false})),
            KeyBindingSpec::new("cmd-k u", "pane::CloseCleanItems")
                .with_input(json!({"close_pinned": false})),
            KeyBindingSpec::new("cmd-k w", "pane::CloseAllItems")
                .with_input(json!({"close_pinned": false})),
            KeyBindingSpec::new("cmd-k cmd-w", "workspace::CloseAllItemsAndPanes"),
            KeyBindingSpec::new("cmd-f", "project_search::ToggleFocus"),
            KeyBindingSpec::new("cmd-g", "search::SelectNextMatch"),
            KeyBindingSpec::new("cmd-shift-g", "search::SelectPreviousMatch"),
            KeyBindingSpec::new("cmd-shift-h", "search::ToggleReplace"),
            KeyBindingSpec::new("cmd-alt-l", "search::ToggleSelection"),
            KeyBindingSpec::new("alt-enter", "search::SelectAllMatches"),
            KeyBindingSpec::new("alt-cmd-c", "search::ToggleCaseSensitive"),
            KeyBindingSpec::new("alt-cmd-w", "search::ToggleWholeWord"),
            KeyBindingSpec::new("alt-cmd-x", "search::ToggleRegex"),
            KeyBindingSpec::new("cmd-k shift-enter", "pane::TogglePinTab"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Editor"),
        true,
        [
            KeyBindingSpec::new("cmd-[", "editor::Outdent"),
            KeyBindingSpec::new("cmd-]", "editor::Indent"),
            KeyBindingSpec::new("cmd-ctrl-p", "editor::AddSelectionAbove")
                .with_input(json!({"skip_soft_wrap": false})),
            KeyBindingSpec::new("cmd-alt-up", "editor::AddSelectionAbove")
                .with_input(json!({"skip_soft_wrap": true})),
            KeyBindingSpec::new("cmd-ctrl-n", "editor::AddSelectionBelow")
                .with_input(json!({"skip_soft_wrap": false})),
            KeyBindingSpec::new("cmd-alt-down", "editor::AddSelectionBelow")
                .with_input(json!({"skip_soft_wrap": true})),
            KeyBindingSpec::new("cmd-shift-k", "editor::DeleteLine"),
            KeyBindingSpec::new("alt-up", "editor::MoveLineUp"),
            KeyBindingSpec::new("alt-down", "editor::MoveLineDown"),
            KeyBindingSpec::new("alt-shift-up", "editor::DuplicateLineUp"),
            KeyBindingSpec::new("alt-shift-down", "editor::DuplicateLineDown"),
            KeyBindingSpec::new("cmd-ctrl-left", "editor::SelectSmallerSyntaxNode"),
            KeyBindingSpec::new("cmd-ctrl-right", "editor::SelectLargerSyntaxNode"),
            KeyBindingSpec::new("cmd-ctrl-up", "editor::SelectPreviousSyntaxNode"),
            KeyBindingSpec::new("ctrl-shift-right", "editor::SelectLargerSyntaxNode"),
            KeyBindingSpec::new("ctrl-shift-left", "editor::SelectSmallerSyntaxNode"),
            KeyBindingSpec::new("cmd-ctrl-down", "editor::SelectNextSyntaxNode"),
            KeyBindingSpec::new("cmd-d", "editor::SelectNext")
                .with_input(json!({"replace_newest": false})),
            KeyBindingSpec::new("cmd-shift-l", "editor::SelectAllMatches"),
            KeyBindingSpec::new("cmd-f2", "editor::SelectAllMatches"),
            KeyBindingSpec::new("cmd-k cmd-d", "editor::SelectNext")
                .with_input(json!({"replace_newest": true})),
            KeyBindingSpec::new("ctrl-cmd-d", "editor::SelectPrevious")
                .with_input(json!({"replace_newest": false})),
            KeyBindingSpec::new("cmd-k ctrl-cmd-d", "editor::SelectPrevious")
                .with_input(json!({"replace_newest": true})),
            KeyBindingSpec::new("cmd-k cmd-i", "editor::Hover"),
            KeyBindingSpec::new("cmd-k cmd-b", "editor::BlameHover"),
            KeyBindingSpec::new("cmd-/", "editor::ToggleComments")
                .with_input(json!({"advance_downwards": false, "comment_empty_lines": true})),
            KeyBindingSpec::new("cmd-k cmd-/", "editor::ToggleBlockComments"),
            KeyBindingSpec::new("shift-alt-a", "editor::ToggleBlockComments"),
            KeyBindingSpec::new("f8", "editor::GoToDiagnostic")
                .with_input(json!({"severity": {"min": "hint", "max": "error"}})),
            KeyBindingSpec::new("shift-f8", "editor::GoToPreviousDiagnostic")
                .with_input(json!({"severity": {"min": "hint", "max": "error"}})),
            KeyBindingSpec::new("f2", "editor::Rename"),
            KeyBindingSpec::new("f12", "editor::GoToDefinition"),
            KeyBindingSpec::new("alt-f12", "editor::GoToDefinitionSplit"),
            KeyBindingSpec::new("cmd-f12", "editor::GoToTypeDefinition"),
            KeyBindingSpec::new("shift-f12", "editor::GoToImplementation"),
            KeyBindingSpec::new("alt-cmd-f12", "editor::GoToTypeDefinitionSplit"),
            KeyBindingSpec::new("alt-shift-f12", "editor::FindAllReferences"),
            KeyBindingSpec::new("cmd-k cmd-h", "call_hierarchy::ShowIncomingCalls"),
            KeyBindingSpec::new("cmd-|", "editor::MoveToEnclosingBracket"),
            KeyBindingSpec::new("ctrl-m", "editor::MoveToEnclosingBracket"),
            KeyBindingSpec::new("alt-cmd-[", "editor::Fold"),
            KeyBindingSpec::new("alt-cmd-]", "editor::UnfoldLines"),
            KeyBindingSpec::new("cmd-k cmd-l", "editor::ToggleFold"),
            KeyBindingSpec::new("cmd-k cmd-[", "editor::FoldRecursive"),
            KeyBindingSpec::new("cmd-k cmd-]", "editor::UnfoldRecursive"),
            KeyBindingSpec::new("cmd-k cmd-1", "editor::FoldAtLevel_1"),
            KeyBindingSpec::new("cmd-k cmd-2", "editor::FoldAtLevel_2"),
            KeyBindingSpec::new("cmd-k cmd-3", "editor::FoldAtLevel_3"),
            KeyBindingSpec::new("cmd-k cmd-4", "editor::FoldAtLevel_4"),
            KeyBindingSpec::new("cmd-k cmd-5", "editor::FoldAtLevel_5"),
            KeyBindingSpec::new("cmd-k cmd-6", "editor::FoldAtLevel_6"),
            KeyBindingSpec::new("cmd-k cmd-7", "editor::FoldAtLevel_7"),
            KeyBindingSpec::new("cmd-k cmd-8", "editor::FoldAtLevel_8"),
            KeyBindingSpec::new("cmd-k cmd-9", "editor::FoldAtLevel_9"),
            KeyBindingSpec::new("cmd-k cmd-0", "editor::FoldAll"),
            KeyBindingSpec::new("cmd-k cmd-j", "editor::UnfoldAll"),
            KeyBindingSpec::new("ctrl-space", "editor::ShowCompletions"),
            KeyBindingSpec::new("ctrl-shift-space", "editor::ShowWordCompletions"),
            KeyBindingSpec::new("cmd-.", "editor::ToggleCodeActions"),
            KeyBindingSpec::new("cmd-k r", "editor::RevealInFileManager"),
            KeyBindingSpec::new("cmd-k p", "editor::CopyPath"),
            KeyBindingSpec::new("cmd-\\", "pane::SplitRight"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Editor && extension == md"),
        true,
        [
            KeyBindingSpec::new("cmd-k v", "markdown::OpenPreviewToTheSide"),
            KeyBindingSpec::new("cmd-shift-v", "markdown::OpenPreview"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Editor && extension == svg"),
        true,
        [
            KeyBindingSpec::new("cmd-k v", "svg::OpenPreviewToTheSide"),
            KeyBindingSpec::new("cmd-shift-v", "svg::OpenPreview"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some(
            "Editor && (extension == csv || extension == tsv || extension == ssv || extension == psv || extension == jsonl || extension == ndjson)",
        ),
        true,
        [
            KeyBindingSpec::new("cmd-k v", "tabular_data::OpenPreviewToTheSide"),
            KeyBindingSpec::new("cmd-shift-v", "tabular_data::OpenPreview"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Editor && mode == full"),
        true,
        [
            KeyBindingSpec::new("cmd-shift-o", "outline::Toggle"),
            KeyBindingSpec::new("ctrl-g", "go_to_line::Toggle"),
            KeyBindingSpec::new("cmd-shift-backspace", "editor::GoToPreviousChange"),
            KeyBindingSpec::new("cmd-shift-alt-backspace", "editor::GoToNextChange"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Pane"),
        true,
        [
            KeyBindingSpec::new("ctrl-1", "pane::ActivateItem").with_input(json!(0)),
            KeyBindingSpec::new("ctrl-2", "pane::ActivateItem").with_input(json!(1)),
            KeyBindingSpec::new("ctrl-3", "pane::ActivateItem").with_input(json!(2)),
            KeyBindingSpec::new("ctrl-4", "pane::ActivateItem").with_input(json!(3)),
            KeyBindingSpec::new("ctrl-5", "pane::ActivateItem").with_input(json!(4)),
            KeyBindingSpec::new("ctrl-6", "pane::ActivateItem").with_input(json!(5)),
            KeyBindingSpec::new("ctrl-7", "pane::ActivateItem").with_input(json!(6)),
            KeyBindingSpec::new("ctrl-8", "pane::ActivateItem").with_input(json!(7)),
            KeyBindingSpec::new("ctrl-9", "pane::ActivateItem").with_input(json!(8)),
            KeyBindingSpec::new("ctrl-0", "pane::ActivateLastItem"),
            KeyBindingSpec::new("ctrl--", "pane::GoBack"),
            KeyBindingSpec::new("ctrl-_", "pane::GoForward"),
            KeyBindingSpec::new("cmd-shift-f", "pane::DeploySearch"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Workspace"),
        true,
        [
            KeyBindingSpec::new("alt-/", "which_key::ShowPendingBindings"),
            KeyBindingSpec::new("f6", "workspace::FocusNextPart"),
            KeyBindingSpec::new("shift-f6", "workspace::FocusPreviousPart"),
            KeyBindingSpec::new("cmd-f6", "workspace::FocusNextPart"),
            KeyBindingSpec::new("fn-f", "zed::ToggleFullScreen"),
            KeyBindingSpec::new("ctrl-cmd-f", "zed::ToggleFullScreen"),
            KeyBindingSpec::new("alt-cmd-o", "projects::OpenRecent"),
            KeyBindingSpec::new("ctrl-r", "projects::OpenRecent"),
            KeyBindingSpec::new("ctrl-cmd-o", "projects::OpenRemote")
                .with_input(json!({"from_existing_connection": false})),
            KeyBindingSpec::new("ctrl-cmd-shift-o", "projects::OpenRemote")
                .with_input(json!({"from_existing_connection": true})),
            KeyBindingSpec::new("cmd-ctrl-b", "branches::OpenRecent"),
            KeyBindingSpec::new("cmd-ctrl-w", "git::Worktree"),
            KeyBindingSpec::new("ctrl-~", "workspace::NewTerminal"),
            KeyBindingSpec::new("cmd-s", "workspace::Save"),
            KeyBindingSpec::new("cmd-k s", "workspace::SaveWithoutFormat"),
            KeyBindingSpec::new("alt-shift-enter", "toast::RunAction"),
            KeyBindingSpec::new("cmd-shift-s", "workspace::SaveAs"),
            KeyBindingSpec::new("cmd-shift-n", "workspace::NewWindow"),
            KeyBindingSpec::new("ctrl-`", "terminal_panel::Toggle"),
            KeyBindingSpec::new("cmd-1", "workspace::ActivatePane").with_input(json!(0)),
            KeyBindingSpec::new("cmd-2", "workspace::ActivatePane").with_input(json!(1)),
            KeyBindingSpec::new("cmd-3", "workspace::ActivatePane").with_input(json!(2)),
            KeyBindingSpec::new("cmd-4", "workspace::ActivatePane").with_input(json!(3)),
            KeyBindingSpec::new("cmd-5", "workspace::ActivatePane").with_input(json!(4)),
            KeyBindingSpec::new("cmd-6", "workspace::ActivatePane").with_input(json!(5)),
            KeyBindingSpec::new("cmd-7", "workspace::ActivatePane").with_input(json!(6)),
            KeyBindingSpec::new("cmd-8", "workspace::ActivatePane").with_input(json!(7)),
            KeyBindingSpec::new("cmd-9", "workspace::ActivatePane").with_input(json!(8)),
            KeyBindingSpec::new("cmd-b", "workspace::ToggleLeftDock"),
            KeyBindingSpec::new("cmd-alt-b", "workspace::ToggleRightDock"),
            KeyBindingSpec::new("cmd-r", "workspace::ToggleRightDock"),
            KeyBindingSpec::new("cmd-j", "workspace::ToggleBottomDock"),
            KeyBindingSpec::new("cmd-alt-j", "multi_workspace::ToggleWorkspaceSidebar"),
            KeyBindingSpec::new("cmd-alt-;", "multi_workspace::FocusWorkspaceSidebar"),
            KeyBindingSpec::new("alt-cmd-y", "workspace::ToggleAllDocks"),
            KeyBindingSpec::new("ctrl-alt-0", "workspace::ResetActiveDockSize"),
            KeyBindingSpec::new("ctrl-alt--", "workspace::DecreaseActiveDockSize")
                .with_input(json!({"px": 0})),
            KeyBindingSpec::new("ctrl-alt-=", "workspace::IncreaseActiveDockSize")
                .with_input(json!({"px": 0})),
            KeyBindingSpec::new("ctrl-alt-)", "workspace::ResetOpenDocksSize"),
            KeyBindingSpec::new("ctrl-alt-_", "workspace::DecreaseOpenDocksSize")
                .with_input(json!({"px": 0})),
            KeyBindingSpec::new("ctrl-alt-+", "workspace::IncreaseOpenDocksSize")
                .with_input(json!({"px": 0})),
            KeyBindingSpec::new("cmd-shift-f", "pane::DeploySearch"),
            KeyBindingSpec::new("cmd-shift-h", "pane::DeploySearch")
                .with_input(json!({"replace_enabled": true})),
            KeyBindingSpec::new("cmd-shift-t", "pane::ReopenClosedItem"),
            KeyBindingSpec::new("cmd-k cmd-p", "workspace::ReopenLastPicker"),
            KeyBindingSpec::new("cmd-k cmd-t", "theme_selector::Toggle"),
            KeyBindingSpec::new("cmd-k cmd-shift-t", "theme::ToggleMode"),
            KeyBindingSpec::new("cmd-t", "project_symbols::Toggle"),
            KeyBindingSpec::new("cmd-p", "file_finder::Toggle"),
            KeyBindingSpec::new("ctrl-shift-tab", "tab_switcher::Toggle")
                .with_input(json!({"select_last": true})),
            KeyBindingSpec::new("ctrl-tab", "tab_switcher::Toggle"),
            KeyBindingSpec::new("cmd-shift-p", "command_palette::Toggle"),
            KeyBindingSpec::new("cmd-shift-m", "diagnostics::Deploy"),
            KeyBindingSpec::new("cmd-shift-e", "project_panel::ToggleFocus"),
            KeyBindingSpec::new("cmd-shift-b", "outline_panel::ToggleFocus"),
            KeyBindingSpec::new("ctrl-shift-g", "git_panel::ToggleFocus"),
            KeyBindingSpec::new("cmd-alt-s", "workspace::SaveAll"),
            KeyBindingSpec::new("cmd-k n", "encoding_selector::Toggle"),
            KeyBindingSpec::new("cmd-k m", "language_selector::Toggle"),
            KeyBindingSpec::new("cmd-k cmd-left", "workspace::ActivatePaneLeft"),
            KeyBindingSpec::new("cmd-k cmd-right", "workspace::ActivatePaneRight"),
            KeyBindingSpec::new("cmd-k cmd-up", "workspace::ActivatePaneUp"),
            KeyBindingSpec::new("cmd-k cmd-down", "workspace::ActivatePaneDown"),
            KeyBindingSpec::new("cmd-k shift-left", "workspace::SwapPaneLeft"),
            KeyBindingSpec::new("cmd-k shift-right", "workspace::SwapPaneRight"),
            KeyBindingSpec::new("cmd-k shift-up", "workspace::SwapPaneUp"),
            KeyBindingSpec::new("cmd-k shift-down", "workspace::SwapPaneDown"),
            KeyBindingSpec::new("cmd-w", "pane::CloseActiveItem")
                .with_input(json!({"close_pinned": false})),
            KeyBindingSpec::new("ctrl-alt-w", "workspace::CloseActiveDock"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Workspace && !Terminal"),
        true,
        [KeyBindingSpec::new("cmd-n", "workspace::NewFile")],
    );
    add_bindings(
        &mut keymap,
        Some("Editor"),
        true,
        [
            KeyBindingSpec::new("cmd-u", "editor::UndoSelection"),
            KeyBindingSpec::new("cmd-shift-u", "editor::RedoSelection"),
            KeyBindingSpec::new("ctrl-j", "editor::JoinLines"),
            KeyBindingSpec::new("ctrl-alt-backspace", "editor::DeleteToPreviousSubwordStart"),
            KeyBindingSpec::new("ctrl-alt-h", "editor::DeleteToPreviousSubwordStart"),
            KeyBindingSpec::new("ctrl-alt-delete", "editor::DeleteToNextSubwordEnd"),
            KeyBindingSpec::new("ctrl-alt-d", "editor::DeleteToNextSubwordEnd"),
            KeyBindingSpec::new("ctrl-alt-left", "editor::MoveToPreviousSubwordStart"),
            KeyBindingSpec::new("ctrl-alt-b", "editor::MoveToPreviousSubwordStart"),
            KeyBindingSpec::new("ctrl-alt-right", "editor::MoveToNextSubwordEnd"),
            KeyBindingSpec::new("ctrl-alt-f", "editor::MoveToNextSubwordEnd"),
            KeyBindingSpec::new(
                "ctrl-alt-shift-left",
                "editor::SelectToPreviousSubwordStart",
            ),
            KeyBindingSpec::new("ctrl-alt-shift-b", "editor::SelectToPreviousSubwordStart"),
            KeyBindingSpec::new("ctrl-alt-shift-right", "editor::SelectToNextSubwordEnd"),
            KeyBindingSpec::new("ctrl-alt-shift-f", "editor::SelectToNextSubwordEnd"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Pane"),
        true,
        [
            KeyBindingSpec::new("cmd-k up", "pane::SplitUp"),
            KeyBindingSpec::new("cmd-k down", "pane::SplitDown"),
            KeyBindingSpec::new("cmd-k left", "pane::SplitLeft"),
            KeyBindingSpec::new("cmd-k right", "pane::SplitRight"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Editor && renaming"),
        true,
        [KeyBindingSpec::new("enter", "editor::ConfirmRename")],
    );
    add_bindings(
        &mut keymap,
        Some("Editor && inline_input"),
        true,
        [KeyBindingSpec::new("enter", "menu::Confirm")],
    );
    add_bindings(
        &mut keymap,
        Some("Editor && showing_completions"),
        true,
        [
            KeyBindingSpec::new("enter", "editor::ConfirmCompletion"),
            KeyBindingSpec::new("shift-enter", "editor::ConfirmCompletionReplace"),
            KeyBindingSpec::new("tab", "editor::ComposeCompletion"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Editor && in_snippet && has_next_tabstop && !showing_completions"),
        true,
        [KeyBindingSpec::new("tab", "editor::NextSnippetTabstop")],
    );
    add_bindings(
        &mut keymap,
        Some("Editor && in_snippet && has_previous_tabstop && !showing_completions"),
        true,
        [KeyBindingSpec::new(
            "shift-tab",
            "editor::PreviousSnippetTabstop",
        )],
    );
    add_bindings(
        &mut keymap,
        Some("Editor && showing_code_actions"),
        true,
        [KeyBindingSpec::new("enter", "editor::ConfirmCodeAction")],
    );
    add_bindings(
        &mut keymap,
        Some("Editor && (showing_code_actions || showing_completions)"),
        true,
        [
            KeyBindingSpec::new("up", "editor::ContextMenuPrevious"),
            KeyBindingSpec::new("ctrl-p", "editor::ContextMenuPrevious"),
            KeyBindingSpec::new("down", "editor::ContextMenuNext"),
            KeyBindingSpec::new("ctrl-n", "editor::ContextMenuNext"),
            KeyBindingSpec::new("pageup", "editor::ContextMenuFirst"),
            KeyBindingSpec::new("pagedown", "editor::ContextMenuLast"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Editor && showing_signature_help && !showing_completions"),
        false,
        [
            KeyBindingSpec::new("up", "editor::SignatureHelpPrevious"),
            KeyBindingSpec::new("down", "editor::SignatureHelpNext"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Editor && mode == full"),
        true,
        [
            KeyBindingSpec::new("alt-enter", "editor::OpenExcerpts"),
            KeyBindingSpec::new("shift-enter", "editor::ExpandExcerpts"),
            KeyBindingSpec::new("cmd-alt-enter", "editor::OpenExcerptsSplit"),
            KeyBindingSpec::new("cmd-shift-e", "pane::RevealInProjectPanel"),
            KeyBindingSpec::new("cmd-f8", "editor::GoToHunk"),
            KeyBindingSpec::new("cmd-shift-f8", "editor::GoToPreviousHunk"),
            KeyBindingSpec::new("ctrl-:", "editor::ToggleInlayHints"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("ProjectSearchBar && !in_replace"),
        true,
        [KeyBindingSpec::new(
            "cmd-enter",
            "project_search::SearchInNew",
        )],
    );
    add_bindings(
        &mut keymap,
        Some("OutlinePanel && not_editing"),
        true,
        [
            KeyBindingSpec::new("escape", "menu::Cancel"),
            KeyBindingSpec::new("left", "outline_panel::CollapseSelectedEntry"),
            KeyBindingSpec::new("right", "outline_panel::ExpandSelectedEntry"),
            KeyBindingSpec::new("cmd-alt-c", "workspace::CopyPath"),
            KeyBindingSpec::new("alt-cmd-shift-c", "workspace::CopyRelativePath"),
            KeyBindingSpec::new("alt-cmd-r", "outline_panel::RevealInFileManager"),
            KeyBindingSpec::new("space", "outline_panel::OpenSelectedEntry"),
            KeyBindingSpec::new("shift-down", "menu::SelectNext"),
            KeyBindingSpec::new("shift-up", "menu::SelectPrevious"),
            KeyBindingSpec::new("alt-enter", "editor::OpenExcerpts"),
            KeyBindingSpec::new("cmd-alt-enter", "editor::OpenExcerptsSplit"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("ProjectPanel"),
        true,
        [
            KeyBindingSpec::new("left", "project_panel::CollapseSelectedEntry"),
            KeyBindingSpec::new("cmd-left", "project_panel::CollapseAllEntries"),
            KeyBindingSpec::new("right", "project_panel::ExpandSelectedEntry"),
            KeyBindingSpec::new("cmd-right", "project_panel::ExpandAllEntries"),
            KeyBindingSpec::new("cmd-n", "project_panel::NewFile"),
            KeyBindingSpec::new("cmd-d", "project_panel::Duplicate"),
            KeyBindingSpec::new("alt-cmd-n", "project_panel::NewDirectory"),
            KeyBindingSpec::new("cmd-x", "project_panel::Cut"),
            KeyBindingSpec::new("cmd-c", "project_panel::Copy"),
            KeyBindingSpec::new("cmd-v", "project_panel::Paste"),
            KeyBindingSpec::new("cmd-alt-c", "workspace::CopyPath"),
            KeyBindingSpec::new("alt-cmd-shift-c", "workspace::CopyRelativePath"),
            KeyBindingSpec::new("cmd-z", "project_panel::Undo"),
            KeyBindingSpec::new("cmd-shift-z", "project_panel::Redo"),
            KeyBindingSpec::new("enter", "project_panel::Rename"),
            KeyBindingSpec::new("f2", "project_panel::Rename"),
            KeyBindingSpec::new("backspace", "project_panel::Trash")
                .with_input(json!({"skip_prompt": false})),
            KeyBindingSpec::new("delete", "project_panel::Trash")
                .with_input(json!({"skip_prompt": false})),
            KeyBindingSpec::new("cmd-backspace", "project_panel::Trash")
                .with_input(json!({"skip_prompt": true})),
            KeyBindingSpec::new("cmd-delete", "project_panel::Delete")
                .with_input(json!({"skip_prompt": false})),
            KeyBindingSpec::new("alt-cmd-r", "project_panel::RevealInFileManager"),
            KeyBindingSpec::new("ctrl-shift-enter", "workspace::OpenWithSystem"),
            KeyBindingSpec::new("alt-d", "project_panel::CompareMarkedFiles"),
            KeyBindingSpec::new("cmd-alt-backspace", "project_panel::Delete")
                .with_input(json!({"skip_prompt": false})),
            KeyBindingSpec::new("cmd-alt-shift-f", "project_panel::NewSearchInDirectory"),
            KeyBindingSpec::new("shift-f10", "project_panel::OpenContextMenu"),
            KeyBindingSpec::new("shift-down", "menu::SelectNext"),
            KeyBindingSpec::new("shift-up", "menu::SelectPrevious"),
            KeyBindingSpec::new("escape", "menu::Cancel"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("ProjectPanel && not_editing"),
        true,
        [KeyBindingSpec::new("space", "project_panel::Open")],
    );
    add_bindings(
        &mut keymap,
        Some("GitPanel && ChangesList && !GitBranchSelector && !GitRepositorySelector"),
        true,
        [
            KeyBindingSpec::new("up", "menu::SelectPrevious"),
            KeyBindingSpec::new("down", "menu::SelectNext"),
            KeyBindingSpec::new("shift-up", "menu::SelectPrevious"),
            KeyBindingSpec::new("shift-down", "menu::SelectNext"),
            KeyBindingSpec::new("enter", "menu::Confirm"),
            KeyBindingSpec::new("escape", "menu::Cancel"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("GitPanel && ChangesList && !GitBranchSelector && !GitRepositorySelector"),
        true,
        [
            KeyBindingSpec::new("up", "git_panel::PreviousEntry"),
            KeyBindingSpec::new("down", "git_panel::NextEntry"),
            KeyBindingSpec::new("cmd-up", "git_panel::FirstEntry"),
            KeyBindingSpec::new("cmd-down", "git_panel::LastEntry"),
            KeyBindingSpec::new("left", "git_panel::CollapseSelectedEntry"),
            KeyBindingSpec::new("right", "git_panel::ExpandSelectedEntry"),
            KeyBindingSpec::new("cmd-alt-y", "git::ToggleStaged"),
            KeyBindingSpec::new("space", "git::ToggleStaged"),
            KeyBindingSpec::new("shift-space", "git::StageRange"),
            KeyBindingSpec::new("cmd-y", "git::StageFile"),
            KeyBindingSpec::new("cmd-shift-y", "git::UnstageFile"),
            KeyBindingSpec::new("cmd-alt-c", "workspace::CopyPath"),
            KeyBindingSpec::new("alt-cmd-shift-c", "workspace::CopyRelativePath"),
            KeyBindingSpec::new("alt-down", "git_panel::FocusEditor"),
            KeyBindingSpec::new("tab", "git_panel::FocusEditor"),
            KeyBindingSpec::new("shift-tab", "git_panel::FocusEditor"),
            KeyBindingSpec::new("backspace", "git::RestoreFile")
                .with_input(json!({"skip_prompt": false})),
            KeyBindingSpec::new("delete", "git::RestoreFile")
                .with_input(json!({"skip_prompt": false})),
            KeyBindingSpec::new("cmd-backspace", "git::RestoreFile")
                .with_input(json!({"skip_prompt": true})),
            KeyBindingSpec::new("cmd-delete", "git::RestoreFile")
                .with_input(json!({"skip_prompt": true})),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("GitPanel && CommitEditor"),
        true,
        [KeyBindingSpec::new("escape", "git::Cancel")],
    );
    add_bindings(
        &mut keymap,
        Some("GitDiff > Editor"),
        true,
        [
            KeyBindingSpec::new("cmd-enter", "git::Commit"),
            KeyBindingSpec::new("cmd-shift-enter", "git::Amend"),
            KeyBindingSpec::new("cmd-ctrl-y", "git::StageAll"),
            KeyBindingSpec::new("cmd-ctrl-shift-y", "git::UnstageAll"),
            KeyBindingSpec::new("cmd-alt-z", "git::RestoreAndNext"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("CommitEditor > Editor"),
        true,
        [
            KeyBindingSpec::new("enter", "editor::Newline"),
            KeyBindingSpec::new("cmd-enter", "git::Commit"),
            KeyBindingSpec::new("cmd-shift-enter", "git::Amend"),
            KeyBindingSpec::new("tab", "git_panel::FocusChanges"),
            KeyBindingSpec::new("shift-tab", "git_panel::FocusChanges"),
            KeyBindingSpec::new("alt-up", "git_panel::FocusChanges"),
            KeyBindingSpec::new("alt-shift-escape", "git::ToggleFillCommitEditor"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("GitPanel"),
        true,
        [
            KeyBindingSpec::new("ctrl-g ctrl-g", "git::Fetch"),
            KeyBindingSpec::new("ctrl-g up", "git::Push"),
            KeyBindingSpec::new("ctrl-g down", "git::Pull"),
            KeyBindingSpec::new("ctrl-g shift-down", "git::PullRebase"),
            KeyBindingSpec::new("ctrl-g shift-up", "git::ForcePush"),
            KeyBindingSpec::new("ctrl-g d", "git::Diff"),
            KeyBindingSpec::new("ctrl-g backspace", "git::RestoreTrackedFiles"),
            KeyBindingSpec::new("ctrl-g shift-backspace", "git::TrashUntrackedFiles"),
            KeyBindingSpec::new("cmd-ctrl-y", "git::StageAll"),
            KeyBindingSpec::new("cmd-ctrl-shift-y", "git::UnstageAll"),
            KeyBindingSpec::new("cmd-enter", "git::Commit"),
            KeyBindingSpec::new("cmd-shift-enter", "git::Amend"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("GitCommit > Editor && mode == auto_height"),
        true,
        [
            KeyBindingSpec::new("enter", "editor::Newline"),
            KeyBindingSpec::new("escape", "menu::Cancel"),
            KeyBindingSpec::new("cmd-enter", "git::Commit"),
            KeyBindingSpec::new("cmd-shift-enter", "git::Amend"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("CallHierarchyPicker > Picker > Editor"),
        true,
        [
            KeyBindingSpec::new("cmd-k cmd-h", "call_hierarchy::ToggleDirection"),
            KeyBindingSpec::new("cmd-k right", "menu::SelectChild"),
            KeyBindingSpec::new("cmd-k left", "menu::SelectParent"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Picker > Editor"),
        true,
        [
            KeyBindingSpec::new("escape", "menu::Cancel"),
            KeyBindingSpec::new("up", "menu::SelectPrevious"),
            KeyBindingSpec::new("down", "menu::SelectNext"),
            KeyBindingSpec::new("tab", "picker::ConfirmCompletion"),
            KeyBindingSpec::new("alt-enter", "picker::ConfirmInput")
                .with_input(json!({"secondary": false})),
            KeyBindingSpec::new("cmd-alt-enter", "picker::ConfirmInput")
                .with_input(json!({"secondary": true})),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("LspCommandSelector > Picker > Editor"),
        true,
        [KeyBindingSpec::new(
            "tab",
            "lsp_command_selector::ToggleArgumentsFocus",
        )],
    );
    add_bindings(
        &mut keymap,
        Some("FileFinder || (FileFinder > Picker > Editor)"),
        true,
        [KeyBindingSpec::new(
            "cmd-shift-i",
            "search::ToggleIncludeIgnored",
        )],
    );
    add_bindings(
        &mut keymap,
        Some("CommandPalette || (CommandPalette > Picker > Editor)"),
        true,
        [KeyBindingSpec::new(
            "shift-backspace",
            "command_palette::RemoveSelected",
        )],
    );
    add_bindings(
        &mut keymap,
        Some("RecentProjects || (RecentProjects > Picker > Editor)"),
        true,
        [
            KeyBindingSpec::new("cmd-k", "recent_projects::ToggleActionsMenu"),
            KeyBindingSpec::new("cmd-shift-a", "workspace::AddFolderToProject"),
            KeyBindingSpec::new("shift-backspace", "recent_projects::RemoveSelected"),
            KeyBindingSpec::new("cmd-shift-enter", "recent_projects::AddToWorkspace"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("TabSwitcher"),
        true,
        [
            KeyBindingSpec::new("ctrl-shift-tab", "menu::SelectPrevious"),
            KeyBindingSpec::new("ctrl-up", "menu::SelectPrevious"),
            KeyBindingSpec::new("ctrl-down", "menu::SelectNext"),
            KeyBindingSpec::new("ctrl-backspace", "tab_switcher::CloseSelectedItem"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("StashList || (StashList > Picker > Editor)"),
        true,
        [
            KeyBindingSpec::new("ctrl-shift-backspace", "stash_picker::DropStashItem"),
            KeyBindingSpec::new("ctrl-shift-v", "stash_picker::ShowStashItem"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Terminal"),
        true,
        [
            KeyBindingSpec::new("ctrl-cmd-space", "terminal::ShowCharacterPalette"),
            KeyBindingSpec::new("cmd-c", "terminal::Copy"),
            KeyBindingSpec::new("cmd-v", "terminal::Paste"),
            KeyBindingSpec::new("ctrl-cmd-v", "terminal::PasteText"),
            KeyBindingSpec::new("cmd-f", "buffer_search::Deploy"),
            KeyBindingSpec::new("cmd-a", "editor::SelectAll"),
            KeyBindingSpec::new("cmd-k", "terminal::Clear"),
            KeyBindingSpec::new("cmd-n", "workspace::NewTerminal"),
            KeyBindingSpec::disabled("ctrl-_"),
            KeyBindingSpec::new("cmd-backspace", "terminal::SendKeystroke")
                .with_input(json!("ctrl-u")),
            KeyBindingSpec::new("alt-delete", "terminal::SendText").with_input(json!("\u{001b}d")),
            KeyBindingSpec::new("cmd-delete", "terminal::SendKeystroke")
                .with_input(json!("ctrl-k")),
            KeyBindingSpec::new("cmd-right", "terminal::SendKeystroke").with_input(json!("ctrl-e")),
            KeyBindingSpec::new("cmd-left", "terminal::SendKeystroke").with_input(json!("ctrl-a")),
            KeyBindingSpec::new("alt-left", "terminal::SendText").with_input(json!("\u{001b}b")),
            KeyBindingSpec::new("alt-right", "terminal::SendText").with_input(json!("\u{001b}f")),
            KeyBindingSpec::new("alt-b", "terminal::SendText").with_input(json!("\u{001b}b")),
            KeyBindingSpec::new("alt-f", "terminal::SendText").with_input(json!("\u{001b}f")),
            KeyBindingSpec::new("ctrl-delete", "terminal::SendText")
                .with_input(json!("\u{001b}[3;5~")),
            KeyBindingSpec::new("up", "terminal::SendKeystroke").with_input(json!("up")),
            KeyBindingSpec::new("pageup", "terminal::SendKeystroke").with_input(json!("pageup")),
            KeyBindingSpec::new("down", "terminal::SendKeystroke").with_input(json!("down")),
            KeyBindingSpec::new("pagedown", "terminal::SendKeystroke")
                .with_input(json!("pagedown")),
            KeyBindingSpec::new("escape", "terminal::SendKeystroke").with_input(json!("escape")),
            KeyBindingSpec::new("enter", "terminal::SendKeystroke").with_input(json!("enter")),
            KeyBindingSpec::new("ctrl-c", "terminal::SendKeystroke").with_input(json!("ctrl-c")),
            KeyBindingSpec::new("ctrl-r", "terminal::SendKeystroke").with_input(json!("ctrl-r")),
            KeyBindingSpec::new("ctrl-backspace", "terminal::SendKeystroke")
                .with_input(json!("ctrl-w")),
            KeyBindingSpec::new("shift-pageup", "terminal::ScrollPageUp"),
            KeyBindingSpec::new("cmd-up", "terminal::ScrollPageUp"),
            KeyBindingSpec::new("shift-pagedown", "terminal::ScrollPageDown"),
            KeyBindingSpec::new("cmd-down", "terminal::ScrollPageDown"),
            KeyBindingSpec::new("shift-up", "terminal::ScrollLineUp"),
            KeyBindingSpec::new("shift-down", "terminal::ScrollLineDown"),
            KeyBindingSpec::new("shift-home", "terminal::ScrollToTop"),
            KeyBindingSpec::new("cmd-home", "terminal::ScrollToTop"),
            KeyBindingSpec::new("shift-end", "terminal::ScrollToBottom"),
            KeyBindingSpec::new("cmd-end", "terminal::ScrollToBottom"),
            KeyBindingSpec::new("ctrl-shift-space", "terminal::ToggleViMode"),
            KeyBindingSpec::new("ctrl-alt-up", "pane::SplitUp"),
            KeyBindingSpec::new("ctrl-alt-down", "pane::SplitDown"),
            KeyBindingSpec::new("ctrl-alt-left", "pane::SplitLeft"),
            KeyBindingSpec::new("ctrl-alt-right", "pane::SplitRight"),
            KeyBindingSpec::new("cmd-d", "pane::SplitRight"),
            KeyBindingSpec::new("cmd-alt-r", "terminal::RerunTask"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("ConfigureContextServerModal > Editor"),
        true,
        [
            KeyBindingSpec::new("escape", "menu::Cancel"),
            KeyBindingSpec::new("enter", "editor::Newline"),
            KeyBindingSpec::new("cmd-enter", "menu::Confirm"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Diagnostics"),
        true,
        [KeyBindingSpec::new(
            "ctrl-r",
            "diagnostics::ToggleDiagnosticsRefresh",
        )],
    );
    add_bindings(
        &mut keymap,
        Some("MarkdownPreview"),
        false,
        [
            KeyBindingSpec::new("pageup", "markdown::ScrollPageUp"),
            KeyBindingSpec::new("pagedown", "markdown::ScrollPageDown"),
            KeyBindingSpec::new("up", "markdown::ScrollUp"),
            KeyBindingSpec::new("down", "markdown::ScrollDown"),
            KeyBindingSpec::new("alt-up", "markdown::ScrollUpByItem"),
            KeyBindingSpec::new("alt-down", "markdown::ScrollDownByItem"),
            KeyBindingSpec::new("cmd-up", "markdown::ScrollToTop"),
            KeyBindingSpec::new("cmd-down", "markdown::ScrollToBottom"),
            KeyBindingSpec::new("cmd-shift-v", "markdown::CloseAndReturnToEditor"),
            KeyBindingSpec::new("cmd-f", "buffer_search::Deploy"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Welcome"),
        true,
        [
            KeyBindingSpec::new("cmd-n", "workspace::NewFile"),
            KeyBindingSpec::new("cmd-=", "zed::IncreaseUiFontSize")
                .with_input(json!({"persist": false})),
            KeyBindingSpec::new("cmd-+", "zed::IncreaseUiFontSize")
                .with_input(json!({"persist": false})),
            KeyBindingSpec::new("cmd--", "zed::DecreaseUiFontSize")
                .with_input(json!({"persist": false})),
            KeyBindingSpec::new("cmd-0", "zed::ResetUiFontSize")
                .with_input(json!({"persist": false})),
            KeyBindingSpec::new("cmd-1", "welcome::OpenRecentProject").with_input(json!(0)),
            KeyBindingSpec::new("cmd-2", "welcome::OpenRecentProject").with_input(json!(1)),
            KeyBindingSpec::new("cmd-3", "welcome::OpenRecentProject").with_input(json!(2)),
            KeyBindingSpec::new("cmd-4", "welcome::OpenRecentProject").with_input(json!(3)),
            KeyBindingSpec::new("cmd-5", "welcome::OpenRecentProject").with_input(json!(4)),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("InvalidBuffer"),
        true,
        [KeyBindingSpec::new(
            "ctrl-shift-enter",
            "workspace::OpenWithSystem",
        )],
    );
    add_bindings(
        &mut keymap,
        Some("StashDiff > Editor"),
        true,
        [
            KeyBindingSpec::new("ctrl-space", "git::ApplyCurrentStash"),
            KeyBindingSpec::new("ctrl-shift-space", "git::PopCurrentStash"),
            KeyBindingSpec::new("ctrl-shift-backspace", "git::DropCurrentStash"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("GitBranchSelector || (GitBranchSelector > Picker > Editor)"),
        true,
        [
            KeyBindingSpec::new("cmd-shift-backspace", "branch_picker::DeleteBranch"),
            KeyBindingSpec::new(
                "cmd-alt-shift-backspace",
                "branch_picker::ForceDeleteBranch",
            ),
            KeyBindingSpec::new("cmd-shift-i", "branch_picker::CycleBranchFilter"),
            KeyBindingSpec::new("cmd-k", "branch_picker::ToggleFilterMenu"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("ImageViewer"),
        true,
        [
            KeyBindingSpec::new("cmd-=", "image_viewer::ZoomIn"),
            KeyBindingSpec::new("cmd-+", "image_viewer::ZoomIn"),
            KeyBindingSpec::new("cmd--", "image_viewer::ZoomOut"),
            KeyBindingSpec::new("cmd-0", "image_viewer::ResetZoom"),
            KeyBindingSpec::new("cmd-1", "image_viewer::ZoomToActualSize"),
            KeyBindingSpec::new("cmd-k r", "editor::RevealInFileManager"),
            KeyBindingSpec::new("cmd-shift-0", "image_viewer::FitToView"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("GitPicker"),
        false,
        [
            KeyBindingSpec::new("cmd-1", "git_picker::ActivateBranchesTab"),
            KeyBindingSpec::new("cmd-2", "git_picker::ActivateStashTab"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("WorktreePicker || (WorktreePicker > Picker > Editor)"),
        true,
        [
            KeyBindingSpec::new("cmd-shift-backspace", "worktree_picker::DeleteWorktree"),
            KeyBindingSpec::new(
                "cmd-alt-shift-backspace",
                "worktree_picker::ForceDeleteWorktree",
            ),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("GitGraph"),
        false,
        [
            KeyBindingSpec::new("tab", "git_graph::FocusNextTabStop"),
            KeyBindingSpec::new("shift-tab", "git_graph::FocusPreviousTabStop"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("GitGraphSearchBar > Editor"),
        false,
        [
            KeyBindingSpec::new("tab", "git_graph::FocusNextTabStop"),
            KeyBindingSpec::new("shift-tab", "git_graph::FocusPreviousTabStop"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("TableView"),
        true,
        [
            KeyBindingSpec::new("up", "tabular_data::MoveFocusedCell")
                .with_input(json!({"direction": "up"})),
            KeyBindingSpec::new("down", "tabular_data::MoveFocusedCell")
                .with_input(json!({"direction": "down"})),
            KeyBindingSpec::new("left", "tabular_data::MoveFocusedCell")
                .with_input(json!({"direction": "left"})),
            KeyBindingSpec::new("right", "tabular_data::MoveFocusedCell")
                .with_input(json!({"direction": "right"})),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Picker > Editor"),
        true,
        [
            KeyBindingSpec::new("cmd-shift-a", "picker::ToggleActionsMenu"),
            KeyBindingSpec::new("cmd-shift-s", "picker::ToggleMultiSelect"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("(Picker && with_preview) > Editor"),
        true,
        [
            KeyBindingSpec::new("cmd-alt-p", "picker::TogglePreview"),
            KeyBindingSpec::new("cmd-alt-right", "picker::SetPreviewRight"),
            KeyBindingSpec::new("cmd-alt-down", "picker::SetPreviewBelow"),
            KeyBindingSpec::new("cmd-alt-up", "picker::SetPreviewHidden"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("FileFinder || (FileFinder > Picker > Editor) || (FileFinder > Picker > menu)"),
        true,
        [
            KeyBindingSpec::new("cmd-shift-p", "file_finder::SelectPrevious"),
            KeyBindingSpec::new("tab", "picker::MultiSelectNext"),
            KeyBindingSpec::new("cmd-j", "pane::SplitDown"),
            KeyBindingSpec::new("cmd-k", "pane::SplitUp"),
            KeyBindingSpec::new("cmd-h", "pane::SplitLeft"),
            KeyBindingSpec::new("cmd-l", "pane::SplitRight"),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("TextFinder || (TextFinder > Picker > Editor) || (TextFinder > Picker > menu)"),
        true,
        [
            KeyBindingSpec::new("alt-cmd-f", "text_finder::ToProjectSearch"),
            KeyBindingSpec::new("tab", "picker::MultiSelectNext"),
            KeyBindingSpec::new("alt-cmd-[", "text_finder::Fold"),
            KeyBindingSpec::new("alt-cmd-]", "text_finder::Unfold"),
            KeyBindingSpec::new("cmd-shift-enter", "text_finder::ToggleFoldAll"),
            KeyBindingSpec::new("cmd-j", "pane::SplitDown"),
            KeyBindingSpec::new("cmd-k", "pane::SplitUp"),
            KeyBindingSpec::new("cmd-h", "pane::SplitLeft"),
            KeyBindingSpec::new("cmd-l", "pane::SplitRight"),
        ],
    );
    // Personal bindings take precedence over the bindings above.
    add_bindings(
        &mut keymap,
        None,
        false,
        [
            KeyBindingSpec::new("cmd-[", "pane::GoBack").with_override(),
            KeyBindingSpec::new("cmd-]", "pane::GoForward").with_override(),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Editor"),
        false,
        [
            KeyBindingSpec::new("f1", "editor::OpenDocs").with_override(),
            KeyBindingSpec::new("cmd-shift-u", "editor::ToggleCase").with_override(),
        ],
    );
    add_bindings(
        &mut keymap,
        Some("Terminal"),
        false,
        [KeyBindingSpec::new("shift-enter", "terminal::SendText")
            .with_input(json!("\u{001b}\r"))
            .with_override()],
    );
    keymap
}

fn add_bindings(
    keymap: &mut Vec<KeyBindingSpec>,
    context: Option<&'static str>,
    use_key_equivalents: bool,
    bindings: impl IntoIterator<Item = KeyBindingSpec>,
) {
    keymap.extend(bindings.into_iter().map(|mut binding| {
        binding.context = context;
        binding.use_key_equivalents = use_key_equivalents;
        binding
    }));
}
