use super::*;

pub(crate) struct MetadataCellStyle {
    pub(crate) row_index: usize,
    pub(crate) is_key: bool,
}

/// Maximum number of nested containers. Deeper containers are folded into the
/// innermost one, so pathological input (e.g. thousands of `>`) cannot overflow
/// the stack during layout, prepaint and paint, which recurse per element.
const MAX_DIV_DEPTH: usize = 64;

pub(crate) struct MarkdownElementBuilder {
    div_stack: Vec<DivStackEntry>,
    /// Containers pushed past [`MAX_DIV_DEPTH`] that were folded away.
    folded_divs: usize,
    rendered_lines: Vec<Rc<RenderedLine>>,
    pending_line: PendingLine,
    pub(crate) rendered_links: Vec<RenderedLink>,
    rendered_image_links: Vec<RenderedImageLink>,
    rendered_footnote_refs: Vec<RenderedFootnoteRef>,
    current_source_index: usize,
    pub(crate) html_comment: bool,
    pub(crate) rendered_footnote_separator: bool,
    base_text_style: TextStyle,
    text_style_stack: Vec<TextStyleRefinement>,
    pub(crate) code_block_stack: Vec<Option<Arc<Language>>>,
    pub(crate) link_depth: usize,
    pub(crate) list_stack: Vec<ListStackEntry>,
    pub(crate) table: TableState,
    syntax_theme: Arc<SyntaxTheme>,
    highlights: MarkdownHighlights,
}

pub(crate) struct MarkdownHighlights {
    /// Search highlights, sorted by range start.
    pub(crate) search_highlights: Rc<[Range<usize>]>,
    pub(crate) active_search_highlight: Option<usize>,
    pub(crate) search_match_color: Hsla,
    pub(crate) active_search_match_color: Hsla,
    pub(crate) selection: Option<(Range<usize>, Hsla)>,
    /// Index of the first search highlight that may intersect the next line.
    pub(crate) next_search_highlight_ix: usize,
    pub(crate) base_background: Hsla,
    pub(crate) minimum_contrast: f32,
}

impl MarkdownHighlights {
    /// Returns the highlighted ranges intersecting the given source range,
    /// clamped to it, in paint order.
    fn highlights_for_line(&mut self, source_range: Range<usize>) -> SmallVec<[(Range<usize>, Hsla); 1]> {
        let mut highlights = SmallVec::new();

        self.next_search_highlight_ix += self.search_highlights[self.next_search_highlight_ix..]
            .iter()
            .take_while(|range| range.end <= source_range.start)
            .count();

        for (ix, range) in self.search_highlights.iter().enumerate().skip(self.next_search_highlight_ix) {
            if range.start >= source_range.end {
                break;
            }
            let clamped = range.start.max(source_range.start)..range.end.min(source_range.end);
            if clamped.start < clamped.end {
                let color = if Some(ix) == self.active_search_highlight {
                    self.active_search_match_color
                } else {
                    self.search_match_color
                };
                highlights.push((clamped, color));
            }
        }
        if let Some((range, color)) = &self.selection {
            let clamped = range.start.max(source_range.start)..range.end.min(source_range.end);
            if clamped.start < clamped.end {
                highlights.push((clamped, *color));
            }
        }
        highlights
    }
}

struct DivStackEntry {
    div: AnyDiv,
    line_break_mode: LineBreakMode,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum LineBreakMode {
    TextLayout,
    FlexWrap,
}

impl DivStackEntry {
    pub(crate) fn new(div: impl Into<AnyDiv>) -> Self {
        Self { div: div.into(), line_break_mode: LineBreakMode::TextLayout }
    }
}

#[derive(Default)]
struct PendingLine {
    text: String,
    runs: Vec<TextRun>,
    source_mappings: Vec<SourceMapping>,
    /// Rendered (not source) indices, so chips hug the glyphs and ignore
    /// unrendered characters like the surrounding backticks
    code_chips: Vec<(Range<usize>, Hsla)>,
}

pub(crate) struct ListStackEntry {
    bullet_index: Option<u64>,
}

impl MarkdownElementBuilder {
    pub(crate) fn new(
        container_style: &StyleRefinement,
        base_text_style: TextStyle,
        syntax_theme: Arc<SyntaxTheme>,
        highlights: MarkdownHighlights,
    ) -> Self {
        Self {
            div_stack: vec![{
                let mut base_div = div();
                base_div.style().refine(container_style);
                DivStackEntry::new(base_div)
            }],
            folded_divs: 0,
            rendered_lines: Vec::new(),
            pending_line: PendingLine::default(),
            rendered_links: Vec::new(),
            rendered_image_links: Vec::new(),
            rendered_footnote_refs: Vec::new(),
            current_source_index: 0,
            html_comment: false,
            rendered_footnote_separator: false,
            base_text_style,
            text_style_stack: Vec::new(),
            code_block_stack: Vec::new(),
            link_depth: 0,
            list_stack: Vec::new(),
            table: TableState::default(),
            syntax_theme,
            highlights,
        }
    }

    pub(crate) fn push_code_chip_text(
        &mut self,
        text: &str,
        source_range: Range<usize>,
        chip_background: Option<Hsla>,
    ) {
        let chip_start = self.pending_line.text.len();
        self.push_text(text, source_range);
        if let Some(background) = chip_background {
            let chip_end = self.pending_line.text.len();
            if chip_start < chip_end {
                self.pending_line.code_chips.push((chip_start..chip_end, background));
            }
        }
    }

    pub(crate) fn push_text_style(&mut self, style: TextStyleRefinement) {
        self.text_style_stack.push(style);
    }

    pub(crate) fn text_style(&self) -> TextStyle {
        let mut style = self.base_text_style.clone();
        for refinement in &self.text_style_stack {
            style.refine(refinement);
        }
        style
    }

    pub(crate) fn pop_text_style(&mut self) {
        self.text_style_stack.pop();
    }

    pub(crate) fn push_div(&mut self, div: impl Into<AnyDiv>, range: &Range<usize>, markdown_end: usize) {
        self.flush_text();
        if self.div_stack.len() >= MAX_DIV_DEPTH {
            self.folded_divs += 1;
            return;
        }
        let mut div = div.into();

        if range.start == 0 {
            // Remove the top margin on the first element.
            div.style().refine(&StyleRefinement {
                margin: gpui::EdgesRefinement {
                    top: Some(Length::Definite(px(0.).into())),
                    left: None,
                    right: None,
                    bottom: None,
                },
                ..Default::default()
            });
        }

        if range.end == markdown_end {
            div.style().refine(&StyleRefinement {
                margin: gpui::EdgesRefinement {
                    top: None,
                    left: None,
                    right: None,
                    bottom: Some(Length::Definite(rems(0.).into())),
                },
                ..Default::default()
            });
        }

        self.div_stack.push(DivStackEntry::new(div));
    }

    pub(crate) fn push_root_block(&mut self, range: &Range<usize>, markdown_end: usize) {
        self.push_div(div().group("markdown-root-block").relative(), range, markdown_end);
        self.push_div(div().pl_4(), range, markdown_end);
    }

    pub(crate) fn push_image_child(&mut self, child: impl IntoElement) {
        let text_align = self.text_style().text_align;
        let table_cell_alignment = self.table.in_cell.then(|| self.table.current_cell_alignment());
        self.modify_current_div(|el| {
            let el = el.flex().flex_row().flex_wrap();
            // Table cells center their content vertically and apply column alignment via a
            // column-direction container. Switching it to a row moves those axes, so the
            // alignment has to be restated for the row.
            match table_cell_alignment {
                Some(alignment) => {
                    let el = el.items_center().content_center();
                    match alignment {
                        Some(Alignment::Center) => el.justify_center(),
                        Some(Alignment::Right) => el.justify_end(),
                        _ => el.justify_start(),
                    }
                }
                None => {
                    let el = el.items_start();
                    match text_align {
                        TextAlign::Left => el.justify_start(),
                        TextAlign::Center => el.justify_center(),
                        TextAlign::Right => el.justify_end(),
                    }
                }
            }
        });
        self.div_stack.last_mut().unwrap().line_break_mode = LineBreakMode::FlexWrap;
        self.append_child(child.into_any_element());
    }

    pub(crate) fn push_line_break(&mut self, source_range: Range<usize>) {
        if self.uses_flex_line_breaks() {
            self.modify_current_div(|el| el.child(div().w_full().h_0()));
        } else {
            self.push_text("\n", source_range);
        }
    }

    pub(crate) fn push_soft_break(&mut self, source_range: Range<usize>) {
        // A soft break right after an item in flex wrap container would otherwise
        // render as a stray leading space before the next wrapped item.
        if self.uses_flex_line_breaks() && self.pending_line.text.is_empty() {
            return;
        }
        self.push_text(" ", source_range);
    }

    fn append_child(&mut self, child: AnyElement) {
        self.div_stack.last_mut().unwrap().div.extend([child]);
    }

    fn uses_flex_line_breaks(&self) -> bool {
        self.div_stack.last().is_some_and(|entry| entry.line_break_mode == LineBreakMode::FlexWrap)
    }

    pub(crate) fn modify_current_div(&mut self, f: impl FnOnce(AnyDiv) -> AnyDiv) {
        self.flush_text();
        if let Some(mut entry) = self.div_stack.pop() {
            entry.div = f(entry.div);
            self.div_stack.push(entry);
        }
    }

    pub(crate) fn pop_root_block(&mut self, is_active: bool, active_gutter_color: Hsla, hovered_gutter_color: Hsla) {
        self.pop_div();
        self.modify_current_div(|el| {
            el.child(
                div()
                    .h_full()
                    .w(px(4.0))
                    .when(is_active, |this| this.bg(active_gutter_color))
                    .group_hover(
                        "markdown-root-block",
                        |this| {
                            if is_active { this } else { this.bg(hovered_gutter_color) }
                        },
                    )
                    .rounded_xs()
                    .absolute()
                    .left_0()
                    .top_0(),
            )
        });
        self.pop_div();
    }

    pub(crate) fn pop_div(&mut self) {
        self.flush_text();
        if self.folded_divs > 0 {
            self.folded_divs -= 1;
            return;
        }
        let div = self.div_stack.pop().unwrap().div.into_any_element();
        self.append_child(div);
    }

    pub(crate) fn push_sourced_element(&mut self, source_range: Range<usize>, element: impl Into<AnyElement>) {
        self.flush_text();
        let anchor = self.render_source_anchor(source_range);
        self.append_child(div().relative().child(anchor).child(element.into()).into_any_element());
    }

    pub(crate) fn push_list(&mut self, bullet_index: Option<u64>) {
        self.list_stack.push(ListStackEntry { bullet_index });
    }

    pub(crate) fn next_bullet_index(&mut self) -> Option<u64> {
        self.list_stack.last_mut().and_then(|entry| {
            let item_index = entry.bullet_index.as_mut()?;
            *item_index += 1;
            Some(*item_index - 1)
        })
    }

    pub(crate) fn pop_list(&mut self) {
        self.list_stack.pop();
    }

    pub(crate) fn push_code_block(&mut self, language: Option<Arc<Language>>) {
        self.code_block_stack.push(language);
    }

    pub(crate) fn pop_code_block(&mut self) {
        self.code_block_stack.pop();
    }

    pub(crate) fn push_link(&mut self, destination_url: SharedString, source_range: Range<usize>) {
        self.rendered_links.push(RenderedLink { source_range, destination_url });
    }

    pub(crate) fn push_image_link(&mut self, destination_url: SharedString, bounds: Rc<Cell<Option<Bounds<Pixels>>>>) {
        self.rendered_image_links.push(RenderedImageLink { bounds, destination_url });
    }

    pub(crate) fn push_footnote_ref(&mut self, label: SharedString, source_range: Range<usize>) {
        self.rendered_footnote_refs.push(RenderedFootnoteRef { source_range, label });
    }

    pub(crate) fn push_text(&mut self, text: &str, source_range: Range<usize>) {
        self.push_highlighted_text(text, source_range, None);
    }

    pub(crate) fn push_highlighted_text(
        &mut self,
        text: &str,
        source_range: Range<usize>,
        highlights: Option<&ResolvedHighlights>,
    ) {
        self.pending_line
            .source_mappings
            .push(SourceMapping { rendered_index: self.pending_line.text.len(), source_index: source_range.start });
        self.pending_line.text.push_str(text);
        self.current_source_index = source_range.end;

        // Compute the base text style once
        let text_style = self.text_style();

        if let Some(language) = self.code_block_stack.last().and_then(Option::as_ref)
            && let Some(resolved) = highlights
        {
            let runs = if resolved.is_current() {
                resolved.runs.clone()
            } else {
                language.highlight_text_resolved(text, 0..text.len()).runs
            };
            let mut offset = 0;
            for (run_range, highlight_id) in runs.iter() {
                if run_range.start > offset {
                    self.pending_line.runs.push(text_style.to_run(run_range.start - offset));
                }

                let run_len = run_range.len();
                let highlight = self.syntax_theme.get(*highlight_id).copied();
                if let Some(highlight) = highlight {
                    self.pending_line.runs.push(text_style.clone().highlight(highlight).to_run(run_len));
                } else {
                    self.pending_line.runs.push(text_style.to_run(run_len));
                }
                offset = run_range.end;
            }

            if offset < text.len() {
                self.pending_line.runs.push(text_style.to_run(text.len() - offset));
            }
        } else {
            self.pending_line.runs.push(text_style.to_run(text.len()));
        }
    }

    pub(crate) fn trim_trailing_newline(&mut self) {
        if self.pending_line.text.ends_with('\n') {
            self.pending_line.text.truncate(self.pending_line.text.len() - 1);
            self.pending_line.runs.last_mut().unwrap().len -= 1;
            self.current_source_index -= 1;
        }
    }

    pub(crate) fn replace_pending_checkbox(&mut self, on_toggle: Option<CheckboxToggleCallback>) {
        let text = &self.pending_line.text;
        let trimmed = text.trim();
        if trimmed != "[x]" && trimmed != "[X]" && trimmed != "[ ]" {
            return;
        }
        let checked = trimmed != "[ ]";

        let leading_ws = text.len() - text.trim_start().len();
        let marker_rendered = leading_ws..leading_ws + trimmed.len();
        let marker_source =
            self.source_range_for_rendered(&marker_rendered).expect("pending checkbox text must have source mappings");

        self.pending_line = PendingLine::default();

        let toggle_state = if checked { ToggleState::Selected } else { ToggleState::Unselected };
        let checkbox = Checkbox::new(
            ElementId::Name(format!("table_checkbox_{}_{}", marker_source.start, marker_source.end).into()),
            toggle_state,
        )
        .fill();

        let checkbox = if let Some(on_toggle) = on_toggle {
            checkbox
                .on_click(move |_state, window, cx| {
                    on_toggle(marker_source.clone(), !checked, window, cx);
                })
                .into_any_element()
        } else {
            checkbox.visualization_only(true).into_any_element()
        };

        let mut checkbox_container = h_flex().w_full();
        checkbox_container = match self.text_style().text_align {
            TextAlign::Left => checkbox_container.justify_start(),
            TextAlign::Center => checkbox_container.justify_center(),
            TextAlign::Right => checkbox_container.justify_end(),
        };

        self.append_child(checkbox_container.child(checkbox).into_any_element());
    }

    fn source_range_for_rendered(&self, rendered: &Range<usize>) -> Option<Range<usize>> {
        source_range_for_rendered(&self.pending_line.source_mappings, rendered)
    }

    fn render_source_anchor(&mut self, source_range: Range<usize>) -> AnyElement {
        let mut text_style = self.base_text_style.clone();
        text_style.color = Hsla::transparent_black();
        let text = "\u{200B}";
        let styled_text = StyledText::new(text).with_runs(vec![text_style.to_run(text.len())]);
        self.rendered_lines.push(Rc::new(RenderedLine {
            layout: styled_text.layout().clone(),
            visible_bounds: Cell::new(None),
            source_mappings: vec![SourceMapping { rendered_index: 0, source_index: source_range.start }],
            source_end: source_range.end,
            language: None,
            text_align: TextAlign::Left,
            highlights: SmallVec::new(),
            code_chips: SmallVec::new(),
        }));
        div().absolute().top_0().left_0().opacity(0.).child(styled_text).into_any_element()
    }

    fn flush_text(&mut self) {
        let text_align = self.text_style().text_align;
        let line = mem::take(&mut self.pending_line);
        if line.text.is_empty() {
            return;
        }

        let highlights = line
            .source_mappings
            .first()
            .map(|first_mapping| {
                self.highlights.highlights_for_line(first_mapping.source_index..self.current_source_index)
            })
            .unwrap_or_default();
        let runs = if highlights.is_empty()
            || self.highlights.minimum_contrast <= 0.
            || !self.highlights.base_background.is_opaque()
        {
            line.runs
        } else {
            let to_rendered = |source_index| {
                rendered_index_for_source_index(
                    &line.source_mappings,
                    self.current_source_index,
                    line.text.len(),
                    source_index,
                )
            };
            let backgrounds = merge_highlight_backgrounds(
                highlights.iter().map(|(range, color)| (to_rendered(range.start)..to_rendered(range.end), *color)),
                self.highlights.base_background,
            );
            ensure_highlight_contrast(&line.runs, &backgrounds, self.highlights.minimum_contrast, 0)
        };
        let text = StyledText::new(line.text).with_runs(runs);
        let rendered_line = Rc::new(RenderedLine {
            layout: text.layout().clone(),
            visible_bounds: Cell::new(None),
            source_mappings: line.source_mappings,
            source_end: self.current_source_index,
            language: self.code_block_stack.last().and_then(|entry| entry.as_ref()).cloned(),
            text_align,
            highlights,
            code_chips: line.code_chips.into_iter().collect(),
        });
        self.rendered_lines.push(rendered_line.clone());
        self.append_child(RenderedLineElement { text, line: rendered_line }.into_any_element());
    }

    pub(crate) fn build(mut self) -> RenderedMarkdown {
        debug_assert_eq!(self.div_stack.len(), 1);
        self.flush_text();
        RenderedMarkdown {
            element: self.div_stack.pop().unwrap().div.into_any_element(),
            text: RenderedText {
                lines: self.rendered_lines.into(),
                links: self.rendered_links.into(),
                image_links: self.rendered_image_links.into(),
                footnote_refs: self.rendered_footnote_refs.into(),
                document_end: None,
            },
        }
    }
}
