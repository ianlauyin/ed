mod builder;
mod element;
pub mod html;
pub mod parser;
mod path_range;
mod rendered_text;
mod selection;
mod style;

use base64::Engine as _;
use gpui::HitboxBehavior;
use language::LanguageName;

use log::Level;
use smallvec::SmallVec;
use util::maybe;

use std::borrow::Cow;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::mem;
use std::ops::Range;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use collections::{HashMap, HashSet};
use gpui::{
    AnyElement, App, BorderStyle, Bounds, ClipboardItem, CursorStyle, DispatchPhase, Edges, Entity,
    EntityId, FocusHandle, Focusable, FontStyle, FontWeight, GlobalElementId, Hitbox, Hsla, Image,
    ImageFormat, ImageSource, InputHandler, KeyContext, Length, MouseButton, MouseDownEvent,
    MouseEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollHandle, Stateful,
    StrikethroughStyle, StyleRefinement, StyledImage, StyledText, Task, TextAlign, TextLayout,
    TextRun, TextStyle, TextStyleRefinement, UTF16Selection, WrappedLineLayout, actions, canvas,
    img, point, quad, size,
};
use language::{
    Bias, CharClassifier, Language, LanguageRegistry, OffsetUtf16, ResolvedHighlights, Rope,
};
use parser::{
    MarkdownEvent, MarkdownTag, MarkdownTagEnd, ParsedMetadataBlock, parse_markdown_with_options,
};
use pulldown_cmark::{Alignment, BlockQuoteKind};
use sum_tree::TreeMap;
use theme::SyntaxTheme;
use ui::utils::{ensure_highlight_contrast, merge_highlight_backgrounds};
use ui::{Checkbox, CopyButton, ScrollAxes, Scrollbars, Tooltip, WithScrollbar, prelude::*};
use util::ResultExt;

use crate::parser::{CodeBlockKind, CodeBlockMetadata};
use builder::*;
use element::*;
pub use element::{AnyDiv, AutoscrollBehavior, MarkdownElement};
pub use rendered_text::RenderedMarkdown;
use rendered_text::*;
pub use style::{BlockQuoteKindColors, HeadingLevelStyles, MarkdownStyle};

pub struct Markdown {
    source: SharedString,
    selection: Selection,
    pressed_link: Option<RenderedLink>,
    pressed_footnote_ref: Option<RenderedFootnoteRef>,
    autoscroll_request: Option<usize>,
    pending_heading_scroll: Option<SharedString>,
    pending_autoscroll: Option<usize>,
    active_root_block: Option<usize>,
    parsed_markdown: ParsedMarkdown,
    images_by_source_offset: HashMap<usize, Arc<Image>>,
    should_reparse: bool,
    pending_parse: Option<Task<()>>,
    focus_handle: FocusHandle,
    language_registry: Option<Arc<LanguageRegistry>>,
    fallback_code_block_language: Option<LanguageName>,
    options: MarkdownOptions,
    code_block_scroll_handles: BTreeMap<usize, ScrollHandle>,
    context_menu_link: Option<SharedString>,
    context_menu_selected_text: Option<SharedString>,
    context_menu_selected_markdown: Option<SharedString>,
    search_highlights: Rc<[Range<usize>]>,
    active_search_highlight: Option<usize>,
}

#[derive(Clone, Copy, Default)]
pub struct MarkdownOptions {
    pub parse_html: bool,
    pub parse_heading_slugs: bool,
    pub render_metadata_blocks: bool,
}

/// A fenced, closed code block, handed to [`MarkdownElement::render_code_block_with`].
pub struct CodeBlock<'a> {
    /// Source offset of the block's opening fence; stable across renders while
    /// the block itself is unchanged.
    pub source_offset: usize,
    pub kind: &'a CodeBlockKind,
    pub content_range: Range<usize>,
    /// The block's contents, without the trailing newline.
    pub contents: &'a str,
}

impl<'a> CodeBlock<'a> {
    fn new(
        source_offset: usize,
        kind: &'a CodeBlockKind,
        metadata: &CodeBlockMetadata,
        source: &'a str,
    ) -> Option<Self> {
        metadata.is_fenced_closed.then(|| Self {
            source_offset,
            kind,
            content_range: metadata.content_range.clone(),
            contents: source[metadata.content_range.clone()].trim_end_matches('\n'),
        })
    }
}

actions!(
    markdown,
    [
        /// Copies the selected text to the clipboard.
        Copy,
        /// Copies the selected text as markdown to the clipboard.
        CopyAsMarkdown
    ]
);

enum EscapeAction {
    PassThrough,
    Nbsp(usize),
    DoubleNewline,
    PrefixBackslash,
}

impl EscapeAction {
    fn output_len(&self, c: char) -> usize {
        match self {
            Self::PassThrough => c.len_utf8(),
            Self::Nbsp(count) => count * '\u{00A0}'.len_utf8(),
            Self::DoubleNewline => 2,
            Self::PrefixBackslash => '\\'.len_utf8() + c.len_utf8(),
        }
    }

    fn write_to(&self, c: char, output: &mut String) {
        match self {
            Self::PassThrough => output.push(c),
            Self::Nbsp(count) => {
                for _ in 0..*count {
                    output.push('\u{00A0}');
                }
            }
            Self::DoubleNewline => {
                output.push('\n');
                output.push('\n');
            }
            Self::PrefixBackslash => {
                // '\\' is a single backslash in Rust, e.g. '|' -> '\|'
                output.push('\\');
                output.push(c);
            }
        }
    }
}

struct MarkdownEscaper {
    in_leading_whitespace: bool,
}

impl MarkdownEscaper {
    const TAB_SIZE: usize = 4;

    fn new() -> Self {
        Self {
            in_leading_whitespace: true,
        }
    }

    fn next(&mut self, c: char) -> EscapeAction {
        let action = if self.in_leading_whitespace && c == '\t' {
            EscapeAction::Nbsp(Self::TAB_SIZE)
        } else if self.in_leading_whitespace && c == ' ' {
            EscapeAction::Nbsp(1)
        } else if c == '\n' {
            EscapeAction::DoubleNewline
        } else if c.is_ascii_punctuation() {
            EscapeAction::PrefixBackslash
        } else {
            EscapeAction::PassThrough
        };

        self.in_leading_whitespace =
            c == '\n' || (self.in_leading_whitespace && (c == ' ' || c == '\t'));
        action
    }
}

impl Markdown {
    pub fn new(
        source: SharedString,
        language_registry: Option<Arc<LanguageRegistry>>,
        fallback_code_block_language: Option<LanguageName>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_with_options(
            source,
            language_registry,
            fallback_code_block_language,
            MarkdownOptions::default(),
            cx,
        )
    }

    pub fn new_with_options(
        source: SharedString,
        language_registry: Option<Arc<LanguageRegistry>>,
        fallback_code_block_language: Option<LanguageName>,
        options: MarkdownOptions,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let mut this = Self {
            source,
            selection: Selection::default(),
            pressed_link: None,
            pressed_footnote_ref: None,
            autoscroll_request: None,
            pending_heading_scroll: None,
            pending_autoscroll: None,
            active_root_block: None,
            should_reparse: false,
            images_by_source_offset: Default::default(),
            parsed_markdown: ParsedMarkdown::default(),
            pending_parse: None,
            focus_handle,
            language_registry,
            fallback_code_block_language,
            options,
            code_block_scroll_handles: BTreeMap::default(),
            context_menu_link: None,
            context_menu_selected_text: None,
            context_menu_selected_markdown: None,
            search_highlights: Rc::default(),
            active_search_highlight: None,
        };
        this.parse(cx);
        this
    }

    fn code_block_scroll_handle(&mut self, id: usize) -> ScrollHandle {
        self.code_block_scroll_handles
            .entry(id)
            .or_insert_with(ScrollHandle::new)
            .clone()
    }

    fn retain_code_block_scroll_handles(&mut self, ids: &HashSet<usize>) {
        self.code_block_scroll_handles
            .retain(|id, _| ids.contains(id));
    }

    fn clear_code_block_scroll_handles(&mut self) {
        self.code_block_scroll_handles.clear();
    }

    fn autoscroll_code_block(&self, source_index: usize, cursor_position: Point<Pixels>) {
        let Some((_, scroll_handle)) = self
            .code_block_scroll_handles
            .range(..=source_index)
            .next_back()
        else {
            return;
        };

        let bounds = scroll_handle.bounds();
        if cursor_position.y < bounds.top() || cursor_position.y > bounds.bottom() {
            return;
        }

        let horizontal_delta = if cursor_position.x < bounds.left() {
            bounds.left() - cursor_position.x
        } else if cursor_position.x > bounds.right() {
            bounds.right() - cursor_position.x
        } else {
            return;
        };

        let offset = scroll_handle.offset();
        scroll_handle.set_offset(point(offset.x + horizontal_delta, offset.y));
    }

    pub fn is_parsing(&self) -> bool {
        self.pending_parse.is_some()
    }

    pub fn scroll_to_heading_when_parsed(&mut self, slug: SharedString, cx: &mut Context<Self>) {
        if self.pending_parse.is_some() || self.source.is_empty() {
            self.pending_heading_scroll = Some(slug);
        } else {
            self.scroll_to_heading(&slug, cx);
        }
    }

    pub fn scroll_to_heading(&mut self, slug: &str, cx: &mut Context<Self>) -> Option<usize> {
        if let Some(source_index) = self.parsed_markdown.heading_slugs.get(slug).copied() {
            self.autoscroll_request = Some(source_index);
            cx.notify();
            Some(source_index)
        } else {
            None
        }
    }

    pub fn source(&self) -> &SharedString {
        &self.source
    }

    pub fn parsed_markdown(&self) -> &ParsedMarkdown {
        &self.parsed_markdown
    }

    pub fn non_rendered_source_ranges(&self) -> Vec<Range<usize>> {
        if self.source != self.parsed_markdown.source {
            return Vec::new();
        }

        self.parsed_markdown.non_rendered_source_ranges()
    }

    pub fn replace(&mut self, source: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.source = source.into();
        self.parse(cx);
    }

    pub fn request_autoscroll_to_source_index(
        &mut self,
        source_index: usize,
        cx: &mut Context<Self>,
    ) {
        if self.pending_parse.is_some() {
            self.pending_autoscroll = Some(source_index);
        } else {
            self.autoscroll_request = Some(source_index);
        }
        cx.refresh_windows();
    }

    fn footnote_definition_content_start(&self, label: &SharedString) -> Option<usize> {
        self.parsed_markdown
            .footnote_definitions
            .get(label)
            .copied()
    }

    pub fn set_active_root_for_source_index(
        &mut self,
        source_index: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let active_root_block =
            source_index.and_then(|index| self.parsed_markdown.root_block_for_source_index(index));
        if self.active_root_block == active_root_block {
            return;
        }

        self.active_root_block = active_root_block;
        cx.notify();
    }

    pub fn reset(&mut self, source: SharedString, cx: &mut Context<Self>) {
        if &source == self.source() {
            if self.pending_parse.is_none() {
                if let Some(slug) = self.pending_heading_scroll.take() {
                    self.pending_autoscroll = None;
                    self.scroll_to_heading(&slug, cx);
                } else if let Some(source_index) = self.pending_autoscroll.take() {
                    self.autoscroll_request = Some(source_index);
                    cx.refresh_windows();
                }
            }
            return;
        }
        if !self.source.is_empty() {
            self.pending_heading_scroll = None;
        }
        self.source = source;
        self.selection = Selection::default();
        self.autoscroll_request = None;
        self.pending_autoscroll = None;
        self.pending_parse = None;
        self.should_reparse = false;
        self.search_highlights = Rc::default();
        self.active_search_highlight = None;
        // Don't clear parsed_markdown here - keep existing content visible until new parse completes
        self.parse(cx);
    }

    pub fn escape(s: &str) -> Cow<'_, str> {
        let output_len: usize = {
            let mut escaper = MarkdownEscaper::new();
            s.chars().map(|c| escaper.next(c).output_len(c)).sum()
        };

        if output_len == s.len() {
            return s.into();
        }

        let mut escaper = MarkdownEscaper::new();
        let mut output = String::with_capacity(output_len);
        for c in s.chars() {
            escaper.next(c).write_to(c, &mut output);
        }
        output.into()
    }

    pub fn has_selection(&self) -> bool {
        self.selection.end > self.selection.start
    }

    pub fn selected_source(&self) -> Option<&str> {
        if self.selection.end <= self.selection.start {
            return None;
        }
        self.source.get(self.selection.start..self.selection.end)
    }

    pub fn set_search_highlights(
        &mut self,
        highlights: Vec<Range<usize>>,
        active: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        debug_assert!(
            highlights
                .windows(2)
                .all(|ranges| (ranges[0].start, ranges[0].end) <= (ranges[1].start, ranges[1].end))
        );
        self.search_highlights = highlights.into();
        self.active_search_highlight =
            active.filter(|active| *active < self.search_highlights.len());
        cx.notify();
    }

    pub fn clear_search_highlights(&mut self, cx: &mut Context<Self>) {
        if !self.search_highlights.is_empty() || self.active_search_highlight.is_some() {
            self.search_highlights = Rc::default();
            self.active_search_highlight = None;
            cx.notify();
        }
    }

    pub fn set_active_search_highlight(&mut self, active: Option<usize>, cx: &mut Context<Self>) {
        let active = active.filter(|active| *active < self.search_highlights.len());
        if self.active_search_highlight != active {
            self.active_search_highlight = active;
            cx.notify();
        }
    }

    pub fn search_highlights(&self) -> &[Range<usize>] {
        &self.search_highlights
    }

    pub fn active_search_highlight(&self) -> Option<usize> {
        self.active_search_highlight
    }

    fn copy(&self, text: &RenderedText, _: &mut Window, cx: &mut Context<Self>) {
        if self.selection.end <= self.selection.start {
            return;
        }
        let text = text.text_for_range(self.selection.start..self.selection.end);
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    fn copy_as_markdown(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.context_menu_selected_markdown.take() {
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
            return;
        }
        if self.selection.end <= self.selection.start {
            return;
        }
        let text = self
            .parsed_markdown
            .rebalanced_markdown_for_selection(self.selection.start..self.selection.end);
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    fn capture_for_context_menu(
        &mut self,
        link: Option<SharedString>,
        rendered_text: Option<&RenderedText>,
    ) {
        let range = self.selection.start..self.selection.end;
        if range.end > range.start {
            self.context_menu_selected_markdown = Some(SharedString::new(
                self.parsed_markdown
                    .rebalanced_markdown_for_selection(range.clone()),
            ));
            self.context_menu_selected_text = rendered_text
                .map(|text| text.text_for_range(range))
                .map(SharedString::new)
                .or_else(|| self.context_menu_selected_markdown.clone());
        } else {
            self.context_menu_selected_markdown = None;
            self.context_menu_selected_text = None;
        }
        self.context_menu_link = link;
    }

    /// Returns the URL of the link that was most recently right-clicked, if any.
    /// This is set during a right-click mouse-down event and can be read by parent
    /// views to include a "Copy Link" item in their context menus.
    pub fn context_menu_link(&self) -> Option<&SharedString> {
        self.context_menu_link.as_ref()
    }

    /// Returns the rendered (plain) text that was selected when the most recent
    /// context menu invocation happened.
    pub fn context_menu_selected_text(&self) -> Option<&SharedString> {
        self.context_menu_selected_text.as_ref()
    }

    /// Returns the markdown that was selected when the most recent context
    /// menu invocation happened, rebalanced via
    /// [`ParsedMarkdown::rebalanced_markdown_for_selection`].
    pub fn context_menu_selected_markdown(&self) -> Option<&SharedString> {
        self.context_menu_selected_markdown.as_ref()
    }

    fn parse(&mut self, cx: &mut Context<Self>) {
        if self.source.is_empty() {
            self.should_reparse = false;
            self.pending_parse.take();
            self.pending_heading_scroll = None;
            self.pending_autoscroll = None;
            self.parsed_markdown = ParsedMarkdown {
                source: self.source.clone(),
                ..Default::default()
            };
            self.active_root_block = None;
            self.images_by_source_offset.clear();
            cx.notify();
            cx.refresh_windows();
            return;
        }

        if self.pending_parse.is_some() {
            self.should_reparse = true;
            return;
        }
        self.should_reparse = false;
        self.pending_parse = Some(self.start_background_parse(cx));
    }

    fn start_background_parse(&self, cx: &Context<Self>) -> Task<()> {
        let source = self.source.clone();
        let should_parse_html = self.options.parse_html;
        let should_parse_heading_slugs = self.options.parse_heading_slugs;
        let should_parse_metadata_blocks = self.options.render_metadata_blocks;
        let language_registry = self.language_registry.clone();
        let fallback = self.fallback_code_block_language.clone();

        let parsed = cx.background_spawn(async move {
            let parsed = parse_markdown_with_options(
                &source,
                should_parse_html,
                should_parse_heading_slugs,
                should_parse_metadata_blocks,
            );
            let events = parsed.events;
            let language_names = parsed.language_names;
            let paths = parsed.language_paths;
            let root_block_starts = parsed.root_block_starts;
            let html_blocks = parsed.html_blocks;
            let metadata_blocks = parsed.metadata_blocks;
            let heading_slugs = parsed.heading_slugs;
            let footnote_definitions = parsed.footnote_definitions;
            let link_definition_spans = parsed.link_definition_spans;
            let has_untagged_code_block = parsed.has_untagged_code_block;
            let mut images_by_source_offset = HashMap::default();
            let mut languages_by_name = TreeMap::default();
            let mut languages_by_path = TreeMap::default();
            let mut fallback_code_block_language = None;
            if let Some(registry) = language_registry.as_ref() {
                for name in language_names {
                    let mut language = registry.language_for_name_or_extension(&name).await;
                    if language.is_err()
                        && let Some((first_word, _)) = name.split_once(char::is_whitespace)
                    {
                        language = registry.language_for_name_or_extension(first_word).await;
                    }
                    if let Ok(language) = language {
                        languages_by_name.insert(name, language);
                    }
                }

                for path in paths {
                    if let Ok(language) = registry
                        .load_language_for_file_path(Path::new(path.as_ref()))
                        .await
                    {
                        languages_by_path.insert(path, language);
                    }
                }

                if has_untagged_code_block && let Some(fallback) = &fallback {
                    fallback_code_block_language =
                        registry.language_for_name(fallback.as_ref()).await.ok();
                }
            }

            for (range, event) in &events {
                if let MarkdownEvent::Start(MarkdownTag::Image { dest_url, .. }) = event
                    && let Some(data_url) = dest_url.strip_prefix("data:")
                {
                    let Some((mime_info, data)) = data_url.split_once(',') else {
                        continue;
                    };
                    let Some((mime_type, encoding)) = mime_info.split_once(';') else {
                        continue;
                    };
                    let Some(format) = ImageFormat::from_mime_type(mime_type) else {
                        continue;
                    };
                    let is_base64 = encoding == "base64";
                    if is_base64
                        && let Some(bytes) = base64::prelude::BASE64_STANDARD
                            .decode(data)
                            .log_with_level(Level::Debug)
                    {
                        let image = Arc::new(Image::from_bytes(format, bytes));
                        images_by_source_offset.insert(range.start, image);
                    }
                }
            }

            let mut parsed = ParsedMarkdown {
                source,
                events: Arc::from(events),
                languages_by_name,
                languages_by_path,
                root_block_starts: Arc::from(root_block_starts),
                html_blocks,
                metadata_blocks,
                heading_slugs,
                footnote_definitions,
                link_definition_spans: Arc::from(link_definition_spans),
                fallback_code_block_language,
                code_block_highlights: Arc::default(),
            };
            parsed.code_block_highlights = Arc::new(compute_code_block_highlights(&parsed));
            (parsed, images_by_source_offset)
        });

        cx.spawn(async move |this, cx| {
            let (parsed, images_by_source_offset) = parsed.await;

            this.update(cx, |this, cx| {
                this.parsed_markdown = parsed;
                this.images_by_source_offset = images_by_source_offset;
                if this.active_root_block.is_some_and(|block_index| {
                    block_index >= this.parsed_markdown.root_block_starts.len()
                }) {
                    this.active_root_block = None;
                }
                this.pending_parse.take();
                if this.should_reparse {
                    this.parse(cx);
                } else if let Some(slug) = this.pending_heading_scroll.take()
                    && let Some(source_index) =
                        this.parsed_markdown.heading_slugs.get(&slug).copied()
                {
                    this.pending_autoscroll = None;
                    this.autoscroll_request = Some(source_index);
                } else if let Some(source_index) = this.pending_autoscroll.take() {
                    this.autoscroll_request = Some(source_index);
                }
                cx.notify();
                cx.refresh_windows();
            })
            .ok();
        })
    }
}

impl Focusable for Markdown {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[derive(Debug, Default, Clone)]
enum SelectMode {
    #[default]
    Character,
    Word(Range<usize>),
    Line(Range<usize>),
    All,
}

#[derive(Clone, Default)]
struct Selection {
    start: usize,
    end: usize,
    reversed: bool,
    pending: bool,
    mode: SelectMode,
}

impl Selection {
    fn set_head(&mut self, head: usize, rendered_text: &RenderedText) {
        match &self.mode {
            SelectMode::Character => {
                if head < self.tail() {
                    if !self.reversed {
                        self.end = self.start;
                        self.reversed = true;
                    }
                    self.start = head;
                } else {
                    if self.reversed {
                        self.start = self.end;
                        self.reversed = false;
                    }
                    self.end = head;
                }
            }
            SelectMode::Word(original_range) | SelectMode::Line(original_range) => {
                let head_range = if matches!(self.mode, SelectMode::Word(_)) {
                    rendered_text.surrounding_word_range(head)
                } else {
                    rendered_text.surrounding_line_range(head)
                };

                if head < original_range.start {
                    self.start = head_range.start;
                    self.end = original_range.end;
                    self.reversed = true;
                } else if head >= original_range.end {
                    self.start = original_range.start;
                    self.end = head_range.end;
                    self.reversed = false;
                } else {
                    self.start = original_range.start;
                    self.end = original_range.end;
                    self.reversed = false;
                }
            }
            SelectMode::All => {
                self.start = 0;
                self.end = rendered_text
                    .lines
                    .last()
                    .map(|line| line.source_end)
                    .unwrap_or(0);
                self.reversed = false;
            }
        }
    }

    fn tail(&self) -> usize {
        if self.reversed { self.end } else { self.start }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ParsedMarkdown {
    pub source: SharedString,
    pub events: Arc<[(Range<usize>, MarkdownEvent)]>,
    pub languages_by_name: TreeMap<SharedString, Arc<Language>>,
    pub languages_by_path: TreeMap<Arc<str>, Arc<Language>>,
    pub root_block_starts: Arc<[usize]>,
    pub(crate) html_blocks: BTreeMap<usize, html::html_parser::ParsedHtmlBlock>,
    pub(crate) metadata_blocks: BTreeMap<usize, ParsedMetadataBlock>,
    pub heading_slugs: HashMap<SharedString, usize>,
    pub footnote_definitions: HashMap<SharedString, usize>,
    pub(crate) link_definition_spans: Arc<[Range<usize>]>,
    pub(crate) fallback_code_block_language: Option<Arc<Language>>,
    pub(crate) code_block_highlights: Arc<CodeBlockHighlights>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct MarkdownEventIndex(usize);

pub(crate) type CodeBlockHighlights = HashMap<MarkdownEventIndex, ResolvedHighlights>;

impl ParsedMarkdown {
    pub fn source(&self) -> &SharedString {
        &self.source
    }

    pub fn events(&self) -> &Arc<[(Range<usize>, MarkdownEvent)]> {
        &self.events
    }

    pub fn root_block_starts(&self) -> &Arc<[usize]> {
        &self.root_block_starts
    }

    /// The fenced, closed code blocks, in source order.
    pub fn code_blocks(&self) -> impl Iterator<Item = CodeBlock<'_>> {
        self.events.iter().filter_map(|(range, event)| match event {
            MarkdownEvent::Start(MarkdownTag::CodeBlock { kind, metadata }) => {
                CodeBlock::new(range.start, kind, metadata, &self.source)
            }
            _ => None,
        })
    }

    fn non_rendered_source_ranges(&self) -> Vec<Range<usize>> {
        let mut ranges = Vec::new();
        let mut active_link = None;
        let mut active_image: Option<Range<usize>> = None;

        for (event_range, event) in self.events.iter() {
            // An image renders as an image, so nothing between its start and end is text on
            // screen - not the alt text, and not the destination. Skipping these events also
            // keeps the alt text from advancing the enclosing link's cursor, which would
            // otherwise carve it out of the link's non-rendered span.
            if let Some(image_range) = &active_image {
                if matches!(event, MarkdownEvent::End(MarkdownTagEnd::Image))
                    && event_range.end >= image_range.end
                {
                    active_image = None;
                }
                continue;
            }

            match event {
                MarkdownEvent::Start(MarkdownTag::Image { .. }) => {
                    active_image = Some(event_range.clone());
                    if active_link.is_none() {
                        ranges.push(event_range.clone());
                    }
                }
                MarkdownEvent::Start(MarkdownTag::Link { .. }) => {
                    active_link = Some((event_range.clone(), event_range.start));
                }
                MarkdownEvent::Text
                | MarkdownEvent::SubstitutedText(_)
                | MarkdownEvent::Code
                | MarkdownEvent::SubstitutedCode(_) => {
                    let Some((link_range, cursor)) = active_link.as_mut() else {
                        continue;
                    };
                    let visible_start = event_range.start.max(link_range.start);
                    let visible_end = event_range.end.min(link_range.end);
                    if visible_start > *cursor {
                        ranges.push(*cursor..visible_start);
                    }
                    *cursor = (*cursor).max(visible_end);
                }
                MarkdownEvent::End(MarkdownTagEnd::Link) => {
                    if let Some((link_range, cursor)) = active_link.take()
                        && cursor < link_range.end
                    {
                        ranges.push(cursor..link_range.end);
                    }
                }
                _ => {}
            }
        }

        ranges.extend(self.link_definition_spans.iter().cloned());
        // Callers binary search these ranges, which requires them to be sorted and disjoint.
        ranges.sort_by_key(|range| range.start);
        ranges.dedup_by(|next, previous| {
            if next.start <= previous.end {
                previous.end = previous.end.max(next.end);
                true
            } else {
                false
            }
        });
        ranges
    }

    pub fn root_block_for_source_index(&self, source_index: usize) -> Option<usize> {
        if self.root_block_starts.is_empty() {
            return None;
        }

        let partition = self
            .root_block_starts
            .partition_point(|block_start| *block_start <= source_index);

        Some(partition.saturating_sub(1))
    }

    /// Extracts the markdown source for a selection, rebalancing inline
    /// delimiters (`**`, backticks, link syntax, etc.) so partial selections of
    /// styled spans stay well-formed.
    ///
    /// With an exception of a single inline code span, which is returned as plain
    /// text, since copying a command or identifier is the dominant use case there.
    pub fn rebalanced_markdown_for_selection(&self, selection: Range<usize>) -> String {
        selection::rebalanced_markdown_for_selection(
            &self.source,
            &self.events,
            &self.root_block_starts,
            selection,
        )
    }

    pub(crate) fn code_block_language(&self, kind: &CodeBlockKind) -> Option<Arc<Language>> {
        match kind {
            CodeBlockKind::FencedLang(name) => self.languages_by_name.get(name).cloned(),
            CodeBlockKind::FencedSrc(path_range) => {
                self.languages_by_path.get(&path_range.path).cloned()
            }
            CodeBlockKind::Fenced => self.fallback_code_block_language.clone(),
            CodeBlockKind::Indented => None,
        }
    }
}

struct PendingCodeBlock<'a> {
    language: Arc<Language>,
    texts: Vec<(MarkdownEventIndex, &'a str)>,
}

fn compute_code_block_highlights(parsed: &ParsedMarkdown) -> CodeBlockHighlights {
    let mut code_block_highlights = CodeBlockHighlights::default();
    let mut pending_block: Option<PendingCodeBlock> = None;
    for (event_index, (range, event)) in parsed.events.iter().enumerate() {
        let event_index = MarkdownEventIndex(event_index);
        match event {
            MarkdownEvent::Start(MarkdownTag::CodeBlock { kind, .. }) => {
                pending_block = parsed
                    .code_block_language(kind)
                    .map(|language| PendingCodeBlock {
                        language,
                        texts: Vec::new(),
                    });
            }
            MarkdownEvent::End(MarkdownTagEnd::CodeBlock) => {
                if let Some(block) = pending_block.take() {
                    highlight_code_block(block, &mut code_block_highlights);
                }
            }
            MarkdownEvent::Text => {
                if let Some(block) = &mut pending_block {
                    block
                        .texts
                        .push((event_index, &parsed.source[range.clone()]));
                }
            }
            MarkdownEvent::SubstitutedText(text) => {
                if let Some(block) = &mut pending_block {
                    block.texts.push((event_index, text.as_str()));
                }
            }
            _ => {}
        }
    }
    code_block_highlights
}

fn highlight_code_block(block: PendingCodeBlock, code_block_highlights: &mut CodeBlockHighlights) {
    if let [(event_index, text)] = block.texts.as_slice() {
        let resolved = block.language.highlight_text_resolved(*text, 0..text.len());
        if !resolved.runs.is_empty() {
            code_block_highlights.insert(*event_index, resolved);
        }
        return;
    }
    let mut combined = String::new();
    let mut text_offsets = Vec::with_capacity(block.texts.len());
    for (_, text) in &block.texts {
        text_offsets.push(combined.len());
        combined.push_str(text);
    }
    let resolved = block
        .language
        .highlight_text_resolved(combined.as_str(), 0..combined.len());
    if resolved.runs.is_empty() {
        return;
    }
    let mut runs = resolved.runs.iter().peekable();
    for ((event_index, text), text_offset) in block.texts.iter().zip(text_offsets) {
        let text_end = text_offset + text.len();
        let mut text_runs = Vec::new();
        while let Some((run_range, highlight_id)) = runs.peek() {
            if run_range.start >= text_end {
                break;
            }
            let start = run_range.start.max(text_offset);
            let end = run_range.end.min(text_end);
            if end > start {
                text_runs.push((start - text_offset..end - text_offset, *highlight_id));
            }
            if run_range.end > text_end {
                break;
            }
            runs.next();
        }
        if !text_runs.is_empty() {
            code_block_highlights.insert(
                *event_index,
                ResolvedHighlights {
                    sources: resolved.sources.clone(),
                    runs: text_runs.into(),
                },
            );
        }
    }
}
