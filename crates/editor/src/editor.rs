use std::any;
use std::fmt;
use std::fmt::Debug;
use std::fmt::Formatter;
use std::marker;

use theme::built_in;
use ui::font;

use crate::mouse_context_menu::MenuPosition;
pub mod actions;
pub mod blink_manager;
pub mod code_context_menus;
mod code_lens;
pub mod display_map;
mod document_colors;
mod document_links;
mod document_symbols;
mod element;
mod fold;
mod folding_ranges;
mod git;
mod highlight_matching_bracket;
pub mod hover_links;
pub mod hover_popover;
mod indent_guides;
mod inlays;
pub mod items;
mod jsx_tag_auto_close;
mod linked_editing_ranges;
mod lsp_ext;
mod mouse_context_menu;
pub mod movement;
mod persistence;
mod runnables;
mod rust_analyzer_ext;
pub mod scroll;
mod selections_collection;
pub mod semantic_tokens;
mod split;
pub mod split_editor_view;

mod spell_check;

mod clipboard;
mod code_actions;
mod columnar_selection;
mod completions;
mod config;
mod cursor_motion;
mod diagnostics;
mod erased_editor;
mod events;
mod highlights;
mod input;
mod line_edits;
mod lsp_actions;
mod lsp_data;
mod markdown_actions;
mod navigation;
mod rewrap;
mod selection;
mod semantics_provider;
mod snapshot;
mod snippets;
mod text_transform;
mod workspace_actions;

use std::any::Any;
use std::any::TypeId;
use std::borrow::Cow;
use std::cell::RefCell;
use std::cmp;
use std::cmp::Ordering;
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::collections::hash_map;
use std::mem;
use std::num::NonZeroU32;
use std::ops::Deref;
use std::ops::DerefMut;
use std::ops::Not;
use std::ops::Range;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use ::git::Blame;
use ::git::status::FileStatus;
pub(crate) use actions::AcceptInlayHint;
pub(crate) use actions::AddSelectionAbove;
pub(crate) use actions::AddSelectionBelow;
pub(crate) use actions::Backspace;
pub(crate) use actions::BlamePreviousRevision;
pub(crate) use actions::BlameRevision;
pub(crate) use actions::Cancel;
pub(crate) use actions::CancelFlycheck;
pub(crate) use actions::ClearFlycheck;
pub(crate) use actions::CodeActionSource;
pub(crate) use actions::CollapseAllDiffHunks;
pub(crate) use actions::ComposeCompletion;
pub(crate) use actions::ConfirmCodeAction;
pub(crate) use actions::ConfirmCompletion;
pub(crate) use actions::ConfirmCompletionInsert;
pub(crate) use actions::ConfirmCompletionReplace;
pub(crate) use actions::ContextMenuFirst;
pub(crate) use actions::ContextMenuLast;
pub(crate) use actions::ContextMenuNext;
pub(crate) use actions::ContextMenuPrevious;
pub(crate) use actions::Copy;
pub(crate) use actions::CopyFileLocation;
pub(crate) use actions::CopyHighlightJson;
pub(crate) use actions::Cut;
pub(crate) use actions::CutToEndOfLine;
pub(crate) use actions::Delete;
pub(crate) use actions::DeleteToBeginningOfLine;
pub(crate) use actions::DeleteToEndOfLine;
pub(crate) use actions::DeleteToNextSubwordEnd;
pub(crate) use actions::DeleteToNextWordEnd;
pub(crate) use actions::DeleteToPreviousSubwordStart;
pub(crate) use actions::DeleteToPreviousWordStart;
pub(crate) use actions::DiffClipboardWithSelection;
pub(crate) use actions::DiffClipboardWithSelectionData;
pub(crate) use actions::ExpandAllDiffHunks;
pub(crate) use actions::ExpandExcerpts;
pub(crate) use actions::ExpandExcerptsDown;
pub(crate) use actions::ExpandExcerptsUp;
pub(crate) use actions::ExpandMacroRecursively;
pub(crate) use actions::FindAllReferences;
pub(crate) use actions::FindNextMatch;
pub(crate) use actions::FindPreviousMatch;
pub(crate) use actions::FoldAtLevel;
pub(crate) use actions::FoldRecursive;
pub(crate) use actions::FoldSelectedRanges;
pub(crate) use actions::GoToDeclaration;
pub(crate) use actions::GoToDeclarationSplit;
pub(crate) use actions::GoToDefinition;
pub(crate) use actions::GoToDefinitionSplit;
pub(crate) use actions::GoToDiagnostic;
pub(crate) use actions::GoToHunk;
pub(crate) use actions::GoToImplementation;
pub(crate) use actions::GoToImplementationSplit;
pub(crate) use actions::GoToNextChange;
pub(crate) use actions::GoToNextDocumentHighlight;
pub(crate) use actions::GoToNextReference;
pub(crate) use actions::GoToNextSymbol;
pub(crate) use actions::GoToParentModule;
pub(crate) use actions::GoToPreviousChange;
pub(crate) use actions::GoToPreviousDiagnostic;
pub(crate) use actions::GoToPreviousDocumentHighlight;
pub(crate) use actions::GoToPreviousHunk;
pub(crate) use actions::GoToPreviousReference;
pub(crate) use actions::GoToPreviousSymbol;
pub(crate) use actions::GoToTypeDefinition;
pub(crate) use actions::GoToTypeDefinitionSplit;
pub(crate) use actions::HalfPageDown;
pub(crate) use actions::HalfPageUp;
pub(crate) use actions::HandleInput;
pub(crate) use actions::Hover;
pub(crate) use actions::InsertSnippet;
pub(crate) use actions::InsertUuidV4;
pub(crate) use actions::InsertUuidV7;
pub(crate) use actions::KillRingCut;
pub(crate) use actions::KillRingYank;
pub(crate) use actions::LineDown;
pub(crate) use actions::LineUp;
pub(crate) use actions::MoveDownByLines;
pub(crate) use actions::MoveLeft;
pub(crate) use actions::MovePageDown;
pub(crate) use actions::MovePageUp;
pub(crate) use actions::MoveRight;
pub(crate) use actions::MoveToBeginning;
pub(crate) use actions::MoveToBeginningOfLine;
pub(crate) use actions::MoveToEnclosingBracket;
pub(crate) use actions::MoveToEnd;
pub(crate) use actions::MoveToEndOfExcerpt;
pub(crate) use actions::MoveToEndOfLargerSyntaxNode;
pub(crate) use actions::MoveToEndOfLine;
pub(crate) use actions::MoveToEndOfParagraph;
pub(crate) use actions::MoveToEndOfPreviousExcerpt;
pub(crate) use actions::MoveToNextCommentParagraph;
pub(crate) use actions::MoveToNextSubwordEnd;
pub(crate) use actions::MoveToNextWordEnd;
pub(crate) use actions::MoveToPreviousCommentParagraph;
pub(crate) use actions::MoveToPreviousSubwordStart;
pub(crate) use actions::MoveToPreviousWordStart;
pub(crate) use actions::MoveToStartOfExcerpt;
pub(crate) use actions::MoveToStartOfLargerSyntaxNode;
pub(crate) use actions::MoveToStartOfNextExcerpt;
pub(crate) use actions::MoveToStartOfParagraph;
pub(crate) use actions::MoveUpByLines;
pub(crate) use actions::Newline;
pub(crate) use actions::NewlineAbove;
pub(crate) use actions::NewlineBelow;
pub(crate) use actions::NextScreen;
pub(crate) use actions::OpenContextMenu;
pub(crate) use actions::OpenDocs;
pub(crate) use actions::OpenGitBlameCommit;
pub use actions::OpenResultsIn;
pub(crate) use actions::OpenSelectedFilename;
pub(crate) use actions::OpenUrl;
pub(crate) use actions::PageDown;
pub(crate) use actions::PageUp;
pub(crate) use actions::Paste;
pub(crate) use actions::Redo;
pub(crate) use actions::RedoSelection;
pub(crate) use actions::Rename;
pub(crate) use actions::Rewrap;
pub(crate) use actions::RunFlycheck;
pub(crate) use actions::SaveLocation;
pub(crate) use actions::ScrollCursorBottom;
pub(crate) use actions::ScrollCursorCenter;
pub(crate) use actions::ScrollCursorCenterTopBottom;
pub(crate) use actions::ScrollCursorTop;
pub(crate) use actions::SelectAll;
pub(crate) use actions::SelectAllMatches;
pub(crate) use actions::SelectAroundDelimiters;
pub(crate) use actions::SelectDown;
pub(crate) use actions::SelectDownByLines;
pub(crate) use actions::SelectEnclosingSymbol;
pub(crate) use actions::SelectInsideDelimiters;
pub(crate) use actions::SelectLargerSyntaxNode;
pub(crate) use actions::SelectLeft;
pub(crate) use actions::SelectLine;
pub(crate) use actions::SelectNext;
pub(crate) use actions::SelectNextSyntaxNode;
pub(crate) use actions::SelectPageDown;
pub(crate) use actions::SelectPageUp;
pub(crate) use actions::SelectPrevious;
pub(crate) use actions::SelectPreviousSyntaxNode;
pub(crate) use actions::SelectRight;
pub(crate) use actions::SelectSmallerSyntaxNode;
pub(crate) use actions::SelectToBeginning;
pub(crate) use actions::SelectToBeginningOfLine;
pub(crate) use actions::SelectToEnd;
pub(crate) use actions::SelectToEndOfExcerpt;
pub(crate) use actions::SelectToEndOfLargerSyntaxNode;
pub(crate) use actions::SelectToEndOfLine;
pub(crate) use actions::SelectToEndOfParagraph;
pub(crate) use actions::SelectToEndOfPreviousExcerpt;
pub(crate) use actions::SelectToNextSubwordEnd;
pub(crate) use actions::SelectToNextWordEnd;
pub(crate) use actions::SelectToPreviousSubwordStart;
pub(crate) use actions::SelectToPreviousWordStart;
pub(crate) use actions::SelectToStartOfExcerpt;
pub(crate) use actions::SelectToStartOfLargerSyntaxNode;
pub(crate) use actions::SelectToStartOfNextExcerpt;
pub(crate) use actions::SelectToStartOfParagraph;
pub(crate) use actions::SelectUp;
pub(crate) use actions::SelectUpByLines;
pub(crate) use actions::ShowCharacterPalette;
pub(crate) use actions::ShowCompletions;
pub(crate) use actions::ShowWordCompletions;
pub(crate) use actions::SplitSelectionIntoLines;
pub(crate) use actions::ToggleAllDiffHunks;
pub(crate) use actions::ToggleBlockComments;
pub(crate) use actions::ToggleBlockQuote;
pub(crate) use actions::ToggleCodeActions;
pub(crate) use actions::ToggleComments;
pub(crate) use actions::ToggleDiagnostics;
pub(crate) use actions::ToggleFold;
pub(crate) use actions::ToggleFoldAll;
pub(crate) use actions::ToggleIndentGuides;
pub(crate) use actions::ToggleMinimap;
pub(crate) use actions::ToggleSelectedDiffHunks;
pub(crate) use actions::ToggleSoftWrap;
pub(crate) use actions::Undo;
pub(crate) use actions::UndoSelection;
pub(crate) use actions::UnfoldLines;
pub(crate) use actions::UnfoldRecursive;
pub(crate) use actions::UnwrapSyntaxNode;
pub(crate) use actions::UuidVersion;
use aho_corasick::AhoCorasick;
use aho_corasick::AhoCorasickBuilder;
use aho_corasick::BuildError;
use anyhow::Result;
use anyhow::bail;
use blink_manager::BlinkManager;
pub use code_actions::CodeActionProvider;
use code_context_menus::AvailableCodeAction;
use code_context_menus::CodeActionContents;
use code_context_menus::CodeActionsItem;
use code_context_menus::CodeActionsMenu;
use code_context_menus::CodeContextMenu;
use code_context_menus::CompletionsMenu;
use code_context_menus::ContextMenuOrigin;
pub use completions::CompletionProvider;
pub(crate) use completions::split_words;
use cursor_motion::CursorMotionStates;
use diagnostics::ActiveDiagnostic;
pub use diagnostics::DiagnosticRenderer;
use diagnostics::GlobalDiagnosticRenderer;
use diagnostics::InlineDiagnostic;
pub use diagnostics::set_diagnostic_renderer;
use display_map::BlockId;
use display_map::BlockProperties;
use display_map::BlockStyle;
pub use display_map::ChunkRenderer;
pub use display_map::ChunkRendererContext;
use display_map::ChunkRendererId;
use display_map::ChunkReplacement;
use display_map::Crease;
use display_map::CustomBlockId;
use display_map::DisplayMap;
pub use display_map::DisplayPoint;
use display_map::DisplayRow;
use display_map::DisplaySnapshot;
pub use display_map::FoldPlaceholder;
pub use display_map::HighlightKey;
use display_map::HighlightStyles;
pub use display_map::NavigationOverlayKey;
use display_map::RenderBlock;
pub use display_map::SemanticTokenHighlight;
use display_map::StickyHeaderExcerpt;
use display_map::ToDisplayPoint;
use document_colors::LspColorData;
use document_links::LspDocumentLinks;
use ed_actions::editor::MoveDown;
use ed_actions::editor::MoveUp;
pub use ed_actions::editor::RevealInFileManager;
pub use element::CursorLayout;
pub use element::EditorElement;
pub use element::HighlightedRange;
pub use element::HighlightedRangeLine;
use element::LineWithInvisibles;
use element::MinimapRowCache;
pub use element::PointForPosition;
use element::PositionMap;
use futures::FutureExt;
use futures::future;
use futures::future::Shared;
use fuzzy::StringMatch;
use fuzzy::StringMatchCandidate;
pub use git::DefaultDiffHunkRenderer;
pub use git::DiffHunkRenderer;
pub(crate) use git::DisplayDiffHunk;
pub use git::HiddenDiffHunkRenderer;
pub use git::HiddenUnstagedDiffHunkRenderer;
pub use git::blame::BlameRenderer;
pub use git::blame::GitBlame;
use git::blame::GlobalBlameRenderer;
pub use git::set_blame_renderer;
use gpui::Action;
use gpui::AnyElement;
use gpui::App;
use gpui::AppContext;
use gpui::Background;
use gpui::Bounds;
use gpui::ClickEvent;
use gpui::ClipboardEntry;
use gpui::ClipboardItem;
use gpui::Context;
use gpui::DispatchPhase;
use gpui::Entity;
use gpui::EntityId;
use gpui::EntityInputHandler;
use gpui::EventEmitter;
use gpui::FocusHandle;
use gpui::Focusable;
use gpui::FontId;
use gpui::Global;
use gpui::HighlightStyle;
use gpui::Hsla;
use gpui::IsZero as _;
use gpui::KeyContext;
use gpui::Modifiers;
use gpui::MouseButton;
use gpui::MouseDownEvent;
use gpui::PaintQuad;
use gpui::ParentElement;
use gpui::Pixels;
use gpui::PressureStage;
use gpui::Render;
use gpui::SharedString;
use gpui::Size;
use gpui::Styled;
use gpui::Subscription;
use gpui::Task;
use gpui::TaskExt as _;
use gpui::TextRun;
use gpui::TextStyle;
use gpui::TextStyleRefinement;
use gpui::UTF16Selection;
use gpui::UnderlineStyle;
use gpui::UniformListScrollHandle;
use gpui::WeakEntity;
use gpui::WeakFocusHandle;
use gpui::Window;
use gpui::div;
use gpui::point;
use gpui::prelude::BorrowAppContext;
use gpui::prelude::Element;
use gpui::prelude::FluentBuilder as _;
use gpui::prelude::InteractiveElement as _;
use gpui::prelude::IntoElement;
use gpui::prelude::Refineable as _;
use gpui::prelude::StatefulInteractiveElement as _;
use gpui::px;
use gpui::relative;
use gpui::size;
use gpui_util::TypeIdHashBuilder;
use hover_links::HoverLink;
use hover_links::HoveredLinkState;
use hover_links::find_file;
use hover_popover::HoverState;
use hover_popover::hide_hover;
pub use hover_popover::hover_markdown_style;
use indent_guides::ActiveIndentGuidesState;
use inlays::InlaySplice;
use inlays::inlay_hints::InlayHintRefreshReason;
pub use items::MAX_TAB_TITLE_LEN;
use itertools::Either;
use itertools::Itertools;
use language::AutoindentMode;
use language::BlockCommentConfig;
use language::BracketMatch;
use language::BracketPair;
use language::Buffer;
use language::BufferRow;
use language::BufferSnapshot;
use language::Capability;
use language::CharClassifier;
use language::CharKind;
use language::CharScopeContext;
use language::CodeLabel;
use language::CursorShape;
use language::DiagnosticEntryRef;
use language::DiffOptions;
use language::IndentKind;
use language::IndentSize;
use language::Language;
use language::LanguageAwareStyling;
use language::LanguageName;
use language::LanguageRegistry;
use language::LanguageScope;
use language::OffsetRangeExt;
use language::OutlineItem;
use language::Point;
use language::Selection;
use language::SelectionGoal;
use language::TextObject;
use language::TransactionId;
use language::TreeSitterOptions;
use language::WordsQuery;
use language::language_settings::INLAY_HINTS;
use language::language_settings::LanguageSettings;
use language::language_settings::RewrapBehavior;
use language::language_settings::WordsCompletionMode;
use language::point_to_lsp;
use language::text_diff_with_options;
pub use linked_editing_ranges::LinkedEdits;
use linked_editing_ranges::refresh_linked_ranges;
pub use lsp::CompletionContext;
use lsp::CompletionItemKind;
use lsp::CompletionTriggerKind;
use lsp::InsertTextFormat;
use lsp::InsertTextMode;
use lsp::LanguageServerId;
pub use lsp_ext::lsp_tasks;
use mouse_context_menu::MouseContextMenu;
use movement::TextLayoutDetails;
pub use multi_buffer::Anchor;
pub use multi_buffer::AnchorRangeExt;
pub use multi_buffer::BufferOffset;
pub use multi_buffer::EXCERPT_CONTEXT_LINES;
use multi_buffer::ExcerptBoundaryInfo;
pub use multi_buffer::ExcerptRange;
use multi_buffer::ExpandExcerptDirection;
pub use multi_buffer::MBTextSummary;
pub use multi_buffer::MultiBuffer;
use multi_buffer::MultiBufferDiffHunk;
pub use multi_buffer::MultiBufferOffset;
pub use multi_buffer::MultiBufferOffsetUtf16;
use multi_buffer::MultiBufferPoint;
use multi_buffer::MultiBufferRow;
pub use multi_buffer::MultiBufferSnapshot;
pub use multi_buffer::PathKey;
pub use multi_buffer::RowInfo;
pub use multi_buffer::ToOffset;
pub use multi_buffer::ToPoint;
use project::CodeAction;
use project::Completion;
use project::CompletionDisplayOptions;
use project::CompletionIntent;
use project::CompletionResponse;
use project::CompletionSource;
use project::Location;
use project::LocationLink;
use project::LspAction;
use project::Project;
use project::ProjectPath;
use project::ProjectTransaction;
use project::git_store::GitStoreEvent;
use project::lsp_store::CompletionDocumentation;
use project::lsp_store::OpenLspBufferHandle;
use project::project_settings::DiagnosticSeverity;
use project::project_settings::GoToDiagnosticSeverityFilter;
use regex::Regex;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use scroll::Autoscroll;
use scroll::ScrollAnchor;
use scroll::ScrollManager;
use scroll::SharedScrollAnchor;
use selections_collection::MutableSelectionsCollection;
use selections_collection::SelectionsCollection;
pub use semantics_provider::SemanticsProvider;
use serde::Deserialize;
use serde::Serialize;
use smallvec::SmallVec;
use smallvec::smallvec;
use snippet::Snippet;
pub use split::DiffStyleControls;
pub use split::DiffViewStyle;
pub use split::GlobalDiffViewStyle;
pub use split::SplittableEditor;
pub use split::ToggleSplitDiff;
pub use text::Bias;
use text::BufferId;
use text::FromAnchor;
use text::OffsetUtf16;
use text::Rope;
use theme::ActiveTheme;
use theme::CursorColors;
use theme::StatusColors;
use theme::SyntaxTheme;
use theme::Theme;
use theme::built_in::BUFFER_LINE_HEIGHT;
use theme::built_in::UNNECESSARY_CODE_FADE;
use ui::ContextMenu;
use ui::Disclosure;
use ui::IconButtonShape;
use ui::Tooltip;
use ui::font::observe_buffer_font_size_adjustment;
use ui::prelude::AbsoluteLength;
use ui::prelude::ButtonCommon;
use ui::prelude::ButtonStyle;
use ui::prelude::Clickable;
use ui::prelude::Color;
use ui::prelude::IconButton;
use ui::prelude::IconName;
use ui::prelude::IconSize;
use ui::prelude::Rems;
use ui::prelude::rems;
use ui::scrollbars::ScrollbarAutoHide;
use util::RangeExt;
use util::ResultExt;
use util::post_inc;
use workspace::Item as WorkspaceItem;
use workspace::ItemNavHistory;
use workspace::NavigationEntry;
use workspace::Pane;
use workspace::Workspace;
use workspace::item::ItemBufferKind;
use workspace::item::ItemHandle;
use workspace::notifications::NotifyResultExt;
use workspace::notifications::NotifyTaskExt as _;
pub use workspace::searchable::Direction;
use workspace::searchable::SelectSearchOptions;

use crate::code_context_menus::CompletionsMenuSource;
use crate::hover_links::find_url;
use crate::hover_links::find_url_from_range;
use crate::inlays::inlay_hints::LspInlayHintData;
use crate::runnables::ResolvedTasks;
use crate::runnables::RunnableData;
use crate::runnables::RunnableTaskStatus;
use crate::scroll::ScrollOffset;
use crate::selections_collection::resolve_selections_wrapping_blocks;
use crate::semantic_tokens::SemanticTokenState;

pub const FILE_HEADER_HEIGHT: u32 = 2;
pub const BUFFER_HEADER_PADDING: Rems = rems(0.25);
pub const MULTI_BUFFER_EXCERPT_HEADER_HEIGHT: u32 = 1;
const CURSOR_BLINK_INTERVAL: Duration = Duration::from_millis(500);
const LSP_HIGHLIGHT_DEBOUNCE: Duration = Duration::from_millis(75);
const INLINE_DIAGNOSTICS_DEBOUNCE: Duration = Duration::from_millis(150);
const PULL_DIAGNOSTICS_DEBOUNCE: Duration = Duration::from_millis(50);
/// Gap between a line's end and its inline diagnostic, in columns.
pub(crate) const INLINE_DIAGNOSTICS_PADDING_COLUMNS: f32 = 4.0;
/// Lines added to an excerpt per expand step.
pub(crate) const EXPAND_EXCERPT_LINES: u32 = 5;
/// Minimum APCA contrast for text drawn over highlight backgrounds.
pub const MINIMUM_CONTRAST_FOR_HIGHLIGHTS: f32 = 45.0;
const MAX_LINE_LEN: usize = 1024;
/// Reserve gutter space for this many line number digits, to avoid resizes as files grow.
pub(crate) const MIN_LINE_NUMBER_DIGITS: usize = 4;
const MIN_NAVIGATION_HISTORY_ROW_DELTA: i64 = 10;
const MAX_SELECTION_HISTORY_LEN: usize = 1024;

pub const CODE_ACTIONS_DEBOUNCE_TIMEOUT: Duration = Duration::from_millis(250);
pub const SELECTION_HIGHLIGHT_DEBOUNCE_TIMEOUT: Duration = Duration::from_millis(100);

pub(crate) const CODE_ACTION_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const SCROLL_CENTER_TOP_BOTTOM_DEBOUNCE_TIMEOUT: Duration = Duration::from_secs(1);
pub const LSP_REQUEST_DEBOUNCE_TIMEOUT: Duration = Duration::from_millis(50);

pub(crate) const MINIMAP_FONT_SIZE: AbsoluteLength = AbsoluteLength::Pixels(px(2.));

pub struct ConflictsOuter(!);
pub struct ConflictsOurs(!);
pub struct ConflictsTheirs(!);
pub struct ConflictsOursMarker(!);
pub struct ConflictsTheirsMarker(!);

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Navigated {
    Yes,
    No,
}

impl Navigated {
    pub const fn from_bool(yes: bool) -> Navigated {
        if yes { Navigated::Yes } else { Navigated::No }
    }
}

pub fn init(cx: &mut App) {
    cx.set_global(GlobalBlameRenderer(Arc::new(())));

    workspace::register_project_item::<Editor>(cx);
    workspace::register_serializable_item::<Editor>(cx);

    cx.observe_new(|workspace: &mut Workspace, window: Option<&mut Window>, cx: &mut Context<Workspace>| {
        workspace.register_action(Editor::new_file);
        workspace.register_action(Editor::new_file_split);
        workspace.register_action(Editor::new_file_vertical);
        workspace.register_action(Editor::new_file_horizontal);
        workspace.register_action(Editor::cancel_language_server_work);
        workspace.register_action(Editor::toggle_focus);
        if let Some(window) = window {
            cx.subscribe_in(workspace.project(), window, |workspace, _, event, window, cx| {
                if let project::Event::LanguageServerShowDocument(request) = event {
                    items::handle_lsp_show_document(workspace, request, window, cx).detach();
                }
            })
            .detach();
        }
    })
    .detach();

    cx.on_action(move |_: &workspace::NewFile, cx| {
        let app_state = workspace::AppState::global(cx);
        workspace::open_new(workspace::OpenOptions::default(), app_state, cx, |workspace, window, cx| {
            Editor::new_file(workspace, &workspace::NewFile, window, cx);
        })
        .detach_and_log_err(cx);
    })
    .on_action(move |_: &workspace::NewWindow, cx| {
        let app_state = workspace::AppState::global(cx);
        workspace::open_new(workspace::OpenOptions::default(), app_state, cx, |workspace, window, cx| {
            cx.activate(true);
            Editor::new_file(workspace, &workspace::NewFile, window, cx);
        })
        .detach_and_log_err(cx);
    });
    _ = ui_input::ERASED_EDITOR_FACTORY
        .set(|window, cx| cx.new(|cx| Editor::single_line(window, cx)).update(cx, |editor, cx| editor.erased(cx)));
}

pub struct SearchWithinRange;

trait InvalidationRegion {
    fn ranges(&self) -> &[Range<Anchor>];
}

#[derive(Clone, Debug, PartialEq)]
pub enum SelectPhase {
    Begin { position: DisplayPoint, add: bool, click_count: usize },
    BeginColumnar { position: DisplayPoint, reset: bool, mode: ColumnarMode, goal_column: u32 },
    Extend { position: DisplayPoint, click_count: usize },
    Update { position: DisplayPoint, goal_column: u32, scroll_delta: gpui::Point<f32> },
    End,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColumnarMode {
    FromMouse,
    FromSelection,
}

#[derive(Clone, Debug)]
pub enum SelectMode {
    Character,
    Word(Range<Anchor>),
    Line(Range<Anchor>),
    All,
}

#[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
pub enum SizingBehavior {
    /// The editor will layout itself using `size_full` and will include the vertical
    /// scroll margin as requested by user settings.
    #[default]
    Default,
    /// The editor will layout itself using `size_full`, but will not have any
    /// vertical overscroll.
    ExcludeOverscrollMargin,
    /// The editor will request a vertical size according to its content and will be
    /// layouted without a vertical scroll margin.
    SizeByContent,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EditorMode {
    SingleLine,
    AutoHeight {
        min_lines: usize,
        max_lines: Option<usize>,
    },
    Full {
        /// When set to `true`, the editor will scale its UI elements with the buffer font size.
        scale_ui_elements_with_buffer_font_size: bool,
        /// When set to `true`, the editor will render a background for the active line.
        show_active_line_background: bool,
        /// Determines the sizing behavior for this editor.
        sizing_behavior: SizingBehavior,
    },
    Minimap {
        parent: WeakEntity<Editor>,
    },
}

impl EditorMode {
    pub const fn full() -> Self {
        Self::Full {
            scale_ui_elements_with_buffer_font_size: true,
            show_active_line_background: true,
            sizing_behavior: SizingBehavior::Default,
        }
    }

    #[inline]
    pub const fn is_full(&self) -> bool {
        matches!(self, Self::Full { .. })
    }

    #[inline]
    pub const fn is_single_line(&self) -> bool {
        matches!(self, Self::SingleLine { .. })
    }

    #[inline]
    const fn is_minimap(&self) -> bool {
        matches!(self, Self::Minimap { .. })
    }
}

#[derive(Copy, Clone, Debug)]
pub enum SoftWrap {
    /// Prefer not to wrap at all.
    ///
    /// Note: this is currently internal, as actually limited by [`crate::MAX_LINE_LEN`] until it wraps.
    /// The mode is used inside git diff hunks, where it's seems currently more useful to not wrap as much as possible.
    GitDiff,
    /// Prefer a single line generally, unless an overly long line is encountered.
    None,
    /// Soft wrap lines that exceed the editor width.
    EditorWidth,
}

#[derive(Clone)]
pub struct EditorStyle {
    pub background: Hsla,
    pub border: Hsla,
    pub cursor_colors: CursorColors,
    pub text: TextStyle,
    pub scrollbar_width: Pixels,
    pub syntax: &'static SyntaxTheme,
    pub status: StatusColors,
    pub inlay_hints_style: HighlightStyle,
    pub unnecessary_code_fade: f32,
    pub show_underlines: bool,
}

impl Default for EditorStyle {
    fn default() -> Self {
        Self {
            background: Hsla::default(),
            border: Hsla::default(),
            cursor_colors: built_in::cursor_colors(),
            text: TextStyle::default(),
            scrollbar_width: Pixels::default(),
            syntax: SyntaxTheme::empty(),
            status: built_in::status_colors(),
            inlay_hints_style: HighlightStyle::default(),
            unnecessary_code_fade: Default::default(),
            show_underlines: true,
        }
    }
}

pub fn make_inlay_hints_style(cx: &App) -> HighlightStyle {
    let show_background = INLAY_HINTS.show_background;

    let mut style = cx.theme().syntax().style_for_name("hint").unwrap_or_default();

    if style.color.is_none() {
        style.color = Some(cx.theme().status().hint);
    }

    if !show_background {
        style.background_color = None;
        return style;
    }

    if style.background_color.is_none() {
        style.background_color = Some(cx.theme().status().hint_background);
    }

    style
}

type CompletionId = usize;

pub struct ContextMenuOptions {
    pub min_entries_visible: usize,
    pub max_entries_visible: usize,
    pub placement: Option<ContextMenuPlacement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextMenuPlacement {
    Above,
    Below,
}

#[derive(Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Debug, Default)]
struct EditorActionId(usize);

impl EditorActionId {
    pub(crate) const fn post_inc(&mut self) -> Self {
        let answer = self.0;

        *self = Self(answer + 1);

        Self(answer)
    }
}

type BackgroundHighlight = (Arc<dyn Fn(&usize, &Theme) -> Hsla + Send + Sync>, Arc<[Range<Anchor>]>);
type GutterHighlight = (fn(&App) -> Hsla, Vec<Range<Anchor>>);

#[derive(Default)]
struct ScrollbarMarkerState {
    /// Minimap width and line height the markers were laid out for.
    track_size: Size<Pixels>,
    dirty: bool,
    markers: Arc<[PaintQuad]>,
    pending_refresh: Option<Task<Result<()>>>,
}

impl ScrollbarMarkerState {
    fn should_refresh(&self, track_size: Size<Pixels>) -> bool {
        self.pending_refresh.is_none() && (self.track_size != track_size || self.dirty)
    }

    /// Stores a finished refresh and returns whether the markers changed, so an
    /// unchanged result doesn't redraw the editor.
    fn finish_refresh(&mut self, markers: Arc<[PaintQuad]>, track_size: Size<Pixels>) -> bool {
        self.pending_refresh = None;
        self.track_size = track_size;
        let changed = self.markers != markers;
        self.markers = markers;
        changed
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MinimapVisibility {
    Disabled,
    Enabled { visible: bool },
}

impl MinimapVisibility {
    const fn for_mode(mode: &EditorMode) -> MinimapVisibility {
        if mode.is_full() { MinimapVisibility::Enabled { visible: true } } else { MinimapVisibility::Disabled }
    }

    const fn hidden(self) -> MinimapVisibility {
        match self {
            MinimapVisibility::Enabled { .. } => MinimapVisibility::Enabled { visible: false },
            MinimapVisibility::Disabled => MinimapVisibility::Disabled,
        }
    }

    const fn disabled(self) -> bool {
        matches!(self, MinimapVisibility::Disabled)
    }

    const fn visible(self) -> bool {
        matches!(self, MinimapVisibility::Enabled { visible: true })
    }

    const fn toggle_visibility(self) -> MinimapVisibility {
        match self {
            MinimapVisibility::Enabled { visible } => MinimapVisibility::Enabled { visible: !visible },
            MinimapVisibility::Disabled => MinimapVisibility::Disabled,
        }
    }
}

/// Addons allow storing per-editor state in other crates (e.g. Vim).
pub trait Addon: 'static {
    fn render_buffer_header_controls(
        &self,
        _: &ExcerptBoundaryInfo,
        _: &language::BufferSnapshot,
        _: &Window,
        _: &App,
    ) -> Option<AnyElement> {
        None
    }

    fn extend_buffer_header_context_menu(
        &self,
        menu: ui::ContextMenu,
        _: &language::BufferSnapshot,
        _: &mut Window,
        _: &mut App,
    ) -> ui::ContextMenu {
        menu
    }

    fn override_status_for_buffer_id(&self, _: BufferId, _: &App) -> Option<FileStatus> {
        None
    }

    /// Replaces "Open File" on buffer headers, e.g. when the excerpt opens
    /// something other than the project file.
    fn open_file_label(&self) -> Option<&'static str> {
        None
    }

    /// Whether buffer headers offer actions on the file's place in the
    /// project tree ("Reveal In Project Panel", "Open in Terminal"). Off for
    /// excerpts from another revision, such as a commit diff.
    fn shows_project_file_actions(&self) -> bool {
        true
    }

    fn to_any(&self) -> &dyn Any;

    fn to_any_mut(&mut self) -> Option<&mut dyn Any> {
        None
    }
}

struct ChangeLocation {
    current: Option<Vec<Anchor>>,
    original: Vec<Anchor>,
}
impl ChangeLocation {
    fn locations(&self) -> &[Anchor] {
        self.current.as_ref().unwrap_or(&self.original)
    }
}

/// A set of caret positions, registered when the editor was edited.
pub struct ChangeList {
    changes: Vec<ChangeLocation>,
    /// Currently "selected" change.
    position: Option<usize>,
}

impl ChangeList {
    pub const fn new() -> Self {
        Self { changes: Vec::new(), position: None }
    }

    /// Moves to the next change in the list (based on the direction given) and returns the caret positions for the next change.
    /// If reaches the end of the list in the direction, returns the corresponding change until called for a different direction.
    pub fn next_change(&mut self, count: usize, direction: Direction) -> Option<&[Anchor]> {
        if self.changes.is_empty() {
            return None;
        }

        let prev = self.position.unwrap_or(self.changes.len());
        let next = if direction == Direction::Prev {
            prev.saturating_sub(count)
        } else {
            (prev + count).min(self.changes.len() - 1)
        };
        self.position = Some(next);
        self.changes.get(next).map(|change| change.locations())
    }

    /// Adds a new change to the list, resetting the change list position.
    pub fn push_to_change_list(&mut self, group: bool, new_positions: Vec<Anchor>) {
        self.position.take();
        if let Some(last) = self.changes.last_mut()
            && group
        {
            last.current = Some(new_positions);
        } else {
            self.changes.push(ChangeLocation { original: new_positions, current: None });
        }
    }

    pub fn last(&self) -> Option<&[Anchor]> {
        self.changes.last().map(|change| change.locations())
    }
}

impl Default for ChangeList {
    fn default() -> Self {
        Self::new()
    }
}

enum ColumnarSelectionState {
    FromMouse { selection_tail: Anchor, display_point: Option<DisplayPoint> },
    FromSelection { selection_tail: Anchor },
}

/// Copy-file-location button shown when hovering a gutter row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GutterHoverButton {
    display_row: DisplayRow,
    /// There's a small debounce between hovering over the line and showing the button.
    /// We don't want to show it when moving the mouse from editor to e.g. project panel.
    is_active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum LspDataKind {
    PullDiagnostics,
    DocumentColors,
    DocumentLinks,
    FoldingRanges,
}

enum CodeActionsForSelection {
    None,
    Fetching(Shared<Task<Option<ActionFetchReady>>>),
    Ready(ActionFetchReady),
}

#[derive(Clone)]
struct ActionFetchReady {
    location: Location,
    actions: Rc<[AvailableCodeAction]>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchResultsStatus {
    pub pending: bool,
    pub results_stale: bool,
    pub query_confirmed: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SearchResultsHold {
    pub status: SearchResultsStatus,
    pub min_line_number_digits: usize,
    settled_scroll_range: Option<SettledScrollRange>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct SettledScrollRange {
    range: Size<Pixels>,
    editor_width: Pixels,
    editor_bounds_size: Size<Pixels>,
}

/// ed's primary implementation of text input, allowing users to edit a [`MultiBuffer`].
///
/// See the [module level documentation](self) for more information.
pub struct Editor {
    focus_handle: FocusHandle,
    last_focused_descendant: Option<WeakFocusHandle>,
    /// The text buffer being edited.
    buffer: Entity<MultiBuffer>,
    /// Map of how text in the buffer should be displayed.
    /// Handles soft wraps, folds, fake inlay text insertions, etc.
    pub display_map: Entity<DisplayMap>,
    placeholder_display_map: Option<Entity<DisplayMap>>,
    pub selections: SelectionsCollection,
    /// Manages the scroll position for the given editor.
    ///
    /// Whenever you want to modify the scroll position of the editor, you should
    /// usually use the existing available APIs as opposed to directly interacting
    /// with the scroll manager.
    pub scroll_manager: ScrollManager,
    columnar_selection_state: Option<ColumnarSelectionState>,
    add_selections_state: Option<AddSelectionsState>,
    select_next_state: Option<SelectNextState>,
    select_prev_state: Option<SelectNextState>,
    selection_history: SelectionHistory,
    defer_selection_effects: bool,
    deferred_selection_effects_state: Option<DeferredSelectionEffectsState>,
    autoclose_regions: Vec<AutocloseRegion>,
    snippet_stack: InvalidationStack<SnippetState>,
    select_syntax_node_history: SelectSyntaxNodeHistory,
    ime_transaction: Option<TransactionId>,
    pub diagnostics_max_severity: DiagnosticSeverity,
    active_diagnostics: ActiveDiagnostic,
    inline_diagnostics_update: Task<()>,
    inline_diagnostics_enabled: bool,
    diagnostics_enabled: bool,
    word_completions_enabled: bool,
    inline_diagnostics: Vec<(Anchor, InlineDiagnostic)>,
    soft_wrap_mode_override: Option<bool>,
    hard_wrap: Option<usize>,
    project: Option<Entity<Project>>,
    semantics_provider: Option<Rc<dyn SemanticsProvider>>,
    completion_provider: Option<Rc<dyn CompletionProvider>>,

    blink_manager: Entity<BlinkManager>,
    cursor_motions: CursorMotionStates,

    pub show_local_selections: bool,
    mode: EditorMode,
    show_gutter: bool,
    show_horizontal_scrollbar: bool,
    minimap_visibility: MinimapVisibility,
    offset_content: bool,
    disable_expand_excerpt_buttons: bool,
    delegate_expand_excerpts: bool,
    delegate_open_excerpts: bool,
    side_card: Option<gpui::AnyView>,
    enable_lsp_data: bool,
    needs_initial_data_update: bool,
    enable_runnables: bool,
    search_results_hold: Option<SearchResultsHold>,
    show_line_numbers: bool,
    show_git_diff_gutter: Option<bool>,
    show_code_actions: Option<bool>,
    show_runnables: Option<bool>,
    show_wrap_guides: Option<bool>,
    show_indent_guides: Option<bool>,
    buffers_with_disabled_indent_guides: FxHashSet<BufferId>,
    highlight_order: usize,
    highlighted_rows: HashMap<TypeId, Vec<RowHighlight>, TypeIdHashBuilder>,
    background_highlights: FxHashMap<HighlightKey, BackgroundHighlight>,
    navigation_overlays: FxHashMap<NavigationOverlayKey, Arc<[NavigationTargetOverlay]>>,
    gutter_highlights: HashMap<TypeId, GutterHighlight, TypeIdHashBuilder>,
    allow_git_diff_scrollbar_markers: bool,
    scrollbar_marker_state: ScrollbarMarkerState,
    active_indent_guides_state: ActiveIndentGuidesState,
    nav_history: Option<ItemNavHistory>,
    context_menu: RefCell<Option<CodeContextMenu>>,
    context_menu_options: Option<ContextMenuOptions>,
    mouse_context_menu: Option<MouseContextMenu>,
    completion_tasks: Vec<(CompletionId, Task<()>)>,
    find_all_references_task_sources: Vec<Anchor>,
    next_completion_id: CompletionId,
    code_actions_for_selection: CodeActionsForSelection,
    runnables_for_selection_toggle: Task<()>,
    quick_selection_highlight_task: Option<(Range<Anchor>, Task<()>)>,
    debounced_selection_highlight_task: Option<(Range<Anchor>, Task<()>)>,
    debounced_selection_highlight_complete: bool,
    document_highlights_task: Option<Task<()>>,
    linked_editing_range_task: Option<Task<Option<()>>>,
    linked_edit_ranges: linked_editing_ranges::LinkedEditingRanges,
    pending_rename: Option<RenameState>,
    searchable: bool,
    cursor_shape: CursorShape,
    /// Whether the cursor is offset one character to the left when something is
    /// selected (needed for vim visual mode).
    cursor_offset_on_selection: bool,
    /// Whether to collapse search match ranges to just their start position.
    /// When true, navigating to a match positions the cursor at the match
    /// without selecting the matched text.
    collapse_matches: bool,
    autoindent_mode: Option<AutoindentMode>,
    workspace: Option<WeakEntity<Workspace>>,
    input_enabled: bool,
    expects_character_input: bool,
    use_modal_editing: bool,
    read_only: bool,

    pub hover_state: HoverState,
    pending_mouse_down: Option<Rc<RefCell<Option<MouseDownEvent>>>>,
    prev_pressure_stage: Option<PressureStage>,
    gutter_hovered: bool,
    hovered_link_state: Option<HoveredLinkState>,

    code_action_providers: Vec<Rc<dyn CodeActionProvider>>,

    /// Used to prevent flickering as the user types while the menu is open.
    show_completions_on_input_override: Option<bool>,

    in_leading_whitespace: bool,
    next_color_inlay_id: usize,
    _subscriptions: Vec<Subscription>,
    pixel_position_of_newest_cursor: Option<gpui::Point<Pixels>>,
    gutter_dimensions: GutterDimensions,
    style: Option<EditorStyle>,
    text_style_refinement: Option<TextStyleRefinement>,
    next_editor_action_id: EditorActionId,
    #[allow(clippy::type_complexity)]
    editor_actions: Rc<RefCell<BTreeMap<EditorActionId, Box<dyn Fn(&Editor, &mut Window, &mut Context<Self>)>>>>,
    use_autoclose: bool,
    use_auto_surround: bool,
    use_selection_highlight: bool,
    auto_replace_emoji_shortcode: bool,
    jsx_tag_auto_close_enabled_in_any_buffer: bool,
    show_git_blame_gutter: bool,
    serialize_buffer: bool,
    blame: Option<Entity<GitBlame>>,
    blame_subscription: Option<Subscription>,
    #[allow(clippy::type_complexity)]
    custom_context_menu: Option<
        Box<
            dyn 'static
                + Fn(&mut Self, DisplayPoint, &mut Window, &mut Context<Self>) -> Option<Entity<ui::ContextMenu>>,
        >,
    >,
    last_bounds: Option<Bounds<Pixels>>,
    last_position_map: Option<Rc<PositionMap>>,
    /// The right margin (vertical scrollbar + minimap width) the editor was
    /// last laid out with, updated on every prepaint.
    /// Used later in the frame by `SplitBufferHeadersElement` to shrink the
    /// width available to buffer headers.
    last_right_margin: Pixels,
    /// Whether the horizontal scrollbar was laid out as visible during the last
    /// prepaint.
    /// Used by `SplitBufferHeadersElement` to clip buffer headers so they don't
    /// paint over the scrollbar.
    last_horizontal_scrollbar_visible: bool,
    expect_bounds_change: Option<Bounds<Pixels>>,
    runnables: RunnableData,
    gutter_hover_button: (Option<GutterHoverButton>, Option<Task<()>>),
    /// Row whose gutter copy button shows the "copied" check, and the task that resets it.
    gutter_copied_row: (Option<DisplayRow>, Option<Task<()>>),
    hovered_diff_hunk_row: Option<DisplayRow>,
    /// One task per buffer and data kind, so refreshing one buffer never cancels another's.
    lsp_data_tasks: FxHashMap<BufferId, FxHashMap<LspDataKind, Shared<Task<()>>>>,
    /// Latest refresh per data kind for a buffer that is not visible.
    offscreen_lsp_data_tasks: FxHashMap<LspDataKind, (BufferId, Task<()>)>,
    in_project_search: bool,
    previous_search_ranges: Option<Arc<[Range<Anchor>]>>,
    focused_block: Option<FocusedBlock>,
    next_scroll_position: NextScrollCursorCenterTopBottom,
    addons: HashMap<TypeId, Box<dyn Addon>, TypeIdHashBuilder>,
    registered_buffers: FxHashMap<BufferId, OpenLspBufferHandle>,
    load_diff_task: Option<Shared<Task<()>>>,
    diff_hunk_renderer: Option<Arc<dyn DiffHunkRenderer>>,
    diff_hunk_action_target: Option<WeakEntity<Editor>>,
    selection_mark_mode: bool,
    toggle_fold_multiple_buffers: Task<()>,
    _scroll_cursor_center_top_bottom_task: Task<()>,
    minimap: Option<Entity<Self>>,
    /// Rows this editor laid out as a minimap, reused across frames.
    minimap_rows: MinimapRowCache,
    pub change_list: ChangeList,
    number_deleted_lines: bool,

    colors: Option<LspColorData>,
    post_scroll_update: Task<()>,
    use_document_folding_ranges: bool,
    inlay_hints: Option<LspInlayHintData>,
    folding_newlines: Task<()>,
    select_next_options: Option<SelectSearchOptions>,
    pub lookup_key: Option<Box<dyn Any + Send + Sync>>,
    #[allow(clippy::type_complexity)]
    on_local_selections_changed: Option<Box<dyn Fn(Point, &mut Window, &mut Context<Self>) + 'static>>,
    suppress_selection_callback: bool,
    applicable_language_settings: FxHashMap<Option<LanguageName>, Arc<LanguageSettings>>,
    semantic_token_state: SemanticTokenState,
    pub(crate) refresh_matching_bracket_highlights_task: Task<()>,
    /// One fetch per buffer, so refreshing one buffer never cancels another's.
    refresh_document_symbols_tasks: FxHashMap<BufferId, Shared<Task<()>>>,
    lsp_document_links: LspDocumentLinks,
    lsp_document_symbols: FxHashMap<BufferId, Vec<OutlineItem<text::Anchor>>>,
    spell_check: Option<spell_check::SpellCheckState>,
}

const fn debounce_value(debounce_ms: u64) -> Option<Duration> {
    if debounce_ms > 0 { Some(Duration::from_millis(debounce_ms)) } else { None }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
enum NextScrollCursorCenterTopBottom {
    #[default]
    Center,
    Top,
    Bottom,
}

impl NextScrollCursorCenterTopBottom {
    const fn next(self) -> Self {
        match self {
            Self::Center => Self::Top,
            Self::Top => Self::Bottom,
            Self::Bottom => Self::Center,
        }
    }
}

#[derive(Clone)]
pub struct EditorSnapshot {
    pub mode: EditorMode,
    show_gutter: bool,
    offset_content: bool,
    sticky_line_number_digits: usize,
    show_line_numbers: bool,
    number_deleted_lines: bool,
    show_git_diff_gutter: Option<bool>,
    show_code_actions: Option<bool>,
    show_runnables: Option<bool>,
    git_blame_gutter_max_author_length: Option<usize>,
    pub display_snapshot: DisplaySnapshot,
    pub placeholder_display_snapshot: Option<DisplaySnapshot>,
    is_focused: bool,
    scroll_anchor: SharedScrollAnchor,
    gutter_hovered: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NavigationTargetOverlay {
    pub target_range: Range<Anchor>,
    pub label: NavigationOverlayLabel,
    pub covered_text_range: Option<Range<Anchor>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NavigationOverlayLabel {
    pub text: SharedString,
    pub text_color: Hsla,
    pub x_offset: Pixels,
    pub scale_factor: f32,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct GutterDimensions {
    pub left_padding: Pixels,
    pub right_padding: Pixels,
    pub width: Pixels,
    pub margin: Pixels,
    pub git_blame_entries_width: Option<Pixels>,
}

impl GutterDimensions {
    fn default_with_margin(font_id: FontId, font_size: Pixels, cx: &App) -> Self {
        Self { margin: Self::default_gutter_margin(font_id, font_size, cx), ..Default::default() }
    }

    fn default_gutter_margin(font_id: FontId, font_size: Pixels, cx: &App) -> Pixels {
        -cx.text_system().descent(font_id, font_size)
    }
    /// The full width of the space taken up by the gutter.
    pub fn full_width(&self) -> Pixels {
        self.margin + self.width
    }
}

struct CharacterDimensions {
    em_width: Pixels,
    em_advance: Pixels,
    line_height: Pixels,
}

#[derive(Clone, Debug)]
struct SelectionHistoryEntry {
    selections: Arc<[Selection<Anchor>]>,
    select_next_state: Option<SelectNextState>,
    select_prev_state: Option<SelectNextState>,
    add_selections_state: Option<AddSelectionsState>,
}

#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
enum SelectionHistoryMode {
    #[default]
    Normal,
    Undoing,
    Redoing,
    Skipping,
}

#[derive(Debug)]
/// SelectionEffects controls the side-effects of updating the selection.
///
/// The default behaviour does "what you mostly want":
/// - it pushes to the nav history if the cursor moved by >10 lines
/// - it re-triggers completion requests
/// - it scrolls to fit
///
/// You might want to modify these behaviours. For example when doing a "jump"
/// like go to definition, we always want to add to nav history; but when scrolling
/// in vim mode we never do.
///
/// Similarly, you might want to disable scrolling if you don't want the viewport to
/// move. Explicit jumps can opt into revealing folded destinations with [`Self::unfold`].
#[derive(Clone)]
pub struct SelectionEffects {
    nav_history: Option<bool>,
    completions: bool,
    scroll: Option<Autoscroll>,
    unfold: bool,
}

impl Default for SelectionEffects {
    fn default() -> Self {
        Self { nav_history: None, completions: true, scroll: Some(Autoscroll::fit()), unfold: false }
    }
}
impl SelectionEffects {
    /// Reveals the selected ranges and their folded buffers before scrolling.
    /// Also applies when selections are unchanged; deferred changes reveal the final selections.
    pub const fn unfold(self) -> Self {
        Self { unfold: true, ..self }
    }

    pub fn scroll(scroll: Autoscroll) -> Self {
        Self { scroll: Some(scroll), ..Default::default() }
    }

    pub fn no_scroll() -> Self {
        Self { scroll: None, ..Default::default() }
    }

    pub const fn completions(self, completions: bool) -> Self {
        Self { completions, ..self }
    }

    pub const fn nav_history(self, nav_history: bool) -> Self {
        Self { nav_history: Some(nav_history), ..self }
    }
}

struct DeferredSelectionEffectsState {
    changed: bool,
    effects: SelectionEffects,
    old_cursor_position: Anchor,
    history_entry: SelectionHistoryEntry,
}

#[derive(Clone, Debug)]
pub struct TransactionSelections {
    pub undo: Arc<[Selection<Anchor>]>,
    pub redo: Option<Arc<[Selection<Anchor>]>>,
    undo_add_selections_state: Option<AddSelectionsState>,
    redo_add_selections_state: Option<AddSelectionsState>,
}

#[derive(Default)]
struct SelectionHistory {
    selections_by_transaction: FxHashMap<TransactionId, TransactionSelections>,
    mode: SelectionHistoryMode,
    undo_stack: VecDeque<SelectionHistoryEntry>,
    redo_stack: VecDeque<SelectionHistoryEntry>,
}

impl SelectionHistory {
    #[track_caller]
    fn insert_transaction(
        &mut self,
        transaction_id: TransactionId,
        selections: Arc<[Selection<Anchor>]>,
        add_selections_state: Option<AddSelectionsState>,
    ) {
        if selections.is_empty() {
            log::error!(
                "SelectionHistory::insert_transaction called with empty selections. Caller: {}",
                std::panic::Location::caller()
            );
            return;
        }
        self.selections_by_transaction.insert(
            transaction_id,
            TransactionSelections {
                undo: selections,
                redo: None,
                undo_add_selections_state: add_selections_state,
                redo_add_selections_state: None,
            },
        );
    }

    fn transaction(&self, transaction_id: TransactionId) -> Option<&TransactionSelections> {
        self.selections_by_transaction.get(&transaction_id)
    }

    fn transaction_mut(&mut self, transaction_id: TransactionId) -> Option<&mut TransactionSelections> {
        self.selections_by_transaction.get_mut(&transaction_id)
    }

    fn push(&mut self, entry: SelectionHistoryEntry) {
        if !entry.selections.is_empty() {
            match self.mode {
                SelectionHistoryMode::Normal => {
                    self.push_undo(entry);
                    self.redo_stack.clear();
                }
                SelectionHistoryMode::Undoing => self.push_redo(entry),
                SelectionHistoryMode::Redoing => self.push_undo(entry),
                SelectionHistoryMode::Skipping => {}
            }
        }
    }

    fn push_undo(&mut self, entry: SelectionHistoryEntry) {
        if self.undo_stack.back().is_none_or(|e| e.selections != entry.selections) {
            self.undo_stack.push_back(entry);
            if self.undo_stack.len() > MAX_SELECTION_HISTORY_LEN {
                self.undo_stack.pop_front();
            }
        }
    }

    fn push_redo(&mut self, entry: SelectionHistoryEntry) {
        if self.redo_stack.back().is_none_or(|e| e.selections != entry.selections) {
            self.redo_stack.push_back(entry);
            if self.redo_stack.len() > MAX_SELECTION_HISTORY_LEN {
                self.redo_stack.pop_front();
            }
        }
    }
}

#[derive(Clone, Copy)]
pub struct RowHighlightOptions {
    pub autoscroll: bool,
    pub include_gutter: bool,
}

impl Default for RowHighlightOptions {
    fn default() -> Self {
        Self { autoscroll: Default::default(), include_gutter: true }
    }
}

struct RowHighlight {
    index: usize,
    range: Range<Anchor>,
    color: fn(&App) -> Hsla,
    options: RowHighlightOptions,
    type_id: TypeId,
}

#[derive(Clone, Debug)]
struct AddSelectionsState {
    groups: Vec<AddSelectionsGroup>,
    skip_soft_wrap: bool,
}

#[derive(Clone, Debug)]
struct AddSelectionsGroup {
    above: bool,
    stack: Vec<usize>,
    goal_source: Option<Range<Anchor>>,
}

#[derive(Clone)]
struct SelectNextState {
    query: AhoCorasick,
    wordwise: bool,
    done: bool,
}

impl Debug for SelectNextState {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct(any::type_name::<Self>()).field("wordwise", &self.wordwise).field("done", &self.done).finish()
    }
}

#[derive(Debug)]
struct AutocloseRegion {
    selection_id: usize,
    range: Range<Anchor>,
    pair: BracketPair,
}

#[derive(Debug)]
struct SnippetState {
    ranges: Vec<Vec<Range<Anchor>>>,
    active_index: usize,
    choices: Vec<Option<Vec<String>>>,
}

pub struct RenameTarget {
    range: Range<text::Anchor>,
    language_server_id: Option<LanguageServerId>,
}

pub struct RenameState {
    pub range: Range<Anchor>,
    pub old_name: Arc<str>,
    pub editor: Entity<Editor>,
    language_server_id: Option<LanguageServerId>,
    block_id: CustomBlockId,
}

struct InvalidationStack<T>(Vec<T>);

// selections, scroll behavior, was newest selection reversed
type SelectSyntaxNodeHistoryState = (Box<[Selection<Anchor>]>, SelectSyntaxNodeScrollBehavior, bool);

#[derive(Default)]
struct SelectSyntaxNodeHistory {
    stack: Vec<SelectSyntaxNodeHistoryState>,
    // disable temporarily to allow changing selections without losing the stack
    pub disable_clearing: bool,
}

impl SelectSyntaxNodeHistory {
    pub(crate) fn try_clear(&mut self) {
        if !self.disable_clearing {
            self.stack.clear();
        }
    }

    pub(crate) fn push(&mut self, selection: SelectSyntaxNodeHistoryState) {
        self.stack.push(selection);
    }

    pub(crate) fn pop(&mut self) -> Option<SelectSyntaxNodeHistoryState> {
        self.stack.pop()
    }
}

enum SelectSyntaxNodeScrollBehavior {
    CursorTop,
    FitSelection,
    CursorBottom,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct NavigationData {
    cursor_anchor: Anchor,
    cursor_position: Point,
    scroll_anchor: ScrollAnchor,
    scroll_top_row: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GotoDefinitionKind {
    Symbol,
    Declaration,
    Type,
    Implementation,
}

pub(crate) struct FocusedBlock {
    id: BlockId,
    focus_handle: WeakFocusHandle,
}

#[derive(Clone, Debug)]
pub enum JumpData {
    MultiBufferRow { row: MultiBufferRow, line_offset_from_top: u32 },
    MultiBufferPoint { anchor: language::Anchor, position: Point, line_offset_from_top: u32 },
}

pub enum MultibufferSelectionMode {
    First,
    All,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RewrapOptions {
    pub override_language_settings: bool,
    pub preserve_existing_whitespace: bool,
    pub line_length: Option<usize>,
}

impl Editor {
    pub fn single_line(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let buffer = cx.new(|cx| Buffer::local("", cx));
        let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
        Self::new(EditorMode::SingleLine, buffer, None, window, cx)
    }

    pub fn multi_line(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let buffer = cx.new(|cx| Buffer::local("", cx));
        let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
        Self::new(EditorMode::full(), buffer, None, window, cx)
    }

    pub fn auto_height(min_lines: usize, max_lines: usize, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let buffer = cx.new(|cx| Buffer::local("", cx));
        let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
        Self::new(EditorMode::AutoHeight { min_lines, max_lines: Some(max_lines) }, buffer, None, window, cx)
    }

    pub fn for_buffer(
        buffer: Entity<Buffer>,
        project: Option<Entity<Project>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
        Self::new(EditorMode::full(), buffer, project, window, cx)
    }

    pub fn for_multibuffer(
        buffer: Entity<MultiBuffer>,
        project: Option<Entity<Project>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new(EditorMode::full(), buffer, project, window, cx)
    }

    pub fn clone(&self, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut clone = Self::new(self.mode.clone(), self.buffer.clone(), self.project.clone(), window, cx);
        let my_snapshot = self.display_map.update(cx, |display_map, cx| {
            let snapshot = display_map.snapshot(cx);
            clone.display_map.update(cx, |display_map, cx| {
                display_map.set_state(&snapshot, cx);
            });
            snapshot
        });
        let clone_snapshot = clone.display_map.update(cx, |map, cx| map.snapshot(cx));
        clone.folds_did_change(cx);
        clone.selections.clone_state(&self.selections);
        clone.scroll_manager.clone_state(&self.scroll_manager, &my_snapshot, &clone_snapshot, cx);
        clone.searchable = self.searchable;
        clone.read_only = self.read_only;
        clone.buffers_with_disabled_indent_guides.clone_from(&self.buffers_with_disabled_indent_guides);
        clone.enable_lsp_data = self.enable_lsp_data;
        clone.needs_initial_data_update = self.enable_lsp_data;
        clone.enable_runnables = self.enable_runnables;
        clone
    }

    pub fn new(
        mode: EditorMode,
        buffer: Entity<MultiBuffer>,
        project: Option<Entity<Project>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Editor::new_internal(mode, buffer, project, None, window, cx)
    }

    fn new_internal(
        mode: EditorMode,
        multi_buffer: Entity<MultiBuffer>,
        project: Option<Entity<Project>>,
        display_map: Option<Entity<DisplayMap>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let full_mode = mode.is_full();
        let is_minimap = mode.is_minimap();
        let diagnostics_max_severity = if full_mode { DiagnosticSeverity::Hint } else { DiagnosticSeverity::Off };
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let editor = cx.entity().downgrade();
        let fold_placeholder = FoldPlaceholder {
            constrain_width: false,
            render: Arc::new(move |fold_id, fold_range, cx| {
                let editor = editor.clone();
                FoldPlaceholder::fold_element(fold_id, cx)
                    .cursor_pointer()
                    .child("⋯")
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |_, _window, cx| {
                        editor
                            .update(cx, |editor, cx| {
                                editor.unfold_ranges(std::slice::from_ref(&fold_range), true, false, cx);
                                cx.stop_propagation();
                            })
                            .ok();
                    })
                    .into_any()
            }),
            merge_adjacent: true,
            ..FoldPlaceholder::default()
        };
        let display_map = display_map.unwrap_or_else(|| {
            cx.new(|cx| {
                DisplayMap::new(
                    multi_buffer.clone(),
                    style.font(),
                    font_size,
                    None,
                    FILE_HEADER_HEIGHT,
                    MULTI_BUFFER_EXCERPT_HEADER_HEIGHT,
                    fold_placeholder,
                    diagnostics_max_severity,
                    cx,
                )
            })
        });

        let selections = SelectionsCollection::new();

        let blink_manager = cx.new(|cx| {
            let mut blink_manager = BlinkManager::new(CURSOR_BLINK_INTERVAL, true);
            if is_minimap {
                blink_manager.disable(cx);
            }
            blink_manager
        });

        let soft_wrap_mode_override = matches!(mode, EditorMode::SingleLine).then_some(false);

        let mut project_subscriptions = Vec::new();
        if full_mode && let Some(project) = project.as_ref() {
            project_subscriptions.push(cx.subscribe_in(project, window, |editor, _, event, window, cx| match event {
                project::Event::RefreshDocumentColors { .. } => {
                    editor.refresh_document_colors(None, window, cx);
                }
                project::Event::RefreshDocumentLinks { .. } => {
                    editor.refresh_document_links(None, cx);
                }
                project::Event::RefreshDocumentHighlights { server_id } => {
                    editor.refresh_document_highlights_for_server(*server_id, cx);
                }
                project::Event::RefreshFoldingRanges { .. } => {
                    editor.refresh_folding_ranges(None, window, cx);
                }
                project::Event::RefreshDocumentSymbols { .. } => {
                    editor.refresh_document_symbols(None, cx);
                }
                project::Event::RefreshInlayHints { server_id } => {
                    editor.refresh_inlay_hints(InlayHintRefreshReason::RefreshRequested { server_id: *server_id }, cx);
                    // A server asks for this once it has loaded the project, which is
                    // when its runnables become available.
                    editor.refresh_lsp_runnables(window, cx);
                }
                project::Event::RefreshSemanticTokens { .. } => {
                    editor.refresh_semantic_tokens(None, true, cx);
                }
                project::Event::LanguageServerRemoved(_) => {
                    editor.registered_buffers.clear();
                    editor.register_visible_buffers(cx);
                    editor.invalidate_semantic_tokens(None);
                    editor.refresh_runnables(None, window, cx);
                    editor.update_lsp_data(None, window, cx);
                    editor.refresh_inlay_hints(InlayHintRefreshReason::ServerRemoved, cx);
                    editor.refresh_document_highlights(cx);
                }
                project::Event::SnippetEdit(id, snippet_edits) => {
                    // todo(lw): Non singletons
                    if let Some(buffer) = editor.buffer.read(cx).as_singleton() {
                        let snapshot = buffer.read(cx).snapshot();
                        let focus_handle = editor.focus_handle(cx);
                        if snapshot.buffer_id() == *id && focus_handle.is_focused(window) {
                            for (range, snippet) in snippet_edits {
                                let buffer_range = language::range_from_lsp(*range).to_offset(&snapshot);
                                editor.insert_snippet(
                                    &[MultiBufferOffset(buffer_range.start)..MultiBufferOffset(buffer_range.end)],
                                    snippet.clone(),
                                    window,
                                    cx,
                                );
                            }
                        }
                    }
                }
                project::Event::LanguageServerBufferRegistered { buffer_id, .. } => {
                    let buffer_id = *buffer_id;
                    if editor.buffer().read(cx).buffer(buffer_id).is_some() {
                        editor.register_buffer(buffer_id, cx);
                        editor.refresh_runnables(Some(buffer_id), window, cx);
                        editor.invalidate_semantic_tokens(Some(buffer_id));
                        editor.update_lsp_data(Some(buffer_id), window, cx);
                        editor.refresh_inlay_hints(InlayHintRefreshReason::LanguageServerRegistered, cx);
                        refresh_linked_ranges(editor, window, cx);
                        editor.refresh_code_actions_for_selection(window, cx);
                        editor.refresh_document_highlights(cx);
                    }
                }

                project::Event::EntryRenamed { transaction, new_project_path, new_abs_path, .. } => {
                    let Some(workspace) = editor.workspace() else {
                        return;
                    };
                    let Some(active_editor) = workspace.read(cx).active_item_as::<Self>(cx) else {
                        return;
                    };

                    if active_editor.entity_id() == cx.entity_id() {
                        let entity_id = cx.entity_id();
                        workspace.update(cx, |this, cx| {
                            this.panes_mut().iter_mut().filter(|pane| pane.entity_id() != entity_id).for_each(|p| {
                                p.update(cx, |pane, _| {
                                    pane.nav_history_mut().rename_item(
                                        entity_id,
                                        new_project_path.clone(),
                                        new_abs_path.clone().into(),
                                    );
                                });
                            });
                        });

                        Self::open_transaction_for_hidden_buffers(
                            workspace,
                            transaction.clone(),
                            "Rename".to_owned(),
                            window,
                            cx,
                        );
                    }
                }

                project::Event::WorkspaceEditApplied(transaction) => {
                    let Some(workspace) = editor.workspace() else {
                        return;
                    };
                    let Some(active_editor) = workspace.read(cx).active_item_as::<Self>(cx) else {
                        return;
                    };

                    if active_editor.entity_id() == cx.entity_id() {
                        Self::open_transaction_for_hidden_buffers(
                            workspace,
                            transaction.clone(),
                            "LSP Edit".to_owned(),
                            window,
                            cx,
                        );
                    }
                }

                _ => {}
            }));

            let git_store = project.read(cx).git_store().clone();
            let project = project.clone();
            project_subscriptions.push(cx.subscribe(&git_store, move |this, _, event, cx| {
                if !matches!(event, GitStoreEvent::RepositoryAdded) {
                    return;
                }
                let buffers = this.buffer.read(cx).all_buffers();
                if buffers.is_empty() {
                    return;
                }
                let task = this.update_uncommitted_diff_for_buffer(&project, buffers, cx);
                this.load_diff_task = Some(task.shared());
            }));
        }

        let focus_handle = cx.focus_handle();
        if !is_minimap {
            cx.on_focus(&focus_handle, window, Self::handle_focus).detach();
            cx.on_focus_in(&focus_handle, window, Self::handle_focus_in).detach();
            cx.on_focus_out(&focus_handle, window, Self::handle_focus_out).detach();
            cx.on_blur(&focus_handle, window, Self::handle_blur).detach();
            cx.observe_pending_input(window, Self::observe_pending_input).detach();
        }

        let show_indent_guides = matches!(mode, EditorMode::SingleLine | EditorMode::Minimap { .. }).then_some(false);

        let mut code_action_providers = Vec::new();
        if let Some(project) = project.clone() {
            code_action_providers.push(Rc::new(project) as Rc<_>);
        }
        let spell_check = spell_check::SpellCheckState::for_mode(&mode);
        if let Some(spell_check) = &spell_check {
            code_action_providers
                .push(Rc::new(spell_check.code_actions(cx.entity().downgrade(), project.is_some())) as Rc<_>);
        }

        let mut editor = Self {
            focus_handle,
            last_focused_descendant: None,
            buffer: multi_buffer.clone(),
            display_map: display_map.clone(),
            placeholder_display_map: None,
            selections,
            scroll_manager: ScrollManager::new(cx),
            columnar_selection_state: None,
            add_selections_state: None,
            select_next_state: None,
            select_prev_state: None,
            selection_history: SelectionHistory::default(),
            defer_selection_effects: false,
            deferred_selection_effects_state: None,
            autoclose_regions: Vec::new(),
            snippet_stack: InvalidationStack::default(),
            select_syntax_node_history: SelectSyntaxNodeHistory::default(),
            ime_transaction: None,
            active_diagnostics: ActiveDiagnostic::None,
            inline_diagnostics_update: Task::ready(()),
            inline_diagnostics: Vec::new(),
            soft_wrap_mode_override,
            diagnostics_max_severity,
            hard_wrap: None,
            completion_provider: project
                .clone()
                .map(|project| Rc::new(project) as Rc<dyn completions::CompletionProvider>),
            semantics_provider: project
                .as_ref()
                .map(|project| Rc::new(project.downgrade()) as Rc<dyn SemanticsProvider>),

            project,
            blink_manager: blink_manager.clone(),
            cursor_motions: CursorMotionStates::default(),
            show_local_selections: true,
            show_horizontal_scrollbar: full_mode,
            minimap_visibility: MinimapVisibility::for_mode(&mode),
            offset_content: !matches!(mode, EditorMode::SingleLine),
            show_gutter: full_mode,
            search_results_hold: None,
            show_line_numbers: full_mode,
            disable_expand_excerpt_buttons: !full_mode,
            delegate_expand_excerpts: false,
            delegate_open_excerpts: false,
            side_card: None,
            enable_lsp_data: full_mode,
            needs_initial_data_update: full_mode,
            enable_runnables: full_mode,
            show_git_diff_gutter: None,
            show_code_actions: None,
            show_runnables: None,
            show_wrap_guides: None,
            show_indent_guides,
            buffers_with_disabled_indent_guides: FxHashSet::default(),
            highlight_order: 0,
            highlighted_rows: HashMap::default(),
            background_highlights: FxHashMap::default(),
            navigation_overlays: FxHashMap::default(),
            gutter_highlights: HashMap::default(),
            allow_git_diff_scrollbar_markers: false,
            scrollbar_marker_state: ScrollbarMarkerState::default(),
            active_indent_guides_state: ActiveIndentGuidesState::default(),
            nav_history: None,
            context_menu: RefCell::new(None),
            context_menu_options: None,
            mouse_context_menu: None,
            completion_tasks: Vec::new(),
            find_all_references_task_sources: Vec::new(),
            next_completion_id: 0,
            code_action_providers,
            code_actions_for_selection: CodeActionsForSelection::None,
            runnables_for_selection_toggle: Task::ready(()),
            quick_selection_highlight_task: None,
            debounced_selection_highlight_task: None,
            debounced_selection_highlight_complete: false,
            document_highlights_task: None,
            linked_editing_range_task: None,
            pending_rename: None,
            searchable: !is_minimap,
            cursor_shape: CursorShape::Bar,
            cursor_offset_on_selection: false,
            autoindent_mode: Some(AutoindentMode::EachLine),
            collapse_matches: false,
            workspace: None,
            input_enabled: !is_minimap,
            expects_character_input: !is_minimap,
            use_modal_editing: full_mode,
            read_only: is_minimap,
            use_autoclose: true,
            use_auto_surround: true,
            use_selection_highlight: true,
            auto_replace_emoji_shortcode: false,
            jsx_tag_auto_close_enabled_in_any_buffer: false,

            hover_state: HoverState::default(),
            pending_mouse_down: None,
            prev_pressure_stage: None,
            hovered_link_state: None,

            inline_diagnostics_enabled: full_mode,
            diagnostics_enabled: full_mode,
            word_completions_enabled: full_mode,
            gutter_hovered: false,
            pixel_position_of_newest_cursor: None,
            last_bounds: None,
            last_position_map: None,
            last_right_margin: Pixels::ZERO,
            last_horizontal_scrollbar_visible: false,
            expect_bounds_change: None,
            gutter_dimensions: GutterDimensions::default(),
            style: None,

            next_editor_action_id: EditorActionId::default(),
            editor_actions: Rc::default(),

            show_completions_on_input_override: None,

            in_leading_whitespace: false,
            custom_context_menu: None,
            show_git_blame_gutter: false,
            serialize_buffer: !is_minimap,
            blame: None,
            blame_subscription: None,

            gutter_hover_button: (None, None),
            gutter_copied_row: (None, None),
            hovered_diff_hunk_row: None,
            _subscriptions: if is_minimap {
                Vec::default()
            } else {
                {
                    vec![
                        cx.observe(&multi_buffer, Self::on_buffer_changed),
                        cx.subscribe_in(&multi_buffer, window, Self::on_buffer_event),
                        cx.observe_in(&display_map, window, Self::on_display_map_changed),
                        cx.observe(&blink_manager, |_, _, cx| cx.notify()),
                        observe_buffer_font_size_adjustment(cx, |_, cx| cx.notify()),
                    ]
                }
            },
            runnables: RunnableData::new(),
            lsp_data_tasks: FxHashMap::default(),
            offscreen_lsp_data_tasks: FxHashMap::default(),
            colors: None,
            use_document_folding_ranges: false,
            inlay_hints: None,
            next_color_inlay_id: 0,
            post_scroll_update: Task::ready(()),
            linked_edit_ranges: linked_editing_ranges::LinkedEditingRanges::default(),
            in_project_search: false,
            previous_search_ranges: None,
            focused_block: None,
            next_scroll_position: NextScrollCursorCenterTopBottom::default(),
            addons: HashMap::default(),
            registered_buffers: FxHashMap::default(),
            _scroll_cursor_center_top_bottom_task: Task::ready(()),
            selection_mark_mode: false,
            toggle_fold_multiple_buffers: Task::ready(()),
            text_style_refinement: None,
            load_diff_task: None,
            diff_hunk_renderer: None,
            diff_hunk_action_target: None,
            minimap: None,
            minimap_rows: MinimapRowCache::default(),
            change_list: ChangeList::new(),
            mode,
            folding_newlines: Task::ready(()),
            lookup_key: None,
            select_next_options: None,
            on_local_selections_changed: None,
            suppress_selection_callback: false,
            applicable_language_settings: FxHashMap::default(),
            semantic_token_state: SemanticTokenState::new(),
            number_deleted_lines: false,
            refresh_matching_bracket_highlights_task: Task::ready(()),
            refresh_document_symbols_tasks: FxHashMap::default(),
            lsp_document_links: LspDocumentLinks::default(),
            lsp_document_symbols: FxHashMap::default(),
            spell_check,
        };

        if let Some(project) = editor.project.clone() {
            editor.load_diff_task = Some(
                editor.update_uncommitted_diff_for_buffer(&project, multi_buffer.read(cx).all_buffers(), cx).shared(),
            );
        }

        if is_minimap {
            return editor;
        }

        editor.applicable_language_settings = editor.fetch_applicable_language_settings(cx);

        editor._subscriptions.extend(project_subscriptions);

        editor._subscriptions.push(cx.subscribe_in(&cx.entity(), window, |editor, _, e: &EditorEvent, window, cx| {
            match e {
                EditorEvent::ScrollPositionChanged { local, .. } => {
                    if *local {
                        let snapshot = editor.snapshot(window, cx);
                        let new_anchor = editor.scroll_manager.native_anchor(&snapshot.display_snapshot, cx);
                        editor.update_restoration_data(cx, move |data| {
                            data.scroll_position = (new_anchor.top_row(snapshot.buffer_snapshot()), new_anchor.offset);
                        });
                    }
                    editor.update_data_on_scroll(true, window, cx);
                }
                EditorEvent::Edited { .. } => {
                    let display_map = editor.display_snapshot(cx);
                    let selections = editor.selections.all_adjusted_display(&display_map);
                    let pop_state = editor.change_list.last().is_some_and(|previous| {
                        previous.len() == selections.len()
                            && previous
                                .iter()
                                .enumerate()
                                .all(|(ix, p)| p.to_display_point(&display_map).row() == selections[ix].head().row())
                    });
                    let new_positions = selections
                        .into_iter()
                        .map(|s| display_map.display_point_to_anchor(s.head(), Bias::Left))
                        .collect();
                    editor.change_list.push_to_change_list(pop_state, new_positions);
                }
                _ => (),
            }
        }));

        // skip adding the initial selection to selection history
        editor.selection_history.mode = SelectionHistoryMode::Skipping;
        editor.end_selection(window, cx);
        editor.selection_history.mode = SelectionHistoryMode::Normal;

        editor.scroll_manager.show_scrollbars(window, cx);
        jsx_tag_auto_close::refresh_enabled_in_any_buffer(&mut editor, &multi_buffer, cx);

        if full_mode {
            let should_auto_hide_scrollbars = cx.should_auto_hide_scrollbars();
            cx.set_global(ScrollbarAutoHide(should_auto_hide_scrollbars));

            editor.minimap = editor.create_minimap(window, cx);
            editor.colors = Some(LspColorData::new());
            editor.use_document_folding_ranges = true;
            editor.inlay_hints = Some(LspInlayHintData::new(INLAY_HINTS));

            if let Some(buffer) = multi_buffer.read(cx).as_singleton() {
                editor.register_buffer(buffer.read(cx).buffer_id(), cx);
            }
        }

        editor
    }

    pub fn display_snapshot(&self, cx: &mut App) -> DisplaySnapshot {
        self.display_map.update(cx, |map, cx| map.snapshot(cx))
    }

    pub fn deploy_mouse_context_menu(
        &mut self,
        position: gpui::Point<Pixels>,
        context_menu: Entity<ContextMenu>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.mouse_context_menu =
            Some(MouseContextMenu::new(self, MenuPosition::PinnedToScreen(position), context_menu, window, cx));
    }

    pub fn key_context(&self, window: &mut Window, cx: &mut App) -> KeyContext {
        self.key_context_internal(window, cx)
    }

    fn key_context_internal(&self, window: &Window, cx: &mut App) -> KeyContext {
        let mut key_context = KeyContext::new_with_defaults();
        key_context.add("Editor");
        let mode = match self.mode {
            EditorMode::SingleLine => "single_line",
            EditorMode::AutoHeight { .. } => "auto_height",
            EditorMode::Minimap { .. } => "minimap",
            EditorMode::Full { .. } => "full",
        };

        key_context.set("mode", mode);
        if self.pending_rename.is_some() {
            key_context.add("renaming");
        }

        if let Some(snippet_stack) = self.snippet_stack.last() {
            key_context.add("in_snippet");

            if snippet_stack.active_index > 0 {
                key_context.add("has_previous_tabstop");
            }

            if snippet_stack.active_index < snippet_stack.ranges.len().saturating_sub(1) {
                key_context.add("has_next_tabstop");
            }
        }

        match self.context_menu.borrow().as_ref() {
            Some(CodeContextMenu::Completions(menu)) if menu.visible() => {
                key_context.add("menu");
                key_context.add("showing_completions");
            }
            Some(CodeContextMenu::CodeActions(menu)) if menu.visible() => {
                key_context.add("menu");
                key_context.add("showing_code_actions");
            }
            _ => {}
        }

        if let Some(singleton_buffer) = self.buffer.read(cx).as_singleton() {
            if let Some(extension) = singleton_buffer
                .read(cx)
                .file()
                .and_then(|file| Some(file.full_path(cx).extension()?.to_string_lossy().to_lowercase()))
            {
                key_context.set("extension", extension);
            }
        } else {
            key_context.add("multibuffer");
        }

        if self.in_leading_whitespace {
            key_context.add("in_leading_whitespace");
        }

        if self.selection_mark_mode {
            key_context.add("selection_mode");
        }

        let disjoint = self.selections.disjoint_anchors();
        if disjoint.len() > 1 {
            key_context.add("multiple_selections");
        }
        if matches!(&self.mode, EditorMode::SingleLine | EditorMode::AutoHeight { .. })
            && let [selection] = disjoint
            && selection.start == selection.end
        {
            let snapshot = self.snapshot(window, cx);
            let snapshot = snapshot.buffer_snapshot();
            let caret_offset = selection.end.to_offset(snapshot);

            if caret_offset == MultiBufferOffset(0) {
                key_context.add("start_of_input");
            }

            if caret_offset == snapshot.len() {
                key_context.add("end_of_input");
            }
        }

        if self.has_any_expanded_diff_hunks(cx) {
            key_context.add("diffs_expanded");
        }

        if self.nav_history.as_ref().is_some_and(ItemNavHistory::is_preview_item) {
            key_context.add("in_preview");
        }

        key_context
    }

    pub const fn last_bounds(&self) -> Option<&Bounds<Pixels>> {
        self.last_bounds.as_ref()
    }

    pub(crate) const fn last_right_margin(&self) -> Pixels {
        self.last_right_margin
    }

    pub(crate) const fn last_horizontal_scrollbar_visible(&self) -> bool {
        self.last_horizontal_scrollbar_visible
    }

    pub const fn buffer(&self) -> &Entity<MultiBuffer> {
        &self.buffer
    }

    pub const fn project(&self) -> Option<&Entity<Project>> {
        self.project.as_ref()
    }

    pub fn workspace(&self) -> Option<Entity<Workspace>> {
        self.workspace.as_ref()?.upgrade()
    }

    /// Detaches a task and shows an error notification in the workspace if available,
    /// otherwise just logs the error.
    pub fn detach_and_notify_err<R, E>(&self, task: Task<Result<R, E>>, window: &mut Window, cx: &mut App)
    where
        E: Debug + fmt::Display + 'static,
        R: 'static,
    {
        if let Some(workspace) = self.workspace() {
            task.detach_and_notify_err(workspace.downgrade(), window, cx);
        } else {
            task.detach_and_log_err(cx);
        }
    }

    pub fn title<'a>(&self, cx: &'a App) -> Cow<'a, str> {
        self.buffer().read(cx).title(cx)
    }

    pub fn language_at<T: ToOffset>(&self, point: T, cx: &App) -> Option<Arc<Language>> {
        self.buffer.read(cx).language_at(point, cx)
    }

    pub fn file_at<T: ToOffset>(&self, point: T, cx: &App) -> Option<Arc<dyn language::File>> {
        self.buffer.read(cx).read(cx).file_at(point).cloned()
    }

    pub fn active_buffer(&self, cx: &App) -> Option<Entity<Buffer>> {
        let multibuffer = self.buffer.read(cx);
        let snapshot = multibuffer.snapshot(cx);
        let (anchor, _) = snapshot.anchor_to_buffer_anchor(self.selections.newest_anchor().head())?;
        multibuffer.buffer(anchor.buffer_id)
    }

    pub const fn mode(&self) -> &EditorMode {
        &self.mode
    }

    pub fn set_mode(&mut self, mode: EditorMode) {
        self.mode = mode;
    }

    pub const fn set_in_project_search(&mut self, in_project_search: bool) {
        self.in_project_search = in_project_search;
    }

    pub fn set_search_results_status(&mut self, status: SearchResultsStatus, cx: &mut Context<Self>) {
        let hold = self.search_results_hold.get_or_insert_default();
        let previous = mem::replace(&mut hold.status, status);
        let results_became_current = previous.results_stale && !status.results_stale;
        let search_settled = previous.pending && !status.pending;
        let allow_shrink = !status.results_stale
            && (self.buffer.read(cx).read(cx).is_empty()
                || status.query_confirmed && (results_became_current || search_settled));
        self.fit_gutter_line_number_width(allow_shrink, cx);
        if previous != status {
            cx.notify();
        }
    }

    /// Shrinks the sticky gutter width down to fit the current content and keeps latching from
    /// there, so a settled search snaps to its real width instead of staying stuck at the widest
    /// line number seen mid-typing.
    fn fit_gutter_line_number_width(&mut self, allow_shrink: bool, cx: &mut Context<Self>) {
        let Some(hold) = &mut self.search_results_hold else {
            return;
        };
        let digits = {
            let snapshot = self.buffer.read(cx).read(cx);
            if snapshot.is_empty() { 0 } else { (snapshot.widest_line_number().max(1).ilog10() + 1) as usize }
        };
        if hold.min_line_number_digits < digits || allow_shrink && hold.min_line_number_digits != digits {
            hold.min_line_number_digits = digits;
            cx.notify();
        }
    }

    fn frozen_scroll_range(
        &mut self,
        is_rewrapping: bool,
        current_range: Size<Pixels>,
        current_editor_width: Pixels,
        current_editor_bounds_size: Size<Pixels>,
    ) -> Option<SettledScrollRange> {
        let hold = self.search_results_hold.as_mut()?;
        let current = SettledScrollRange {
            range: current_range,
            editor_width: current_editor_width,
            editor_bounds_size: current_editor_bounds_size,
        };
        if !hold.status.pending && !is_rewrapping {
            hold.settled_scroll_range = Some(current);
            return None;
        }
        let settled = hold.settled_scroll_range.get_or_insert(current);
        if settled.editor_bounds_size != current_editor_bounds_size {
            *settled = current;
        }
        Some(*settled)
    }

    pub fn set_custom_context_menu(
        &mut self,
        f: impl 'static + Fn(&mut Self, DisplayPoint, &mut Window, &mut Context<Self>) -> Option<Entity<ui::ContextMenu>>,
    ) {
        self.custom_context_menu = Some(Box::new(f));
    }

    pub fn semantics_provider(&self) -> Option<Rc<dyn SemanticsProvider>> {
        self.semantics_provider.clone()
    }

    pub fn set_semantics_provider(&mut self, provider: Option<Rc<dyn SemanticsProvider>>) {
        self.semantics_provider = provider;
    }

    pub fn placeholder_text(&self, cx: &mut App) -> Option<String> {
        self.placeholder_display_map
            .as_ref()
            .map(|display_map| display_map.update(cx, |map, cx| map.snapshot(cx)).text())
    }

    pub fn set_placeholder_text(&mut self, placeholder_text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let multibuffer = cx.new(|cx| MultiBuffer::singleton(cx.new(|cx| Buffer::local(placeholder_text, cx)), cx));

        let style = window.text_style();

        self.placeholder_display_map = Some(cx.new(|cx| {
            DisplayMap::new(
                multibuffer,
                style.font(),
                style.font_size.to_pixels(window.rem_size()),
                None,
                FILE_HEADER_HEIGHT,
                MULTI_BUFFER_EXCERPT_HEADER_HEIGHT,
                FoldPlaceholder::default(),
                DiagnosticSeverity::Off,
                cx,
            )
        }));
        cx.notify();
    }

    pub fn set_cursor_shape(&mut self, cursor_shape: CursorShape, cx: &mut Context<Self>) {
        self.cursor_shape = cursor_shape;

        // Disrupt blink for immediate user feedback that the cursor shape has changed
        self.blink_manager.update(cx, BlinkManager::show_cursor);

        cx.notify();
    }

    pub fn show_cursor(&mut self, cx: &mut Context<Self>) {
        self.blink_manager.update(cx, BlinkManager::show_cursor);
    }

    pub const fn cursor_shape(&self) -> CursorShape {
        self.cursor_shape
    }

    pub const fn set_cursor_offset_on_selection(&mut self, set_cursor_offset_on_selection: bool) {
        self.cursor_offset_on_selection = set_cursor_offset_on_selection;
    }

    pub fn range_for_match<T: marker::Copy>(&self, range: &Range<T>) -> Range<T> {
        if self.collapse_matches {
            return range.start..range.start;
        }
        range.clone()
    }

    pub fn clip_at_line_ends(&mut self, cx: &mut Context<Self>) -> bool {
        self.display_map.read(cx).clip_at_line_ends
    }

    pub fn capability(&self, cx: &App) -> Capability {
        if self.read_only { Capability::ReadOnly } else { self.buffer.read(cx).capability() }
    }

    pub fn read_only(&self, cx: &App) -> bool {
        self.read_only || self.buffer.read(cx).read_only()
    }

    pub const fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
    }

    pub const fn set_use_selection_highlight(&mut self, highlight: bool) {
        self.use_selection_highlight = highlight;
    }

    pub const fn set_should_serialize(&mut self, should_serialize: bool) {
        self.serialize_buffer = should_serialize;
    }

    pub const fn set_use_modal_editing(&mut self, to: bool) {
        self.use_modal_editing = to;
    }

    pub const fn use_modal_editing(&self) -> bool {
        self.use_modal_editing
    }

    /// Inserted text is normalized to LF line endings before being applied.
    /// Normalize before measuring inserted text for post-edit offsets.
    pub fn edit<I, S, T>(&mut self, edits: I, cx: &mut Context<Self>)
    where
        I: IntoIterator<Item = (Range<S>, T)>,
        S: ToOffset,
        T: Into<Arc<str>>,
    {
        if self.read_only(cx) {
            return;
        }

        self.buffer.update(cx, |buffer, cx| buffer.edit(edits, None, cx));
    }

    pub fn edit_with_autoindent<I, S, T>(&mut self, edits: I, cx: &mut Context<Self>)
    where
        I: IntoIterator<Item = (Range<S>, T)>,
        S: ToOffset,
        T: Into<Arc<str>>,
    {
        if self.read_only(cx) {
            return;
        }

        self.buffer.update(cx, |buffer, cx| buffer.edit(edits, self.autoindent_mode.clone(), cx));
    }

    pub fn cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        self.selection_mark_mode = false;

        if self.dismiss_menus_and_popups(true, window, cx) {
            cx.notify();
            return;
        }
        if self.clear_expanded_diff_hunks(cx) {
            cx.notify();
            return;
        }
        if self.show_git_blame_gutter && !self.blame.as_ref().is_some_and(|blame| blame.read(cx).is_static()) {
            self.show_git_blame_gutter = false;
            cx.notify();
            return;
        }

        let cancelling_group =
            self.selections.pending_anchor().is_none() && self.selections.disjoint_anchors().len() > 1;
        if self.mode.is_full() && self.change_selections(SelectionEffects::default(), window, cx, |s| s.try_cancel()) {
            if cancelling_group {
                self.add_selections_state = None;
            }
            cx.notify();
            return;
        }

        cx.propagate();
    }

    pub fn dismiss_menus_and_popups(
        &mut self,
        _is_user_requested: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut dismissed = false;

        dismissed |= self.take_rename(false, window, cx).is_some();
        dismissed |= hide_hover(self, cx);
        dismissed |= self.hide_context_menu(cx).is_some();
        dismissed |= self.mouse_context_menu.take().is_some();
        dismissed |= self.snippet_stack.pop().is_some();

        if self.mode.is_full() && self.has_active_diagnostic_group() {
            self.dismiss_diagnostics(cx);
            dismissed = true;
        }

        dismissed
    }

    pub const fn has_mouse_context_menu(&self) -> bool {
        self.mouse_context_menu.is_some()
    }

    pub fn multi_buffer_visible_range(&self, display_snapshot: &DisplaySnapshot, cx: &App) -> Range<Point> {
        let visible_start = self
            .scroll_manager
            .native_anchor(display_snapshot, cx)
            .anchor
            .to_point(display_snapshot.buffer_snapshot())
            .to_display_point(display_snapshot);

        let mut target_end = visible_start;
        *target_end.row_mut() += self.visible_line_count().unwrap_or(0.).ceil() as u32;

        visible_start.to_point(display_snapshot)
            ..display_snapshot.clip_point(target_end, Bias::Right).to_point(display_snapshot)
    }

    fn handle_modifiers_changed(
        &mut self,
        modifiers: Modifiers,
        position_map: &PositionMap,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_selection_mode(modifiers, position_map, window, cx);

        let mouse_position = window.mouse_position();
        if !position_map.text_hitbox.is_hovered(window) {
            if self.gutter_hover_button.0.is_some() {
                cx.notify();
            }
            return;
        }

        self.update_hovered_link(
            position_map.point_for_position(mouse_position),
            position_map.inlay_hint_glyph_for_position(mouse_position),
            Some(mouse_position),
            &position_map.snapshot,
            modifiers,
            window,
            cx,
        );
    }

    /// Columnar selection mode for a mouse gesture: shift+cmd/ctrl selects from the mouse,
    /// shift+alt extends from the current selection.
    const fn columnar_selection_mode(modifiers: Modifiers) -> Option<ColumnarMode> {
        if modifiers.shift && modifiers.number_of_modifiers() == 2 {
            if modifiers.secondary() {
                Some(ColumnarMode::FromMouse)
            } else if modifiers.alt {
                Some(ColumnarMode::FromSelection)
            } else {
                None
            }
        } else {
            None
        }
    }

    fn update_selection_mode(
        &mut self,
        modifiers: Modifiers,
        position_map: &PositionMap,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mode) = Self::columnar_selection_mode(modifiers) else {
            return;
        };
        if self.selections.pending_anchor().is_none() {
            return;
        }

        let mouse_position = window.mouse_position();
        let point_for_position = position_map.point_for_position(mouse_position);
        let position = point_for_position.previous_valid;

        self.select(
            SelectPhase::BeginColumnar {
                position,
                reset: false,
                mode,
                goal_column: point_for_position.exact_unclipped.column(),
            },
            window,
            cx,
        );
    }

    fn render_gutter_hover_button(&self, row: DisplayRow, cx: &Context<Self>) -> IconButton {
        let (icon, color, tooltip) = if self.gutter_copied_row.0 == Some(row) {
            (IconName::Check, Color::Success, "Copied!")
        } else {
            (IconName::Bookmark, Color::Info, "Copy File Location")
        };
        IconButton::new(("copy_file_location_button", row.0 as usize), icon)
            .icon_size(IconSize::XSmall)
            .size(ui::ButtonSize::None)
            .icon_color(color)
            .style(ButtonStyle::Transparent)
            .on_click(cx.listener(move |editor, _: &ClickEvent, _window, cx| {
                editor.copy_file_location_at_row(row, cx);
            }))
            .tooltip(Tooltip::text(tooltip))
    }

    fn gutter_context_menu(
        &self,
        display_row: DisplayRow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ContextMenu> {
        let weak_editor = cx.weak_entity();
        let focus_handle = self.focus_handle(cx);

        let git_blame_msg = if self.show_git_blame_gutter { "Close Git Blame" } else { "Open Git Blame" };

        let clear_runnable_task_status =
            self.runnable_task_key_for_display_row(display_row, window, cx).filter(|(buffer_id, buffer_row)| {
                matches!(
                    self.runnable_task_status(*buffer_id, *buffer_row),
                    Some(RunnableTaskStatus::Passed | RunnableTaskStatus::Failed)
                )
            });

        ContextMenu::build(window, cx, |menu, _, _cx| {
            menu.on_blur_subscription(Subscription::new(|| {}))
                .context(focus_handle)
                .when_some(clear_runnable_task_status, |this, (buffer_id, buffer_row)| {
                    this.entry("Clear Run Status", None, {
                        let weak_editor = weak_editor.clone();
                        move |_window, cx| {
                            weak_editor
                                .update(cx, |this, cx| {
                                    this.clear_runnable_task_status(buffer_id, buffer_row, cx);
                                })
                                .log_err();
                        }
                    })
                    .separator()
                })
                .entry(git_blame_msg, Some(Blame.boxed_clone()), {
                    let weak_editor = weak_editor.clone();
                    move |window, cx| {
                        weak_editor
                            .update(cx, |this, cx| {
                                this.toggle_git_blame(&Blame, window, cx);
                            })
                            .log_err();
                    }
                })
                .entry("Copy File Location", Some(CopyFileLocation.boxed_clone()), {
                    move |_window, cx| {
                        weak_editor
                            .update(cx, |this, cx| {
                                this.copy_file_location_at_row(display_row, cx);
                            })
                            .log_err();
                    }
                })
        })
    }

    pub fn context_menu_visible(&self) -> bool {
        self.context_menu.borrow().as_ref().is_some_and(|menu| menu.visible())
    }

    pub fn context_menu_origin(&self) -> Option<ContextMenuOrigin> {
        self.context_menu.borrow().as_ref().map(|menu| menu.origin())
    }

    fn current_cursor_colors(&self, cx: &App) -> CursorColors {
        if self.read_only(cx) {
            cx.theme().cursor_colors().read_only()
        } else {
            self.style.as_ref().expect("style is set before rendering").cursor_colors
        }
    }

    pub fn render_context_menu(
        &mut self,
        max_height_in_lines: u32,
        window: &mut Window,
        cx: &mut Context<Editor>,
    ) -> Option<AnyElement> {
        let menu = self.context_menu.borrow();
        let menu = menu.as_ref()?;
        if !menu.visible() {
            return None;
        }
        self.style.as_ref().map(|style| menu.render(style, max_height_in_lines, window, cx))
    }

    fn render_context_menu_aside(
        &self,
        max_size: Size<Pixels>,
        window: &mut Window,
        cx: &mut Context<Editor>,
    ) -> Option<AnyElement> {
        self.context_menu
            .borrow_mut()
            .as_mut()
            .and_then(|menu| if menu.visible() { menu.render_aside(max_size, window, cx) } else { None })
    }

    fn hide_context_menu(&mut self, cx: &mut Context<Self>) -> Option<CodeContextMenu> {
        cx.notify();
        self.completion_tasks.clear();
        self.context_menu.borrow_mut().take()
    }

    fn set_gutter_context_menu(
        &mut self,
        display_row: DisplayRow,
        position: Option<Anchor>,
        clicked_point: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let display_snapshot = self.display_snapshot(cx);
        let display_point = display_row.as_display_point();
        let source = display_snapshot.display_point_to_anchor(display_point, Bias::Left);
        let anchor = position.unwrap_or(source);

        // Git blame requires a worktree-file-backed buffer; skip untitled buffers.
        if !display_snapshot
            .buffer_snapshot()
            .anchor_to_buffer_anchor(anchor)
            .is_some_and(|(_, buffer_snapshot)| project::File::from_dyn(buffer_snapshot.file()).is_some())
        {
            return;
        }

        let context_menu = self.gutter_context_menu(display_row, window, cx);

        self.mouse_context_menu =
            MouseContextMenu::pinned_to_editor(self, source, clicked_point, context_menu, window, cx);
    }

    #[allow(clippy::type_complexity)]
    fn restore_selections(
        &mut self,
        selections: Option<(Arc<[Selection<Anchor>]>, Option<AddSelectionsState>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((selections, add_selections_state)) = selections.filter(|(selections, _)| !selections.is_empty()) {
            self.with_selection_effects_deferred(window, cx, |editor, window, cx| {
                editor.change_selections(SelectionEffects::no_scroll(), window, cx, |selection_collection| {
                    selection_collection.select_anchors_unexpanded(selections.to_vec());
                });
                editor.add_selections_state = add_selections_state;
            });
        }
    }

    pub fn undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only(cx) {
            return;
        }

        if let Some(transaction_id) = self.buffer.update(cx, |buffer, cx| buffer.undo(cx)) {
            let transaction = self.selection_history.transaction(transaction_id);
            if transaction.is_none() {
                log::error!("No selection history for undone transaction; selection unchanged");
            }
            let selections = transaction
                .map(|transaction| (Arc::clone(&transaction.undo), transaction.undo_add_selections_state.clone()));
            self.restore_selections(selections, window, cx);
            self.request_autoscroll(Autoscroll::fit(), cx);
            self.unmark_text(window, cx);

            cx.emit(EditorEvent::Edited { transaction_id });
            cx.emit(EditorEvent::TransactionUndone { transaction_id });
        }
    }

    pub fn redo(&mut self, _: &Redo, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only(cx) {
            return;
        }

        if let Some(transaction_id) = self.buffer.update(cx, |buffer, cx| buffer.redo(cx)) {
            let selections = self.selection_history.transaction(transaction_id).and_then(|transaction| {
                transaction.redo.clone().map(|selections| (selections, transaction.redo_add_selections_state.clone()))
            });
            self.restore_selections(selections, window, cx);
            self.request_autoscroll(Autoscroll::fit(), cx);
            self.unmark_text(window, cx);

            cx.emit(EditorEvent::Edited { transaction_id });
        }
    }

    pub fn finalize_last_transaction(&mut self, cx: &mut Context<Self>) {
        self.buffer.update(cx, |buffer, cx| buffer.finalize_last_transaction(cx));
    }

    pub fn group_until_transaction(&mut self, tx_id: TransactionId, cx: &mut Context<Self>) {
        self.buffer.update(cx, |buffer, cx| buffer.group_until_transaction(tx_id, cx));
    }

    pub fn context_menu_first(&mut self, _: &ContextMenuFirst, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(context_menu) = self.context_menu.borrow_mut().as_mut() {
            context_menu.select_first(self.completion_provider.as_deref(), cx);
        }
    }

    pub fn context_menu_prev(&mut self, _: &ContextMenuPrevious, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(context_menu) = self.context_menu.borrow_mut().as_mut() {
            context_menu.select_prev(self.completion_provider.as_deref(), cx);
        }
    }

    pub fn context_menu_next(&mut self, _: &ContextMenuNext, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(context_menu) = self.context_menu.borrow_mut().as_mut() {
            context_menu.select_next(self.completion_provider.as_deref(), cx);
        }
    }

    pub fn context_menu_last(&mut self, _: &ContextMenuLast, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(context_menu) = self.context_menu.borrow_mut().as_mut() {
            context_menu.select_last(self.completion_provider.as_deref(), cx);
        }
    }

    fn show_character_palette(&mut self, _: &ShowCharacterPalette, window: &mut Window, _: &mut Context<Self>) {
        window.show_character_palette();
    }

    pub fn supports_minimap(&self, cx: &App) -> bool {
        !self.minimap_visibility.disabled() && self.buffer_kind(cx) == ItemBufferKind::Singleton
    }

    pub fn toggle_minimap(&mut self, _: &ToggleMinimap, window: &mut Window, cx: &mut Context<Editor>) {
        if self.supports_minimap(cx) {
            self.set_minimap_visibility(self.minimap_visibility.toggle_visibility(), window, cx);
        }
    }

    pub fn transact(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>),
    ) -> Option<TransactionId> {
        self.with_selection_effects_deferred(window, cx, |this, window, cx| {
            this.start_transaction_at(Instant::now(), window, cx);
            update(this, window, cx);
            this.end_transaction_at(Instant::now(), cx)
        })
    }

    pub fn start_transaction_at(
        &mut self,
        now: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<TransactionId> {
        self.end_selection(window, cx);
        if let Some(tx_id) = self.buffer.update(cx, |buffer, cx| buffer.start_transaction_at(now, cx)) {
            self.selection_history.insert_transaction(
                tx_id,
                self.selections.disjoint_anchors_arc(),
                self.add_selections_state.clone(),
            );
            cx.emit(EditorEvent::TransactionBegun { transaction_id: tx_id });
            Some(tx_id)
        } else {
            None
        }
    }

    pub fn end_transaction_at(&mut self, now: Instant, cx: &mut Context<Self>) -> Option<TransactionId> {
        if let Some(transaction_id) = self.buffer.update(cx, |buffer, cx| buffer.end_transaction_at(now, cx)) {
            if let Some(transaction) = self.selection_history.transaction_mut(transaction_id) {
                transaction.redo = Some(self.selections.disjoint_anchors_arc());
                transaction.redo_add_selections_state.clone_from(&self.add_selections_state);
            } else {
                log::error!("unexpectedly ended a transaction that wasn't started by this editor");
            }

            cx.emit(EditorEvent::Edited { transaction_id });
            Some(transaction_id)
        } else {
            None
        }
    }

    pub fn toggle_focus(
        workspace: &mut Workspace,
        _: &actions::ToggleFocus,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let Some(item) = workspace.recent_active_item_by_type::<Self>(cx) else {
            return;
        };
        workspace.activate_item(&item, true, true, window, cx);
    }

    pub fn set_gutter_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if hovered != self.gutter_hovered {
            self.gutter_hovered = hovered;
            cx.notify();
        }
    }

    pub fn insert_blocks(
        &mut self,
        blocks: impl IntoIterator<Item = BlockProperties<Anchor>>,
        autoscroll: Option<Autoscroll>,
        cx: &mut Context<Self>,
    ) -> Vec<CustomBlockId> {
        let blocks = self.display_map.update(cx, |display_map, cx| display_map.insert_blocks(blocks, cx));
        self.inlay_hint_visibility_changed(cx);
        if let Some(autoscroll) = autoscroll {
            self.request_autoscroll(autoscroll, cx);
        }
        cx.notify();
        blocks
    }

    pub fn resize_blocks(
        &mut self,
        heights: FxHashMap<CustomBlockId, u32>,
        autoscroll: Option<Autoscroll>,
        cx: &mut Context<Self>,
    ) {
        self.display_map.update(cx, |display_map, cx| display_map.resize_blocks(heights, cx));
        if let Some(autoscroll) = autoscroll {
            self.request_autoscroll(autoscroll, cx);
        }
        cx.notify();
    }

    pub fn replace_blocks(
        &mut self,
        renderers: FxHashMap<CustomBlockId, RenderBlock>,
        autoscroll: Option<Autoscroll>,
        cx: &mut Context<Self>,
    ) {
        self.display_map.update(cx, |display_map, _cx| display_map.replace_blocks(renderers));
        if let Some(autoscroll) = autoscroll {
            self.request_autoscroll(autoscroll, cx);
        }
        cx.notify();
    }

    pub fn remove_blocks(
        &mut self,
        block_ids: FxHashSet<CustomBlockId>,
        autoscroll: Option<Autoscroll>,
        cx: &mut Context<Self>,
    ) {
        self.display_map.update(cx, |display_map, cx| display_map.remove_blocks(block_ids, cx));
        self.inlay_hint_visibility_changed(cx);
        if let Some(autoscroll) = autoscroll {
            self.request_autoscroll(autoscroll, cx);
        }
        cx.notify();
    }

    pub fn row_for_block(&self, block_id: CustomBlockId, cx: &mut Context<Self>) -> Option<DisplayRow> {
        self.display_map.update(cx, |map, cx| map.row_for_block(block_id, cx))
    }

    pub(crate) fn set_focused_block(&mut self, focused_block: FocusedBlock) {
        self.focused_block = Some(focused_block);
    }

    pub(crate) const fn take_focused_block(&mut self) -> Option<FocusedBlock> {
        self.focused_block.take()
    }

    pub fn longest_row(&self, cx: &mut App) -> DisplayRow {
        self.display_map.update(cx, |map, cx| map.snapshot(cx)).longest_row()
    }

    pub fn max_point(&self, cx: &mut App) -> DisplayPoint {
        self.display_map.update(cx, |map, cx| map.snapshot(cx)).max_point()
    }

    pub fn text(&self, cx: &App) -> String {
        self.buffer.read(cx).read(cx).text()
    }

    pub fn is_empty(&self, cx: &App) -> bool {
        self.buffer.read(cx).read(cx).is_empty()
    }

    pub fn set_text(&mut self, text: impl Into<Arc<str>>, window: &mut Window, cx: &mut Context<Self>) {
        self.transact(window, cx, |this, _, cx| {
            this.buffer
                .read(cx)
                .as_singleton()
                .expect("you can only call set_text on editors for singleton buffers")
                .update(cx, |buffer, cx| buffer.set_text(text, cx));
        });
    }

    pub fn display_text(&self, cx: &mut App) -> String {
        self.display_map.update(cx, |map, cx| map.snapshot(cx)).text()
    }

    fn create_minimap(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Self>> {
        (self.buffer_kind(cx) == ItemBufferKind::Singleton).then(|| self.initialize_new_minimap(window, cx))
    }

    fn initialize_new_minimap(&self, window: &mut Window, cx: &mut Context<Self>) -> Entity<Self> {
        const MINIMAP_FONT_WEIGHT: gpui::FontWeight = gpui::FontWeight::BLACK;

        let mut minimap = Editor::new_internal(
            EditorMode::Minimap { parent: cx.weak_entity() },
            self.buffer.clone(),
            None,
            Some(self.display_map.clone()),
            window,
            cx,
        );
        let my_snapshot = self.display_map.update(cx, |map, cx| map.snapshot(cx));
        let minimap_snapshot = minimap.display_map.update(cx, |map, cx| map.snapshot(cx));
        minimap.scroll_manager.clone_state(&self.scroll_manager, &my_snapshot, &minimap_snapshot, cx);
        minimap.set_text_style_refinement(TextStyleRefinement {
            font_size: Some(MINIMAP_FONT_SIZE),
            font_weight: Some(MINIMAP_FONT_WEIGHT),
            ..Default::default()
        });
        cx.new(|_| minimap)
    }

    pub fn minimap(&self) -> Option<&Entity<Self>> {
        self.minimap.as_ref().filter(|_| self.minimap_visibility.visible())
    }

    pub fn set_masked(&mut self, masked: bool, cx: &mut Context<Self>) {
        if self.display_map.read(cx).masked != masked {
            self.display_map.update(cx, |map, _| map.masked = masked);
        }
        cx.notify();
    }

    pub fn insert_uuid_v4(&mut self, _: &InsertUuidV4, window: &mut Window, cx: &mut Context<Self>) {
        self.insert_uuid(UuidVersion::V4, window, cx);
    }

    pub fn insert_uuid_v7(&mut self, _: &InsertUuidV7, window: &mut Window, cx: &mut Context<Self>) {
        self.insert_uuid(UuidVersion::V7, window, cx);
    }

    fn insert_uuid(&mut self, version: UuidVersion, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only(cx) {
            return;
        }
        self.transact(window, cx, |this, _window, cx| {
            let edits = this.selections.all::<Point>(&this.display_snapshot(cx)).into_iter().map(|selection| {
                let uuid = match version {
                    UuidVersion::V4 => uuid::Uuid::new_v4(),
                    UuidVersion::V7 => uuid::Uuid::now_v7(),
                };

                (selection.range(), uuid.to_string())
            });
            this.edit(edits, cx);
        });
    }

    pub fn show_cursors(&self, window: &mut Window, cx: &mut App) -> bool {
        (self.read_only(cx) || self.blink_manager.read(cx).visible()) && self.focus_handle.is_focused(window)
    }

    fn fetch_applicable_language_settings(&self, cx: &App) -> FxHashMap<Option<LanguageName>, Arc<LanguageSettings>> {
        if !self.mode.is_full() {
            return FxHashMap::default();
        }

        self.buffer().read(cx).all_buffers().into_iter().fold(FxHashMap::default(), |mut acc, buffer| {
            let buffer = buffer.read(cx);
            let language = buffer.language().map(|language| language.name());
            if let hash_map::Entry::Vacant(v) = acc.entry(language) {
                v.insert(LanguageSettings::for_buffer(buffer, cx));
            }
            acc
        })
    }

    pub const fn set_searchable(&mut self, searchable: bool) {
        self.searchable = searchable;
    }

    pub const fn searchable(&self) -> bool {
        self.searchable
    }

    fn selection_replacement_ranges(
        &self,
        range: Range<MultiBufferOffsetUtf16>,
        cx: &mut App,
    ) -> Vec<Range<MultiBufferOffsetUtf16>> {
        let selections = self.selections.all::<MultiBufferOffsetUtf16>(&self.display_snapshot(cx));
        let newest_selection =
            selections.iter().max_by_key(|selection| selection.id).expect("selections are never empty");
        let start_delta = range.start.0.0.cast_signed() - newest_selection.start.0.0.cast_signed();
        let end_delta = range.end.0.0.cast_signed() - newest_selection.end.0.0.cast_signed();
        let snapshot = self.buffer.read(cx).read(cx);
        selections
            .into_iter()
            .map(|mut selection| {
                selection.start.0.0 = selection.start.0.0.cast_signed().saturating_add(start_delta) as usize;
                selection.end.0.0 = selection.end.0.0.cast_signed().saturating_add(end_delta) as usize;
                snapshot.clip_offset_utf16(selection.start, Bias::Left)
                    ..snapshot.clip_offset_utf16(selection.end, Bias::Right)
            })
            .collect()
    }

    /// Copy the highlighted chunks to the clipboard as JSON. The format is an array of lines,
    /// with each line being an array of {text, highlight} objects.
    fn copy_highlight_json(&mut self, _: &CopyHighlightJson, _: &mut Window, cx: &mut Context<Self>) {
        #[derive(Serialize)]
        struct Chunk<'a> {
            text: String,
            highlight: Option<&'a str>,
        }

        let snapshot = self.buffer.read(cx).snapshot(cx);
        let mut selection = self.selections.newest::<Point>(&self.display_snapshot(cx));
        let max_point = snapshot.max_point();

        let range = if self.selections.line_mode() {
            selection.start = Point::new(selection.start.row, 0);
            selection.end = cmp::min(max_point, Point::new(selection.end.row + 1, 0));
            selection.goal = SelectionGoal::None;
            selection.range()
        } else if selection.is_empty() {
            Point::new(0, 0)..max_point
        } else {
            selection.range()
        };

        let chunks = snapshot.chunks(range, LanguageAwareStyling { tree_sitter: true, diagnostics: true });
        let mut lines = Vec::new();
        let mut line: VecDeque<Chunk> = VecDeque::new();

        let Some(style) = self.style.as_ref() else {
            return;
        };

        for chunk in chunks {
            let highlight = chunk.syntax_highlight_id.and_then(|id| style.syntax.get_capture_name(id));

            let mut chunk_lines = chunk.text.split('\n').peekable();
            while let Some(text) = chunk_lines.next() {
                let merged_with_last_token = if let Some(last_token) = line.back_mut()
                    && last_token.highlight == highlight
                {
                    last_token.text.push_str(text);
                    true
                } else {
                    false
                };

                if !merged_with_last_token {
                    line.push_back(Chunk { text: text.into(), highlight });
                }

                if chunk_lines.peek().is_some() {
                    if line.len() > 1 && line.front().expect("line is non-empty").text.is_empty() {
                        line.pop_front();
                    }
                    if line.len() > 1 && line.back().expect("line is non-empty").text.is_empty() {
                        line.pop_back();
                    }

                    lines.push(mem::take(&mut line));
                }
            }
        }

        if line.iter().any(|chunk| !chunk.text.is_empty()) {
            lines.push(line);
        }

        let Some(lines) = serde_json::to_string_pretty(&lines).log_err() else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(lines));
    }

    pub fn open_context_menu(&mut self, _: &OpenContextMenu, window: &mut Window, cx: &mut Context<Self>) {
        self.request_autoscroll(Autoscroll::newest(), cx);
        let position = self.selections.newest_display(&self.display_snapshot(cx)).start;
        mouse_context_menu::deploy_context_menu(self, None, position, window, cx);
    }

    pub fn register_action_renderer(
        &mut self,
        listener: impl Fn(&Editor, &mut Window, &mut Context<Editor>) + 'static,
    ) -> Subscription {
        let id = self.next_editor_action_id.post_inc();
        self.editor_actions.borrow_mut().insert(id, Box::new(listener));

        let editor_actions = Rc::clone(&self.editor_actions);
        Subscription::new(move || {
            editor_actions.borrow_mut().remove(&id);
        })
    }

    pub fn register_action<A: Action>(
        &mut self,
        listener: impl Fn(&A, &mut Window, &mut App) + 'static,
    ) -> Subscription {
        self.register_action_erased(
            TypeId::of::<A>(),
            Arc::new(move |action, window, cx| {
                listener(action.downcast_ref().expect("action type matches listener"), window, cx);
            }),
        )
    }

    #[allow(clippy::type_complexity)]
    fn register_action_erased(
        &mut self,
        action_type: TypeId,
        listener: Arc<dyn Fn(&dyn Any, &mut Window, &mut App)>,
    ) -> Subscription {
        let id = self.next_editor_action_id.post_inc();
        self.editor_actions.borrow_mut().insert(
            id,
            Box::new(move |_, window, _| {
                let listener = Arc::clone(&listener);
                window.on_action(action_type, move |action, phase, window, cx| {
                    if phase == DispatchPhase::Bubble {
                        listener(action, window, cx);
                    }
                });
            }),
        );

        let editor_actions = Rc::clone(&self.editor_actions);
        Subscription::new(move || {
            editor_actions.borrow_mut().remove(&id);
        })
    }

    pub fn restore(
        &mut self,
        revert_changes: FxHashMap<BufferId, Vec<(Range<text::Anchor>, Rope)>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.buffer().update(cx, |multi_buffer, cx| {
            for (buffer_id, changes) in revert_changes {
                if let Some(buffer) = multi_buffer.buffer(buffer_id) {
                    buffer.update(cx, |buffer, cx| {
                        buffer.edit(changes.into_iter().map(|(range, text)| (range, text.to_string())), None, cx);
                    });
                }
            }
        });
        let selections = self.selections.all::<MultiBufferOffset>(&self.display_snapshot(cx));
        self.change_selections(SelectionEffects::no_scroll(), window, cx, |s| {
            s.select(selections);
        });
    }

    pub fn to_pixel_point(
        &mut self,
        source: Anchor,
        editor_snapshot: &EditorSnapshot,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<gpui::Point<Pixels>> {
        let source_point = source.to_display_point(editor_snapshot);
        self.display_to_pixel_point(source_point, editor_snapshot, window, cx)
    }

    pub fn display_to_pixel_point(
        &mut self,
        source: DisplayPoint,
        editor_snapshot: &EditorSnapshot,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<gpui::Point<Pixels>> {
        let line_height = self.style(cx).text.line_height_in_pixels(window.rem_size());
        let text_layout_details = self.text_layout_details(window, cx);
        let mut scroll_top = text_layout_details.scroll_anchor.scroll_position(editor_snapshot).y;
        if !line_height.is_zero() {
            scroll_top = window.pixel_snap_f64(scroll_top * f64::from(line_height)) / f64::from(line_height);
        }

        if source.row().as_f64() < scroll_top.floor() {
            return None;
        }
        let source_x = editor_snapshot.x_for_display_point(source, &text_layout_details);
        let source_y = line_height * (source.row().as_f64() - scroll_top) as f32;
        Some(gpui::Point::new(source_x, source_y))
    }

    pub fn register_addon<T: Addon>(&mut self, instance: T) {
        if self.mode.is_minimap() {
            return;
        }
        self.addons.insert(TypeId::of::<T>(), Box::new(instance));
    }

    pub fn addon<T: Addon>(&self) -> Option<&T> {
        let type_id = TypeId::of::<T>();
        let item = self.addons.get(&type_id)?;
        item.to_any().downcast_ref::<T>()
    }

    pub fn addon_mut<T: Addon>(&mut self) -> Option<&mut T> {
        let type_id = TypeId::of::<T>();
        let item = self.addons.get_mut(&type_id)?;
        item.to_any_mut()?.downcast_mut::<T>()
    }

    fn character_dimensions(&self, window: &mut Window, cx: &mut App) -> CharacterDimensions {
        let text_layout_details = self.text_layout_details(window, cx);
        let style = &text_layout_details.editor_style;
        let font_id = window.text_system().resolve_font(&style.text.font());
        let font_size = style.text.font_size.to_pixels(window.rem_size());
        let line_height = style.text.line_height_in_pixels(window.rem_size());
        let em_width = window.text_system().em_width(font_id, font_size).expect("font is resolved");
        let em_advance = window.text_system().em_advance(font_id, font_size).expect("font is resolved");

        CharacterDimensions { em_width, em_advance, line_height }
    }

    fn create_style(&self, cx: &App) -> EditorStyle {
        let mut text_style = match self.mode {
            EditorMode::SingleLine | EditorMode::AutoHeight { .. } => TextStyle {
                color: cx.theme().colors().editor_foreground,
                font_family: font::ui_font().family.clone(),
                font_features: font::ui_font().features.clone(),
                font_fallbacks: font::ui_font().fallbacks.clone(),
                font_size: rems(0.875).into(),
                font_weight: font::ui_font().weight,
                line_height: relative(BUFFER_LINE_HEIGHT),
                ..Default::default()
            },
            EditorMode::Full { .. } | EditorMode::Minimap { .. } => TextStyle {
                color: cx.theme().colors().editor_foreground,
                font_family: font::buffer_font().family.clone(),
                font_features: font::buffer_font().features.clone(),
                font_fallbacks: font::buffer_font().fallbacks.clone(),
                font_size: font::buffer_font_size(cx).into(),
                font_weight: font::buffer_font().weight,
                line_height: relative(BUFFER_LINE_HEIGHT),
                ..Default::default()
            },
        };
        if let Some(text_style_refinement) = &self.text_style_refinement {
            text_style.refine(text_style_refinement);
        }

        let background = match self.mode {
            EditorMode::SingleLine => cx.theme().system().transparent,
            EditorMode::AutoHeight { .. } => cx.theme().system().transparent,
            EditorMode::Full { .. } => cx.theme().colors().editor_background,
            EditorMode::Minimap { .. } => cx.theme().colors().editor_background.opacity(0.7),
        };

        EditorStyle {
            background,
            border: cx.theme().colors().border,
            cursor_colors: cx.theme().cursor_colors(),
            text: text_style,
            scrollbar_width: EditorElement::SCROLLBAR_WIDTH,
            syntax: cx.theme().syntax(),
            status: cx.theme().status().clone(),
            inlay_hints_style: make_inlay_hints_style(cx),
            unnecessary_code_fade: UNNECESSARY_CODE_FADE,
            show_underlines: !self.mode.is_minimap(),
        }
    }
}

pub fn column_pixels(style: &EditorStyle, column: usize, window: &Window) -> Pixels {
    let font_size = style.text.font_size.to_pixels(window.rem_size());
    let layout = window.text_system().shape_line(
        SharedString::from(" ".repeat(column)),
        font_size,
        &[TextRun { len: column, font: style.text.font(), color: Hsla::default(), ..Default::default() }],
        None,
    );

    layout.width
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorEvent {
    InputIgnored {
        text: Arc<str>,
    },
    InputHandled {
        utf16_range_to_replace: Option<Range<isize>>,
        text: Arc<str>,
    },
    BufferRangesUpdated {
        buffer: Entity<Buffer>,
        path_key: PathKey,
        ranges: Vec<ExcerptRange<text::Anchor>>,
    },
    BuffersRemoved {
        removed_buffer_ids: Vec<BufferId>,
    },
    BuffersEdited {
        buffer_ids: Vec<BufferId>,
    },
    BufferFoldToggled {
        ids: Vec<BufferId>,
        folded: bool,
    },
    ExpandExcerptsRequested {
        excerpt_anchors: Vec<Anchor>,
        lines: u32,
        direction: ExpandExcerptDirection,
    },
    OpenExcerptsRequested {
        selections_by_buffer: FxHashMap<BufferId, (Vec<Range<BufferOffset>>, Option<u32>)>,
        split: bool,
    },
    /// Emitted when an underlying buffer changes, including edits made through another editor.
    BufferEdited,
    /// Emitted when this editor creates, undoes, or redoes an edit transaction.
    Edited {
        /// The transaction that changed the editor's buffer.
        transaction_id: clock::Lamport,
    },
    Reparsed(BufferId),
    /// A buffer's language changed, e.g. once a newly opened file is detected.
    LanguageChanged,
    Focused,
    FocusedIn,
    Blurred,
    DirtyChanged,
    Saved,
    CapabilityChanged,
    TitleChanged,
    FileHandleChanged,
    SelectionsChanged {
        local: bool,
    },
    ScrollPositionChanged {
        local: bool,
        autoscroll: bool,
    },
    TransactionUndone {
        transaction_id: clock::Lamport,
    },
    TransactionBegun {
        transaction_id: clock::Lamport,
    },
    PushedToNavHistory {
        anchor: Anchor,
        is_deactivate: bool,
    },
}

impl EventEmitter<EditorEvent> for Editor {}

impl Focusable for Editor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        EditorElement::new(&cx.entity(), self.create_style(cx))
    }
}

trait SelectionExt {
    fn display_range(&self, map: &DisplaySnapshot) -> Range<DisplayPoint>;
    fn spanned_rows(&self, include_end_if_at_line_start: bool, map: &DisplaySnapshot) -> Range<MultiBufferRow>;
}

impl<T: ToPoint + ToOffset> SelectionExt for Selection<T> {
    fn display_range(&self, map: &DisplaySnapshot) -> Range<DisplayPoint> {
        let start = self.start.to_point(map.buffer_snapshot()).to_display_point(map);
        let end = self.end.to_point(map.buffer_snapshot()).to_display_point(map);
        if self.reversed { end..start } else { start..end }
    }

    fn spanned_rows(&self, include_end_if_at_line_start: bool, map: &DisplaySnapshot) -> Range<MultiBufferRow> {
        let start = self.start.to_point(map.buffer_snapshot());
        let mut end = self.end.to_point(map.buffer_snapshot());
        if !include_end_if_at_line_start && start.row != end.row && end.column == 0 {
            end.row -= 1;
        }

        let buffer_start = map.prev_line_boundary(start).0;
        let buffer_end = map.next_line_boundary(end).0;
        MultiBufferRow(buffer_start.row)..MultiBufferRow(buffer_end.row + 1)
    }
}

impl<T: InvalidationRegion> InvalidationStack<T> {
    fn invalidate<S>(&mut self, selections: &[Selection<S>], buffer: &MultiBufferSnapshot)
    where
        S: Clone + ToOffset,
    {
        while let Some(region) = self.last() {
            let all_selections_inside_invalidation_ranges = if selections.len() == region.ranges().len() {
                selections.iter().zip(region.ranges().iter().map(|r| r.to_offset(buffer))).all(
                    |(selection, invalidation_range)| {
                        let head = selection.head().to_offset(buffer);
                        invalidation_range.start <= head && invalidation_range.end >= head
                    },
                )
            } else {
                false
            };

            if all_selections_inside_invalidation_ranges {
                break;
            }
            self.pop();
        }
    }
}

impl<T> Default for InvalidationStack<T> {
    fn default() -> Self {
        Self(Vec::default())
    }
}

impl<T> Deref for InvalidationStack<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for InvalidationStack<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl InvalidationRegion for SnippetState {
    fn ranges(&self) -> &[Range<Anchor>] {
        &self.ranges[self.active_index]
    }
}

pub fn styled_runs_for_code_label<'a>(
    label: &'a CodeLabel,
    syntax_theme: &'a theme::SyntaxTheme,
    cursor_colors: &'a theme::CursorColors,
) -> impl 'a + Iterator<Item = (Range<usize>, HighlightStyle)> {
    let fade_out = HighlightStyle { fade_out: Some(0.35), ..Default::default() };

    if label.runs.is_empty() {
        let desc_start = label.filter_range.end;
        let fade_run = (desc_start < label.text.len()).then_some((desc_start..label.text.len(), fade_out));
        return Either::Left(fade_run.into_iter());
    }

    let mut prev_end = label.filter_range.end;
    Either::Right(label.runs.iter().enumerate().flat_map(move |(ix, (range, highlight_id))| {
        let style = if *highlight_id == language::HighlightId::TABSTOP_INSERT_ID {
            HighlightStyle { color: Some(cursor_colors.cursor), ..Default::default() }
        } else if *highlight_id == language::HighlightId::TABSTOP_REPLACE_ID {
            HighlightStyle { background_color: Some(cursor_colors.selection), ..Default::default() }
        } else if let Some(style) = syntax_theme.get(*highlight_id).copied() {
            style
        } else {
            return SmallVec::default();
        };

        let mut runs = SmallVec::<[(Range<usize>, HighlightStyle); 3]>::new();
        let muted_style = style.highlight(fade_out);
        if range.start >= label.filter_range.end {
            if range.start > prev_end {
                runs.push((prev_end..range.start, fade_out));
            }
            runs.push((range.clone(), muted_style));
        } else if range.end <= label.filter_range.end {
            runs.push((range.clone(), style));
        } else {
            runs.push((range.start..label.filter_range.end, style));
            runs.push((label.filter_range.end..range.end, muted_style));
        }
        prev_end = cmp::max(prev_end, range.end);

        if ix + 1 == label.runs.len() && label.text.len() > prev_end {
            runs.push((prev_end..label.text.len(), fade_out));
        }

        runs
    }))
}

pub trait RangeToAnchorExt: Sized {
    fn to_anchors(self, snapshot: &MultiBufferSnapshot) -> Range<Anchor>;

    fn to_display_points(self, snapshot: &EditorSnapshot) -> Range<DisplayPoint> {
        let anchor_range = self.to_anchors(snapshot.buffer_snapshot());
        anchor_range.start.to_display_point(snapshot)..anchor_range.end.to_display_point(snapshot)
    }
}

impl<T: ToOffset> RangeToAnchorExt for Range<T> {
    fn to_anchors(self, snapshot: &MultiBufferSnapshot) -> Range<Anchor> {
        let start_offset = self.start.to_offset(snapshot);
        let end_offset = self.end.to_offset(snapshot);
        if start_offset == end_offset {
            snapshot.anchor_before(start_offset)..snapshot.anchor_before(end_offset)
        } else {
            snapshot.anchor_after(self.start)..snapshot.anchor_before(self.end)
        }
    }
}

pub trait RowExt {
    fn as_f64(&self) -> f64;

    fn next_row(&self) -> Self;

    fn previous_row(&self) -> Self;

    fn minus(&self, other: Self) -> u32;
}

impl RowExt for DisplayRow {
    fn as_f64(&self) -> f64 {
        self.0 as f64
    }

    fn next_row(&self) -> Self {
        Self(self.0 + 1)
    }

    fn previous_row(&self) -> Self {
        Self(self.0.saturating_sub(1))
    }

    fn minus(&self, other: Self) -> u32 {
        self.0 - other.0
    }
}

impl RowExt for MultiBufferRow {
    fn as_f64(&self) -> f64 {
        self.0 as f64
    }

    fn next_row(&self) -> Self {
        Self(self.0 + 1)
    }

    fn previous_row(&self) -> Self {
        Self(self.0.saturating_sub(1))
    }

    fn minus(&self, other: Self) -> u32 {
        self.0 - other.0
    }
}

trait RowRangeExt {
    type Row;

    fn len(&self) -> usize;

    fn iter_rows(&self) -> impl DoubleEndedIterator<Item = Self::Row>;
}

impl RowRangeExt for Range<MultiBufferRow> {
    type Row = MultiBufferRow;

    fn len(&self) -> usize {
        (self.end.0 - self.start.0) as usize
    }

    fn iter_rows(&self) -> impl DoubleEndedIterator<Item = MultiBufferRow> {
        (self.start.0..self.end.0).map(MultiBufferRow)
    }
}

impl RowRangeExt for Range<DisplayRow> {
    type Row = DisplayRow;

    fn len(&self) -> usize {
        (self.end.0 - self.start.0) as usize
    }

    fn iter_rows(&self) -> impl DoubleEndedIterator<Item = DisplayRow> {
        (self.start.0..self.end.0).map(DisplayRow)
    }
}

/// If select range has more than one line, we
/// just point the cursor to range.start.
const fn collapse_multiline_range(range: Range<Point>) -> Range<Point> {
    if range.start.row == range.end.row { range } else { range.start..range.start }
}

const UPDATE_DEBOUNCE: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineHighlight {
    pub background: Background,
    pub border: Option<gpui::Hsla>,
    pub include_gutter: bool,
    pub type_id: Option<TypeId>,
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::sync::Arc;

    use gpui::Modifiers;
    use gpui::px;
    use gpui::size;

    use super::Addon;
    use super::ColumnarMode;
    use super::DiffViewStyle;
    use super::Editor;
    use super::EditorMode;
    use super::MinimapVisibility;
    use super::ScrollbarMarkerState;

    struct DefaultAddon;

    impl Addon for DefaultAddon {
        fn to_any(&self) -> &dyn Any {
            self
        }
    }

    #[test]
    fn addon_shows_project_file_actions_by_default() {
        assert!(DefaultAddon.shows_project_file_actions());
    }

    #[test]
    fn scrollbar_marker_refresh_reports_whether_markers_changed() {
        let quad =
            |x: f32| gpui::fill(gpui::Bounds::new(gpui::point(px(x), px(0.)), size(px(2.), px(2.))), gpui::red());
        let track_size = size(px(10.), px(2.));
        let mut state = ScrollbarMarkerState { dirty: true, ..ScrollbarMarkerState::default() };

        assert!(!state.finish_refresh(Arc::default(), track_size));
        assert_eq!(state.track_size, track_size);
        assert!(state.finish_refresh(Arc::from(vec![quad(1.)]), track_size));
        assert!(!state.finish_refresh(Arc::from(vec![quad(1.)]), track_size));
        assert!(state.finish_refresh(Arc::from(vec![quad(2.)]), track_size));
    }

    #[test]
    fn minimap_visible_by_default_only_in_full_editors() {
        assert!(MinimapVisibility::for_mode(&EditorMode::full()).visible());
        assert!(MinimapVisibility::for_mode(&EditorMode::SingleLine).disabled());
    }

    #[test]
    fn minimap_toggle_and_hide() {
        let shown = MinimapVisibility::for_mode(&EditorMode::full());
        let hidden = shown.hidden();
        assert!(!hidden.visible());
        assert!(!hidden.disabled());
        assert!(hidden.toggle_visibility().visible());
        assert!(!shown.toggle_visibility().visible());
    }

    #[test]
    fn columnar_selection_from_mouse_with_shift_and_secondary() {
        let modifiers = Modifiers { shift: true, ..Modifiers::secondary_key() };
        assert!(matches!(Editor::columnar_selection_mode(modifiers), Some(ColumnarMode::FromMouse)));
    }

    #[test]
    fn columnar_selection_from_selection_with_shift_and_alt() {
        let modifiers = Modifiers { shift: true, alt: true, ..Modifiers::default() };
        assert!(matches!(Editor::columnar_selection_mode(modifiers), Some(ColumnarMode::FromSelection)));
    }

    #[test]
    fn no_columnar_selection_without_shift_combo() {
        assert!(Editor::columnar_selection_mode(Modifiers { alt: true, ..Modifiers::default() }).is_none());
        assert!(Editor::columnar_selection_mode(Modifiers { shift: true, ..Modifiers::default() }).is_none());
        let three = Modifiers { shift: true, alt: true, control: true, ..Modifiers::default() };
        assert!(Editor::columnar_selection_mode(three).is_none());
    }

    #[test]
    fn diff_view_style_defaults_to_split() {
        assert_eq!(DiffViewStyle::default(), DiffViewStyle::Split);
    }

    #[test]
    fn disabled_minimap_stays_disabled() {
        let disabled = MinimapVisibility::Disabled;
        assert!(!disabled.visible());
        assert!(disabled.hidden().disabled());
        assert!(disabled.toggle_visibility().disabled());
    }
}
