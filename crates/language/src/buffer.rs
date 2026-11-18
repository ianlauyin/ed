use crate::fallback_highlight;
mod bracket_ranges;
pub mod row_chunk;

use std::any::Any;
use std::cell::Cell;
use std::cmp;
use std::cmp::Ordering;
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::future::Future;
use std::iter;
use std::iter::Iterator;
use std::iter::Peekable;
use std::mem;
use std::num::NonZeroU32;
use std::ops::Deref;
use std::ops::Range;
use std::path::PathBuf;
use std::rc;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use std::vec;

use anyhow::Result;
use async_macos::yield_now;
pub use bracket_ranges::BracketMatch;
use encoding_rs::Encoding;
use file_content::ByteContent;
use file_content::decode_byte_header;
use fs::MTime;
use futures::channel::oneshot;
use gpui::App;
use gpui::AppContext as _;
use gpui::Context;
use gpui::EventEmitter;
use gpui::HighlightStyle;
use gpui::SharedString;
use gpui::Task;
use language_core::highlight_cache::ChunkHighlightCache;
use language_core::highlight_cache::ResolvedHighlights;
pub use lsp::DiagnosticSeverity;
use lsp::LanguageServerId;
use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use sum_tree::TreeMap;
pub use text::Anchor;
pub use text::Bias;
pub use text::Buffer as TextBuffer;
pub use text::BufferId;
pub use text::BufferSnapshot as TextBufferSnapshot;
use text::ChunkBitmaps;
pub use text::Edit;
use text::FromAnchor;
use text::LineEnding;
pub use text::LineIndent;
pub use text::OffsetRangeExt;
pub use text::OffsetUtf16;
pub use text::Patch;
pub use text::Point;
pub use text::PointUtf16;
pub use text::Rope;
pub use text::Selection;
pub use text::SelectionGoal;
pub use text::Subscription;
pub use text::TextDimension;
pub use text::TextSummary;
pub use text::ToOffset;
pub use text::ToOffsetUtf16;
pub use text::ToPoint;
pub use text::ToPointUtf16;
pub use text::Transaction;
pub use text::TransactionId;
pub use text::Unclipped;
use theme::SyntaxTheme;
use util::RangeExt as _;
use util::rel_path::RelPath;

pub(crate) use crate::CaptureId;
use crate::FallbackHighlighter;
pub(crate) use crate::Grammar;
pub(crate) use crate::HighlightId;
pub(crate) use crate::HighlightMap;
pub(crate) use crate::Language;
pub(crate) use crate::LanguageRegistry;
use crate::LanguageScope;
use crate::Outline;
use crate::OutlineConfig;
use crate::PLAIN_TEXT;
use crate::RunnableTag;
use crate::TextObject;
use crate::TreeSitterOptions;
use crate::WorktreeId;
use crate::diagnostic_set::DiagnosticEntry;
use crate::diagnostic_set::DiagnosticEntryRef;
pub use crate::diagnostic_set::DiagnosticSet;
use crate::language_settings::LanguageSettings;
use crate::outline::OutlineItem;
use crate::row_chunk::RowChunkId;
use crate::row_chunk::RowChunks;
use crate::runnable;
use crate::runnable::RunnableRange;
use crate::syntax_map::MAX_BYTES_TO_QUERY;
use crate::syntax_map::SyntaxLayer;
use crate::syntax_map::SyntaxMap;
use crate::syntax_map::SyntaxMapCapture;
use crate::syntax_map::SyntaxMapCaptures;
use crate::syntax_map::SyntaxMapMatch;
use crate::syntax_map::SyntaxMapMatches;
use crate::syntax_map::SyntaxSnapshot;
use crate::syntax_map::ToTreeSitterPoint as _;
use crate::syntax_map::flattened_highlight_regions;
use crate::text_diff::text_diff;

/// Indicate whether a [`Buffer`] has permissions to edit.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum Capability {
    /// The buffer is a mutable replica.
    ReadWrite,
    /// The buffer is a read-only replica.
    ReadOnly,
}

impl Capability {
    /// Returns `true` if the capability is `ReadWrite`.
    pub const fn editable(self) -> bool {
        matches!(self, Capability::ReadWrite)
    }
}

pub type BufferRow = u32;

/// An in-memory representation of a source code file, including its text,
/// syntax trees, git status, and diagnostics.
pub struct Buffer {
    text: TextBuffer,
    /// Filesystem state, `None` when there is no path.
    file: Option<Arc<dyn File>>,
    /// The mtime of the file when this buffer was last loaded from
    /// or saved to disk.
    saved_mtime: Option<MTime>,
    /// The version vector when this buffer was last loaded from
    /// or saved to disk.
    saved_version: clock::Global,
    preview_version: clock::Global,
    transaction_depth: usize,
    was_dirty_before_starting_transaction: Option<bool>,
    reload_task: Option<Task<Result<()>>>,
    language: Option<Arc<Language>>,
    autoindent_requests: Vec<Arc<AutoindentRequest>>,
    wait_for_autoindent_txs: Vec<oneshot::Sender<Option<clock::Lamport>>>,
    pending_autoindent: Option<Task<()>>,
    sync_parse_timeout: Option<Duration>,
    syntax_map: Mutex<SyntaxMap>,
    reparse: Option<Task<()>>,
    /// Set by [`Buffer::defer_parsing`]: no syntax tree is built until
    /// [`Buffer::ensure_parsed`] is called or an edit needs autoindent.
    parse_deferred: bool,
    parse_status: (watch::Sender<ParseStatus>, watch::Receiver<ParseStatus>),
    non_text_state_update_count: usize,
    diagnostics: TreeMap<LanguageServerId, DiagnosticSet>,
    completion_triggers: BTreeSet<String>,
    completion_triggers_per_language_server: FxHashMap<LanguageServerId, BTreeSet<String>>,
    capability: Capability,
    has_conflict: bool,
    /// Memoize calls to has_changes_since(saved_version).
    /// The contents of a cell are (self.version, has_changes) at the time of a last call.
    has_unsaved_edits: Cell<(clock::Global, bool)>,
    change_bits: Vec<rc::Weak<Cell<bool>>>,
    _subscriptions: Vec<gpui::Subscription>,
    resolved_settings: Option<Arc<LanguageSettings>>,
    tree_sitter_data: Arc<TreeSitterData>,
    fallback_highlighter: Option<Arc<FallbackHighlighter>>,
    fallback_syntax_override: Option<&'static str>,
    encoding: &'static Encoding,
    has_bom: bool,
    reload_with_encoding_txns: FxHashMap<TransactionId, (&'static Encoding, bool)>,
}

#[derive(Debug)]
pub struct TreeSitterData {
    chunks: RowChunks,
    brackets_by_chunks: Mutex<FxHashMap<RowChunkId, Vec<BracketMatch>>>,
    highlights_by_chunks: ChunkHighlightCache,
}

pub(crate) const MAX_ROWS_IN_A_CHUNK: u32 = 50;
pub(crate) const MAX_BYTES_TO_HIGHLIGHT_IN_A_CHUNK: usize = 4 * MAX_BYTES_TO_QUERY;

impl TreeSitterData {
    fn clear(&mut self, snapshot: &text::BufferSnapshot) {
        self.chunks = RowChunks::new(snapshot, MAX_ROWS_IN_A_CHUNK);
        self.brackets_by_chunks.get_mut().clear();
        self.highlights_by_chunks.clear();
    }

    fn new(snapshot: &text::BufferSnapshot) -> Self {
        Self {
            chunks: RowChunks::new(snapshot, MAX_ROWS_IN_A_CHUNK),
            brackets_by_chunks: Mutex::new(FxHashMap::default()),
            highlights_by_chunks: ChunkHighlightCache::default(),
        }
    }

    const fn version(&self) -> &clock::Global {
        self.chunks.version()
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ParseStatus {
    Idle,
    Parsing,
}

/// An immutable, cheaply cloneable representation of a fixed
/// state of a buffer.
pub struct BufferSnapshot {
    pub text: text::BufferSnapshot,
    pub(crate) syntax: SyntaxSnapshot,
    tree_sitter_data: Arc<TreeSitterData>,
    fallback_highlighter: Option<Arc<FallbackHighlighter>>,
    diagnostics: TreeMap<LanguageServerId, DiagnosticSet>,
    language: Option<Arc<Language>>,
    file: Option<Arc<dyn File>>,
    non_text_state_update_count: usize,
    pub capability: Capability,
    resolved_settings: Option<Arc<LanguageSettings>>,
}

/// The kind and amount of indentation in a particular line. For now,
/// assumes that indentation is all the same character.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct IndentSize {
    /// The number of bytes that comprise the indentation.
    pub len: u32,
    /// The kind of whitespace used for indentation.
    pub kind: IndentKind,
}

/// A whitespace character that's used for indentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum IndentKind {
    /// An ASCII space character.
    #[default]
    Space,
    /// An ASCII tab character.
    Tab,
}

/// The shape of a selection cursor.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum CursorShape {
    /// A vertical bar.
    #[default]
    Bar,
    /// A block that surrounds the following character.
    Block,
    /// An underline that runs along the following character.
    Underline,
    /// A box drawn around the following character.
    Hollow,
}

/// An event that occurs in a buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BufferEvent {
    /// The buffer was edited.
    Edited,
    /// The buffer's `dirty` bit changed.
    DirtyChanged,
    /// The buffer's line ending changed.
    LineEndingChanged,
    /// The buffer was saved.
    Saved,
    /// The buffer's file was changed on disk.
    FileHandleChanged,
    /// The buffer was reloaded.
    Reloaded,
    /// The buffer is in need of a reload.
    ReloadNeeded,
    /// The buffer's language was changed.
    /// The boolean indicates whether this buffer did not have a language before, but does now.
    LanguageChanged(bool),
    /// The buffer's syntax trees were updated.
    Reparsed,
    /// The buffer's resolved language settings were changed.
    SettingsChanged,
    /// The buffer's diagnostics were updated.
    DiagnosticsUpdated,
    /// The buffer gained or lost editing capabilities.
    CapabilityChanged,
}

/// The file associated with a buffer.
pub trait File: Send + Sync + Any {
    /// Returns the [`LocalFile`] associated with this file, if the
    /// file is local.
    fn as_local(&self) -> Option<&dyn LocalFile>;

    /// Returns whether the file is new, exists in storage, or has been deleted. Includes metadata
    /// only available in some states, such as modification time.
    fn disk_state(&self) -> DiskState;

    /// Returns the path of this file relative to the worktree's root directory.
    fn path(&self) -> &Arc<RelPath>;

    /// Returns the path of this file relative to the worktree's parent directory (this means it
    /// includes the name of the worktree's root folder).
    fn full_path(&self, cx: &App) -> PathBuf;

    /// Returns the absolute path to this file on disk, if it has one.
    fn file_system_abs_path(&self, cx: &App) -> Option<PathBuf> {
        self.as_local().map(|file| file.abs_path(cx))
    }

    /// Returns the last component of this handle's absolute path. If this handle refers to the root
    /// of its worktree, then this method will return the name of the worktree itself.
    fn file_name<'a>(&'a self, cx: &'a App) -> &'a str;

    /// Returns the id of the worktree to which this file belongs.
    ///
    /// This is needed for looking up project-specific settings.
    fn worktree_id(&self, cx: &App) -> WorktreeId;

    fn can_open(&self) -> bool {
        true
    }

    /// Returns whether this file belongs to a worktree shown as a project root.
    /// Files elsewhere (e.g. external library sources) are labeled by their full path.
    fn is_in_visible_worktree(&self, _cx: &App) -> bool {
        true
    }
}

/// The file's storage status - whether it's stored (`Present`), and if so when it was last
/// modified. In the case where the file is not stored, it can be either `New` or `Deleted`. In the
/// UI these two states are distinguished. For example, the buffer tab does not display a deletion
/// indicator for new files.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DiskState {
    /// File created in ed that has not been saved.
    New,
    /// File present on the filesystem.
    Present { mtime: MTime, size: u64 },
    /// Deleted file that was previously present.
    Deleted,
    /// An old version of a file that was previously present
    /// usually from a version control system. e.g. A git blob.
    Historic { was_deleted: bool },
}

impl DiskState {
    /// Returns the file's last known modification time on disk.
    pub const fn mtime(self) -> Option<MTime> {
        match self {
            DiskState::New => None,
            DiskState::Present { mtime, .. } => Some(mtime),
            DiskState::Deleted => None,
            DiskState::Historic { .. } => None,
        }
    }

    /// Returns the file's size on disk in bytes.
    pub const fn size(self) -> Option<u64> {
        match self {
            DiskState::New => None,
            DiskState::Present { size, .. } => Some(size),
            DiskState::Deleted => None,
            DiskState::Historic { .. } => None,
        }
    }

    pub const fn exists(&self) -> bool {
        match self {
            DiskState::New => false,
            DiskState::Present { .. } => true,
            DiskState::Deleted => false,
            DiskState::Historic { .. } => false,
        }
    }

    /// Returns true if this state represents a deleted file.
    pub const fn is_deleted(&self) -> bool {
        match self {
            DiskState::Deleted => true,
            DiskState::Historic { was_deleted } => *was_deleted,
            _ => false,
        }
    }
}

/// The file associated with a buffer, in the case where the file is on the local disk.
pub trait LocalFile: File {
    /// Returns the absolute path of this file.
    fn abs_path(&self, cx: &App) -> PathBuf;

    /// Loads the file contents from disk and returns them as a UTF-8 encoded string.
    fn load(&self, cx: &App) -> Task<Result<String>>;

    /// Loads the file's contents from disk.
    fn load_bytes(&self, cx: &App) -> Task<Result<Vec<u8>>>;
}

/// The auto-indent behavior associated with an editing operation.
/// For some editing operations, each affected line of text has its
/// indentation recomputed. For other operations, the entire block
/// of edited text is adjusted uniformly.
#[derive(Clone, Debug)]
pub enum AutoindentMode {
    /// Indent each line of inserted text.
    EachLine,
    /// Autoindent multiline edits, but only apply syntax-triggered outdents to single-line edits.
    PreserveSingleLine,
    /// Apply the same indentation adjustment to all of the lines
    /// in a given insertion.
    Block {
        /// The original indentation column of the first line of each
        /// insertion, if it has been copied.
        ///
        /// Knowing this makes it possible to preserve the relative indentation
        /// of every line in the insertion from when it was copied.
        ///
        /// If the original indent column is `a`, and the first line of insertion
        /// is then auto-indented to column `b`, then every other line of
        /// the insertion will be auto-indented to column `b - a`.
        original_indent_columns: Vec<Option<u32>>,
    },
}

#[derive(Clone)]
struct AutoindentRequest {
    before_edit: BufferSnapshot,
    entries: Vec<AutoindentRequestEntry>,
    is_block_mode: bool,
    ignore_empty_lines: bool,
}

#[derive(Debug, Clone)]
struct AutoindentRequestEntry {
    /// A range of the buffer whose indentation should be adjusted.
    range: Range<Anchor>,
    /// The row of the edit start in the buffer before the edit was applied.
    /// This is stored here because the anchor in range is created after
    /// the edit, so it cannot be used with the before_edit snapshot.
    old_row: Option<u32>,
    indent_size: IndentSize,
    original_indent_column: Option<u32>,
    only_explicit_outdents: bool,
}

#[derive(Debug)]
struct IndentSuggestion {
    basis_row: u32,
    delta: Ordering,
    within_error: bool,
    explicit_outdent: bool,
}

struct BufferChunkHighlights<'a> {
    captures: SyntaxMapCaptures<'a>,
    next_capture: Option<SyntaxMapCapture<'a>>,
    stack: Vec<(usize, HighlightId)>,
    highlight_maps: Vec<HighlightMap>,
}

type HighlightRun = (Range<usize>, HighlightId);

struct CachedChunkHighlightsIter {
    runs: Vec<HighlightRun>,
    ix: usize,
}

/// An iterator that yields chunks of a buffer's text, along with their
/// syntax highlights and diagnostic status.
pub struct BufferChunks<'a> {
    buffer_snapshot: Option<&'a BufferSnapshot>,
    range: Range<usize>,
    chunks: text::Chunks<'a>,
    diagnostic_endpoints: Option<Peekable<vec::IntoIter<DiagnosticEndpoint>>>,
    error_depth: usize,
    warning_depth: usize,
    information_depth: usize,
    hint_depth: usize,
    unnecessary_depth: usize,
    underline: bool,
    highlights: Option<BufferChunkHighlights<'a>>,
    cached_highlights: Option<CachedChunkHighlightsIter>,
}

/// A chunk of a buffer's text, along with its syntax highlight and
/// diagnostic status.
#[derive(Clone, Debug, Default)]
pub struct Chunk<'a> {
    /// The text of the chunk.
    pub text: &'a str,
    /// The syntax highlighting style of the chunk.
    pub syntax_highlight_id: Option<HighlightId>,
    /// The highlight style that has been applied to this chunk in
    /// the editor.
    pub highlight_style: Option<HighlightStyle>,
    /// The severity of diagnostic associated with this chunk, if any.
    pub diagnostic_severity: Option<DiagnosticSeverity>,
    /// A bitset of which characters are tabs in this string.
    pub tabs: u128,
    /// Bitmap of character indices in this chunk.
    pub chars: u128,
    /// Bitmap of newline indices in this chunk.
    pub newlines: u128,
    /// Whether this chunk of text is marked as unnecessary.
    pub is_unnecessary: bool,
    /// Whether this chunk of text was originally a tab character.
    pub is_tab: bool,
    /// Whether this chunk of text was originally an inlay.
    pub is_inlay: bool,
    /// Whether to underline the corresponding text range in the editor.
    pub underline: bool,
}

/// A set of edits to a given version of a buffer, computed asynchronously.
#[derive(Debug, Clone)]
pub struct Diff {
    pub base_version: clock::Global,
    pub line_ending: LineEnding,
    pub edits: Vec<(Range<usize>, Arc<str>)>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DiagnosticEndpoint {
    offset: usize,
    is_start: bool,
    underline: bool,
    severity: DiagnosticSeverity,
    is_unnecessary: bool,
}

/// A class of characters, used for characterizing a run of text.
#[derive(Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Debug)]
pub enum CharKind {
    /// Whitespace.
    Whitespace,
    /// Punctuation.
    Punctuation,
    /// Word.
    Word,
}

/// Context for character classification within a specific scope.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum CharScopeContext {
    /// Character classification for completion queries.
    ///
    /// This context treats certain characters as word constituents that would
    /// normally be considered punctuation, such as '-' in Tailwind classes
    /// ("bg-yellow-100") or '.' in import paths ("foo.ts").
    Completion,
    /// Character classification for linked edits.
    ///
    /// This context handles characters that should be treated as part of
    /// identifiers during linked editing operations, such as '.' in JSX
    /// component names like `<Animated.View>`.
    LinkedEdit,
}

/// A runnable is a set of data about a region that could be resolved into a task.
pub struct Runnable {
    pub tags: SmallVec<[RunnableTag; 1]>,
    pub language: Arc<Language>,
    pub buffer: BufferId,
}

#[derive(Default, Clone, Debug)]
pub struct HighlightedText {
    pub text: SharedString,
    pub highlights: Vec<(Range<usize>, HighlightStyle)>,
}

#[derive(Default, Debug)]
pub struct HighlightedTextBuilder {
    text: String,
    highlights: Vec<(Range<usize>, HighlightStyle)>,
}

impl HighlightedText {}

impl HighlightedTextBuilder {
    pub fn build(self) -> HighlightedText {
        HighlightedText { text: self.text.into(), highlights: self.highlights }
    }
}

/// Which pre-existing line to exclude from auto-indentation when inserting
/// text.
#[derive(Clone, Copy, Debug)]
pub enum AutoIndentExclusion {
    /// Exclude the preceding line when the inserted text starts with a newline.
    PrecedingLine,
    /// Exclude the following line when the inserted text ends with a newline.
    FollowingLine,
}

impl Buffer {
    /// Create a new buffer with the given base text.
    pub fn local<T: Into<String>>(base_text: T, cx: &mut Context<Self>) -> Self {
        Self::build(
            TextBuffer::new(cx.entity_id().as_non_zero_u64().into(), base_text.into()),
            None,
            Capability::ReadWrite,
            cx,
        )
    }

    /// Assign a language to the buffer, blocking for up to 1ms to reparse the buffer, returning the buffer.
    pub fn with_language(mut self, language: Arc<Language>, cx: &mut Context<Self>) -> Self {
        self.set_language(Some(language), cx);
        self
    }

    /// Returns the [`Capability`] of this buffer.
    pub const fn capability(&self) -> Capability {
        self.capability
    }

    /// Whether this buffer can only be read.
    pub const fn read_only(&self) -> bool {
        !self.capability.editable()
    }

    /// Builds a [`Buffer`] with the given underlying [`TextBuffer`], diff base, [`File`] and [`Capability`].
    pub fn build(
        buffer: TextBuffer,
        file: Option<Arc<dyn File>>,
        capability: Capability,
        cx: &mut Context<Self>,
    ) -> Self {
        let saved_mtime = file.as_ref().and_then(|file| file.disk_state().mtime());
        let snapshot = buffer.snapshot();
        let syntax_map = Mutex::new(SyntaxMap::new(snapshot));
        let tree_sitter_data = TreeSitterData::new(snapshot);
        let mut this = Self {
            saved_mtime,
            tree_sitter_data: Arc::new(tree_sitter_data),
            fallback_highlighter: None,
            fallback_syntax_override: None,
            saved_version: buffer.version(),
            preview_version: buffer.version(),
            reload_task: None,
            transaction_depth: 0,
            was_dirty_before_starting_transaction: None,
            has_unsaved_edits: Cell::new((buffer.version(), false)),
            text: buffer,
            file,
            capability,
            syntax_map,
            reparse: None,
            parse_deferred: false,
            non_text_state_update_count: 0,
            sync_parse_timeout: Some(Duration::from_millis(1)),
            parse_status: watch::channel(ParseStatus::Idle),
            autoindent_requests: Vec::default(),
            wait_for_autoindent_txs: Vec::default(),
            pending_autoindent: Option::default(),
            language: None,
            diagnostics: TreeMap::default(),
            completion_triggers: BTreeSet::default(),
            completion_triggers_per_language_server: FxHashMap::default(),
            has_conflict: false,
            change_bits: Vec::default(),
            _subscriptions: Vec::new(),
            resolved_settings: None,
            encoding: encoding_rs::UTF_8,
            has_bom: false,
            reload_with_encoding_txns: FxHashMap::default(),
        };
        this.resolved_settings = Some(this.compute_resolved_settings(cx));
        this.refresh_fallback_highlighter(cx);
        this
    }

    fn compute_resolved_settings(&self, cx: &App) -> Arc<LanguageSettings> {
        LanguageSettings::resolve_uncached(self, None, cx)
    }

    fn refresh_resolved_settings(&mut self, cx: &mut Context<Self>) {
        let resolved = self.compute_resolved_settings(cx);
        if Some(&*resolved) != self.resolved_settings.as_deref() {
            self.resolved_settings = Some(resolved);
            self.non_text_state_update_count += 1;
            self.was_changed();
            cx.emit(BufferEvent::SettingsChanged);
            cx.notify();
        }
    }

    pub(crate) const fn resolved_settings(&self) -> Option<&Arc<LanguageSettings>> {
        self.resolved_settings.as_ref()
    }

    /// Retrieve a snapshot of the buffer's current state. This is computationally
    /// cheap, and allows reading from the buffer on a background thread.
    pub fn snapshot(&self) -> BufferSnapshot {
        let text = self.text.snapshot();

        let syntax = {
            let mut syntax_map = self.syntax_map.lock();
            syntax_map.interpolate(text);
            syntax_map.snapshot()
        };

        let tree_sitter_data = if self.text.version() == *self.tree_sitter_data.version() {
            Arc::clone(&self.tree_sitter_data)
        } else {
            Arc::new(TreeSitterData::new(text))
        };

        BufferSnapshot {
            text: text.clone(),
            syntax,
            tree_sitter_data,
            fallback_highlighter: self.fallback_highlighter.clone(),
            file: self.file.clone(),
            diagnostics: self.diagnostics.clone(),
            language: self.language.clone(),
            non_text_state_update_count: self.non_text_state_update_count,
            capability: self.capability,
            resolved_settings: self.resolved_settings.clone(),
        }
    }

    pub fn as_text_snapshot(&self) -> &text::BufferSnapshot {
        &self.text
    }

    /// Retrieve a snapshot of the buffer's raw text, without any
    /// language-related state like the syntax tree or diagnostics.
    pub fn text_snapshot(&self) -> text::BufferSnapshot {
        // todo lw
        self.text.snapshot().clone()
    }

    /// The file associated with the buffer, if any.
    pub fn file(&self) -> Option<&Arc<dyn File>> {
        self.file.as_ref()
    }

    /// The version of the buffer that was last saved or reloaded from disk.
    pub const fn saved_version(&self) -> &clock::Global {
        &self.saved_version
    }

    /// The mtime of the buffer's file when the buffer was last saved or reloaded from disk.
    pub const fn saved_mtime(&self) -> Option<MTime> {
        self.saved_mtime
    }

    /// Returns the character encoding of the buffer's file.
    pub const fn encoding(&self) -> &'static Encoding {
        self.encoding
    }

    /// Sets the character encoding of the buffer.
    pub const fn set_encoding(&mut self, encoding: &'static Encoding) {
        self.encoding = encoding;
    }

    /// Returns whether the buffer has a Byte Order Mark.
    pub const fn has_bom(&self) -> bool {
        self.has_bom
    }

    /// Sets whether the buffer has a Byte Order Mark.
    pub const fn set_has_bom(&mut self, has_bom: bool) {
        self.has_bom = has_bom;
    }

    /// Assign a language to the buffer.
    pub fn set_language_async(&mut self, language: Option<Arc<Language>>, cx: &mut Context<Self>) {
        self.set_language_(language, false, cx);
    }

    /// Assign a language to the buffer, blocking for up to 1ms to reparse the buffer.
    pub fn set_language(&mut self, language: Option<Arc<Language>>, cx: &mut Context<Self>) {
        self.set_language_(language, true, cx);
    }

    fn set_language_(&mut self, language: Option<Arc<Language>>, may_block: bool, cx: &mut Context<Self>) {
        if language == self.language {
            return;
        }
        self.non_text_state_update_count += 1;
        self.syntax_map.lock().clear(&self.text);
        Self::invalidate_tree_sitter_data(&mut self.tree_sitter_data, self.text.snapshot());
        let old_language = mem::replace(&mut self.language, language);
        self.refresh_fallback_highlighter(cx);
        self.refresh_resolved_settings(cx);
        self.was_changed();
        self.reparse(cx, may_block);
        let has_fresh_language = self.language.is_some() && old_language.is_none_or(|old| old == *PLAIN_TEXT);
        cx.emit(BufferEvent::LanguageChanged(has_fresh_language));
    }

    /// Highlights this buffer with the named bundled fallback syntax (e.g.
    /// "Regular Expression") instead of picking one from its language or file.
    pub fn set_fallback_syntax(&mut self, name: Option<&'static str>, cx: &mut Context<Self>) {
        if self.fallback_syntax_override == name {
            return;
        }
        self.fallback_syntax_override = name;
        self.refresh_fallback_highlighter(cx);
        self.was_changed();
        self.non_text_state_update_count += 1;
        cx.notify();
    }

    fn refresh_fallback_highlighter(&mut self, cx: &App) {
        let highlighter = if let Some(name) = self.fallback_syntax_override {
            FallbackHighlighter::for_syntax_name(name, &self.text)
        } else {
            // `file_name` (not `path`) so a file opened as its own worktree root
            // still has a name to detect the syntax from.
            let file_name = self.file.as_ref().map(|file| file.file_name(cx));
            FallbackHighlighter::for_buffer(self.language.as_deref(), file_name, &self.text)
        };
        let unchanged = match (&self.fallback_highlighter, &highlighter) {
            (Some(old), Some(new)) => old.syntax_name() == new.syntax_name(),
            (None, None) => true,
            _ => false,
        };
        if !unchanged {
            self.fallback_highlighter = highlighter.map(Arc::new);
        }
    }

    /// Assign a language registry to the buffer. This allows the buffer to retrieve
    /// other languages if parts of the buffer are written in different languages.
    pub fn set_language_registry(&self, language_registry: Arc<LanguageRegistry>) {
        self.syntax_map.lock().set_language_registry(language_registry);
    }

    pub fn language_registry(&self) -> Option<Arc<LanguageRegistry>> {
        self.syntax_map.lock().language_registry()
    }

    /// Assign the line ending type to the buffer.
    pub fn set_line_ending(&mut self, line_ending: LineEnding, cx: &mut Context<Self>) {
        self.text.set_line_ending(line_ending);
        self.was_changed();
        cx.emit(BufferEvent::LineEndingChanged);
    }

    /// Assign the buffer a new [`Capability`].
    pub fn set_capability(&mut self, capability: Capability, cx: &mut Context<Self>) {
        if self.capability != capability {
            self.capability = capability;
            // Multibuffers resnapshot on this, so their per-excerpt edit filter sees the new capability.
            self.non_text_state_update_count += 1;
            cx.emit(BufferEvent::CapabilityChanged);
            cx.notify();
        }
    }

    /// This method is called to signal that the buffer has been saved.
    pub fn did_save(&mut self, version: clock::Global, mtime: Option<MTime>, cx: &mut Context<Self>) {
        self.saved_version.clone_from(&version);
        self.has_unsaved_edits.set((version, false));
        self.has_conflict = false;
        self.saved_mtime = mtime;
        self.was_changed();
        cx.emit(BufferEvent::Saved);
        cx.notify();
    }

    /// Reloads the contents of the buffer from disk.
    pub fn reload(&mut self, cx: &Context<Self>) -> oneshot::Receiver<Option<Transaction>> {
        self.reload_impl(None, cx)
    }

    /// Reloads the contents of the buffer from disk using the specified encoding.
    ///
    /// This bypasses automatic encoding detection heuristics (like BOM checks) for non-Unicode encodings,
    /// allowing users to force a specific interpretation of the bytes.
    pub fn reload_with_encoding(
        &mut self,
        encoding: &'static Encoding,
        cx: &Context<Self>,
    ) -> oneshot::Receiver<Option<Transaction>> {
        self.reload_impl(Some(encoding), cx)
    }

    fn reload_impl(
        &mut self,
        force_encoding: Option<&'static Encoding>,
        cx: &Context<Self>,
    ) -> oneshot::Receiver<Option<Transaction>> {
        let (tx, rx) = oneshot::channel();
        let prev_version = self.text.version();

        self.reload_task = Some(cx.spawn(async move |this, cx| {
            let Some((new_mtime, load_bytes_task, current_encoding)) = this.update(cx, |this, cx| {
                let file = this.file.as_ref()?.as_local()?;
                Some((file.disk_state().mtime(), file.load_bytes(cx), this.encoding))
            })?
            else {
                return Ok(());
            };

            let target_encoding = force_encoding.unwrap_or(current_encoding);

            let bytes = load_bytes_task.await?;

            anyhow::ensure!(decode_byte_header(&bytes).1 != ByteContent::Binary, "Binary files are not supported");

            let is_unicode = target_encoding == encoding_rs::UTF_8
                || target_encoding == encoding_rs::UTF_16LE
                || target_encoding == encoding_rs::UTF_16BE;

            let (new_text, has_bom, encoding_used) = if force_encoding.is_some() && !is_unicode {
                let (cow, _had_errors) = target_encoding.decode_without_bom_handling(&bytes);
                (cow.into_owned(), false, target_encoding)
            } else {
                let (cow, used_enc, _had_errors) = target_encoding.decode(&bytes);

                let actual_has_bom = if used_enc == encoding_rs::UTF_8 {
                    bytes.starts_with(&[0xEF, 0xBB, 0xBF])
                } else if used_enc == encoding_rs::UTF_16LE {
                    bytes.starts_with(&[0xFF, 0xFE])
                } else if used_enc == encoding_rs::UTF_16BE {
                    bytes.starts_with(&[0xFE, 0xFF])
                } else {
                    false
                };
                (cow.into_owned(), actual_has_bom, used_enc)
            };

            let diff = this.update(cx, |this, cx| this.diff(new_text, cx))?.await;
            this.update(cx, |this, cx| {
                if this.version() == diff.base_version {
                    this.finalize_last_transaction();
                    let old_encoding = this.encoding;
                    let old_has_bom = this.has_bom;
                    this.apply_diff(diff, cx);
                    this.encoding = encoding_used;
                    this.has_bom = has_bom;
                    let transaction = this.finalize_last_transaction().cloned();
                    if let Some(ref txn) = transaction
                        && (old_encoding != encoding_used || old_has_bom != has_bom)
                    {
                        this.reload_with_encoding_txns.insert(txn.id, (old_encoding, old_has_bom));
                    }
                    tx.send(transaction).ok();
                    this.has_conflict = false;
                    this.did_reload(this.version(), this.line_ending(), new_mtime, cx);
                } else {
                    if !diff.edits.is_empty() || this.edits_since::<usize>(&diff.base_version).next().is_some() {
                        this.has_conflict = true;
                    }

                    this.did_reload(prev_version, this.line_ending(), this.saved_mtime, cx);
                }

                this.reload_task.take();
            })
        }));
        rx
    }

    /// This method is called to signal that the buffer has been reloaded.
    pub fn did_reload(
        &mut self,
        version: clock::Global,
        line_ending: LineEnding,
        mtime: Option<MTime>,
        cx: &mut Context<Self>,
    ) {
        self.saved_version = version;
        self.has_unsaved_edits.set((self.saved_version.clone(), false));
        self.text.set_line_ending(line_ending);
        self.saved_mtime = mtime;
        cx.emit(BufferEvent::Reloaded);
        cx.notify();
    }

    /// Updates the [`File`] backing this buffer. This should be called when
    /// the file has changed or has been deleted.
    pub fn file_updated(&mut self, new_file: Arc<dyn File>, cx: &mut Context<Self>) {
        let was_dirty = self.is_dirty();
        let mut file_changed = false;

        if let Some(old_file) = self.file.as_ref() {
            if new_file.path() != old_file.path() {
                file_changed = true;
            }

            let old_state = old_file.disk_state();
            let new_state = new_file.disk_state();
            if old_state != new_state {
                file_changed = true;
                if !was_dirty && matches!(new_state, DiskState::Present { .. }) {
                    cx.emit(BufferEvent::ReloadNeeded);
                }
            }
        } else {
            file_changed = true;
        }

        let clear_contents = self.file.as_ref().is_some_and(|old_file| {
            clears_on_file_deletion(
                matches!(old_file.disk_state(), DiskState::Present { .. }),
                new_file.disk_state() == DiskState::Deleted,
                was_dirty,
            )
        });
        self.file = Some(new_file);
        if clear_contents && !self.is_empty() {
            // A clean buffer mirrors its file, so a deleted file leaves it empty,
            // like any other external change. Undo brings the text back.
            self.finalize_last_transaction();
            self.edit([(0..self.len(), "")], None, cx);
            self.finalize_last_transaction();
            self.did_reload(self.version(), self.line_ending(), None, cx);
        }
        if file_changed {
            self.refresh_fallback_highlighter(cx);
            self.refresh_resolved_settings(cx);
            self.was_changed();
            self.non_text_state_update_count += 1;
            if was_dirty != self.is_dirty() {
                cx.emit(BufferEvent::DirtyChanged);
            }
            cx.emit(BufferEvent::FileHandleChanged);
            cx.notify();
        }
    }

    /// Returns the name of the syntax used for fallback highlighting, if this
    /// buffer has no tree-sitter grammar but a bundled syntax matches it.
    pub fn fallback_syntax_name(&self) -> Option<&str> {
        Some(self.fallback_highlighter.as_ref()?.syntax_name())
    }

    /// Returns the primary [`Language`] assigned to this [`Buffer`].
    pub const fn language(&self) -> Option<&Arc<Language>> {
        self.language.as_ref()
    }

    /// Returns the [`Language`] at the given location.
    pub fn language_at<D: ToOffset>(&self, position: D) -> Option<Arc<Language>> {
        let offset = position.to_offset(self);
        let text: &TextBufferSnapshot = &self.text;
        self.syntax_map
            .lock()
            .layers_for_range(offset..offset, text, false)
            .filter(|layer| layer.included_sub_ranges.is_none_or(|ranges| offset_in_sub_ranges(ranges, offset, text)))
            .last()
            .map(|info| Arc::clone(info.language))
            .or_else(|| self.language.clone())
    }

    /// Returns each [`Language`] for the active syntax layers at the given location.
    pub fn languages_at<D: ToOffset>(&self, position: D) -> Vec<Arc<Language>> {
        let offset = position.to_offset(self);
        let text: &TextBufferSnapshot = &self.text;
        let mut languages: Vec<Arc<Language>> = self
            .syntax_map
            .lock()
            .layers_for_range(offset..offset, text, false)
            .filter(|layer| {
                // For combined injections, check if offset is within the actual sub-ranges.
                layer.included_sub_ranges.is_none_or(|ranges| offset_in_sub_ranges(ranges, offset, text))
            })
            .map(|info| Arc::clone(info.language))
            .collect();

        if languages.is_empty()
            && let Some(buffer_language) = self.language()
        {
            languages.push(Arc::clone(buffer_language));
        }

        languages
    }

    /// An integer version number that accounts for all updates besides
    /// the buffer's text itself (which is versioned via a version vector).
    pub const fn non_text_state_update_count(&self) -> usize {
        self.non_text_state_update_count
    }

    /// Indicates whether the buffer contains any regions that may be
    /// written in a language that hasn't been loaded yet.
    pub fn contains_unknown_injections(&self) -> bool {
        self.syntax_map.lock().contains_unknown_injections()
    }

    fn invalidate_tree_sitter_data(tree_sitter_data: &mut Arc<TreeSitterData>, snapshot: &text::BufferSnapshot) {
        if let Some(tree_sitter_data) = Arc::get_mut(tree_sitter_data) {
            tree_sitter_data.clear(snapshot);
        } else {
            let new_tree_sitter_data = TreeSitterData::new(snapshot);
            *tree_sitter_data = Arc::new(new_tree_sitter_data);
        }
    }

    /// Stops this buffer from building a syntax tree until it is needed. Used
    /// for buffers loaded in bulk (project search, multi-file diffs), most of
    /// which are never displayed.
    pub const fn defer_parsing(&mut self) {
        self.parse_deferred = true;
    }

    pub const fn is_parse_deferred(&self) -> bool {
        self.parse_deferred
    }

    /// Starts parsing a buffer whose parsing was deferred. Cheap when it
    /// already parses.
    pub fn ensure_parsed(&mut self, cx: &mut Context<Self>) {
        if self.parse_deferred {
            self.parse_deferred = false;
            self.reparse(cx, true);
        }
    }

    /// Called after an edit to synchronize the buffer's main parse tree with
    /// the buffer's new underlying state.
    ///
    /// Locks the syntax map and interpolates the edits since the last reparse
    /// into the foreground syntax tree.
    ///
    /// Then takes a stable snapshot of the syntax map before unlocking it.
    /// The snapshot with the interpolated edits is sent to a background thread,
    /// where we ask Tree-sitter to perform an incremental parse.
    ///
    /// Meanwhile, in the foreground if `may_block` is true, we block the main
    /// thread for up to 1ms waiting on the parse to complete. As soon as it
    /// completes, we proceed synchronously, unless a 1ms timeout elapses.
    ///
    /// If we time out waiting on the parse, we spawn a second task waiting
    /// until the parse does complete and return with the interpolated tree still
    /// in the foreground. When the background parse completes, call back into
    /// the main thread and assign the foreground parse state.
    ///
    /// If the buffer or grammar changed since the start of the background parse,
    /// initiate an additional reparse recursively. To avoid concurrent parses
    /// for the same buffer, we only initiate a new parse if we are not already
    /// parsing in the background.
    pub fn reparse(&mut self, cx: &mut Context<Self>, may_block: bool) {
        if self.text.version() != *self.tree_sitter_data.version() {
            Self::invalidate_tree_sitter_data(&mut self.tree_sitter_data, self.text.snapshot());
        }
        if self.reparse.is_some() {
            return;
        }
        let Some(language) = self.language.clone() else {
            return;
        };
        if self.parse_deferred {
            return;
        }

        let text = self.text_snapshot();
        let parsed_version = self.version();

        let mut syntax_map = self.syntax_map.lock();
        syntax_map.interpolate(&text);
        let language_registry = syntax_map.language_registry();
        let mut syntax_snapshot = syntax_map.snapshot();
        drop(syntax_map);
        let file = self.file.clone();

        self.parse_status.0.send(ParseStatus::Parsing).expect("parse status receiver is owned by the buffer");
        if may_block
            && let Some(sync_parse_timeout) = self.sync_parse_timeout
            && matches!(
                syntax_snapshot.reparse_with_timeout(
                    &text,
                    file.as_deref(),
                    language_registry.clone(),
                    Arc::clone(&language),
                    sync_parse_timeout,
                ),
                Ok(())
            )
        {
            self.did_finish_parsing(syntax_snapshot, Some(Duration::from_millis(300)), false, cx);
            self.reparse = None;
            return;
        }

        let parse_task = cx.background_spawn({
            let language = Arc::clone(&language);
            let language_registry = language_registry.clone();
            async move {
                syntax_snapshot.reparse(&text, file.as_deref(), language_registry, language);
                syntax_snapshot
            }
        });

        self.reparse = Some(cx.spawn(async move |this, cx| {
            let new_syntax_map = parse_task.await;
            this.update(cx, move |this, cx| {
                let grammar_changed =
                    || this.language.as_ref().is_none_or(|current_language| !Arc::ptr_eq(&language, current_language));
                let language_registry_changed = || {
                    new_syntax_map.contains_unknown_injections()
                        && language_registry
                            .is_some_and(|registry| registry.version() != new_syntax_map.language_registry_version())
                };
                let parse_again =
                    this.version.changed_since(&parsed_version) || language_registry_changed() || grammar_changed();
                this.did_finish_parsing(new_syntax_map, None, parse_again, cx);
                this.reparse = None;
                if parse_again {
                    this.reparse(cx, false);
                }
            })
            .ok();
        }));
    }

    fn did_finish_parsing(
        &mut self,
        syntax_snapshot: SyntaxSnapshot,
        block_budget: Option<Duration>,
        parse_again: bool,
        cx: &mut Context<Self>,
    ) {
        self.non_text_state_update_count += 1;
        self.syntax_map.lock().did_parse(syntax_snapshot);
        self.was_changed();

        let parsing_complete = !parse_again || self.language.is_none();
        if self.language.is_none() {
            self.syntax_map.lock().clear(&self.text);
        }
        if parsing_complete {
            self.request_autoindent(cx, block_budget);
            self.parse_status.0.send(ParseStatus::Idle).expect("parse status receiver is owned by the buffer");
        }
        Self::invalidate_tree_sitter_data(&mut self.tree_sitter_data, self.text.snapshot());
        cx.emit(BufferEvent::Reparsed);
        cx.notify();
    }

    pub fn parse_status(&self) -> watch::Receiver<ParseStatus> {
        self.parse_status.1.clone()
    }

    /// Wait until the buffer is no longer parsing.
    pub fn parsing_idle(&self) -> impl Future<Output = ()> + use<> {
        let mut parse_status = self.parse_status();
        async move {
            while *parse_status.borrow() != ParseStatus::Idle {
                if parse_status.changed().await.is_err() {
                    break;
                }
            }
        }
    }

    /// Assign to the buffer a set of diagnostics created by a given language server.
    pub fn update_diagnostics(
        &mut self,
        server_id: LanguageServerId,
        diagnostics: DiagnosticSet,
        cx: &mut Context<Self>,
    ) {
        if diagnostics.is_empty() {
            self.diagnostics.remove(&server_id);
        } else {
            self.diagnostics.insert(server_id, diagnostics);
        }
        self.non_text_state_update_count += 1;
        self.was_changed();
        cx.notify();
        cx.emit(BufferEvent::DiagnosticsUpdated);
    }

    pub fn buffer_diagnostics(&self, for_server: Option<LanguageServerId>) -> Vec<&DiagnosticEntry<Anchor>> {
        match for_server {
            Some(server_id) => {
                self.diagnostics.get(&server_id).map_or_else(Vec::new, |diagnostics| diagnostics.iter().collect())
            }
            None => self.diagnostics.iter().flat_map(|(_, diagnostic_set)| diagnostic_set.iter()).collect(),
        }
    }

    fn request_autoindent(&mut self, cx: &mut Context<Self>, block_budget: Option<Duration>) {
        if let Some(indent_sizes) = self.compute_autoindents() {
            let indent_sizes = cx.background_spawn(indent_sizes);
            let Some(block_budget) = block_budget else {
                self.pending_autoindent = Some(cx.spawn(async move |this, cx| {
                    let indent_sizes = indent_sizes.await;
                    this.update(cx, |this, cx| {
                        this.apply_autoindents(indent_sizes, cx);
                    })
                    .ok();
                }));
                return;
            };
            match cx.foreground_executor().block_with_timeout(block_budget, indent_sizes) {
                Ok(indent_sizes) => self.apply_autoindents(indent_sizes, cx),
                Err(indent_sizes) => {
                    self.pending_autoindent = Some(cx.spawn(async move |this, cx| {
                        let indent_sizes = indent_sizes.await;
                        this.update(cx, |this, cx| {
                            this.apply_autoindents(indent_sizes, cx);
                        })
                        .ok();
                    }));
                }
            }
        } else {
            self.autoindent_requests.clear();
            for tx in self.wait_for_autoindent_txs.drain(..) {
                tx.send(None).ok();
            }
        }
    }

    fn compute_autoindents(&self) -> Option<impl Future<Output = BTreeMap<u32, IndentSize>> + use<>> {
        let max_rows_between_yields = 100;
        let snapshot = self.snapshot();
        if snapshot.syntax.is_empty() || self.autoindent_requests.is_empty() {
            return None;
        }

        let autoindent_requests = self.autoindent_requests.clone();
        Some(async move {
            let mut indent_sizes = BTreeMap::<u32, (IndentSize, bool)>::new();
            for request in autoindent_requests {
                // Resolve each edited range to its row in the current buffer and in the
                // buffer before this batch of edits.
                let mut row_ranges = Vec::new();
                let mut old_to_new_rows = BTreeMap::new();
                let mut language_indent_sizes_by_new_row = Vec::new();
                for entry in &request.entries {
                    let position = entry.range.start;
                    let new_row = position.to_point(&snapshot).row;
                    let new_end_row = entry.range.end.to_point(&snapshot).row + 1;
                    language_indent_sizes_by_new_row.push((new_row, entry.indent_size));

                    if let Some(old_row) = entry.old_row {
                        old_to_new_rows.insert(old_row, new_row);
                    }
                    row_ranges.push((new_row..new_end_row, entry.original_indent_column, entry.only_explicit_outdents));
                }

                // Build a map containing the suggested indentation for each of the edited lines
                // with respect to the state of the buffer before these edits. This map is keyed
                // by the rows for these lines in the current state of the buffer.
                let mut old_suggestions = BTreeMap::<u32, (IndentSize, bool)>::default();
                let old_edited_ranges = contiguous_ranges(old_to_new_rows.keys().copied(), max_rows_between_yields);
                let mut language_indent_sizes = language_indent_sizes_by_new_row.iter().peekable();
                let mut language_indent_size = IndentSize::default();
                for old_edited_range in old_edited_ranges {
                    let suggestions =
                        request.before_edit.suggest_autoindents(old_edited_range.clone()).into_iter().flatten();
                    for (old_row, suggestion) in old_edited_range.zip(suggestions) {
                        if let Some(suggestion) = suggestion {
                            let new_row = old_to_new_rows[&old_row];

                            // Find the indent size based on the language for this row.
                            while let Some((row, size)) = language_indent_sizes.peek() {
                                if *row > new_row {
                                    break;
                                }
                                language_indent_size = *size;
                                language_indent_sizes.next();
                            }

                            let suggested_indent = old_to_new_rows
                                .get(&suggestion.basis_row)
                                .and_then(|from_row| Some(old_suggestions.get(from_row).copied()?.0))
                                .unwrap_or_else(|| {
                                    request.before_edit.logical_indent_size_for_line(suggestion.basis_row)
                                })
                                .with_delta(suggestion.delta, language_indent_size);
                            old_suggestions.insert(new_row, (suggested_indent, suggestion.within_error));
                        }
                    }
                    yield_now().await;
                }

                // Compute new suggestions for each line, but only include them in the result
                // if they differ from the old suggestion for that line.
                let mut language_indent_sizes = language_indent_sizes_by_new_row.iter().peekable();
                let mut language_indent_size = IndentSize::default();
                for (row_range, original_indent_column, only_explicit_outdents) in row_ranges {
                    let new_edited_row_range =
                        if request.is_block_mode { row_range.start..row_range.start + 1 } else { row_range.clone() };

                    let suggestions = snapshot.suggest_autoindents(new_edited_row_range.clone()).into_iter().flatten();
                    for (new_row, suggestion) in new_edited_row_range.zip(suggestions) {
                        if let Some(suggestion) = suggestion {
                            // Find the indent size based on the language for this row.
                            while let Some((row, size)) = language_indent_sizes.peek() {
                                if *row > new_row {
                                    break;
                                }
                                language_indent_size = *size;
                                language_indent_sizes.next();
                            }

                            let suggested_indent = indent_sizes
                                .get(&suggestion.basis_row)
                                .copied()
                                .map_or_else(|| snapshot.logical_indent_size_for_line(suggestion.basis_row), |e| e.0)
                                .with_delta(suggestion.delta, language_indent_size);

                            if old_suggestions.get(&new_row).is_none_or(|(old_indentation, was_within_error)| {
                                suggested_indent != *old_indentation && (!suggestion.within_error || *was_within_error)
                            }) && (!only_explicit_outdents || suggestion.explicit_outdent)
                            {
                                indent_sizes.insert(new_row, (suggested_indent, request.ignore_empty_lines));
                            }
                        }
                    }

                    if let (true, Some(original_indent_column)) = (request.is_block_mode, original_indent_column) {
                        let new_indent = if let Some((indent, _)) = indent_sizes.get(&row_range.start) {
                            *indent
                        } else {
                            snapshot.indent_size_for_line(row_range.start)
                        };
                        let delta = new_indent.len as i64 - original_indent_column as i64;
                        if delta != 0 {
                            for row in row_range.skip(1) {
                                indent_sizes.entry(row).or_insert_with(|| {
                                    let mut size = snapshot.indent_size_for_line(row);
                                    // A line with no indentation has an arbitrary
                                    // indent kind, so it can adopt the new kind.
                                    if size.len == 0 {
                                        size.kind = new_indent.kind;
                                    }
                                    if size.kind == new_indent.kind {
                                        match delta.cmp(&0) {
                                            Ordering::Greater => size.len += delta as u32,
                                            Ordering::Less => size.len = size.len.saturating_sub(-delta as u32),
                                            Ordering::Equal => {}
                                        }
                                    }
                                    (size, request.ignore_empty_lines)
                                });
                            }
                        }
                    }

                    yield_now().await;
                }
            }

            indent_sizes
                .into_iter()
                .filter_map(
                    |(row, (indent, ignore_empty_lines))| {
                        if ignore_empty_lines && snapshot.line_len(row) == 0 { None } else { Some((row, indent)) }
                    },
                )
                .collect()
        })
    }

    fn apply_autoindents(&mut self, indent_sizes: BTreeMap<u32, IndentSize>, cx: &mut Context<Self>) {
        self.autoindent_requests.clear();
        let wait_for_autoindent_txs = mem::take(&mut self.wait_for_autoindent_txs);

        let edits: Vec<_> = indent_sizes
            .into_iter()
            .filter_map(|(row, indent_size)| {
                let current_size = indent_size_for_line(self, row);
                Self::edit_for_indent_size_adjustment(row, current_size, indent_size)
            })
            .collect();

        let preserve_preview = self.preserve_preview();
        let edit_id = self.edit(edits, None, cx);
        if preserve_preview {
            self.refresh_preview();
        }
        for tx in wait_for_autoindent_txs {
            tx.send(edit_id).ok();
        }
    }

    /// Create a minimal edit that will cause the given row to be indented
    /// with the given size. After applying this edit, the length of the line
    /// will always be at least `new_size.len`.
    pub fn edit_for_indent_size_adjustment(
        row: u32,
        current_size: IndentSize,
        new_size: IndentSize,
    ) -> Option<(Range<Point>, String)> {
        if new_size.kind == current_size.kind {
            match new_size.len.cmp(&current_size.len) {
                Ordering::Greater => {
                    let point = Point::new(row, 0);
                    Some((
                        point..point,
                        iter::repeat_n(new_size.char(), (new_size.len - current_size.len) as usize).collect::<String>(),
                    ))
                }

                Ordering::Less => {
                    Some((Point::new(row, 0)..Point::new(row, current_size.len - new_size.len), String::new()))
                }

                Ordering::Equal => None,
            }
        } else {
            Some((
                Point::new(row, 0)..Point::new(row, current_size.len),
                iter::repeat_n(new_size.char(), new_size.len as usize).collect::<String>(),
            ))
        }
    }

    /// Spawns a background task that asynchronously computes a `Diff` between the buffer's text
    /// and the given new text.
    pub fn diff<T>(&self, new_text: T, cx: &App) -> Task<Diff>
    where
        T: AsRef<str> + Send + 'static,
    {
        let old_text = self.as_rope().clone();
        let base_version = self.version();
        cx.background_spawn(async move {
            let old_text = old_text.to_string();
            let mut new_text = new_text.as_ref().to_owned();
            let line_ending = LineEnding::detect(&new_text);
            LineEnding::normalize(&mut new_text);
            let edits = text_diff(&old_text, &new_text);
            Diff { base_version, line_ending, edits }
        })
    }

    /// Applies a diff to the buffer. If the buffer has changed since the given diff was
    /// calculated, then adjust the diff to account for those changes, and discard any
    /// parts of the diff that conflict with those changes.
    pub fn apply_diff(&mut self, diff: Diff, cx: &mut Context<Self>) -> Option<TransactionId> {
        let snapshot = self.snapshot();
        let mut edits_since = snapshot.edits_since::<usize>(&diff.base_version).peekable();
        let mut delta = 0;
        let adjusted_edits = diff.edits.into_iter().filter_map(|(range, new_text)| {
            while let Some(edit_since) = edits_since.peek() {
                // If the edit occurs after a diff hunk, then it does not
                // affect that hunk.
                if edit_since.old.start > range.end {
                    break;
                }
                // If the edit precedes the diff hunk, then adjust the hunk
                // to reflect the edit.
                else if edit_since.old.end < range.start {
                    delta += (edit_since.new_len() as u64).cast_signed() - (edit_since.old_len() as u64).cast_signed();
                    edits_since.next();
                }
                // If the edit intersects a diff hunk, then discard that hunk.
                else {
                    return None;
                }
            }

            let start = ((range.start as u64).cast_signed() + delta) as usize;
            let end = ((range.end as u64).cast_signed() + delta) as usize;
            Some((start..end, new_text))
        });

        self.start_transaction();
        self.text.set_line_ending(diff.line_ending);
        self.edit(adjusted_edits, None, cx);
        self.end_transaction(cx)
    }

    pub fn has_unsaved_edits(&self) -> bool {
        let (last_version, has_unsaved_edits) = self.has_unsaved_edits.take();

        if last_version == self.version {
            self.has_unsaved_edits.set((last_version, has_unsaved_edits));
            return has_unsaved_edits;
        }

        let has_edits = self.has_edits_since(&self.saved_version);
        self.has_unsaved_edits.set((self.version.clone(), has_edits));
        has_edits
    }

    /// Checks if the buffer has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        if self.capability == Capability::ReadOnly {
            return false;
        }
        if self.has_conflict {
            return true;
        }
        match self.file.as_ref().map(|f| f.disk_state()) {
            Some(DiskState::New | DiskState::Deleted) => !self.is_empty() && self.has_unsaved_edits(),
            _ => self.has_unsaved_edits(),
        }
    }

    /// Checks if the buffer and its file have both changed since the buffer
    /// was last saved or reloaded.
    pub fn has_conflict(&self) -> bool {
        if self.has_conflict {
            return true;
        }
        let Some(file) = self.file.as_ref() else {
            return false;
        };
        match file.disk_state() {
            DiskState::New => false,
            DiskState::Present { mtime, .. } => match self.saved_mtime {
                Some(saved_mtime) => mtime.bad_is_greater_than(saved_mtime) && self.has_unsaved_edits(),
                None => true,
            },
            DiskState::Deleted => false,
            DiskState::Historic { .. } => false,
        }
    }

    /// Gets a [`Subscription`] that tracks all of the changes to the buffer's text.
    pub fn subscribe(&mut self) -> Subscription<usize> {
        self.text.subscribe()
    }

    /// Adds a bit to the list of bits that are set when the buffer's text changes.
    ///
    /// This allows downstream code to check if the buffer's text has changed without
    /// waiting for an effect cycle, which would be required if using eents.
    pub fn record_changes(&mut self, bit: rc::Weak<Cell<bool>>) {
        if let Err(ix) = self.change_bits.binary_search_by_key(&rc::Weak::as_ptr(&bit), rc::Weak::as_ptr) {
            self.change_bits.insert(ix, bit);
        }
    }

    /// Set the change bit for all "listeners".
    fn was_changed(&mut self) {
        self.change_bits.retain(|change_bit| {
            change_bit
                .upgrade()
                .inspect(|bit| {
                    _ = bit.replace(true);
                })
                .is_some()
        });
    }

    /// Starts a transaction, if one is not already in-progress. When undoing or
    /// redoing edits, all of the edits performed within a transaction are undone
    /// or redone together.
    pub fn start_transaction(&mut self) -> Option<TransactionId> {
        self.start_transaction_at(Instant::now())
    }

    /// Starts a transaction, providing the current time. Subsequent transactions
    /// that occur within a short period of time will be grouped together. This
    /// is controlled by the buffer's undo grouping duration.
    pub fn start_transaction_at(&mut self, now: Instant) -> Option<TransactionId> {
        self.transaction_depth += 1;
        if self.was_dirty_before_starting_transaction.is_none() {
            self.was_dirty_before_starting_transaction = Some(self.is_dirty());
        }
        self.text.start_transaction_at(now)
    }

    /// Terminates the current transaction, if this is the outermost transaction.
    pub fn end_transaction(&mut self, cx: &mut Context<Self>) -> Option<TransactionId> {
        self.end_transaction_at(Instant::now(), cx)
    }

    /// Terminates the current transaction, providing the current time. Subsequent transactions
    /// that occur within a short period of time will be grouped together. This
    /// is controlled by the buffer's undo grouping duration.
    pub fn end_transaction_at(&mut self, now: Instant, cx: &mut Context<Self>) -> Option<TransactionId> {
        self.end_transaction_at_internal(now, cx)
    }

    fn end_transaction_at_internal(&mut self, now: Instant, cx: &mut Context<Self>) -> Option<TransactionId> {
        assert!(self.transaction_depth > 0);
        self.transaction_depth -= 1;
        let was_dirty = if self.transaction_depth == 0 {
            self.was_dirty_before_starting_transaction.take().expect("transaction was started")
        } else {
            false
        };
        if let Some((transaction_id, start_version)) = self.text.end_transaction_at(now) {
            self.did_edit(&start_version, was_dirty, cx);
            Some(transaction_id)
        } else {
            None
        }
    }

    /// Manually add a transaction to the buffer's undo history.
    pub fn push_transaction(&mut self, transaction: Transaction, now: Instant) {
        self.text.push_transaction(transaction, now);
    }

    /// Differs from `push_transaction` in that it does not clear the redo
    /// stack. Intended to be used to create a parent transaction to merge
    /// potential child transactions into.
    ///
    /// The caller is responsible for removing it from the undo history using
    /// `forget_transaction` if no edits are merged into it. Otherwise, if edits
    /// are merged into this transaction, the caller is responsible for ensuring
    /// the redo stack is cleared. The easiest way to ensure the redo stack is
    /// cleared is to create transactions with the usual `start_transaction` and
    /// `end_transaction` methods and merging the resulting transactions into
    /// the transaction created by this method.
    pub fn push_empty_transaction(&mut self, now: Instant) -> TransactionId {
        self.text.push_empty_transaction(now)
    }

    /// Prevent the last transaction from being grouped with any subsequent transactions,
    /// even if they occur with the buffer's undo grouping duration.
    pub fn finalize_last_transaction(&mut self) -> Option<&Transaction> {
        self.text.finalize_last_transaction()
    }

    /// Manually group all changes since a given transaction.
    pub fn group_until_transaction(&mut self, transaction_id: TransactionId) {
        self.text.group_until_transaction(transaction_id);
    }

    /// Manually remove a transaction from the buffer's undo history.
    pub fn forget_transaction(&mut self, transaction_id: TransactionId) -> Option<Transaction> {
        self.text.forget_transaction(transaction_id)
    }

    /// Retrieve a transaction from the buffer's undo history.
    pub fn get_transaction(&self, transaction_id: TransactionId) -> Option<&Transaction> {
        self.text.get_transaction(transaction_id)
    }

    /// Manually merge two transactions in the buffer's undo history.
    pub fn merge_transactions(&mut self, transaction: TransactionId, destination: TransactionId) {
        self.text.merge_transactions(transaction, destination);
    }

    /// Resolves with the id of the edit that applied the pending autoindent, if it changed anything.
    pub fn wait_for_autoindent_applied(&mut self) -> Option<oneshot::Receiver<Option<clock::Lamport>>> {
        if self.autoindent_requests.is_empty() {
            return None;
        }
        let (tx, rx) = oneshot::channel();
        self.wait_for_autoindent_txs.push(tx);
        Some(rx)
    }

    /// Replaces the buffer's entire text.
    pub fn set_text<T>(&mut self, text: T, cx: &mut Context<Self>) -> Option<clock::Lamport>
    where
        T: Into<Arc<str>>,
    {
        self.autoindent_requests.clear();
        self.edit([(0..self.len(), text)], None, cx)
    }

    /// Appends the given text to the end of the buffer.
    pub fn append<T>(&mut self, text: T, cx: &mut Context<Self>) -> Option<clock::Lamport>
    where
        T: Into<Arc<str>>,
    {
        self.edit([(self.len()..self.len(), text)], None, cx)
    }

    /// Applies the given edits to the buffer. Each edit is specified as a range of text to
    /// delete, and a string of text to insert at that location. Adjacent edits are coalesced.
    /// Inserted text is normalized to LF line endings before being applied.
    ///
    /// If an [`AutoindentMode`] is provided, then the buffer will enqueue an auto-indent
    /// request for the edited ranges, which will be processed when the buffer finishes
    /// parsing.
    ///
    /// Parsing takes place at the end of a transaction, and may compute synchronously
    /// or asynchronously, depending on the changes.
    pub fn edit<I, S, T>(
        &mut self,
        edits_iter: I,
        autoindent_mode: Option<AutoindentMode>,
        cx: &mut Context<Self>,
    ) -> Option<clock::Lamport>
    where
        I: IntoIterator<Item = (Range<S>, T)>,
        S: ToOffset,
        T: Into<Arc<str>>,
    {
        self.edit_internal(edits_iter, autoindent_mode, true, AutoIndentExclusion::PrecedingLine, cx)
    }

    /// Like [`edit`](Self::edit), but preserves the following line's
    /// indentation when the inserted text ends with a newline.
    pub fn edit_before<I, S, T>(
        &mut self,
        edits_iter: I,
        autoindent_mode: Option<AutoindentMode>,
        cx: &mut Context<Self>,
    ) -> Option<clock::Lamport>
    where
        I: IntoIterator<Item = (Range<S>, T)>,
        S: ToOffset,
        T: Into<Arc<str>>,
    {
        self.edit_internal(edits_iter, autoindent_mode, true, AutoIndentExclusion::FollowingLine, cx)
    }

    /// Like [`edit`](Self::edit), but does not coalesce adjacent edits.
    pub fn edit_non_coalesce<I, S, T>(
        &mut self,
        edits_iter: I,
        autoindent_mode: Option<AutoindentMode>,
        cx: &mut Context<Self>,
    ) -> Option<clock::Lamport>
    where
        I: IntoIterator<Item = (Range<S>, T)>,
        S: ToOffset,
        T: Into<Arc<str>>,
    {
        self.edit_internal(edits_iter, autoindent_mode, false, AutoIndentExclusion::PrecedingLine, cx)
    }

    fn edit_internal<I, S, T>(
        &mut self,
        edits_iter: I,
        autoindent_mode: Option<AutoindentMode>,
        coalesce_adjacent: bool,
        autoindent_exclusion: AutoIndentExclusion,
        cx: &mut Context<Self>,
    ) -> Option<clock::Lamport>
    where
        I: IntoIterator<Item = (Range<S>, T)>,
        S: ToOffset,
        T: Into<Arc<str>>,
    {
        // Skip invalid edits and coalesce contiguous ones.
        let mut edits: Vec<(Range<usize>, Arc<str>)> = Vec::new();

        for (range, new_text) in edits_iter {
            let mut range = range.start.to_offset(self)..range.end.to_offset(self);

            if range.start > range.end {
                mem::swap(&mut range.start, &mut range.end);
            }
            let new_text = new_text.into();
            if !new_text.is_empty() || !range.is_empty() {
                let prev_edit = edits.last_mut();
                let should_coalesce = prev_edit.as_ref().is_some_and(|(prev_range, _)| {
                    if coalesce_adjacent { prev_range.end >= range.start } else { prev_range.end > range.start }
                });

                if let Some((prev_range, prev_text)) = prev_edit
                    && should_coalesce
                {
                    prev_range.end = cmp::max(prev_range.end, range.end);
                    *prev_text = format!("{prev_text}{new_text}").into();
                } else {
                    edits.push((range, new_text));
                }
            }
        }
        if edits.is_empty() {
            return None;
        }

        self.start_transaction();
        self.pending_autoindent.take();
        let autoindent_request = autoindent_mode
            .filter(|_| autoindents_edits(self.language.is_some(), self.parse_deferred))
            .map(|mode| (self.snapshot(), mode));

        let edit_id = self.text.edit(edits.iter().cloned());

        if let Some((before_edit, mode)) = autoindent_request {
            let mut delta = 0_isize;
            let entries: Vec<_> = edits
                .into_iter()
                .enumerate()
                .map(|(ix, (range, new_text))| {
                    let new_text = LineEnding::normalize_arc(new_text);
                    let new_text_length = new_text.len();
                    let old_start = range.start.to_point(&before_edit);
                    let old_end = range.end.to_point(&before_edit);
                    let new_start = (delta + range.start.cast_signed()) as usize;
                    let range_len = range.end - range.start;
                    delta += new_text_length.cast_signed() - range_len.cast_signed();

                    // Decide what range of the insertion to auto-indent, and whether
                    // the first line of the insertion should be considered a newly-inserted line
                    // or an edit to an existing line.
                    let mut range_of_insertion_to_indent = 0..new_text_length;
                    let mut first_line_is_new = true;

                    let old_line_start = before_edit.indent_size_for_line(old_start.row).len;
                    let old_line_end = before_edit.line_len(old_start.row);

                    if old_start.column > old_line_start {
                        first_line_is_new = false;
                    }

                    if !new_text.contains('\n') && (old_end.row == old_start.row || old_line_end == old_line_start) {
                        first_line_is_new = false;
                    }

                    // When the insertion introduces a line boundary, exclude
                    // the adjacent pre-existing line from auto-indentation.
                    match autoindent_exclusion {
                        AutoIndentExclusion::PrecedingLine => {
                            if new_text.starts_with('\n') {
                                range_of_insertion_to_indent.start += 1;
                                first_line_is_new = true;
                            }
                        }
                        AutoIndentExclusion::FollowingLine => {
                            if new_text.ends_with('\n') {
                                range_of_insertion_to_indent.end -= 1;
                                first_line_is_new = true;
                            }
                        }
                    }

                    let mut original_indent_column = None;
                    if let AutoindentMode::Block { original_indent_columns } = &mode {
                        original_indent_column = Some(if new_text.starts_with('\n') {
                            indent_size_for_text(new_text[range_of_insertion_to_indent.clone()].chars()).len
                        } else {
                            original_indent_columns.get(ix).copied().flatten().unwrap_or_else(|| {
                                indent_size_for_text(new_text[range_of_insertion_to_indent.clone()].chars()).len
                            })
                        });

                        // Avoid auto-indenting the line after the edit.
                        if new_text[range_of_insertion_to_indent.clone()].ends_with('\n') {
                            range_of_insertion_to_indent.end -= 1;
                        }
                    }

                    AutoindentRequestEntry {
                        only_explicit_outdents: matches!(mode, AutoindentMode::PreserveSingleLine)
                            && old_start.row == old_end.row
                            && !new_text.contains('\n'),
                        original_indent_column,
                        old_row: if first_line_is_new { None } else { Some(old_start.row) },
                        indent_size: before_edit.language_indent_size_at(range.start, cx),
                        range: self.anchor_before(new_start + range_of_insertion_to_indent.start)
                            ..self.anchor_after(new_start + range_of_insertion_to_indent.end),
                    }
                })
                .collect();

            if !entries.is_empty() {
                self.autoindent_requests.push(Arc::new(AutoindentRequest {
                    before_edit,
                    entries,
                    is_block_mode: matches!(mode, AutoindentMode::Block { .. }),
                    ignore_empty_lines: false,
                }));
            }
        }

        self.end_transaction(cx);
        self.was_changed();
        Some(edit_id)
    }

    fn did_edit(&mut self, old_version: &clock::Global, was_dirty: bool, cx: &mut Context<Self>) {
        self.was_changed();

        if self.edits_since::<usize>(old_version).next().is_none() {
            return;
        }

        self.pending_autoindent.take();
        self.reparse(cx, true);
        cx.emit(BufferEvent::Edited);
        let is_dirty = self.is_dirty();
        if was_dirty != is_dirty {
            cx.emit(BufferEvent::DirtyChanged);
        }
        if was_dirty
            && !is_dirty
            && let Some(file) = self.file.as_ref()
            && matches!(file.disk_state(), DiskState::Present { .. })
            && file.disk_state().mtime() != self.saved_mtime
        {
            cx.emit(BufferEvent::ReloadNeeded);
        }
        cx.notify();
    }

    pub fn autoindent_ranges<I, T>(&mut self, ranges: I, cx: &mut Context<Self>)
    where
        I: IntoIterator<Item = Range<T>>,
        T: ToOffset + Copy,
    {
        let before_edit = self.snapshot();
        let entries = ranges
            .into_iter()
            .map(|range| AutoindentRequestEntry {
                range: before_edit.anchor_before(range.start)..before_edit.anchor_after(range.end),
                old_row: None,
                indent_size: before_edit.language_indent_size_at(range.start, cx),
                original_indent_column: None,
                only_explicit_outdents: false,
            })
            .collect();
        self.autoindent_requests.push(Arc::new(AutoindentRequest {
            before_edit,
            entries,
            is_block_mode: false,
            ignore_empty_lines: true,
        }));
        self.request_autoindent(cx, Some(Duration::from_micros(300)));
    }

    /// Undoes the most recent transaction.
    pub fn undo(&mut self, cx: &mut Context<Self>) -> Option<TransactionId> {
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();

        if let Some(transaction_id) = self.text.undo() {
            self.did_edit(&old_version, was_dirty, cx);
            self.restore_encoding_for_transaction(transaction_id, was_dirty);
            Some(transaction_id)
        } else {
            None
        }
    }

    /// Manually undoes a specific transaction in the buffer's undo history.
    pub fn undo_transaction(&mut self, transaction_id: TransactionId, cx: &mut Context<Self>) -> bool {
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();
        let undone = self.text.undo_transaction(transaction_id);
        if undone {
            self.did_edit(&old_version, was_dirty, cx);
        }
        undone
    }

    /// Manually undoes all changes after a given transaction in the buffer's undo history.
    pub fn undo_to_transaction(&mut self, transaction_id: TransactionId, cx: &mut Context<Self>) -> bool {
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();

        let undone = self.text.undo_to_transaction(transaction_id);
        if undone {
            self.did_edit(&old_version, was_dirty, cx);
        }
        undone
    }

    /// Manually redoes a specific transaction in the buffer's redo history.
    pub fn redo(&mut self, cx: &mut Context<Self>) -> Option<TransactionId> {
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();

        if let Some(transaction_id) = self.text.redo() {
            self.did_edit(&old_version, was_dirty, cx);
            self.restore_encoding_for_transaction(transaction_id, was_dirty);
            Some(transaction_id)
        } else {
            None
        }
    }

    fn restore_encoding_for_transaction(&mut self, transaction_id: TransactionId, was_dirty: bool) {
        if let Some((old_encoding, old_has_bom)) = self.reload_with_encoding_txns.get(&transaction_id) {
            let current_encoding = self.encoding;
            let current_has_bom = self.has_bom;
            self.encoding = *old_encoding;
            self.has_bom = *old_has_bom;
            if !was_dirty {
                self.saved_version = self.version.clone();
                self.has_unsaved_edits.set((self.saved_version.clone(), false));
            }
            self.reload_with_encoding_txns.insert(transaction_id, (current_encoding, current_has_bom));
        }
    }

    /// Manually undoes all changes until a given transaction in the buffer's redo history.
    pub fn redo_to_transaction(&mut self, transaction_id: TransactionId, cx: &mut Context<Self>) -> bool {
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();

        let redone = self.text.redo_to_transaction(transaction_id);
        if redone {
            self.did_edit(&old_version, was_dirty, cx);
        }
        redone
    }

    /// Override current completion triggers with the user-provided completion triggers.
    pub fn set_completion_triggers(
        &mut self,
        server_id: LanguageServerId,
        triggers: BTreeSet<String>,
        cx: &mut Context<Self>,
    ) {
        if triggers.is_empty() {
            self.completion_triggers_per_language_server.remove(&server_id);
        } else {
            self.completion_triggers_per_language_server.insert(server_id, triggers);
        }
        self.completion_triggers = self
            .completion_triggers_per_language_server
            .values()
            .flat_map(|triggers| triggers.iter().cloned())
            .collect();
        self.was_changed();
        cx.notify();
    }

    /// Returns a list of strings which trigger a completion menu for this language.
    /// Usually this is driven by LSP server which returns a list of trigger characters for completions.
    pub const fn completion_triggers(&self) -> &BTreeSet<String> {
        &self.completion_triggers
    }

    /// Call this directly after performing edits to prevent the preview tab
    /// from being dismissed by those edits. It causes `should_dismiss_preview`
    /// to return false until there are additional edits.
    pub fn refresh_preview(&mut self) {
        self.preview_version = self.version.clone();
    }

    /// Whether we should preserve the preview status of a tab containing this buffer.
    pub fn preserve_preview(&self) -> bool {
        !self.has_edits_since(&self.preview_version)
    }

    pub const fn set_group_interval(&mut self, group_interval: Duration) {
        self.text.set_group_interval(group_interval);
    }

    // TODO: see if ep can use this instead of Buffer::branch
    pub fn snapshot_with_edits<I, S, T>(&mut self, edits: I, cx: &mut Context<Self>) -> Task<EditedBufferSnapshot>
    where
        I: IntoIterator<Item = (Range<S>, T)>,
        S: ToOffset,
        T: Into<Arc<str>>,
    {
        let mut snapshot = self.snapshot();
        let text = snapshot.text.clone();
        let mut syntax = snapshot.syntax.clone();
        let language = self.language().cloned();
        let registry = self.language_registry();
        let new_text = self.text.snapshot_with_edits(edits);
        cx.background_spawn(async move {
            if let Some(language) = language.clone() {
                syntax.reparse(&text, snapshot.file.as_deref(), registry.clone(), language);
            }

            syntax.interpolate(&new_text.snapshot);

            if let Some(language) = language {
                syntax.reparse(&new_text.snapshot, snapshot.file.as_deref(), registry, language);
            }

            snapshot.text = new_text.snapshot.clone();
            snapshot.syntax = syntax;
            snapshot.tree_sitter_data = Arc::new(TreeSitterData::new(&snapshot.text));

            EditedBufferSnapshot { text: new_text, snapshot }
        })
    }

    pub fn fast_forward(&mut self, edited: EditedBufferSnapshot, cx: &mut Context<Self>) {
        let base_version = edited.text.base_version.clone();
        let did_edit = edited.text.did_edit;
        self.text.fast_forward(edited.text);
        if edited.snapshot.language == self.language {
            self.reparse = None;
            self.did_finish_parsing(edited.snapshot.syntax, None, false, cx);
            if did_edit {
                cx.emit(BufferEvent::Edited);
            }
        } else {
            self.did_edit(&base_version, false, cx);
        }
    }
}

pub struct EditedBufferSnapshot {
    text: text::EditedBufferSnapshot,
    snapshot: BufferSnapshot,
}

impl EditedBufferSnapshot {
    pub const fn snapshot(&self) -> &BufferSnapshot {
        &self.snapshot
    }

    pub const fn base_version(&self) -> &clock::Global {
        &self.text.base_version
    }
}

impl EventEmitter<BufferEvent> for Buffer {}

fn offset_in_sub_ranges(sub_ranges: &[Range<Anchor>], offset: usize, snapshot: &TextBufferSnapshot) -> bool {
    let start_anchor = snapshot.anchor_before(offset);
    let end_anchor = snapshot.anchor_after(offset);

    sub_ranges.iter().any(|sub_range| {
        let is_before_start = sub_range.end.cmp(&start_anchor, snapshot).is_lt();
        let is_after_end = sub_range.start.cmp(&end_anchor, snapshot).is_gt();
        !is_before_start && !is_after_end
    })
}

impl Deref for Buffer {
    type Target = TextBuffer;

    fn deref(&self) -> &Self::Target {
        &self.text
    }
}

impl BufferSnapshot {
    /// Returns [`IndentSize`] for a given line that respects user settings and
    /// language preferences.
    pub fn indent_size_for_line(&self, row: u32) -> IndentSize {
        indent_size_for_line(self, row)
    }

    /// The indentation that the block comment closed on `position`'s row belongs
    /// at, or `None` if that row is not the closing line of a multi-line block
    /// comment.
    ///
    /// A closing delimiter is conventionally indented one column past its opening
    /// delimiter, so that it lines up with the comment's prefixes:
    ///
    /// ```text
    /// /**
    ///  * doc
    ///  */
    /// ```
    ///
    /// That extra column belongs to the comment rather than to the surrounding
    /// code, which makes the closing row's own indentation a poor basis for
    /// indenting whatever follows it. The opening row's indentation is used
    /// instead.
    ///
    /// Requires the row to hold nothing but whitespace and the closing delimiter,
    /// and `position` to be at or past the end of that delimiter, so that
    /// splitting the delimiter itself is left alone. Also requires the syntax node
    /// containing the delimiter to end there, so that a line which merely looks
    /// like a closing delimiter is not mistaken for one.
    pub fn block_comment_closing_indent(&self, position: Point) -> Option<IndentSize> {
        let row = position.row;
        let indent_len = self.indent_size_for_line(row).len;
        let delimiter_start = Point::new(row, indent_len);
        let language = self.language_scope_at(delimiter_start)?;
        // A few languages describe a string literal in `block_comment` rather
        // than a comment (Python's `"""`), so either scope is accepted. Relying
        // on the override scope keeps this out of the business of guessing at
        // grammar node names.
        if !matches!(language.override_name(), Some("comment" | "string")) {
            return None;
        }

        let delimiter_len =
            [language.documentation_comment(), language.block_comment()].into_iter().flatten().find_map(|config| {
                let delimiter = config.end.trim_start();
                if delimiter.is_empty() {
                    return None;
                }
                let mut chars = self.chars_at(delimiter_start);
                if !delimiter.chars().all(|expected| chars.next() == Some(expected)) {
                    return None;
                }
                if !chars.take_while(|c| *c != '\n').all(char::is_whitespace) {
                    return None;
                }
                Some(delimiter.len() as u32)
            })?;
        if position.column < indent_len + delimiter_len {
            return None;
        }

        let delimiter_end = Point::new(row, indent_len + delimiter_len);
        let node = self.syntax_ancestor(delimiter_start..delimiter_end)?;
        if Point::from_ts_point(node.end_position()) != delimiter_end {
            return None;
        }
        let opening_row = Point::from_ts_point(node.start_position()).row;
        (opening_row < row).then(|| self.indent_size_for_line(opening_row))
    }

    /// Like [`Self::indent_size_for_line`], but reports the indentation a row
    /// logically sits at, which differs from its physical indentation on the
    /// closing line of a block comment. See [`Self::block_comment_closing_indent`].
    fn logical_indent_size_for_line(&self, row: u32) -> IndentSize {
        self.block_comment_closing_indent(Point::new(row, self.line_len(row)))
            .unwrap_or_else(|| self.indent_size_for_line(row))
    }

    /// Returns [`IndentSize`] for a given position that respects user settings
    /// and language preferences.
    pub fn language_indent_size_at<T: ToOffset>(&self, position: T, cx: &App) -> IndentSize {
        let settings = self.settings_at(position, cx);
        if settings.hard_tabs { IndentSize::tab() } else { IndentSize::spaces(settings.tab_size.get()) }
    }

    /// Retrieve the suggested indent size for all of the given rows. The unit of indentation
    /// is passed in as `single_indent_size`.
    pub fn suggested_indents(
        &self,
        rows: impl Iterator<Item = u32>,
        single_indent_size: IndentSize,
    ) -> BTreeMap<u32, IndentSize> {
        let mut result = BTreeMap::new();

        for row_range in contiguous_ranges(rows, 10) {
            let Some(suggestions) = self.suggest_autoindents(row_range.clone()) else {
                break;
            };

            for (row, suggestion) in row_range.zip(suggestions) {
                let indent_size = if let Some(suggestion) = suggestion {
                    result
                        .get(&suggestion.basis_row)
                        .copied()
                        .unwrap_or_else(|| self.logical_indent_size_for_line(suggestion.basis_row))
                        .with_delta(suggestion.delta, single_indent_size)
                } else {
                    self.indent_size_for_line(row)
                };

                result.insert(row, indent_size);
            }
        }

        result
    }

    fn suggest_autoindents(
        &self,
        row_range: Range<u32>,
    ) -> Option<impl Iterator<Item = Option<IndentSuggestion>> + '_> {
        let config = &self.language.as_ref()?.config;
        let prev_non_blank_row = self.prev_non_blank_row(row_range.start);

        #[derive(Debug, Clone)]
        struct StartPosition {
            start: Point,
            suffix: SharedString,
            language: Arc<Language>,
        }

        // Find the suggested indentation ranges based on the syntax tree.
        let start = Point::new(prev_non_blank_row.unwrap_or(row_range.start), 0);
        let end = Point::new(row_range.end, 0);
        let range = (start..end).to_offset(&self.text);
        let mut matches = self.syntax.matches_with_options(
            range.clone(),
            &self.text,
            TreeSitterOptions { max_bytes_to_query: Some(MAX_BYTES_TO_QUERY), max_start_depth: None },
            |grammar| Some(&grammar.indents_config.as_ref()?.query),
        );
        let indent_configs = matches
            .grammars()
            .iter()
            .map(|grammar| grammar.indents_config.as_ref().expect("grammar in indent match set has indents_config"))
            .collect::<Vec<_>>();

        let mut indent_ranges = Vec::<Range<Point>>::new();
        let mut start_positions = Vec::<StartPosition>::new();
        let mut outdent_positions = Vec::<Point>::new();
        while let Some(mat) = matches.peek() {
            let mut start: Option<Point> = None;
            let mut end: Option<Point> = None;

            let config = indent_configs[mat.grammar_index];
            for capture in mat.captures {
                if capture.index == config.indent_capture_ix {
                    start.get_or_insert_with(|| Point::from_ts_point(capture.node.start_position()));
                    end.get_or_insert_with(|| Point::from_ts_point(capture.node.end_position()));
                } else if Some(capture.index) == config.start_capture_ix {
                    start = Some(Point::from_ts_point(capture.node.end_position()));
                } else if Some(capture.index) == config.end_capture_ix {
                    end = Some(Point::from_ts_point(capture.node.start_position()));
                } else if Some(capture.index) == config.outdent_capture_ix {
                    outdent_positions.push(Point::from_ts_point(capture.node.start_position()));
                } else if let Some(suffix) = config.suffixed_start_captures.get(&capture.index) {
                    start_positions.push(StartPosition {
                        start: Point::from_ts_point(capture.node.start_position()),
                        suffix: suffix.clone(),
                        language: Arc::clone(&mat.language),
                    });
                }
            }

            matches.advance();
            if let Some((start, end)) = start.zip(end) {
                if start.row == end.row {
                    continue;
                }
                let range = start..end;
                match indent_ranges.binary_search_by_key(&range.start, |r| r.start) {
                    Err(ix) => indent_ranges.insert(ix, range),
                    Ok(ix) => {
                        let prev_range = &mut indent_ranges[ix];
                        prev_range.end = prev_range.end.max(range.end);
                    }
                }
            }
        }

        let mut error_ranges = Vec::<Range<Point>>::new();
        let mut matches = self.syntax.matches(range, &self.text, |grammar| grammar.error_query.as_ref());
        while let Some(mat) = matches.peek() {
            let node = mat.captures[0].node;
            let start = Point::from_ts_point(node.start_position());
            let end = Point::from_ts_point(node.end_position());
            let range = start..end;
            let ix = match error_ranges.binary_search_by_key(&range.start, |r| r.start) {
                Ok(ix) | Err(ix) => ix,
            };
            let mut end_ix = ix;
            while let Some(existing_range) = error_ranges.get(end_ix) {
                if existing_range.end < end {
                    end_ix += 1;
                } else {
                    break;
                }
            }
            error_ranges.splice(ix..end_ix, [range]);
            matches.advance();
        }

        outdent_positions.sort();
        for outdent_position in outdent_positions {
            // find the innermost indent range containing this outdent_position
            // set its end to the outdent position
            if let Some(range_to_truncate) =
                indent_ranges.iter_mut().rfind(|indent_range| indent_range.contains(&outdent_position))
            {
                range_to_truncate.end = outdent_position;
            }
        }

        start_positions.sort_by_key(|b| b.start);

        // Find the suggested indentation increases and decreased based on regexes.
        let mut regex_outdent_map = FxHashMap::default();
        let mut last_seen_suffix: FxHashMap<String, Vec<StartPosition>> = FxHashMap::default();
        let mut start_positions_iter = start_positions.iter().peekable();

        let mut indent_change_rows = Vec::<(u32, Ordering)>::new();
        self.for_each_line(
            Point::new(prev_non_blank_row.unwrap_or(row_range.start), 0)..Point::new(row_range.end, 0),
            |row, line| {
                let indent_len = self.indent_size_for_line(row).len;
                let row_language = self.language_at(Point::new(row, indent_len)).cloned();
                let row_language_config = row_language.as_ref().map_or(config, |lang| lang.config());

                if row_language_config.decrease_indent_pattern.as_ref().is_some_and(|regex| regex.is_match(line)) {
                    indent_change_rows.push((row, Ordering::Less));
                }
                if row_language_config.increase_indent_pattern.as_ref().is_some_and(|regex| regex.is_match(line)) {
                    indent_change_rows.push((row + 1, Ordering::Greater));
                }
                while let Some(pos) = start_positions_iter.peek() {
                    if pos.start.row < row {
                        let pos = start_positions_iter.next().expect("peeked item exists").clone();
                        last_seen_suffix.entry(pos.suffix.to_string()).or_default().push(pos);
                    } else {
                        break;
                    }
                }
                for rule in &row_language_config.decrease_indent_patterns {
                    if rule.pattern.as_ref().is_some_and(|r| r.is_match(line)) {
                        let row_start_column = self.indent_size_for_line(row).len;
                        let basis_row = rule
                            .valid_after
                            .iter()
                            .filter_map(|valid_suffix| last_seen_suffix.get(valid_suffix))
                            .flatten()
                            .filter(|pos| {
                                row_language
                                    .as_ref()
                                    .or(self.language.as_ref())
                                    .is_some_and(|lang| Arc::ptr_eq(lang, &pos.language))
                            })
                            .filter(|pos| pos.start.column <= row_start_column)
                            .max_by_key(|pos| pos.start.row);
                        if let Some(outdent_to) = basis_row {
                            regex_outdent_map.insert(row, outdent_to.start.row);
                        }
                        break;
                    }
                }
            },
        );

        let mut indent_changes = indent_change_rows.into_iter().peekable();
        let mut prev_row = if config.auto_indent_using_last_non_empty_line {
            prev_non_blank_row.unwrap_or(0)
        } else {
            row_range.start.saturating_sub(1)
        };

        let mut prev_row_start = Point::new(prev_row, self.indent_size_for_line(prev_row).len);
        Some(row_range.map(move |row| {
            let row_start = Point::new(row, self.indent_size_for_line(row).len);

            let mut indent_from_prev_row = false;
            let mut outdent_from_prev_row = false;
            let mut outdent_to_row = u32::MAX;
            let mut from_regex = false;

            while let Some((indent_row, delta)) = indent_changes.peek() {
                match indent_row.cmp(&row) {
                    Ordering::Equal => match delta {
                        Ordering::Less => {
                            from_regex = true;
                            outdent_from_prev_row = true;
                        }
                        Ordering::Greater => {
                            indent_from_prev_row = true;
                            from_regex = true;
                        }
                        Ordering::Equal => {}
                    },

                    Ordering::Greater => break,
                    Ordering::Less => {}
                }

                indent_changes.next();
            }

            for range in &indent_ranges {
                if range.start.row >= row {
                    break;
                }
                if range.start.row == prev_row && range.end > row_start {
                    indent_from_prev_row = true;
                }
                if range.end > prev_row_start && range.end <= row_start {
                    outdent_to_row = outdent_to_row.min(range.start.row);
                }
            }

            if let Some(basis_row) = regex_outdent_map.get(&row) {
                indent_from_prev_row = false;
                outdent_to_row = *basis_row;
                from_regex = true;
            }

            let within_error = error_ranges.iter().any(|e| e.start.row < row && e.end > row_start);

            let suggestion = if outdent_to_row == prev_row || (outdent_from_prev_row && indent_from_prev_row) {
                Some(IndentSuggestion {
                    basis_row: prev_row,
                    delta: Ordering::Equal,
                    within_error: within_error && !from_regex,
                    explicit_outdent: true,
                })
            } else if indent_from_prev_row {
                Some(IndentSuggestion {
                    basis_row: prev_row,
                    delta: Ordering::Greater,
                    within_error: within_error && !from_regex,
                    explicit_outdent: false,
                })
            } else if outdent_to_row < prev_row {
                Some(IndentSuggestion {
                    basis_row: outdent_to_row,
                    delta: Ordering::Equal,
                    within_error: within_error && !from_regex,
                    explicit_outdent: true,
                })
            } else if outdent_from_prev_row {
                Some(IndentSuggestion {
                    basis_row: prev_row,
                    delta: Ordering::Less,
                    within_error: within_error && !from_regex,
                    explicit_outdent: true,
                })
            } else if config.auto_indent_using_last_non_empty_line || !self.is_line_blank(prev_row) {
                Some(IndentSuggestion {
                    basis_row: prev_row,
                    delta: Ordering::Equal,
                    within_error: within_error && !from_regex,
                    explicit_outdent: false,
                })
            } else {
                None
            };

            prev_row = row;
            prev_row_start = row_start;
            suggestion
        }))
    }

    fn prev_non_blank_row(&self, mut row: u32) -> Option<u32> {
        while row > 0 {
            row -= 1;
            if !self.is_line_blank(row) {
                return Some(row);
            }
        }
        None
    }

    pub fn captures(
        &self,
        range: Range<usize>,
        query: fn(&Grammar) -> Option<&tree_sitter::Query>,
    ) -> SyntaxMapCaptures<'_> {
        self.syntax.captures(range, &self.text, query)
    }

    fn get_highlights(&self, range: Range<usize>) -> (SyntaxMapCaptures<'_>, Vec<HighlightMap>) {
        let captures = self
            .syntax
            .captures(range, &self.text, |grammar| grammar.highlights_config.as_ref().map(|config| &config.query));
        let highlight_maps = captures.grammars().iter().map(|grammar| grammar.highlight_map()).collect();
        (captures, highlight_maps)
    }

    /// Iterates over chunks of text in the given range of the buffer. Text is chunked
    /// in an arbitrary way due to being stored in a [`Rope`](text::Rope). The text is also
    /// returned in chunks where each chunk has a single syntax highlighting style and
    /// diagnostic status.
    pub fn chunks<T: ToOffset>(&self, range: Range<T>, language_aware: LanguageAwareStyling) -> BufferChunks<'_> {
        let range = range.start.to_offset(self)..range.end.to_offset(self);

        let mut syntax = None;
        if language_aware.tree_sitter {
            match self.cached_highlight_runs(range.clone()) {
                Some(runs) => {
                    return BufferChunks::with_cached_highlights(
                        self.text.as_rope(),
                        range,
                        runs,
                        language_aware.diagnostics,
                        self,
                    );
                }
                None => syntax = Some(self.get_highlights(range.clone())),
            }
        }
        BufferChunks::new(self.text.as_rope(), range, syntax, language_aware.diagnostics, Some(self))
    }

    pub(crate) fn cached_highlight_runs(&self, range: Range<usize>) -> Option<Vec<HighlightRun>> {
        if let Some(fallback_highlighter) = &self.fallback_highlighter {
            return fallback_highlighter.highlight_runs(&self.text, range);
        }
        self.language.as_ref()?.grammar()?;
        if range.is_empty() {
            return Some(Vec::new());
        }
        let mut runs = Vec::<HighlightRun>::new();
        for chunk in self.tree_sitter_data.chunks.applicable_chunks(&[range.to_point(self)]) {
            let chunk_range = chunk.anchor_range().to_offset(self);
            if chunk_range.end <= range.start || chunk_range.start >= range.end {
                continue;
            }
            if chunk_range.len() > MAX_BYTES_TO_HIGHLIGHT_IN_A_CHUNK {
                return None;
            }
            let chunk_highlights =
                if let Some(chunk_highlights) = self.tree_sitter_data.highlights_by_chunks.get(chunk.id) {
                    chunk_highlights
                } else {
                    let chunk_highlights = self.compute_chunk_highlights(chunk_range);
                    self.tree_sitter_data.highlights_by_chunks.insert(chunk.id, chunk_highlights.clone());
                    chunk_highlights
                };
            for (run_range, highlight_id) in chunk_highlights.runs.iter() {
                if run_range.end <= range.start {
                    continue;
                }
                if run_range.start >= range.end {
                    break;
                }
                match runs.last_mut() {
                    Some((last_range, last_highlight_id))
                        if last_highlight_id == highlight_id && last_range.end == run_range.start =>
                    {
                        last_range.end = run_range.end;
                    }
                    _ => runs.push((run_range.clone(), *highlight_id)),
                }
            }
        }
        Some(runs)
    }

    fn compute_chunk_highlights(&self, range: Range<usize>) -> ResolvedHighlights {
        let captures = self.syntax.captures(range.clone(), &self.text, |grammar| {
            grammar.highlights_config.as_ref().map(|config| &config.query)
        });
        let sources = captures
            .grammars()
            .iter()
            .map(|&grammar| (Arc::clone(grammar), grammar.highlight_map()))
            .collect::<SmallVec<[(Arc<Grammar>, HighlightMap); 2]>>();
        let mut runs = Vec::<(Range<usize>, HighlightId)>::new();
        for region in flattened_highlight_regions(captures, range.clone()) {
            let highlight_id = region.stack.iter().rev().find_map(|capture| {
                let (_, highlight_map) = sources.get(capture.grammar_index)?;
                highlight_map.get(capture.capture_id)
            });
            let Some(highlight_id) = highlight_id else {
                continue;
            };
            match runs.last_mut() {
                Some((last_range, last_highlight_id))
                    if *last_highlight_id == highlight_id && last_range.end == region.range.start =>
                {
                    last_range.end = region.range.end;
                }
                _ => runs.push((region.range, highlight_id)),
            }
        }
        for (language_name, content_range) in self.syntax.pending_injections(range.clone(), self) {
            let injection_runs = fallback_highlight::highlight_injection(
                &language_name,
                &self.text,
                content_range.clone(),
                range.clone(),
            );
            if injection_runs.is_empty() {
                continue;
            }
            // The injection replaces the host's highlights within its content.
            let content_range = content_range.start.max(range.start)..content_range.end.min(range.end);
            let mut merged = Vec::with_capacity(runs.len() + injection_runs.len());
            for (run_range, highlight_id) in runs {
                if run_range.start < content_range.start {
                    merged.push((run_range.start..run_range.end.min(content_range.start), highlight_id));
                }
                if run_range.end > content_range.end {
                    merged.push((run_range.start.max(content_range.end)..run_range.end, highlight_id));
                }
            }
            merged.extend(injection_runs);
            merged.sort_unstable_by_key(|(run_range, _)| run_range.start);
            runs = merged;
        }
        ResolvedHighlights { sources, runs: runs.into() }
    }

    /// Invokes the given callback for each line of text in the given range of the buffer.
    /// Uses callback to avoid allocating a string for each line.
    fn for_each_line(&self, range: Range<Point>, mut callback: impl FnMut(u32, &str)) {
        let mut line = String::new();
        let mut row = range.start.row;
        for chunk in self.as_rope().chunks_in_range(range.to_offset(self)).chain(["\n"]) {
            for (newline_ix, text) in chunk.split('\n').enumerate() {
                if newline_ix > 0 {
                    callback(row, &line);
                    row += 1;
                    line.clear();
                }
                line.push_str(text);
            }
        }
    }

    /// Iterates over every [`SyntaxLayer`] in the buffer.
    pub fn syntax_layers(&self) -> impl Iterator<Item = SyntaxLayer<'_>> + '_ {
        self.syntax_layers_for_range(0..self.len(), true)
    }

    pub fn syntax_layer_at<D: ToOffset>(&self, position: D) -> Option<SyntaxLayer<'_>> {
        let offset = position.to_offset(self);
        self.syntax_layers_for_range(offset..offset, false)
            .filter(|l| {
                if let Some(ranges) = l.included_sub_ranges {
                    ranges.iter().any(|range| {
                        let start = range.start.to_offset(self);
                        start <= offset && {
                            let end = range.end.to_offset(self);
                            offset < end
                        }
                    })
                } else {
                    l.node().start_byte() <= offset && l.node().end_byte() > offset
                }
            })
            .last()
    }

    pub fn syntax_layers_for_range<D: ToOffset>(
        &self,
        range: Range<D>,
        include_hidden: bool,
    ) -> impl Iterator<Item = SyntaxLayer<'_>> + '_ {
        self.syntax.layers_for_range(range, &self.text, include_hidden)
    }

    pub fn syntax_layers_languages(&self) -> impl Iterator<Item = &Arc<Language>> {
        self.syntax.languages(self, true)
    }

    pub fn smallest_syntax_layer_containing<D: ToOffset>(&self, range: Range<D>) -> Option<SyntaxLayer<'_>> {
        let range = range.to_offset(self);
        self.syntax.layers_for_range(range, &self.text, false).max_by(|a, b| {
            if a.depth != b.depth {
                a.depth.cmp(&b.depth)
            } else if a.offset.0 != b.offset.0 {
                a.offset.0.cmp(&b.offset.0)
            } else {
                a.node().end_byte().cmp(&b.node().end_byte()).reverse()
            }
        })
    }

    pub(crate) const fn resolved_settings(&self) -> Option<&Arc<LanguageSettings>> {
        self.resolved_settings.as_ref()
    }

    /// Returns the main [`Language`].
    pub const fn language(&self) -> Option<&Arc<Language>> {
        self.language.as_ref()
    }

    /// Returns the [`Language`] at the given location.
    pub fn language_at<D: ToOffset>(&self, position: D) -> Option<&Arc<Language>> {
        self.syntax_layer_at(position).map(|info| info.language).or(self.language.as_ref())
    }

    /// Returns the settings for the language at the given location.
    pub fn settings_at<'a, D: ToOffset>(&'a self, position: D, cx: &'a App) -> Arc<LanguageSettings> {
        LanguageSettings::for_buffer_snapshot(self, Some(position.to_offset(self)), cx)
    }

    pub fn char_classifier_at<T: ToOffset>(&self, point: T) -> CharClassifier {
        CharClassifier::new(self.language_scope_at(point))
    }

    /// Returns the [`LanguageScope`] at the given location.
    pub fn language_scope_at<D: ToOffset>(&self, position: D) -> Option<LanguageScope> {
        let offset = position.to_offset(self);
        let mut scope = None;
        let mut smallest_range_and_depth: Option<(Range<usize>, usize)> = None;
        let text: &TextBufferSnapshot = self;

        // Use the layer that has the smallest node intersecting the given point.
        for layer in self.syntax.layers_for_range(offset..offset, &self.text, false) {
            if let Some(ranges) = layer.included_sub_ranges
                && !offset_in_sub_ranges(ranges, offset, text)
            {
                continue;
            }

            let mut cursor = layer.node().walk();

            let mut range = None;
            loop {
                let child_range = cursor.node().byte_range();
                let contains_offset = child_range.contains(&offset)
                // `Range::contains` is end-exclusive, which rejects every node at EOF
                // (including the root). Accept the end boundary only at the buffer's end,
                // so mid-buffer behavior is unchanged.
                    || (child_range.end == offset && offset == text.len());
                if !contains_offset {
                    break;
                }

                range = Some(child_range);
                if cursor.goto_first_child_for_byte(offset).is_none() {
                    break;
                }
            }

            if let Some(range) = range
                && smallest_range_and_depth.as_ref().is_none_or(|(smallest_range, smallest_range_depth)| {
                    match layer.depth.cmp(smallest_range_depth) {
                        Ordering::Greater => true,
                        Ordering::Equal => range.len() < smallest_range.len(),
                        Ordering::Less => false,
                    }
                })
            {
                smallest_range_and_depth = Some((range, layer.depth));
                scope = Some(LanguageScope {
                    language: Arc::clone(layer.language),
                    override_id: layer.override_id(offset, &self.text),
                });
            }
        }

        scope.or_else(|| self.language.clone().map(|language| LanguageScope { language, override_id: None }))
    }

    /// Returns a tuple of the range and character kind of the word
    /// surrounding the given position.
    pub fn surrounding_word<T: ToOffset>(
        &self,
        start: T,
        scope_context: Option<CharScopeContext>,
    ) -> (Range<usize>, Option<CharKind>) {
        let mut start = start.to_offset(self);
        let mut end = start;
        let mut next_chars = self.chars_at(start).take(128).peekable();
        let mut prev_chars = self.reversed_chars_at(start).take(128).peekable();

        let classifier = self.char_classifier_at(start).scope_context(scope_context);
        let word_kind = cmp::max(
            prev_chars.peek().copied().map(|c| classifier.kind(c)),
            next_chars.peek().copied().map(|c| classifier.kind(c)),
        );

        for ch in prev_chars {
            if Some(classifier.kind(ch)) == word_kind && ch != '\n' {
                start -= ch.len_utf8();
            } else {
                break;
            }
        }

        for ch in next_chars {
            if Some(classifier.kind(ch)) == word_kind && ch != '\n' {
                end += ch.len_utf8();
            } else {
                break;
            }
        }

        (start..end, word_kind)
    }

    /// Moves the TreeCursor to the smallest descendant or ancestor syntax node enclosing the given
    /// range. When `require_larger` is true, the node found must be larger than the query range.
    ///
    /// Returns true if a node was found, and false otherwise. In the `false` case the cursor will
    /// be moved to the root of the tree.
    fn goto_node_enclosing_range(
        cursor: &mut tree_sitter::TreeCursor,
        query_range: &Range<usize>,
        require_larger: bool,
    ) -> bool {
        let mut ascending = false;
        loop {
            let mut range = cursor.node().byte_range();
            if query_range.is_empty() {
                // When the query range is empty and the current node starts after it, move to the
                // previous sibling to find the node the containing node.
                if range.start > query_range.start {
                    cursor.goto_previous_sibling();
                    range = cursor.node().byte_range();
                }
            } else {
                // When the query range is non-empty and the current node ends exactly at the start,
                // move to the next sibling to find a node that extends beyond the start.
                if range.end == query_range.start {
                    cursor.goto_next_sibling();
                    range = cursor.node().byte_range();
                }
            }

            let encloses =
                range.contains_inclusive(query_range) && (!require_larger || range.len() > query_range.len());
            if !encloses {
                ascending = true;
                if !cursor.goto_parent() {
                    return false;
                }
                continue;
            } else if ascending {
                return true;
            }

            // Descend into the current node.
            if cursor.goto_first_child_for_byte(query_range.start).is_none() {
                return true;
            }
        }
    }

    pub fn syntax_ancestor<'a, T: ToOffset>(&'a self, range: Range<T>) -> Option<tree_sitter::Node<'a>> {
        let range = range.start.to_offset(self)..range.end.to_offset(self);
        let mut result: Option<tree_sitter::Node<'a>> = None;
        for layer in self.syntax.layers_for_range(range.clone(), &self.text, true) {
            let mut cursor = layer.node().walk();

            // Find the node that both contains the range and is larger than it.
            if !Self::goto_node_enclosing_range(&mut cursor, &range, true) {
                continue;
            }

            let left_node = cursor.node();
            let mut layer_result = left_node;

            // For an empty range, try to find another node immediately to the right of the range.
            if left_node.end_byte() == range.start {
                let mut right_node = None;
                while !cursor.goto_next_sibling() {
                    if !cursor.goto_parent() {
                        break;
                    }
                }

                while cursor.node().start_byte() == range.start {
                    right_node = Some(cursor.node());
                    if !cursor.goto_first_child() {
                        break;
                    }
                }

                // If there is a candidate node on both sides of the (empty) range, then
                // decide between the two by favoring a named node over an anonymous token.
                // If both nodes are the same in that regard, favor the right one.
                if let Some(right_node) = right_node
                    && (right_node.is_named() || !left_node.is_named())
                {
                    layer_result = right_node;
                }
            }

            if let Some(previous_result) = &result
                && previous_result.byte_range().len() < layer_result.byte_range().len()
            {
                continue;
            }
            result = Some(layer_result);
        }

        result
    }

    /// Find the previous sibling syntax node at the given range.
    ///
    /// This function locates the syntax node that precedes the node containing
    /// the given range. It searches hierarchically by:
    /// 1. Finding the node that contains the given range
    /// 2. Looking for the previous sibling at the same tree level
    /// 3. If no sibling is found, moving up to parent levels and searching for siblings
    ///
    /// Returns `None` if there is no previous sibling at any ancestor level.
    pub fn syntax_prev_sibling<'a, T: ToOffset>(&'a self, range: Range<T>) -> Option<tree_sitter::Node<'a>> {
        let range = range.start.to_offset(self)..range.end.to_offset(self);
        let mut result: Option<tree_sitter::Node<'a>> = None;

        for layer in self.syntax.layers_for_range(range.clone(), &self.text, true) {
            let mut cursor = layer.node().walk();

            // Find the node that contains the range
            if !Self::goto_node_enclosing_range(&mut cursor, &range, false) {
                continue;
            }

            // Look for the previous sibling, moving up ancestor levels if needed
            loop {
                if cursor.goto_previous_sibling() {
                    let layer_result = cursor.node();

                    if let Some(previous_result) = &result
                        && previous_result.byte_range().end < layer_result.byte_range().end
                    {
                        continue;
                    }
                    result = Some(layer_result);
                    break;
                }

                // No sibling found at this level, try moving up to parent
                if !cursor.goto_parent() {
                    break;
                }
            }
        }

        result
    }

    /// Find the next sibling syntax node at the given range.
    ///
    /// This function locates the syntax node that follows the node containing
    /// the given range. It searches hierarchically by:
    /// 1. Finding the node that contains the given range
    /// 2. Looking for the next sibling at the same tree level
    /// 3. If no sibling is found, moving up to parent levels and searching for siblings
    ///
    /// Returns `None` if there is no next sibling at any ancestor level.
    pub fn syntax_next_sibling<'a, T: ToOffset>(&'a self, range: Range<T>) -> Option<tree_sitter::Node<'a>> {
        let range = range.start.to_offset(self)..range.end.to_offset(self);
        let mut result: Option<tree_sitter::Node<'a>> = None;

        for layer in self.syntax.layers_for_range(range.clone(), &self.text, true) {
            let mut cursor = layer.node().walk();

            // Find the node that contains the range
            if !Self::goto_node_enclosing_range(&mut cursor, &range, false) {
                continue;
            }

            // Look for the next sibling, moving up ancestor levels if needed
            loop {
                if cursor.goto_next_sibling() {
                    let layer_result = cursor.node();

                    if let Some(previous_result) = &result
                        && previous_result.byte_range().start > layer_result.byte_range().start
                    {
                        continue;
                    }
                    result = Some(layer_result);
                    break;
                }

                // No sibling found at this level, try moving up to parent
                if !cursor.goto_parent() {
                    break;
                }
            }
        }

        result
    }

    /// Returns the outline for the buffer.
    ///
    /// This method allows passing an optional [`SyntaxTheme`] to
    /// syntax-highlight the returned symbols.
    pub fn outline(&self, theme: Option<&SyntaxTheme>) -> Outline<Anchor> {
        Outline::new(self.outline_items_containing(0..self.len(), true, theme))
    }

    /// Returns all the symbols that contain the given position.
    ///
    /// This method allows passing an optional [`SyntaxTheme`] to
    /// syntax-highlight the returned symbols.
    pub fn symbols_containing<T: ToOffset>(
        &self,
        position: T,
        theme: Option<&SyntaxTheme>,
    ) -> Vec<OutlineItem<Anchor>> {
        let position = position.to_offset(self);
        let start = self.clip_offset(position.saturating_sub(1), Bias::Left);
        let end = self.clip_offset(position + 1, Bias::Right);
        let mut items = self.outline_items_containing(start..end, false, theme);
        let mut prev_depth = None;
        items.retain(|item| {
            let result = prev_depth.is_none_or(|prev_depth| item.depth > prev_depth);
            prev_depth = Some(item.depth);
            result
        });
        items
    }

    pub fn outline_ranges_containing<T: ToOffset>(&self, range: Range<T>) -> impl Iterator<Item = Range<Point>> + '_ {
        let range = range.to_offset(self);
        let mut matches =
            self.syntax.matches(range.clone(), &self.text, |grammar| grammar.outline_config.as_ref().map(|c| &c.query));
        let configs = matches
            .grammars()
            .iter()
            .map(|g| g.outline_config.as_ref().expect("grammars were filtered by outline config"))
            .collect::<Vec<_>>();

        iter::from_fn(move || {
            while let Some(mat) = matches.peek() {
                let config = &configs[mat.grammar_index];
                let containing_item_node = (|| {
                    let item_node = mat
                        .captures
                        .iter()
                        .find_map(|cap| (cap.index == config.item_capture_ix).then_some(cap.node))?;

                    let item_byte_range = item_node.byte_range();
                    if item_byte_range.end < range.start || item_byte_range.start > range.end {
                        None
                    } else {
                        Some(item_node)
                    }
                })();

                let range = containing_item_node.as_ref().map(|item_node| {
                    Point::from_ts_point(item_node.start_position())..Point::from_ts_point(item_node.end_position())
                });
                matches.advance();
                if range.is_some() {
                    return range;
                }
            }
            None
        })
    }

    pub fn outline_range_containing<T: ToOffset>(&self, range: Range<T>) -> Option<Range<Point>> {
        self.outline_ranges_containing(range).next()
    }

    pub fn outline_items_containing<T: ToOffset>(
        &self,
        range: Range<T>,
        include_extra_context: bool,
        theme: Option<&SyntaxTheme>,
    ) -> Vec<OutlineItem<Anchor>> {
        self.outline_items_containing_internal(range, include_extra_context, theme, |this, range| {
            this.anchor_after(range.start)..this.anchor_before(range.end)
        })
    }

    fn outline_items_containing_internal<T: ToOffset, U>(
        &self,
        range: Range<T>,
        include_extra_context: bool,
        theme: Option<&SyntaxTheme>,
        range_callback: fn(&Self, Range<Point>) -> Range<U>,
    ) -> Vec<OutlineItem<U>> {
        let range = range.to_offset(self);
        let mut matches =
            self.syntax.matches(range.clone(), &self.text, |grammar| grammar.outline_config.as_ref().map(|c| &c.query));

        let mut items = Vec::new();
        let mut annotation_row_ranges: Vec<Range<u32>> = Vec::new();
        while let Some(mat) = matches.peek() {
            let config = matches.grammars()[mat.grammar_index]
                .outline_config
                .as_ref()
                .expect("grammars were filtered by outline config");
            if let Some(item) = self.next_outline_item(config, &mat, &range, include_extra_context, theme) {
                items.push(item);
            } else if let Some(capture) =
                mat.captures.iter().find(|capture| Some(capture.index) == config.annotation_capture_ix)
            {
                let capture_range = capture.node.start_position()..capture.node.end_position();
                let mut capture_row_range = capture_range.start.row as u32..capture_range.end.row as u32;
                if capture_range.end.row > capture_range.start.row && capture_range.end.column == 0 {
                    capture_row_range.end -= 1;
                }
                if let Some(last_row_range) = annotation_row_ranges.last_mut() {
                    if last_row_range.end >= capture_row_range.start.saturating_sub(1) {
                        last_row_range.end = capture_row_range.end;
                    } else {
                        annotation_row_ranges.push(capture_row_range);
                    }
                } else {
                    annotation_row_ranges.push(capture_row_range);
                }
            }
            matches.advance();
        }

        items.sort_by_key(|item| (item.range.start, Reverse(item.range.end)));

        // Assign depths based on containment relationships and convert to anchors.
        let mut item_ends_stack = Vec::<Point>::new();
        let mut anchor_items = Vec::new();
        let mut annotation_row_ranges = annotation_row_ranges.into_iter().peekable();
        for item in items {
            while let Some(last_end) = item_ends_stack.last().copied() {
                if last_end < item.range.end {
                    item_ends_stack.pop();
                } else {
                    break;
                }
            }

            let mut annotation_row_range = None;
            while let Some(next_annotation_row_range) = annotation_row_ranges.peek() {
                let row_preceding_item = item.range.start.row.saturating_sub(1);
                if next_annotation_row_range.end < row_preceding_item {
                    annotation_row_ranges.next();
                } else {
                    if next_annotation_row_range.end == row_preceding_item {
                        annotation_row_range = Some(next_annotation_row_range.clone());
                        annotation_row_ranges.next();
                    }
                    break;
                }
            }

            anchor_items.push(OutlineItem {
                depth: item_ends_stack.len(),
                range: range_callback(self, item.range.clone()),
                selection_range: range_callback(self, item.selection_range.clone()),
                source_range_for_text: range_callback(self, item.source_range_for_text.clone()),
                text: item.text,
                highlight_ranges: item.highlight_ranges,
                name_ranges: item.name_ranges,
                body_range: item.body_range.map(|r| range_callback(self, r)),
                annotation_range: annotation_row_range.map(|annotation_range| {
                    let point_range = Point::new(annotation_range.start, 0)
                        ..Point::new(annotation_range.end, self.line_len(annotation_range.end));
                    range_callback(self, point_range)
                }),
            });
            item_ends_stack.push(item.range.end);
        }

        anchor_items
    }

    fn next_outline_item(
        &self,
        config: &OutlineConfig,
        mat: &SyntaxMapMatch,
        range: &Range<usize>,
        include_extra_context: bool,
        theme: Option<&SyntaxTheme>,
    ) -> Option<OutlineItem<Point>> {
        let item_node =
            mat.captures.iter().find_map(|cap| (cap.index == config.item_capture_ix).then_some(cap.node))?;

        let item_byte_range = item_node.byte_range();
        if item_byte_range.end < range.start || item_byte_range.start > range.end {
            return None;
        }
        let item_point_range =
            Point::from_ts_point(item_node.start_position())..Point::from_ts_point(item_node.end_position());

        let mut open_point = None;
        let mut close_point = None;

        let mut buffer_ranges = Vec::new();
        let mut add_to_buffer_ranges = |node: tree_sitter::Node, node_is_name| {
            let mut range = node.start_byte()..node.end_byte();
            let start = node.start_position();
            if node.end_position().row > start.row {
                range.end = range.start + self.line_len(start.row as u32) as usize - start.column;
            }

            if !range.is_empty() {
                buffer_ranges.push((range, node_is_name));
            }
        };

        for capture in mat.captures {
            if capture.index == config.name_capture_ix {
                add_to_buffer_ranges(capture.node, true);
            } else if Some(capture.index) == config.context_capture_ix
                || (Some(capture.index) == config.extra_context_capture_ix && include_extra_context)
            {
                add_to_buffer_ranges(capture.node, false);
            } else if Some(capture.index) == config.open_capture_ix {
                open_point = Some(Point::from_ts_point(capture.node.end_position()));
            } else if Some(capture.index) == config.close_capture_ix {
                close_point = Some(Point::from_ts_point(capture.node.start_position()));
            }
        }

        if buffer_ranges.is_empty() {
            return None;
        }
        let source_range_for_text = buffer_ranges.first().expect("buffer ranges are non-empty").0.start
            ..buffer_ranges.last().expect("buffer ranges are non-empty").0.end;
        let selection_range = buffer_ranges
            .iter()
            .filter(|(_, node_is_name)| *node_is_name)
            .map(|(buffer_range, _)| buffer_range.clone())
            .reduce(|mut combined_range, next_range| {
                combined_range.end = next_range.end;
                combined_range
            })?;

        let mut text = String::new();
        let mut highlight_ranges = Vec::new();
        let mut name_ranges = Vec::new();
        let mut chunks =
            self.chunks(source_range_for_text.clone(), LanguageAwareStyling { tree_sitter: true, diagnostics: true });
        let mut last_buffer_range_end = 0;
        for (buffer_range, is_name) in buffer_ranges {
            let space_added = !text.is_empty() && buffer_range.start > last_buffer_range_end;
            if space_added {
                text.push(' ');
            }
            let before_append_len = text.len();
            let mut offset = buffer_range.start;
            chunks.seek(buffer_range.clone());
            for mut chunk in chunks.by_ref() {
                if chunk.text.len() > buffer_range.end - offset {
                    chunk.text = &chunk.text[0..(buffer_range.end - offset)];
                    offset = buffer_range.end;
                } else {
                    offset += chunk.text.len();
                }
                let style =
                    chunk.syntax_highlight_id.zip(theme).and_then(|(highlight, theme)| theme.get(highlight).copied());

                if let Some(style) = style {
                    let start = text.len();
                    let end = start + chunk.text.len();
                    highlight_ranges.push((start..end, style));
                }
                text.push_str(chunk.text);
                if offset >= buffer_range.end {
                    break;
                }
            }
            if is_name {
                let after_append_len = text.len();
                let start =
                    if space_added && !name_ranges.is_empty() { before_append_len - 1 } else { before_append_len };
                name_ranges.push(start..after_append_len);
            }
            last_buffer_range_end = buffer_range.end;
        }

        Some(OutlineItem {
            depth: 0, // We'll calculate the depth later
            range: item_point_range,
            selection_range: selection_range.to_point(self),
            source_range_for_text: source_range_for_text.to_point(self),
            text: text.into(),
            highlight_ranges,
            name_ranges,
            body_range: open_point.zip(close_point).map(|(start, end)| start..end),
            annotation_range: None,
        })
    }

    /// For each grammar in the language, runs the provided
    /// [`tree_sitter::Query`] against the given range.
    pub fn matches(
        &self,
        range: Range<usize>,
        query: fn(&Grammar) -> Option<&tree_sitter::Query>,
    ) -> SyntaxMapMatches<'_> {
        self.syntax.matches(range, self, query)
    }

    pub fn text_object_ranges<T: ToOffset>(
        &self,
        range: Range<T>,
        options: TreeSitterOptions,
    ) -> impl Iterator<Item = (Range<usize>, TextObject)> + '_ {
        let range = range.start.to_previous_offset(self)..self.len().min(range.end.to_next_offset(self));

        let mut matches = self.syntax.matches_with_options(range.clone(), &self.text, options, |grammar| {
            grammar.text_object_config.as_ref().map(|c| &c.query)
        });

        let configs = matches.grammars().iter().map(|grammar| grammar.text_object_config.as_ref()).collect::<Vec<_>>();

        let mut captures = Vec::<(Range<usize>, TextObject)>::new();

        iter::from_fn(move || {
            loop {
                while let Some(capture) = captures.pop() {
                    if capture.0.overlaps(&range) {
                        return Some(capture);
                    }
                }

                let mat = matches.peek()?;

                let Some(config) = configs[mat.grammar_index].as_ref() else {
                    matches.advance();
                    continue;
                };

                for capture in mat.captures {
                    let Some(ix) = config.text_objects_by_capture_ix.binary_search_by_key(&capture.index, |e| e.0).ok()
                    else {
                        continue;
                    };
                    let text_object = config.text_objects_by_capture_ix[ix].1;
                    let byte_range = capture.node.byte_range();

                    let mut found = false;
                    for (range, existing) in &mut captures {
                        if existing == &text_object {
                            range.start = range.start.min(byte_range.start);
                            range.end = range.end.max(byte_range.end);
                            found = true;
                            break;
                        }
                    }

                    if !found {
                        captures.push((byte_range, text_object));
                    }
                }

                matches.advance();
            }
        })
    }

    /// Returns the `@spell` and `@nospell` capture ranges of the spell check
    /// queries of every syntax layer intersecting `range`. Captured nodes may
    /// extend beyond `range`.
    pub fn spell_check_ranges<T: ToOffset>(&self, range: Range<T>) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
        let offset_range = range.start.to_offset(self)..range.end.to_offset(self);
        let mut syntax_matches = self
            .syntax
            .matches(offset_range, self, |grammar| grammar.spell_check_config.as_ref().map(|config| &config.query));
        let configs =
            syntax_matches.grammars().iter().map(|grammar| grammar.spell_check_config.as_ref()).collect::<Vec<_>>();

        let mut spell = Vec::new();
        let mut nospell = Vec::new();
        while let Some(mat) = syntax_matches.peek() {
            if let Some(config) = configs[mat.grammar_index] {
                for capture in mat.captures {
                    if Some(capture.index) == config.spell_capture_ix {
                        spell.push(capture.node.byte_range());
                    } else if Some(capture.index) == config.nospell_capture_ix {
                        nospell.push(capture.node.byte_range());
                    }
                }
            }
            syntax_matches.advance();
        }
        (spell, nospell)
    }

    pub fn runnable_ranges(&self, offset_range: Range<usize>) -> impl Iterator<Item = RunnableRange> + '_ {
        runnable::runnable_ranges(self, offset_range)
    }

    /// Returns if the buffer contains any diagnostics.
    pub fn has_diagnostics(&self) -> bool {
        !self.diagnostics.is_empty()
    }

    /// Returns all the diagnostics intersecting the given range.
    pub fn diagnostics_in_range<'a, T, O>(
        &'a self,
        search_range: Range<T>,
        reversed: bool,
    ) -> impl 'a + Iterator<Item = DiagnosticEntryRef<'a, O>>
    where
        T: 'a + Clone + ToOffset,
        O: 'a + FromAnchor,
    {
        self.diagnostic_entries_in_range(search_range, reversed).map(|entry| entry.resolve(self))
    }

    /// Returns the stored entries that intersect the given range, with their ranges
    /// and related information left in the buffer's own coordinates.
    pub fn diagnostic_entries_in_range<'a, T>(
        &'a self,
        search_range: Range<T>,
        reversed: bool,
    ) -> impl 'a + Iterator<Item = &'a DiagnosticEntry<Anchor>>
    where
        T: 'a + Clone + ToOffset,
    {
        self.diagnostic_entries_in_range_with_server_id(search_range, reversed).map(|(_, entry)| entry)
    }

    /// Returns the stored entries that intersect the given range along with the
    /// language server that produced each diagnostic.
    pub fn diagnostic_entries_in_range_with_server_id<'a, T>(
        &'a self,
        search_range: Range<T>,
        reversed: bool,
    ) -> impl 'a + Iterator<Item = (LanguageServerId, &'a DiagnosticEntry<Anchor>)>
    where
        T: 'a + Clone + ToOffset,
    {
        let mut iterators: Vec<_> = self
            .diagnostics
            .iter()
            .map(|(server_id, collection)| {
                (*server_id, collection.entries_in_range::<T>(search_range.clone(), self, true, reversed).peekable())
            })
            .collect();

        iter::from_fn(move || {
            let (next_ix, _) =
                iterators.iter_mut().enumerate().filter_map(|(ix, (_, iter))| Some((ix, iter.peek()?))).min_by(
                    |(_, a), (_, b)| {
                        let cmp = a
                            .range
                            .start
                            .cmp(&b.range.start, self)
                            // when range is equal, sort by diagnostic severity
                            .then(a.diagnostic.severity.cmp(&b.diagnostic.severity))
                            // and stabilize order with group_id
                            .then(a.diagnostic.group_id.cmp(&b.diagnostic.group_id));
                        if reversed { cmp.reverse() } else { cmp }
                    },
                )?;
            let (server_id, iterator) = iterators.get_mut(next_ix)?;
            let server_id = *server_id;
            iterator.next().map(|entry| (server_id, entry))
        })
    }

    /// Returns an iterator over the diagnostics for the given group.
    pub fn diagnostic_group<O>(&self, group_id: usize) -> impl Iterator<Item = DiagnosticEntryRef<'_, O>> + use<'_, O>
    where
        O: FromAnchor + 'static,
    {
        self.diagnostics.iter().flat_map(move |(_, set)| set.group(group_id, self))
    }

    /// An integer version number that accounts for all updates besides
    /// the buffer's text itself (which is versioned via a version vector).
    pub const fn non_text_state_update_count(&self) -> usize {
        self.non_text_state_update_count
    }

    /// Returns a snapshot of underlying file.
    pub fn file(&self) -> Option<&Arc<dyn File>> {
        self.file.as_ref()
    }

    pub fn resolve_file_path(&self, include_root: bool, cx: &App) -> Option<String> {
        if let Some(file) = self.file() {
            if file.path().file_name().is_none() || include_root || !file.is_in_visible_worktree(cx) {
                Some(file.full_path(cx).to_string_lossy().into_owned())
            } else {
                Some(file.path().display().to_owned())
            }
        } else {
            None
        }
    }

    pub fn words_in_range(&self, query: WordsQuery) -> BTreeMap<String, Range<Anchor>> {
        let query_str = query.fuzzy_contents;
        if query_str.is_some_and(str::is_empty) {
            return BTreeMap::default();
        }

        let classifier =
            CharClassifier::new(self.language.clone().map(|language| LanguageScope { language, override_id: None }));

        let mut query_ix = 0;
        let query_chars = query_str.map(|query| query.chars().collect::<Vec<_>>());
        let query_len = query_chars.as_ref().map_or(0, Vec::len);

        let mut words = BTreeMap::default();
        let mut current_word_start_ix = None;
        let mut chunk_ix = query.range.start;
        for chunk in self.chunks(query.range, LanguageAwareStyling { tree_sitter: false, diagnostics: false }) {
            for (i, c) in chunk.text.char_indices() {
                let ix = chunk_ix + i;
                if classifier.is_word(c) {
                    if current_word_start_ix.is_none() {
                        current_word_start_ix = Some(ix);
                    }

                    if let Some(query_chars) = &query_chars
                        && query_ix < query_len
                        && c.to_lowercase().eq(query_chars[query_ix].to_lowercase())
                    {
                        query_ix += 1;
                    }
                    continue;
                } else if let Some(word_start) = current_word_start_ix.take()
                    && query_ix == query_len
                {
                    let word_range = self.anchor_before(word_start)..self.anchor_after(ix);
                    let mut word_text = self.text_for_range(word_start..ix).peekable();
                    let first_char = word_text.peek().and_then(|first_chunk| first_chunk.chars().next());
                    // Skip empty and "words" starting with digits as a heuristic to reduce useless completions
                    if !query.skip_digits || first_char.is_none_or(|first_char| !first_char.is_ascii_digit()) {
                        words.insert(word_text.collect(), word_range);
                    }
                }
                query_ix = 0;
            }
            chunk_ix += chunk.text.len();
        }

        words
    }
}

/// A configuration to use when producing styled text chunks.
#[derive(Clone, Copy)]
pub struct LanguageAwareStyling {
    /// Whether to highlight text chunks using tree-sitter.
    pub tree_sitter: bool,
    /// Whether to highlight text chunks based on the diagnostics data.
    pub diagnostics: bool,
}

pub struct WordsQuery<'a> {
    /// Only returns words with all chars from the fuzzy string in them.
    pub fuzzy_contents: Option<&'a str>,
    /// Skips words that start with a digit.
    pub skip_digits: bool,
    /// Buffer offset range, to look for words.
    pub range: Range<usize>,
}

/// Whether an edit requesting autoindent gets it. Autoindent compares the
/// syntax before and after the edit, and a deferred buffer has no tree for the
/// pre-edit state, so its edits skip autoindent rather than force a parse.
const fn autoindents_edits(has_language: bool, parse_deferred: bool) -> bool {
    has_language && !parse_deferred
}

/// Whether a buffer empties itself when its file changes on disk: a clean
/// buffer follows its file's deletion (`was_present` and now `is_deleted`).
const fn clears_on_file_deletion(was_present: bool, is_deleted: bool, was_dirty: bool) -> bool {
    was_present && is_deleted && !was_dirty
}

fn indent_size_for_line(text: &text::BufferSnapshot, row: u32) -> IndentSize {
    indent_size_for_text(text.chars_at(Point::new(row, 0)))
}

fn indent_size_for_text(text: impl Iterator<Item = char>) -> IndentSize {
    let mut result = IndentSize::spaces(0);
    for c in text {
        let kind = match c {
            ' ' => IndentKind::Space,
            '\t' => IndentKind::Tab,
            _ => break,
        };
        if result.len == 0 {
            result.kind = kind;
        }
        result.len += 1;
    }
    result
}

impl Clone for BufferSnapshot {
    fn clone(&self) -> Self {
        Self {
            text: self.text.clone(),
            syntax: self.syntax.clone(),
            file: self.file.clone(),
            diagnostics: self.diagnostics.clone(),
            language: self.language.clone(),
            tree_sitter_data: Arc::clone(&self.tree_sitter_data),
            fallback_highlighter: self.fallback_highlighter.clone(),
            non_text_state_update_count: self.non_text_state_update_count,
            capability: self.capability,
            resolved_settings: self.resolved_settings.clone(),
        }
    }
}

impl Deref for BufferSnapshot {
    type Target = text::BufferSnapshot;

    fn deref(&self) -> &Self::Target {
        &self.text
    }
}

unsafe impl Send for BufferChunks<'_> {}

impl<'a> BufferChunks<'a> {
    pub(crate) fn new(
        text: &'a Rope,
        range: Range<usize>,
        syntax: Option<(SyntaxMapCaptures<'a>, Vec<HighlightMap>)>,
        diagnostics: bool,
        buffer_snapshot: Option<&'a BufferSnapshot>,
    ) -> Self {
        let highlights = syntax.map(|(captures, highlight_maps)| BufferChunkHighlights {
            captures,
            next_capture: None,
            stack: Vec::new(),
            highlight_maps,
        });
        Self::init(text, range, highlights, None, diagnostics, buffer_snapshot)
    }

    fn with_cached_highlights(
        text: &'a Rope,
        range: Range<usize>,
        runs: Vec<HighlightRun>,
        diagnostics: bool,
        buffer_snapshot: &'a BufferSnapshot,
    ) -> Self {
        Self::init(
            text,
            range,
            None,
            Some(CachedChunkHighlightsIter { runs, ix: 0 }),
            diagnostics,
            Some(buffer_snapshot),
        )
    }

    fn init(
        text: &'a Rope,
        range: Range<usize>,
        highlights: Option<BufferChunkHighlights<'a>>,
        cached_highlights: Option<CachedChunkHighlightsIter>,
        diagnostics: bool,
        buffer_snapshot: Option<&'a BufferSnapshot>,
    ) -> Self {
        let diagnostic_endpoints = diagnostics.then(|| Vec::new().into_iter().peekable());
        let chunks = text.chunks_in_range(range.clone());

        let mut this = BufferChunks {
            range,
            buffer_snapshot,
            chunks,
            diagnostic_endpoints,
            error_depth: 0,
            warning_depth: 0,
            information_depth: 0,
            hint_depth: 0,
            unnecessary_depth: 0,
            underline: true,
            highlights,
            cached_highlights,
        };
        this.initialize_diagnostic_endpoints();
        this
    }

    /// Seeks to the given byte offset in the buffer.
    pub fn seek(&mut self, range: Range<usize>) {
        let old_range = mem::replace(&mut self.range, range.clone());
        self.chunks.set_range(self.range.clone());
        if let Some(cached) = self.cached_highlights.as_mut() {
            if old_range.start <= self.range.start && old_range.end >= self.range.end {
                cached.ix = cached.runs.partition_point(|(run_range, _)| run_range.end <= range.start);
            } else if let Some(snapshot) = self.buffer_snapshot {
                if let Some(runs) = snapshot.cached_highlight_runs(self.range.clone()) {
                    cached.runs = runs;
                    cached.ix = 0;
                } else {
                    let (captures, highlight_maps) = snapshot.get_highlights(self.range.clone());
                    self.cached_highlights = None;
                    self.highlights =
                        Some(BufferChunkHighlights { captures, next_capture: None, stack: Vec::new(), highlight_maps });
                }
            }
        } else if let Some(highlights) = self.highlights.as_mut() {
            if old_range.start <= self.range.start && old_range.end >= self.range.end {
                // Reuse existing highlights stack, as the new range is a subrange of the old one.
                highlights.stack.retain(|(end_offset, _)| *end_offset > range.start);
                if let Some(capture) = &highlights.next_capture
                    && range.start >= capture.node.start_byte()
                {
                    let next_capture_end = capture.node.end_byte();
                    if range.start < next_capture_end
                        && let Some(capture_id) =
                            highlights.highlight_maps[capture.grammar_index].get(CaptureId(capture.index))
                    {
                        highlights.stack.push((next_capture_end, capture_id));
                    }
                    highlights.next_capture.take();
                }
            } else if let Some(snapshot) = self.buffer_snapshot {
                let (captures, highlight_maps) = snapshot.get_highlights(self.range.clone());
                *highlights = BufferChunkHighlights { captures, next_capture: None, stack: Vec::new(), highlight_maps };
            }

            highlights.captures.set_byte_range(self.range.clone());
        }
        self.initialize_diagnostic_endpoints();
    }

    fn initialize_diagnostic_endpoints(&mut self) {
        if let Some(diagnostics) = self.diagnostic_endpoints.as_mut()
            && let Some(buffer) = self.buffer_snapshot
        {
            let mut diagnostic_endpoints = Vec::new();
            for entry in buffer.diagnostics_in_range::<_, usize>(self.range.clone(), false) {
                diagnostic_endpoints.push(DiagnosticEndpoint {
                    offset: entry.range.start,
                    is_start: true,
                    severity: entry.diagnostic.severity,
                    is_unnecessary: entry.diagnostic.is_unnecessary,
                    underline: entry.diagnostic.underline,
                });
                diagnostic_endpoints.push(DiagnosticEndpoint {
                    offset: entry.range.end,
                    is_start: false,
                    severity: entry.diagnostic.severity,
                    is_unnecessary: entry.diagnostic.is_unnecessary,
                    underline: entry.diagnostic.underline,
                });
            }
            diagnostic_endpoints.sort_unstable_by_key(|endpoint| (endpoint.offset, !endpoint.is_start));
            *diagnostics = diagnostic_endpoints.into_iter().peekable();
            self.hint_depth = 0;
            self.error_depth = 0;
            self.warning_depth = 0;
            self.information_depth = 0;
            self.unnecessary_depth = 0;
        }
    }

    /// The current byte offset in the buffer.
    pub const fn offset(&self) -> usize {
        self.range.start
    }

    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    const fn update_diagnostic_depths(&mut self, endpoint: DiagnosticEndpoint) {
        let depth = match endpoint.severity {
            DiagnosticSeverity::ERROR => &mut self.error_depth,
            DiagnosticSeverity::WARNING => &mut self.warning_depth,
            DiagnosticSeverity::INFORMATION => &mut self.information_depth,
            DiagnosticSeverity::HINT => &mut self.hint_depth,
            _ => return,
        };
        if endpoint.is_start {
            *depth += 1;
        } else {
            *depth -= 1;
        }

        if endpoint.is_unnecessary {
            if endpoint.is_start {
                self.unnecessary_depth += 1;
            } else {
                self.unnecessary_depth -= 1;
            }
        }
    }

    const fn current_diagnostic_severity(&self) -> Option<DiagnosticSeverity> {
        if self.error_depth > 0 {
            Some(DiagnosticSeverity::ERROR)
        } else if self.warning_depth > 0 {
            Some(DiagnosticSeverity::WARNING)
        } else if self.information_depth > 0 {
            Some(DiagnosticSeverity::INFORMATION)
        } else if self.hint_depth > 0 {
            Some(DiagnosticSeverity::HINT)
        } else {
            None
        }
    }

    const fn current_code_is_unnecessary(&self) -> bool {
        self.unnecessary_depth > 0
    }
}

impl<'a> Iterator for BufferChunks<'a> {
    type Item = Chunk<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut next_capture_start = usize::MAX;
        let mut next_diagnostic_endpoint = usize::MAX;

        if let Some(highlights) = self.highlights.as_mut() {
            while let Some((parent_capture_end, _)) = highlights.stack.last() {
                if *parent_capture_end <= self.range.start {
                    highlights.stack.pop();
                } else {
                    break;
                }
            }

            if highlights.next_capture.is_none() {
                highlights.next_capture = highlights.captures.next();
            }

            while let Some(capture) = highlights.next_capture.as_ref() {
                if self.range.start < capture.node.start_byte() {
                    next_capture_start = capture.node.start_byte();
                    break;
                }
                let highlight_id = highlights.highlight_maps[capture.grammar_index].get(CaptureId(capture.index));
                if let Some(highlight_id) = highlight_id {
                    highlights.stack.push((capture.node.end_byte(), highlight_id));
                }
                highlights.next_capture = highlights.captures.next();
            }
        }

        if let Some(cached) = self.cached_highlights.as_mut() {
            while cached.runs.get(cached.ix).is_some_and(|(run_range, _)| run_range.end <= self.range.start) {
                cached.ix += 1;
            }
            if let Some((run_range, _)) = cached.runs.get(cached.ix)
                && self.range.start < run_range.start
            {
                next_capture_start = run_range.start;
            }
        }

        let mut diagnostic_endpoints = mem::take(&mut self.diagnostic_endpoints);
        if let Some(diagnostic_endpoints) = diagnostic_endpoints.as_mut() {
            while let Some(endpoint) = diagnostic_endpoints.peek().copied() {
                if endpoint.offset <= self.range.start {
                    self.update_diagnostic_depths(endpoint);
                    diagnostic_endpoints.next();
                    self.underline = endpoint.underline;
                } else {
                    next_diagnostic_endpoint = endpoint.offset;
                    break;
                }
            }
        }
        self.diagnostic_endpoints = diagnostic_endpoints;

        if let Some(ChunkBitmaps { text: chunk, chars: chars_map, tabs, newlines }) = self.chunks.peek_with_bitmaps() {
            let chunk_start = self.range.start;
            let mut chunk_end =
                (self.chunks.offset() + chunk.len()).min(next_capture_start).min(next_diagnostic_endpoint);
            let parent_capture = self.highlights.as_ref().and_then(|highlights| highlights.stack.last());
            if let Some((parent_capture_end, _)) = parent_capture {
                chunk_end = chunk_end.min(*parent_capture_end);
            }
            let cached_run = self
                .cached_highlights
                .as_ref()
                .and_then(|cached| cached.runs.get(cached.ix))
                .filter(|(run_range, _)| run_range.start <= chunk_start);
            if let Some((run_range, _)) = cached_run {
                chunk_end = chunk_end.min(run_range.end);
            }
            // A cached run takes precedence over the enclosing capture.
            let highlight_id = cached_run
                .map(|(_, run_highlight_id)| *run_highlight_id)
                .or_else(|| parent_capture.map(|(_, parent_highlight_id)| *parent_highlight_id));
            let bit_start = chunk_start - self.chunks.offset();
            let bit_end = chunk_end - self.chunks.offset();

            let slice = &chunk[bit_start..bit_end];

            let mask = 1_u128.unbounded_shl((bit_end - bit_start) as u32).wrapping_sub(1);
            let tabs = (tabs >> bit_start) & mask;
            let chars = (chars_map >> bit_start) & mask;
            let newlines = (newlines >> bit_start) & mask;

            self.range.start = chunk_end;
            if self.range.start == self.chunks.offset() + chunk.len() {
                self.chunks.next().expect("chunk exists at current offset");
            }

            Some(Chunk {
                text: slice,
                syntax_highlight_id: highlight_id,
                underline: self.underline,
                diagnostic_severity: self.current_diagnostic_severity(),
                is_unnecessary: self.current_code_is_unnecessary(),
                tabs,
                chars,
                newlines,
                ..Chunk::default()
            })
        } else {
            None
        }
    }
}

impl IndentSize {
    /// Returns an [`IndentSize`] representing the given spaces.
    pub const fn spaces(len: u32) -> Self {
        Self { len, kind: IndentKind::Space }
    }

    /// Returns an [`IndentSize`] representing a tab.
    pub const fn tab() -> Self {
        Self { len: 1, kind: IndentKind::Tab }
    }

    /// An iterator over the characters represented by this [`IndentSize`].
    pub fn chars(&self) -> impl Iterator<Item = char> {
        iter::repeat_n(self.char(), self.len as usize)
    }

    /// The character representation of this [`IndentSize`].
    pub const fn char(&self) -> char {
        match self.kind {
            IndentKind::Space => ' ',
            IndentKind::Tab => '\t',
        }
    }

    /// Consumes the current [`IndentSize`] and returns a new one that has
    /// been shrunk or enlarged by the given size along the given direction.
    pub fn with_delta(mut self, direction: Ordering, size: IndentSize) -> Self {
        match direction {
            Ordering::Less => {
                if self.kind == size.kind && self.len >= size.len {
                    self.len -= size.len;
                }
            }
            Ordering::Equal => {}
            Ordering::Greater => {
                if self.len == 0 {
                    self = size;
                } else if self.kind == size.kind {
                    self.len += size.len;
                }
            }
        }
        self
    }

    /// Returns the number of indentation characters to remove when outdenting to the
    /// previous editor tab stop.
    pub const fn outdent_len(self, tab_size: NonZeroU32) -> u32 {
        if self.len == 0 {
            return 0;
        }

        match self.kind {
            IndentKind::Space => {
                let tab_size = tab_size.get();
                let columns_to_prev_tab_stop = self.len % tab_size;
                if columns_to_prev_tab_stop == 0 { tab_size } else { columns_to_prev_tab_stop }
            }
            IndentKind::Tab => 1,
        }
    }
}

pub(crate) fn contiguous_ranges(values: impl Iterator<Item = u32>, max_len: usize) -> impl Iterator<Item = Range<u32>> {
    let mut values = values;
    let mut current_range: Option<Range<u32>> = None;
    iter::from_fn(move || {
        loop {
            if let Some(value) = values.next() {
                if let Some(range) = &mut current_range
                    && value == range.end
                    && range.len() < max_len
                {
                    range.end += 1;
                    continue;
                }

                let prev_range = current_range.clone();
                current_range = Some(value..(value + 1));
                if prev_range.is_some() {
                    return prev_range;
                }
            } else {
                return current_range.take();
            }
        }
    })
}

#[derive(Clone, Default, Debug)]
pub struct CharClassifier {
    scope: Option<LanguageScope>,
    scope_context: Option<CharScopeContext>,
    ignore_punctuation: bool,
}

impl CharClassifier {
    pub const fn new(scope: Option<LanguageScope>) -> Self {
        Self { scope, scope_context: None, ignore_punctuation: false }
    }

    pub fn scope_context(self, scope_context: Option<CharScopeContext>) -> Self {
        Self { scope_context, ..self }
    }

    pub fn is_whitespace(&self, c: char) -> bool {
        self.kind(c) == CharKind::Whitespace
    }

    pub fn is_word(&self, c: char) -> bool {
        self.kind(c) == CharKind::Word
    }

    pub fn is_punctuation(&self, c: char) -> bool {
        self.kind(c) == CharKind::Punctuation
    }

    pub fn kind_with(&self, c: char, ignore_punctuation: bool) -> CharKind {
        if c.is_alphanumeric() || c == '_' {
            return CharKind::Word;
        }

        if let Some(scope) = &self.scope {
            let characters = match self.scope_context {
                Some(CharScopeContext::Completion) => scope.completion_query_characters(),
                Some(CharScopeContext::LinkedEdit) => scope.linked_edit_characters(),
                None => scope.word_characters(),
            };
            if let Some(characters) = characters
                && characters.contains(&c)
            {
                return CharKind::Word;
            }
        }

        if c.is_whitespace() {
            return CharKind::Whitespace;
        }

        if ignore_punctuation { CharKind::Word } else { CharKind::Punctuation }
    }

    pub fn kind(&self, c: char) -> CharKind {
        self.kind_with(c, self.ignore_punctuation)
    }
}

#[cfg(test)]
mod file_deletion_tests {
    use super::clears_on_file_deletion;

    #[test]
    fn clears_on_file_deletion_works() {
        assert!(clears_on_file_deletion(true, true, false));
        // Unsaved edits are kept, so they can be saved to recreate the file.
        assert!(!clears_on_file_deletion(true, true, true));
        // Only a file that was on disk can be deleted from it (not a new or
        // already deleted one), and only a deletion clears.
        assert!(!clears_on_file_deletion(false, true, false));
        assert!(!clears_on_file_deletion(true, false, false));
    }
}

#[cfg(test)]
mod parse_deferral_tests {
    use super::autoindents_edits;

    #[test]
    fn autoindents_edits_works() {
        assert!(autoindents_edits(true, false));
        // No pre-edit syntax tree to compare against.
        assert!(!autoindents_edits(true, true));
        assert!(!autoindents_edits(false, false));
        assert!(!autoindents_edits(false, true));
    }
}
