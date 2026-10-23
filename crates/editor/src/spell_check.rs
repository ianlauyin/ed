//! Spell checking of comments, strings, definition names and prose, drawn as
//! wavy underlines over the visible rows. Suggestions and adding a word to the
//! project's word list are code actions.
//!
//! What gets checked comes from each grammar's `spellcheck.scm`; Git Commit
//! and `.txt` buffers, which have no grammar, are checked as a whole.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use anyhow::Result;
use collections::HashMap;
use collections::HashSet;
use gpui::App;
use gpui::AppContext as _;
use gpui::Context;
use gpui::Entity;
use gpui::HighlightStyle;
use gpui::Subscription;
use gpui::Task;
use gpui::UnderlineStyle;
use gpui::WeakEntity;
use gpui::Window;
use gpui::px;
use language::Buffer;
use language::BufferEvent;
use language::BufferSnapshot;
use language::Point;
use lsp::LanguageServerId;
use multi_buffer::BufferOffset;
use project::CodeAction;
use project::LspAction;
use project::Project;
use project::ProjectPath;
use project::ProjectTransaction;
use project::WorktreeId;
use spell_check::PROJECT_WORDS_FILE;
use text::BufferId;
use text::OffsetRangeExt as _;
use theme::ActiveTheme as _;
use util::ResultExt as _;
use util::rel_path::RelPath;

use crate::CodeActionProvider;
use crate::Editor;
use crate::EditorMode;
use crate::HighlightKey;

const DEBOUNCE: Duration = Duration::from_millis(50);
const GIT_SCISSORS: &str = "# ------------------------ >8 ------------------------";

#[derive(Default)]
pub(crate) struct SpellCheckState {
    refresh_task: Option<Task<()>>,
    project_words: HashMap<WorktreeId, ProjectWords>,
    /// Shared with [`SpellCheckActions`], which runs while the editor is
    /// being updated and so cannot read it.
    misspellings: Rc<RefCell<Vec<(BufferId, Range<text::Anchor>)>>>,
}

struct ProjectWords {
    words: Arc<HashSet<String>>,
    _load: Task<()>,
    /// Held strongly: the project only keeps a weak handle, and a dropped
    /// buffer would stop reporting edits and reloads of the file.
    _buffer: Option<(Entity<Buffer>, Subscription)>,
}

#[derive(Clone, Copy)]
enum CheckMode {
    /// Regions captured by the grammar's spell check query.
    Syntax,
    /// Every line, minus Git Commit comments and the diff below the scissors.
    WholeText { git_commit: bool },
}

impl SpellCheckState {
    pub(crate) fn for_mode(mode: &EditorMode) -> Option<Self> {
        matches!(mode, EditorMode::Full { .. } | EditorMode::AutoHeight { .. }).then(Self::default)
    }

    pub(crate) fn code_actions(&self, editor: WeakEntity<Editor>, has_project: bool) -> SpellCheckActions {
        SpellCheckActions { editor, misspellings: self.misspellings.clone(), has_project }
    }
}

impl Editor {
    pub(crate) fn refresh_spell_check(&mut self, cx: &mut Context<Self>) {
        if self.spell_check.is_none() {
            return;
        }

        let mut jobs = Vec::new();
        for (buffer_snapshot, range, _) in self.visible_buffer_ranges(cx) {
            let Some(buffer) = self.buffer.read(cx).buffer(buffer_snapshot.remote_id()) else {
                continue;
            };
            let Some(mode) = check_mode(buffer.read(cx)) else {
                continue;
            };
            let words = self.project_words(&buffer, cx);
            jobs.push((buffer_snapshot, range, mode, words));
        }
        let multi_buffer_snapshot = self.buffer.read(cx).snapshot(cx);

        let Some(state) = self.spell_check.as_mut() else {
            return;
        };
        state.refresh_task = Some(cx.spawn(async move |editor, cx| {
            cx.background_executor().timer(DEBOUNCE).await;
            let (misspellings, highlights) = cx
                .background_spawn(async move {
                    let mut misspellings = Vec::new();
                    let mut highlights = Vec::new();
                    for (snapshot, range, mode, words) in jobs {
                        for range in misspelled_ranges(&snapshot, range, mode, &words) {
                            let anchors = snapshot.anchor_range_inside(range);
                            if let Some(start) = multi_buffer_snapshot.anchor_in_buffer(anchors.start)
                                && let Some(end) = multi_buffer_snapshot.anchor_in_buffer(anchors.end)
                            {
                                highlights.push(start..end);
                            }
                            misspellings.push((snapshot.remote_id(), anchors));
                        }
                    }
                    (misspellings, highlights)
                })
                .await;

            editor
                .update(cx, |editor, cx| {
                    let style = HighlightStyle {
                        underline: Some(UnderlineStyle {
                            color: Some(cx.theme().status().info),
                            thickness: px(1.),
                            wavy: true,
                        }),
                        ..HighlightStyle::default()
                    };
                    editor.highlight_text(HighlightKey::SpellCheck, highlights, style, cx);
                    if let Some(state) = editor.spell_check.as_mut() {
                        *state.misspellings.borrow_mut() = misspellings;
                    }
                })
                .ok();
        }));
    }

    /// Words from `spellcheck.txt` at the root of the buffer's worktree, if
    /// the file exists. The file is opened as a buffer, so unsaved edits to it
    /// apply immediately, and remote projects work the same way.
    fn project_words(&mut self, buffer: &Entity<Buffer>, cx: &mut Context<Self>) -> Arc<HashSet<String>> {
        let Some(project) = self.project.clone() else {
            return Arc::default();
        };
        let Some(worktree_id) = buffer.read(cx).file().map(|file| file.worktree_id(cx)) else {
            return Arc::default();
        };
        let Some(state) = self.spell_check.as_ref() else {
            return Arc::default();
        };
        if let Some(words) = state.project_words.get(&worktree_id) {
            return words.words.clone();
        }
        let words_file_exists = project
            .read(cx)
            .worktree_for_id(worktree_id, cx)
            .is_some_and(|worktree| worktree.read(cx).entry_for_path(words_path()).is_some());
        if words_file_exists {
            let open = open_words_buffer(&project, worktree_id, cx);
            self.load_project_words(worktree_id, open, cx);
        }
        Arc::default()
    }

    fn load_project_words(&mut self, worktree_id: WorktreeId, open: Task<Result<Entity<Buffer>>>, cx: &Context<Self>) {
        let load = cx.spawn(async move |editor, cx| {
            let Some(buffer) = open.await.log_err() else {
                return;
            };
            editor
                .update(cx, |editor, cx| {
                    let subscription = cx.subscribe(&buffer, move |editor, buffer, event, cx| {
                        if matches!(
                            event,
                            BufferEvent::Edited { .. } | BufferEvent::Reloaded | BufferEvent::FileHandleChanged
                        ) {
                            editor.update_project_words(worktree_id, &buffer, cx);
                        }
                    });
                    if let Some(words) =
                        editor.spell_check.as_mut().and_then(|state| state.project_words.get_mut(&worktree_id))
                    {
                        words._buffer = Some((buffer.clone(), subscription));
                    }
                    editor.update_project_words(worktree_id, &buffer, cx);
                })
                .ok();
        });
        if let Some(state) = self.spell_check.as_mut() {
            state.project_words.insert(worktree_id, ProjectWords { words: Arc::default(), _load: load, _buffer: None });
        }
    }

    fn update_project_words(&mut self, worktree_id: WorktreeId, buffer: &Entity<Buffer>, cx: &mut Context<Self>) {
        let buffer = buffer.read(cx);
        let deleted = buffer.file().is_some_and(|file| file.disk_state() == language::DiskState::Deleted);
        let words = if deleted { HashSet::default() } else { spell_check::parse_project_words(&buffer.text()) };
        if let Some(entry) = self.spell_check.as_mut().and_then(|state| state.project_words.get_mut(&worktree_id)) {
            entry.words = Arc::new(words);
            self.refresh_spell_check(cx);
        }
    }

    fn add_project_word(&self, worktree_id: WorktreeId, word: String, cx: &mut Context<Self>) -> Task<Result<()>> {
        let Some(project) = self.project.clone() else {
            return Task::ready(Ok(()));
        };
        let open = open_words_buffer(&project, worktree_id, cx);
        cx.spawn(async move |editor, cx| {
            let buffer = open.await?;
            buffer.update(cx, |buffer, cx| {
                if let Some((offset, text)) = spell_check::project_word_insertion(&buffer.text(), &word) {
                    buffer.edit([(offset..offset, text)], None, cx);
                }
            });
            project
                .update(cx, |project, cx| project.save_buffer(buffer.clone(), cx))
                .await
                .with_context(|| format!("saving {PROJECT_WORDS_FILE}"))?;
            editor.update(cx, |editor, cx| {
                let loaded =
                    editor.spell_check.as_ref().is_some_and(|state| state.project_words.contains_key(&worktree_id));
                if !loaded {
                    editor.load_project_words(worktree_id, Task::ready(Ok(buffer)), cx);
                }
            })?;
            Ok(())
        })
    }
}

fn words_path() -> &'static RelPath {
    RelPath::from_unix_str(PROJECT_WORDS_FILE).expect("valid relative path")
}

fn open_words_buffer(project: &Entity<Project>, worktree_id: WorktreeId, cx: &mut App) -> Task<Result<Entity<Buffer>>> {
    project.update(cx, |project, cx| project.open_buffer(ProjectPath { worktree_id, path: words_path().into() }, cx))
}

fn check_mode(buffer: &Buffer) -> Option<CheckMode> {
    let language = buffer.language()?;
    if language.grammar().is_some_and(|grammar| grammar.spell_check_config.is_some()) {
        return Some(CheckMode::Syntax);
    }
    match language.name().as_ref() {
        "Git Commit" => Some(CheckMode::WholeText { git_commit: true }),
        // Plain Text also covers files of languages this editor dropped
        // (TOML, SQL, ...), so only real text files are checked.
        "Plain Text" if buffer.file().is_none_or(|file| file.path().extension() == Some("txt")) => {
            Some(CheckMode::WholeText { git_commit: false })
        }
        _ => None,
    }
}

fn misspelled_ranges(
    snapshot: &BufferSnapshot,
    range: Range<BufferOffset>,
    mode: CheckMode,
    words: &HashSet<String>,
) -> Vec<Range<usize>> {
    // Whole lines, so no word is cut at the edge of the visible range.
    let start_row = snapshot.offset_to_point(range.start.0).row;
    let end_row = snapshot.offset_to_point(range.end.0).row;
    let range = snapshot.point_to_offset(Point::new(start_row, 0))
        ..snapshot.point_to_offset(Point::new(end_row, snapshot.line_len(end_row)));

    let regions = match mode {
        CheckMode::Syntax => {
            let (spell, nospell) = snapshot.spell_check_ranges(range.clone());
            let spell = spell
                .into_iter()
                .map(|region| region.start.max(range.start)..region.end.min(range.end))
                .filter(|region| !region.is_empty());
            subtract(merge(spell), merge(nospell.into_iter()))
        }
        CheckMode::WholeText { git_commit } => {
            let scissors_row = git_commit
                .then(|| {
                    (0..=end_row).find(|&row| {
                        snapshot
                            .text_for_range(Point::new(row, 0)..Point::new(row, snapshot.line_len(row)))
                            .eq([GIT_SCISSORS])
                    })
                })
                .flatten();
            (start_row..=end_row)
                .take_while(|&row| scissors_row.is_none_or(|scissors| row < scissors))
                .map(|row| {
                    snapshot.point_to_offset(Point::new(row, 0))
                        ..snapshot.point_to_offset(Point::new(row, snapshot.line_len(row)))
                })
                .filter(|line| !git_commit || !snapshot.chars_at(line.start).next().is_some_and(|c| c == '#'))
                .collect()
        }
    };

    let mut misspelled = Vec::new();
    for region in regions {
        let text = snapshot.text_for_range(region.clone()).collect::<String>();
        misspelled.extend(
            spell_check::misspelled_words(&text, words)
                .into_iter()
                .map(|word| region.start + word.start..region.start + word.end),
        );
    }
    misspelled
}

/// Sorts and unions overlapping ranges.
fn merge(ranges: impl Iterator<Item = Range<usize>>) -> Vec<Range<usize>> {
    let mut ranges = ranges.collect::<Vec<_>>();
    ranges.sort_unstable_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

/// Removes `holes` from `ranges`; both sorted and disjoint.
fn subtract(ranges: Vec<Range<usize>>, holes: Vec<Range<usize>>) -> Vec<Range<usize>> {
    let mut result = Vec::new();
    let mut holes = holes.iter().peekable();
    for range in ranges {
        let mut start = range.start;
        while holes.next_if(|hole| hole.end <= start).is_some() {}
        for hole in holes.clone() {
            if hole.start >= range.end {
                break;
            }
            if hole.start > start {
                result.push(start..hole.start);
            }
            start = start.max(hole.end);
        }
        if start < range.end {
            result.push(start..range.end);
        }
    }
    result
}

/// Offers dictionary suggestions and "add to `spellcheck.txt`" for the
/// misspelled word under the cursor.
pub(crate) struct SpellCheckActions {
    editor: WeakEntity<Editor>,
    misspellings: Rc<RefCell<Vec<(BufferId, Range<text::Anchor>)>>>,
    has_project: bool,
}

impl SpellCheckActions {
    fn action(title: String, range: Range<text::Anchor>, data: serde_json::Value) -> CodeAction {
        CodeAction {
            server_id: LanguageServerId(usize::MAX),
            range,
            lsp_action: LspAction::Action(Box::new(lsp::CodeAction {
                title,
                kind: Some(lsp::CodeActionKind::QUICKFIX),
                data: Some(data),
                ..lsp::CodeAction::default()
            })),
            resolved: true,
        }
    }
}

impl CodeActionProvider for SpellCheckActions {
    fn id(&self) -> Arc<str> {
        "spell_check".into()
    }

    fn code_actions(
        &self,
        buffer: &Entity<Buffer>,
        range: Range<text::Anchor>,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Vec<CodeAction>>> {
        let snapshot = buffer.read(cx).snapshot();
        let range = range.to_offset(&snapshot);
        let Some(word_range) = self
            .misspellings
            .borrow()
            .iter()
            .filter(|(buffer_id, _)| *buffer_id == snapshot.remote_id())
            .map(|(_, anchors)| anchors.to_offset(&snapshot))
            .find(|word| word.start <= range.start && range.end <= word.end)
        else {
            return Task::ready(Ok(Vec::new()));
        };
        let word = snapshot.text_for_range(word_range.clone()).collect::<String>();
        let anchors = snapshot.anchor_range_inside(word_range);
        let can_add = self.has_project && buffer.read(cx).file().is_some();
        cx.background_spawn(async move {
            let mut actions = spell_check::suggestions(&word)
                .into_iter()
                .map(|suggestion| {
                    Self::action(
                        format!("Change to \"{suggestion}\""),
                        anchors.clone(),
                        serde_json::json!({ "replace": suggestion }),
                    )
                })
                .collect::<Vec<_>>();
            if can_add {
                actions.push(Self::action(
                    format!("Add \"{word}\" to {PROJECT_WORDS_FILE}"),
                    anchors,
                    serde_json::json!({ "add": word }),
                ));
            }
            Ok(actions)
        })
    }

    fn apply_code_action(
        &self,
        buffer: Entity<Buffer>,
        action: CodeAction,
        _push_to_history: bool,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<ProjectTransaction>> {
        let LspAction::Action(lsp_action) = &action.lsp_action else {
            return Task::ready(Ok(ProjectTransaction::default()));
        };
        let data = lsp_action.data.as_ref();
        if let Some(replacement) = data.and_then(|data| data["replace"].as_str()) {
            buffer.update(cx, |buffer, cx| {
                buffer.edit([(action.range, replacement)], None, cx);
            });
            return Task::ready(Ok(ProjectTransaction::default()));
        }
        let word = data.and_then(|data| data["add"].as_str()).map(str::to_owned);
        let worktree_id = buffer.read(cx).file().map(|file| file.worktree_id(cx));
        let (Some(word), Some(worktree_id)) = (word, worktree_id) else {
            return Task::ready(Ok(ProjectTransaction::default()));
        };
        // Called while the editor is being updated; touch it once that ends.
        let editor = self.editor.clone();
        cx.spawn(async move |cx| {
            editor.update(cx, |editor, cx| editor.add_project_word(worktree_id, word, cx))?.await?;
            Ok(ProjectTransaction::default())
        })
    }
}
