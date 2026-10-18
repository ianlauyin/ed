//! Rough syntax highlighting for buffers whose language has no tree-sitter
//! grammar, using the TextMate/Sublime syntaxes bundled by `two-face`.
//!
//! Lines are tokenized lazily, only up to the last row that is being rendered.
//! Every [`CHECKPOINT_INTERVAL`] rows the parser state is stored, so an edit
//! only re-tokenizes from the closest checkpoint above it until the parser
//! state matches the one previously recorded below the edit.

use std::{ops::Range, sync::LazyLock};

use clock::Global;
use language_core::{HighlightId, HighlightMap};
use parking_lot::{Mutex, RwLock};
use smallvec::SmallVec;
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};
use text::{BufferSnapshot as TextBufferSnapshot, Point, ToPoint as _};
use theme::SyntaxTheme;

use crate::{Language, build_highlight_map};

/// Buffers larger than this are left unhighlighted: tokenizing runs at roughly
/// 75k lines per second, and jumping to the end of a buffer tokenizes it all.
const MAX_BUFFER_LEN: usize = 1024 * 1024;
/// Lines longer than this are left unhighlighted, as regex-based
/// tokenization gets very slow on them (e.g. minified files).
const MAX_LINE_LEN: u32 = 4096;
const CHECKPOINT_INTERVAL: u32 = 16;
/// Injections larger than this are left unhighlighted, as they are tokenized
/// from their start for every chunk that is highlighted.
const MAX_INJECTION_LEN: usize = 64 * 1024;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

/// Theme capture names that TextMate scopes are mapped to.
const CAPTURE_NAMES: &[&str] = &[
    "comment",
    "string",
    "string.escape",
    "string.regex",
    "string.special.symbol",
    "number",
    "boolean",
    "constant",
    "keyword",
    "operator",
    "function",
    "type",
    "namespace",
    "tag",
    "attribute",
    "property",
    "label",
    "variable.parameter",
    "variable.special",
    "title",
    "emphasis",
    "emphasis.strong",
    "link_uri",
    "text.literal",
    "diff.plus",
    "diff.minus",
    "preproc",
    "punctuation",
];

/// TextMate scope prefixes and the capture they map to. The innermost scope
/// that matches wins, except that punctuation inside a string or comment takes
/// the enclosing string or comment style.
const SCOPE_RULES: &[(&str, &str)] = &[
    ("comment", "comment"),
    ("constant.character.escape", "string.escape"),
    ("string.regexp", "string.regex"),
    ("string", "string"),
    ("constant.numeric", "number"),
    ("constant.other.symbol", "string.special.symbol"),
    ("constant.language", "boolean"),
    ("constant", "constant"),
    ("support.constant", "constant"),
    ("keyword.operator", "operator"),
    ("keyword", "keyword"),
    ("storage", "keyword"),
    ("entity.name.function", "function"),
    ("support.function", "function"),
    ("variable.function", "function"),
    ("entity.name.tag", "tag"),
    ("entity.other.attribute-name", "attribute"),
    ("entity.name.namespace", "namespace"),
    ("entity.name.label", "label"),
    ("entity.name.section", "title"),
    ("entity.name", "type"),
    ("entity.other.inherited-class", "type"),
    ("support.type.property-name", "property"),
    ("support.type", "type"),
    ("support.class", "type"),
    ("variable.parameter", "variable.parameter"),
    ("variable.language", "variable.special"),
    ("variable.other.member", "property"),
    ("variable.other.property", "property"),
    ("markup.heading", "title"),
    ("markup.bold", "emphasis.strong"),
    ("markup.italic", "emphasis"),
    ("markup.underline.link", "link_uri"),
    ("markup.raw", "text.literal"),
    ("markup.inserted", "diff.plus"),
    ("markup.deleted", "diff.minus"),
    ("meta.preprocessor", "preproc"),
    ("punctuation", "punctuation"),
];

struct ScopeRules {
    rules: Vec<(Scope, u8)>,
    punctuation: u8,
    comment: u8,
    string: u8,
}

static SCOPE_CLASSES: LazyLock<ScopeRules> = LazyLock::new(|| {
    let class = |name: &str| {
        CAPTURE_NAMES
            .iter()
            .position(|capture| *capture == name)
            .expect("scope rule maps to an unknown capture") as u8
    };
    ScopeRules {
        rules: SCOPE_RULES
            .iter()
            .filter_map(|(scope, capture)| Some((Scope::new(scope).ok()?, class(capture))))
            .collect(),
        punctuation: class("punctuation"),
        comment: class("comment"),
        string: class("string"),
    }
});

static HIGHLIGHT_MAP: LazyLock<RwLock<HighlightMap>> = LazyLock::new(Default::default);

/// Resolves the fallback captures against the given theme.
pub(crate) fn set_theme(theme: &SyntaxTheme) {
    *HIGHLIGHT_MAP.write() = build_highlight_map(CAPTURE_NAMES, theme);
}

fn classify(stack: &[Scope]) -> Option<u8> {
    let classes = &*SCOPE_CLASSES;
    let mut punctuation = false;
    for scope in stack.iter().rev() {
        let Some(class) = classes
            .rules
            .iter()
            .find(|(prefix, _)| prefix.is_prefix_of(*scope))
            .map(|(_, class)| *class)
        else {
            continue;
        };
        if !punctuation && class == classes.punctuation {
            punctuation = true;
            continue;
        }
        if !punctuation || class == classes.comment || class == classes.string {
            return Some(class);
        }
    }
    punctuation.then_some(classes.punctuation)
}

/// Highlight run within a line: byte columns and capture class.
type LineRun = (u32, u32, u8);

#[derive(Clone, PartialEq, Eq)]
struct Checkpoint {
    parse: ParseState,
    scopes: ScopeStack,
}

// SAFETY: with the Oniguruma backend, `ParseState` holds an `onig::Region`,
// which is a plain heap allocation behind raw pointers and is not tied to the
// thread that created it. Checkpoints are only accessed under `Cache`'s mutex.
unsafe impl Send for Checkpoint {}

#[derive(Default)]
struct Line {
    runs: SmallVec<[LineRun; 4]>,
    /// Parser state at the start of this line.
    checkpoint: Option<Box<Checkpoint>>,
    /// The line's text changed since it was tokenized.
    dirty: bool,
}

struct Cache {
    version: Global,
    /// Tokenized lines, starting at row 0. Rows past the end are not tokenized yet.
    lines: Vec<Line>,
    /// First row whose highlights are not known to be up to date.
    frontier: u32,
    line_text: String,
}

pub struct FallbackHighlighter {
    syntax: &'static SyntaxReference,
    cache: Mutex<Cache>,
}

impl FallbackHighlighter {
    /// Picks a syntax for a buffer whose language has no tree-sitter grammar,
    /// based on the file name, then the language name, then the first line.
    pub(crate) fn for_buffer(
        language: Option<&Language>,
        file_name: Option<&str>,
        text: &TextBufferSnapshot,
    ) -> Option<Self> {
        if language.is_some_and(|language| language.grammar().is_some()) {
            return None;
        }
        let syntax_set = &*SYNTAX_SET;
        let syntax = file_name
            .and_then(|file_name| {
                syntax_set.find_syntax_by_extension(file_name).or_else(|| {
                    let (_, extension) = file_name.rsplit_once('.')?;
                    syntax_set.find_syntax_by_extension(extension)
                })
            })
            .or_else(|| {
                let name = language?.name();
                syntax_set.find_syntax_by_token(name.as_ref())
            })
            .or_else(|| {
                let first_line_end = text.line_len(0).min(256);
                let first_line = text
                    .text_for_range(Point::zero()..Point::new(0, first_line_end))
                    .collect::<String>();
                syntax_set.find_syntax_by_first_line(&first_line)
            })?;
        if syntax.name == syntax_set.find_syntax_plain_text().name {
            return None;
        }
        Some(Self::new(syntax, text))
    }

    /// Highlights with the bundled syntax of the given name, regardless of the
    /// buffer's language or file.
    pub(crate) fn for_syntax_name(name: &str, text: &TextBufferSnapshot) -> Option<Self> {
        Some(Self::new(SYNTAX_SET.find_syntax_by_name(name)?, text))
    }

    fn new(syntax: &'static SyntaxReference, text: &TextBufferSnapshot) -> Self {
        Self {
            syntax,
            cache: Mutex::new(Cache {
                version: text.version().clone(),
                lines: Vec::new(),
                frontier: 0,
                line_text: String::new(),
            }),
        }
    }

    pub fn syntax_name(&self) -> &str {
        &self.syntax.name
    }

    /// Returns highlight runs covering `range`, or `None` if the buffer cannot
    /// be highlighted at this version.
    pub(crate) fn highlight_runs(
        &self,
        text: &TextBufferSnapshot,
        range: Range<usize>,
    ) -> Option<Vec<(Range<usize>, HighlightId)>> {
        if text.len() > MAX_BUFFER_LEN {
            return None;
        }
        if range.is_empty() {
            return Some(Vec::new());
        }
        let start = range.start.to_point(text);
        let end = range.end.to_point(text);

        let mut cache = self.cache.lock();
        if !cache.sync(text) {
            return None;
        }
        cache.tokenize_through(self.syntax, text, end.row);

        let highlight_map = HIGHLIGHT_MAP.read();
        let mut runs = Vec::<(Range<usize>, HighlightId)>::new();
        for row in start.row..=end.row {
            let Some(line) = cache.lines.get(row as usize) else {
                break;
            };
            let line_start = text.point_to_offset(Point::new(row, 0));
            for &(run_start, run_end, class) in &line.runs {
                let run_range = (line_start + run_start as usize).max(range.start)
                    ..(line_start + run_end as usize).min(range.end);
                if run_range.is_empty() {
                    continue;
                }
                let Some(highlight_id) = highlight_map.get(language_core::CaptureId(class as u32))
                else {
                    continue;
                };
                match runs.last_mut() {
                    Some((last_range, last_id))
                        if *last_id == highlight_id && last_range.end == run_range.start =>
                    {
                        last_range.end = run_range.end;
                    }
                    _ => runs.push((run_range, highlight_id)),
                }
            }
        }
        Some(runs)
    }
}

impl Cache {
    /// Brings the cached lines to the given text's version, marking edited
    /// rows dirty. Returns false if the text is older than the cache.
    fn sync(&mut self, text: &TextBufferSnapshot) -> bool {
        if *text.version() == self.version {
            return true;
        }
        if !text.version().observed_all(&self.version) {
            return false;
        }
        let edits = text.edits_since::<Point>(&self.version).collect::<Vec<_>>();
        // Splice from the last edit backwards, so that the old rows of the
        // remaining edits still index `lines`.
        for edit in edits.iter().rev() {
            self.frontier = self.frontier.min(edit.new.start.row);
            let old_start_row = edit.old.start.row as usize;
            let old_end_row = edit.old.end.row as usize;
            if old_start_row >= self.lines.len() {
                continue;
            }
            if old_end_row + 1 >= self.lines.len() {
                self.lines.truncate(old_start_row);
            } else {
                let new_lines = (edit.new.end.row - edit.new.start.row + 1) as usize;
                self.lines.splice(
                    old_start_row..=old_end_row,
                    (0..new_lines).map(|_| Line {
                        dirty: true,
                        ..Line::default()
                    }),
                );
            }
        }
        self.frontier = self.frontier.min(self.lines.len() as u32);
        self.version = text.version().clone();
        true
    }

    /// Tokenizes lines until every row up to `target_row` is up to date.
    fn tokenize_through(
        &mut self,
        syntax: &'static SyntaxReference,
        text: &TextBufferSnapshot,
        target_row: u32,
    ) {
        let row_count = text.max_point().row + 1;
        while self.frontier <= target_row && self.frontier < row_count {
            let first_stale_row = self.frontier;
            // Resume from the closest checkpoint above the first stale row.
            // Rows above it are up to date, so their checkpoints are valid.
            let (mut row, mut state) = self.lines[..first_stale_row as usize]
                .iter()
                .enumerate()
                .rev()
                .find_map(|(row, line)| Some((row as u32, line.checkpoint.as_deref()?.clone())))
                .unwrap_or_else(|| {
                    (
                        0,
                        Checkpoint {
                            parse: ParseState::new(syntax),
                            scopes: ScopeStack::new(),
                        },
                    )
                });

            self.frontier = loop {
                if row >= row_count {
                    break row_count;
                }
                if row > target_row {
                    break row;
                }
                // Past the stale row, stop once the parser state matches what
                // was recorded there earlier: the following lines are
                // unchanged up to the next edited one.
                if row > first_stale_row
                    && let Some(line) = self.lines.get(row as usize)
                    && !line.dirty
                    && line.checkpoint.as_deref() == Some(&state)
                {
                    break self.lines[row as usize..]
                        .iter()
                        .position(|line| line.dirty)
                        .map_or(self.lines.len() as u32, |ix| row + ix as u32);
                }

                let checkpoint = (row % CHECKPOINT_INTERVAL == 0).then(|| Box::new(state.clone()));
                let runs = tokenize_line(&mut self.line_text, text, row, &mut state);
                let line = Line {
                    runs,
                    checkpoint,
                    dirty: false,
                };
                if let Some(existing) = self.lines.get_mut(row as usize) {
                    *existing = line;
                } else {
                    self.lines.push(line);
                }
                row += 1;
            };
        }
    }
}

fn tokenize_line(
    line_text: &mut String,
    text: &TextBufferSnapshot,
    row: u32,
    state: &mut Checkpoint,
) -> SmallVec<[LineRun; 4]> {
    let mut runs = SmallVec::new();
    let line_len = text.line_len(row);
    if line_len > MAX_LINE_LEN {
        return runs;
    }
    line_text.clear();
    line_text.extend(text.text_for_range(Point::new(row, 0)..Point::new(row, line_len)));
    line_text.push('\n');
    let Ok(ops) = state.parse.parse_line(line_text, &SYNTAX_SET) else {
        return runs;
    };

    let mut push_run = |start: usize, end: usize, scopes: &ScopeStack| {
        let end = end.min(line_len as usize);
        if start >= end {
            return;
        }
        let Some(class) = classify(scopes.as_slice()) else {
            return;
        };
        match runs.last_mut() {
            Some((_, last_end, last_class))
                if *last_class == class && *last_end == start as u32 =>
            {
                *last_end = end as u32;
            }
            _ => runs.push((start as u32, end as u32, class)),
        }
    };
    let mut start = 0;
    for (offset, op) in ops {
        push_run(start, offset, &state.scopes);
        start = offset;
        state.scopes.apply(&op).ok();
    }
    push_run(start, line_len as usize, &state.scopes);
    runs
}

/// Highlights the part of an injection (e.g. a Markdown code block) that lies
/// in `range`, for an injected language without a tree-sitter grammar. The
/// syntax is looked up by the injection's language name. Tokenizing starts at
/// the beginning of the injection every time, so this is only meant for
/// injections of moderate size.
pub(crate) fn highlight_injection(
    language_name: &str,
    text: &TextBufferSnapshot,
    content_range: Range<usize>,
    range: Range<usize>,
) -> Vec<(Range<usize>, HighlightId)> {
    let mut runs = Vec::new();
    if content_range.len() > MAX_INJECTION_LEN {
        return runs;
    }
    let syntax_set = &*SYNTAX_SET;
    let Some(syntax) = syntax_set
        .find_syntax_by_token(language_name)
        .filter(|syntax| syntax.name != syntax_set.find_syntax_plain_text().name)
    else {
        return runs;
    };
    let range = range.start.max(content_range.start)..range.end.min(content_range.end);
    if range.is_empty() {
        return runs;
    }

    let start_row = content_range.start.to_point(text).row;
    let end_row = range.end.to_point(text).row;
    let mut state = Checkpoint {
        parse: ParseState::new(syntax),
        scopes: ScopeStack::new(),
    };
    let mut line_text = String::new();
    let highlight_map = HIGHLIGHT_MAP.read();
    for row in start_row..=end_row {
        let line_runs = tokenize_line(&mut line_text, text, row, &mut state);
        let line_start = text.point_to_offset(Point::new(row, 0));
        if line_start + (text.line_len(row) as usize) < range.start {
            continue;
        }
        for (run_start, run_end, class) in line_runs {
            let run_range = (line_start + run_start as usize).max(range.start)
                ..(line_start + run_end as usize).min(range.end);
            if run_range.is_empty() {
                continue;
            }
            if let Some(highlight_id) = highlight_map.get(language_core::CaptureId(class as u32)) {
                runs.push((run_range, highlight_id));
            }
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::HighlightStyle;
    use text::{Buffer as TextBuffer, BufferId, ReplicaId};

    fn theme() -> SyntaxTheme {
        SyntaxTheme::new(
            CAPTURE_NAMES
                .iter()
                .map(|name| (name.to_string(), HighlightStyle::default())),
        )
    }

    fn highlighted(
        highlighter: &FallbackHighlighter,
        text: &TextBufferSnapshot,
        theme: &SyntaxTheme,
    ) -> Vec<(String, String)> {
        highlighter
            .highlight_runs(text, 0..text.len())
            .unwrap()
            .into_iter()
            .map(|(range, id)| {
                (
                    text.text_for_range(range).collect(),
                    theme.get_capture_name(id).unwrap().to_string(),
                )
            })
            .collect()
    }

    #[test]
    fn test_highlights_c_and_tracks_edits() {
        let theme = theme();
        set_theme(&theme);
        let mut buffer = TextBuffer::new(
            ReplicaId::LOCAL,
            BufferId::new(1).unwrap(),
            "int main() {\n    return 0; // done\n}\n",
        );
        let highlighter =
            FallbackHighlighter::for_buffer(None, Some("main.c"), &buffer.snapshot()).unwrap();
        assert_eq!(highlighter.syntax_name(), "C");

        let runs = highlighted(&highlighter, &buffer.snapshot(), &theme);
        assert!(runs.contains(&("int".into(), "keyword".into())), "{runs:?}");
        assert!(
            runs.contains(&("return".into(), "keyword".into())),
            "{runs:?}"
        );
        assert!(runs.contains(&("0".into(), "number".into())), "{runs:?}");
        assert!(
            runs.contains(&("// done".into(), "comment".into())),
            "{runs:?}"
        );

        // Opening a block comment turns the following lines into a comment.
        buffer.edit([(0..0, "/*\n")]);
        let runs = highlighted(&highlighter, &buffer.snapshot(), &theme);
        assert!(
            runs.iter()
                .all(|(text, capture)| capture == "comment" || text.trim().is_empty()),
            "{runs:?}"
        );

        buffer.undo();
        let runs = highlighted(&highlighter, &buffer.snapshot(), &theme);
        assert!(
            runs.contains(&("return".into(), "keyword".into())),
            "{runs:?}"
        );
    }

    #[test]
    fn test_highlights_regex_query() {
        let theme = theme();
        set_theme(&theme);
        let buffer = TextBuffer::new(
            ReplicaId::LOCAL,
            BufferId::new(1).unwrap(),
            r"^fn\s+(\w+)\(.*$",
        );
        let highlighter =
            FallbackHighlighter::for_syntax_name("Regular Expression", &buffer.snapshot())
                .unwrap();
        let runs = highlighted(&highlighter, &buffer.snapshot(), &theme);
        assert!(runs.iter().any(|(text, _)| text == "^"), "{runs:?}");
        assert!(runs.iter().any(|(text, _)| text == r"\s"), "{runs:?}");
    }

    #[test]
    fn test_highlights_injection() {
        let theme = theme();
        set_theme(&theme);
        let source = "# q\n```sql\nSELECT 1\nFROM t;\n```\n";
        let buffer = TextBuffer::new(ReplicaId::LOCAL, BufferId::new(1).unwrap(), source);
        let text = buffer.snapshot();
        let content_start = source.find("SELECT").unwrap();
        let content_range = content_start..source.rfind("```").unwrap();
        let from_row = source.find("FROM").unwrap();
        let runs = |range: Range<usize>| {
            highlight_injection("sql", &text, content_range.clone(), range)
                .into_iter()
                .map(|(range, id)| {
                    (
                        source[range].to_string(),
                        theme.get_capture_name(id).unwrap().to_string(),
                    )
                })
                .collect::<Vec<_>>()
        };

        let all = runs(0..source.len());
        assert!(
            all.contains(&("SELECT".into(), "keyword".into())),
            "{all:?}"
        );
        assert!(all.contains(&("FROM".into(), "keyword".into())), "{all:?}");
        // Only the requested range is returned, even though tokenizing
        // starts at the beginning of the injection.
        let tail = runs(from_row..source.len());
        assert!(tail.iter().all(|(text, _)| text != "SELECT"), "{tail:?}");
        assert!(
            tail.contains(&("FROM".into(), "keyword".into())),
            "{tail:?}"
        );
        assert!(
            highlight_injection("no-such-language", &text, content_range, 0..source.len())
                .is_empty()
        );
    }

    #[test]
    fn test_syntax_selection() {
        let text = |text: &str| {
            TextBuffer::new(ReplicaId::LOCAL, BufferId::new(1).unwrap(), text)
                .snapshot()
                .clone()
        };
        let syntax = |file_name: Option<&str>, language: Option<&str>, first_line: &str| {
            let language = language.map(|name| {
                Language::new(
                    crate::LanguageConfig {
                        name: crate::LanguageName::new(name),
                        ..Default::default()
                    },
                    None,
                )
            });
            FallbackHighlighter::for_buffer(language.as_ref(), file_name, &text(first_line))
                .map(|highlighter| highlighter.syntax_name().to_string())
        };
        assert_eq!(
            syntax(Some("Cargo.toml"), None, "").as_deref(),
            Some("TOML")
        );
        assert_eq!(
            syntax(Some("Dockerfile"), None, "").as_deref(),
            Some("Dockerfile")
        );
        assert_eq!(syntax(Some("app.lua"), None, "").as_deref(), Some("Lua"));
        assert_eq!(syntax(None, Some("C++"), "").as_deref(), Some("C++"));
        assert_eq!(syntax(None, Some("SQL"), "").as_deref(), Some("SQL"));
        assert_eq!(
            syntax(Some("run"), None, "#!/usr/bin/env ruby").as_deref(),
            Some("Ruby")
        );
        assert_eq!(syntax(Some("notes.txt"), None, "hello"), None);
    }

    #[test]
    fn test_incremental_matches_fresh_highlighting() {
        let theme = theme();
        set_theme(&theme);
        let line = "fn main() { let s = \"a /* b\"; } // c\n";
        let mut buffer = TextBuffer::new(
            ReplicaId::LOCAL,
            BufferId::new(1).unwrap(),
            line.repeat(200),
        );
        let highlighter =
            FallbackHighlighter::for_buffer(None, Some("main.rs"), &buffer.snapshot()).unwrap();
        highlighted(&highlighter, &buffer.snapshot(), &theme);

        let mut rng = 7u64;
        let mut next = |bound: usize| {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (rng >> 33) as usize % bound.max(1)
        };
        let inserts = ["/*", "*/", "\"", "\n", "x", "// ", "\n\n"];
        for _ in 0..200 {
            let len = buffer.len();
            let start = next(len + 1);
            let start = buffer.clip_offset(start, text::Bias::Left);
            if next(3) == 0 {
                let end = buffer.clip_offset((start + next(40)).min(len), text::Bias::Right);
                buffer.edit([(start..end, "")]);
            } else {
                buffer.edit([(start..start, inserts[next(inserts.len())])]);
            }

            let snapshot = buffer.snapshot();
            // Only request part of the buffer some of the time, to leave
            // stale rows below the frontier.
            if next(2) == 0 {
                let end = snapshot.point_to_offset(Point::new(snapshot.max_point().row / 2, 0));
                highlighter.highlight_runs(&snapshot, 0..end);
                continue;
            }
            let fresh = FallbackHighlighter::for_buffer(None, Some("main.rs"), &snapshot).unwrap();
            assert_eq!(
                highlighted(&highlighter, &snapshot, &theme),
                highlighted(&fresh, &snapshot, &theme)
            );
        }
    }

    #[test]
    #[ignore]
    fn bench_large_file() {
        set_theme(&theme());
        let source = "static int add(int a, int b) {\n    /* sum */ return a + b; // ok\n}\n";
        let buffer = TextBuffer::new(
            ReplicaId::LOCAL,
            BufferId::new(1).unwrap(),
            source.repeat(10_000),
        );
        let snapshot = buffer.snapshot();
        let start = std::time::Instant::now();
        LazyLock::force(&SYNTAX_SET);
        eprintln!("load syntaxes: {:?}", start.elapsed());
        let highlighter = FallbackHighlighter::for_buffer(None, Some("main.c"), &snapshot).unwrap();
        let start = std::time::Instant::now();
        highlighter.highlight_runs(&snapshot, 0..snapshot.len());
        eprintln!(
            "highlight {} lines: {:?}",
            snapshot.max_point().row,
            start.elapsed()
        );
    }
}
