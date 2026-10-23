use super::*;

/// Wraps a rendered line so its code chips and highlights share the glyphs'
/// ancestor content masks, and records the clipped bounds for platform text hit testing.
pub(crate) struct RenderedLineElement {
    pub(crate) text: StyledText,
    pub(crate) line: Rc<RenderedLine>,
}

impl Element for RenderedLineElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        self.text.request_layout(None, window, cx)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.line.visible_bounds.set(Some(bounds.intersect(&window.content_mask().bounds)));
        self.text.prepaint(None, bounds, request_layout, window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let layout = self.text.layout();
        self.line.paint_code_chips(window);
        layout.paint_background(window, cx).log_err();
        self.line.paint_highlights(window);
        layout.paint_foreground(window, cx).log_err();
    }
}

impl IntoElement for RenderedLineElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

pub(crate) struct RenderedLine {
    pub(crate) layout: TextLayout,
    pub(crate) visible_bounds: Cell<Option<Bounds<Pixels>>>,
    pub(crate) source_mappings: Vec<SourceMapping>,
    pub(crate) source_end: usize,
    pub(crate) language: Option<Arc<Language>>,
    pub(crate) text_align: TextAlign,
    /// Highlighted source ranges intersecting this line, in paint order
    pub(crate) highlights: SmallVec<[(Range<usize>, Hsla); 1]>,
    /// Inline code chip ranges intersecting this line, in rendered indices
    pub(crate) code_chips: SmallVec<[(Range<usize>, Hsla); 1]>,
}

impl RenderedLine {
    /// Painted before the glyphs so the text renders on top of the chips
    fn paint_code_chips(&self, window: &mut Window) {
        const CHIP_CORNER_RADIUS: Pixels = px(4.);

        if self.code_chips.is_empty() {
            return;
        }
        let wrapped_line_segments = self.wrapped_line_segments();
        if wrapped_line_segments.is_empty() {
            return;
        }

        for (rendered_range, color) in &self.code_chips {
            self.for_each_bounds_in_rendered_range(&wrapped_line_segments, rendered_range.clone(), |bounds| {
                // Kept to a hair since the layout reserves no padding and
                // anything wider eats the gap to neighboring words
                let horizontal_outset = px(1.);
                // Inset vertically so the chip hugs the glyphs like a badge
                // instead of filling the whole line box
                let vertical_inset = bounds.size.height * 0.1;
                let chip_bounds = Bounds {
                    origin: point(bounds.origin.x - horizontal_outset, bounds.origin.y + vertical_inset),
                    size: size(bounds.size.width + horizontal_outset * 2., bounds.size.height - vertical_inset * 2.),
                };
                window.paint_quad(quad(
                    chip_bounds,
                    CHIP_CORNER_RADIUS,
                    *color,
                    Edges::default(),
                    Hsla::transparent_black(),
                    BorderStyle::default(),
                ));
            });
        }
    }

    /// Painted between the text run backgrounds and the glyphs, so opaque highlight
    /// colors neither hide the text nor get hidden by run backgrounds
    fn paint_highlights(&self, window: &mut Window) {
        if self.highlights.is_empty() {
            return;
        }
        let wrapped_line_segments = self.wrapped_line_segments();
        if wrapped_line_segments.is_empty() {
            return;
        }

        for (source_range, color) in &self.highlights {
            self.for_each_bounds_in_source_range(&wrapped_line_segments, source_range.clone(), |bounds| {
                window.paint_quad(quad(
                    bounds,
                    Pixels::ZERO,
                    *color,
                    Edges::default(),
                    Hsla::transparent_black(),
                    BorderStyle::default(),
                ));
            });
        }
    }

    fn wrapped_line_segments(&self) -> SmallVec<[WrappedLineSegment; 1]> {
        let layout = &self.layout;
        let line_layouts = layout.line_layouts();
        let line_height = layout.line_height();
        let mut row_top = layout.bounds().top();
        let mut wrapped_line_start = 0;
        let mut segments = SmallVec::with_capacity(line_layouts.len());

        for wrapped_line in line_layouts {
            let wrapped_line_end = wrapped_line_start + wrapped_line.len();
            let wrapped_line_height = wrapped_line.size(line_height).height;
            segments.push(WrappedLineSegment {
                start: wrapped_line_start,
                end: wrapped_line_end,
                row_top,
                layout: wrapped_line,
            });
            row_top += wrapped_line_height;
            wrapped_line_start = wrapped_line_end + 1;
        }

        segments
    }

    fn for_each_bounds_in_source_range(
        &self,
        wrapped_line_segments: &[WrappedLineSegment],
        range: Range<usize>,
        f: impl FnMut(Bounds<Pixels>),
    ) {
        if range.start >= range.end {
            return;
        }

        let rendered_start = self.rendered_index_for_source_index(range.start);
        let rendered_end = self.rendered_index_for_source_index(range.end);
        self.for_each_bounds_in_rendered_range(wrapped_line_segments, rendered_start..rendered_end, f);
    }

    fn for_each_bounds_in_rendered_range(
        &self,
        wrapped_line_segments: &[WrappedLineSegment],
        rendered_range: Range<usize>,
        mut f: impl FnMut(Bounds<Pixels>),
    ) {
        let layout = &self.layout;
        let line_bounds = layout.bounds();
        let line_height = layout.line_height();

        let rendered_start = rendered_range.start;
        let rendered_end = rendered_range.end;

        for wrapped_line_segment in wrapped_line_segments {
            if wrapped_line_segment.start >= rendered_end {
                break;
            }
            if wrapped_line_segment.end <= rendered_start {
                continue;
            }

            let wrapped_line = &wrapped_line_segment.layout;
            let unwrapped_layout = &wrapped_line.unwrapped_layout;
            let wrapped_line_start = wrapped_line_segment.start;
            let wrapped_line_end = wrapped_line_segment.end;
            let mut row_top = wrapped_line_segment.row_top;

            let row_ends = wrapped_line
                .wrap_boundaries()
                .iter()
                .map(|wrap_boundary| {
                    let glyph = &unwrapped_layout.runs[wrap_boundary.run_ix].glyphs[wrap_boundary.glyph_ix];
                    (wrapped_line_start + glyph.index, glyph.position.x)
                })
                .chain([(wrapped_line_end, unwrapped_layout.width)]);

            let mut row_start = wrapped_line_start;
            let mut row_start_x = Pixels::ZERO;

            for (row_end, row_end_x) in row_ends {
                let selection_start = rendered_start.max(row_start);
                let selection_end = rendered_end.min(row_end);

                if selection_start < selection_end {
                    let alignment_offset =
                        self.alignment_offset_for_segment(line_bounds.size.width, row_start_x, row_end_x);
                    let x_for_index = |index| {
                        line_bounds.left() + alignment_offset + unwrapped_layout.x_for_index(index - wrapped_line_start)
                            - row_start_x
                    };
                    f(Bounds::from_corners(
                        point(x_for_index(selection_start), row_top),
                        point(x_for_index(selection_end), row_top + line_height),
                    ));
                }

                row_start = row_end;
                row_start_x = row_end_x;
                row_top += line_height;
            }
        }
    }

    fn rendered_index_for_source_index(&self, source_index: usize) -> usize {
        rendered_index_for_source_index(&self.source_mappings, self.source_end, self.layout.len(), source_index)
    }

    fn source_index_for_rendered_index(&self, rendered_index: usize) -> usize {
        if rendered_index >= self.layout.len() {
            return self.source_end;
        }

        let mapping = match self.source_mappings.binary_search_by_key(&rendered_index, |probe| probe.rendered_index) {
            Ok(ix) => &self.source_mappings[ix],
            Err(ix) => &self.source_mappings[ix - 1],
        };
        mapping.source_index + (rendered_index - mapping.rendered_index)
    }

    /// Returns the source index for use as an exclusive range end at a word/selection boundary.
    /// When the rendered index is exactly at the start of a segment with a gap from the previous
    /// segment (e.g., after stripped markdown syntax like backticks), this returns the end of the
    /// previous segment rather than the start of the current one.
    fn source_index_for_exclusive_rendered_end(&self, rendered_index: usize) -> usize {
        if rendered_index >= self.layout.len() {
            return self.source_end;
        }

        let ix = match self.source_mappings.binary_search_by_key(&rendered_index, |probe| probe.rendered_index) {
            Ok(ix) => ix,
            Err(ix) => {
                return self.source_mappings[ix - 1].source_index
                    + (rendered_index - self.source_mappings[ix - 1].rendered_index);
            }
        };

        // Exact match at the start of a segment. Check if there's a gap from the previous segment.
        if ix > 0 {
            let prev_mapping = &self.source_mappings[ix - 1];
            let mapping = &self.source_mappings[ix];
            let prev_segment_len = mapping.rendered_index - prev_mapping.rendered_index;
            let prev_source_end = prev_mapping.source_index + prev_segment_len;
            if prev_source_end < mapping.source_index {
                return prev_source_end;
            }
        }

        self.source_mappings[ix].source_index
    }

    fn alignment_offset_for_segment(
        &self,
        available_width: Pixels,
        segment_start_x: Pixels,
        segment_end_x: Pixels,
    ) -> Pixels {
        let segment_width = segment_end_x - segment_start_x;
        match self.text_align {
            TextAlign::Left => px(0.),
            TextAlign::Center => ((available_width - segment_width) / 2.).max(px(0.)),
            TextAlign::Right => (available_width - segment_width).max(px(0.)),
        }
    }

    pub(crate) fn source_index_for_position(&self, position: Point<Pixels>) -> Result<usize, usize> {
        let adjusted_position = maybe!({
            if self.text_align == TextAlign::Left {
                return None;
            }

            let wrapped_line = self.layout.line_layout_for_index(0)?;

            let bounds = self.layout.bounds();
            let line_height = self.layout.line_height();
            let relative_y = (position.y - bounds.top()).max(px(0.));
            let wrapped_row_ix = (relative_y / line_height) as usize;
            let boundaries = wrapped_line.wrap_boundaries();

            let segment_start_x = if wrapped_row_ix == 0 {
                px(0.)
            } else {
                boundaries
                    .get(wrapped_row_ix - 1)
                    .map(|b| wrapped_line.unwrapped_layout.runs[b.run_ix].glyphs[b.glyph_ix].position.x)
                    .unwrap_or(px(0.))
            };
            let segment_end_x = boundaries
                .get(wrapped_row_ix)
                .map(|b| wrapped_line.unwrapped_layout.runs[b.run_ix].glyphs[b.glyph_ix].position.x)
                .unwrap_or(wrapped_line.unwrapped_layout.width);

            let alignment_offset = self.alignment_offset_for_segment(bounds.size.width, segment_start_x, segment_end_x);
            Some(point(position.x - alignment_offset, position.y))
        })
        .unwrap_or(position);

        let (line_rendered_index, out_of_bounds) = match self.layout.index_for_position(adjusted_position) {
            Ok(ix) => (ix, false),
            Err(ix) => (ix, true),
        };
        let source_index = self.source_index_for_rendered_index(line_rendered_index);
        if out_of_bounds { Err(source_index) } else { Ok(source_index) }
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct SourceMapping {
    pub(crate) rendered_index: usize,
    pub(crate) source_index: usize,
}

pub(crate) fn rendered_index_for_source_index(
    mappings: &[SourceMapping],
    source_end: usize,
    rendered_len: usize,
    source_index: usize,
) -> usize {
    if source_index >= source_end {
        return rendered_len;
    }

    let mapping = match mappings.binary_search_by_key(&source_index, |probe| probe.source_index) {
        Ok(ix) => &mappings[ix],
        Err(ix) => &mappings[ix - 1],
    };
    (mapping.rendered_index + (source_index - mapping.source_index)).min(rendered_len)
}

pub(crate) fn source_range_for_rendered(mappings: &[SourceMapping], rendered: &Range<usize>) -> Option<Range<usize>> {
    if rendered.start >= rendered.end {
        return None;
    }
    let start = source_index_for_rendered(mappings, rendered.start)?;
    let end = source_index_for_rendered(mappings, rendered.end - 1)? + 1;
    Some(start..end)
}

fn source_index_for_rendered(mappings: &[SourceMapping], rendered_index: usize) -> Option<usize> {
    let mut last: Option<&SourceMapping> = None;
    for mapping in mappings {
        if mapping.rendered_index <= rendered_index {
            last = Some(mapping);
        } else {
            break;
        }
    }
    last.map(|m| m.source_index + (rendered_index - m.rendered_index))
}

pub(crate) struct RenderedMarkdown {
    pub(crate) element: AnyElement,
    pub(crate) text: RenderedText,
}

#[derive(Clone, Default)]
pub(crate) struct RenderedText {
    pub(crate) lines: Rc<[Rc<RenderedLine>]>,
    pub(crate) links: Rc<[RenderedLink]>,
    pub(crate) image_links: Rc<[RenderedImageLink]>,
    pub(crate) footnote_refs: Rc<[RenderedFootnoteRef]>,
    /// Where selecting all ends, when the lines don't cover the whole document.
    pub(crate) document_end: Option<usize>,
}

struct WrappedLineSegment {
    start: usize,
    end: usize,
    row_top: Pixels,
    layout: Arc<WrappedLineLayout>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct RenderedLink {
    pub(crate) source_range: Range<usize>,
    pub(crate) destination_url: SharedString,
}

#[derive(Clone)]
pub(crate) struct RenderedImageLink {
    // Populated once the image's `canvas` overlay is painted; images aren't part of the
    // text layout, so their hit-test region can't be derived from a source range.
    pub(crate) bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub(crate) destination_url: SharedString,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct RenderedFootnoteRef {
    pub(crate) source_range: Range<usize>,
    pub(crate) label: SharedString,
}

impl RenderedText {
    /// Joins the texts of consecutive blocks, which cover only part of a
    /// document ending at `document_end`.
    pub(crate) fn concat(texts: Vec<RenderedText>, document_end: usize) -> Self {
        Self {
            lines: texts.iter().flat_map(|text| text.lines.iter().cloned()).collect(),
            links: texts.iter().flat_map(|text| text.links.iter().cloned()).collect(),
            image_links: texts.iter().flat_map(|text| text.image_links.iter().cloned()).collect(),
            footnote_refs: texts.iter().flat_map(|text| text.footnote_refs.iter().cloned()).collect(),
            document_end: Some(document_end),
        }
    }

    pub(crate) fn select_all_end(&self) -> usize {
        self.document_end.or_else(|| self.lines.last().map(|line| line.source_end)).unwrap_or(0)
    }

    pub(crate) fn bounds_for_source_range(&self, range: Range<usize>) -> Vec<Bounds<Pixels>> {
        let mut all_bounds = Vec::new();
        for line in self.lines.iter() {
            let Some(first_mapping) = line.source_mappings.first() else {
                continue;
            };
            let line_source_start = first_mapping.source_index;
            if range.end <= line_source_start {
                break;
            }
            if range.start >= line.source_end {
                continue;
            }
            let wrapped_line_segments = line.wrapped_line_segments();
            line.for_each_bounds_in_source_range(
                &wrapped_line_segments,
                range.start.max(line_source_start)..range.end.min(line.source_end),
                |bounds| all_bounds.push(bounds),
            );
        }
        all_bounds
    }

    pub(crate) fn utf16_index_for_source_index(&self, source_index: usize) -> usize {
        self.text_for_range(0..source_index).encode_utf16().count()
    }

    pub(crate) fn utf16_range_for_source_range(&self, range: Range<usize>) -> Range<usize> {
        self.utf16_index_for_source_index(range.start)..self.utf16_index_for_source_index(range.end)
    }

    fn source_index_for_utf16_index(&self, utf16_index: usize) -> usize {
        let mut line_start_utf16 = 0;
        let mut last_source_end = 0;

        for (line_index, line) in self.lines.iter().enumerate() {
            let line_text = line.layout.text();
            let line_utf16_len = line_text.encode_utf16().count();
            let line_end_utf16 = line_start_utf16 + line_utf16_len;
            last_source_end = line.source_end;

            if utf16_index <= line_end_utf16 {
                let local_utf16_index = utf16_index.saturating_sub(line_start_utf16);
                let rendered_text = Rope::from(line_text.as_str());
                let rendered_index = rendered_text.offset_utf16_to_offset(
                    rendered_text.clip_offset_utf16(OffsetUtf16(local_utf16_index), Bias::Left),
                );
                return line.source_index_for_rendered_index(rendered_index);
            }

            line_start_utf16 = line_end_utf16;
            if line_index + 1 < self.lines.len() {
                line_start_utf16 += 1;
            }
        }

        last_source_end
    }

    pub(crate) fn source_range_for_utf16_range(&self, range_utf16: Range<usize>) -> Range<usize> {
        self.source_index_for_utf16_index(range_utf16.start)..self.source_index_for_utf16_index(range_utf16.end)
    }

    pub(crate) fn text_for_utf16_range(
        &self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
    ) -> String {
        let mut rendered_text = String::new();
        for (line_index, line) in self.lines.iter().enumerate() {
            if line_index > 0 {
                rendered_text.push('\n');
            }
            rendered_text.push_str(&line.layout.text());
        }

        let rendered_text = Rope::from(rendered_text);
        let start = rendered_text.clip_offset_utf16(OffsetUtf16(range_utf16.start), Bias::Left);
        let end = rendered_text.clip_offset_utf16(OffsetUtf16(range_utf16.end), Bias::Right);

        if (start.0..end.0) != range_utf16 {
            adjusted_range.replace(start.0..end.0);
        }

        rendered_text
            .chunks_in_range(rendered_text.offset_utf16_to_offset(start)..rendered_text.offset_utf16_to_offset(end))
            .collect()
    }

    pub(crate) fn source_index_for_position(&self, position: Point<Pixels>) -> Result<usize, usize> {
        let mut lines = self.lines.iter().peekable();
        let mut fallback_line: Option<&Rc<RenderedLine>> = None;
        let mut gap_source_index: Option<usize> = None;

        while let Some(line) = lines.next() {
            let line_bounds = line.layout.bounds();

            // Exact match: position is within bounds (handles overlapping bounds like table columns)
            if line_bounds.contains(&position) {
                return line.source_index_for_position(position);
            }

            // Track fallback for Y-coordinate based matching
            if position.y <= line_bounds.bottom() && fallback_line.is_none() {
                fallback_line = Some(line);
            }

            // Handle gap between lines
            if position.y > line_bounds.bottom()
                && let Some(next_line) = lines.peek()
                && position.y < next_line.layout.bounds().top()
                && gap_source_index.is_none()
            {
                // A later table cell may contain the position, so defer the gap fallback.
                gap_source_index = Some(line.source_end);
            }
        }

        if let Some(source_index) = gap_source_index {
            return Err(source_index);
        }

        // Fall back to Y-coordinate matched line
        if let Some(line) = fallback_line {
            return line.source_index_for_position(position);
        }

        Err(self.lines.last().map_or(0, |line| line.source_end))
    }

    pub(crate) fn source_index_for_visible_position(&self, position: Point<Pixels>) -> Option<usize> {
        self.lines.iter().find_map(|line| {
            if line.visible_bounds.get()?.contains(&position) {
                line.source_index_for_position(position).ok()
            } else {
                None
            }
        })
    }

    pub(crate) fn position_for_source_index(&self, source_index: usize) -> Option<(Point<Pixels>, Pixels)> {
        for line in self.lines.iter() {
            if source_index > line.source_end {
                continue;
            }
            let line_source_start = line.source_mappings.first().unwrap().source_index;
            let source_index = source_index.max(line_source_start);
            let line_height = line.layout.line_height();
            let rendered_index_within_line = line.rendered_index_for_source_index(source_index);
            let position = line.layout.position_for_index(rendered_index_within_line)?;
            return Some((position, line_height));
        }
        None
    }

    pub(crate) fn surrounding_word_range(&self, source_index: usize) -> Range<usize> {
        for line in self.lines.iter() {
            if source_index > line.source_end {
                continue;
            }

            let line_rendered_start = line.source_mappings.first().unwrap().rendered_index;
            let rendered_index_in_line = line.rendered_index_for_source_index(source_index) - line_rendered_start;
            let text = line.layout.text();

            let scope = line.language.as_ref().map(|l| l.default_scope());
            let classifier = CharClassifier::new(scope);

            let mut prev_chars = text[..rendered_index_in_line].chars().rev().peekable();
            let mut next_chars = text[rendered_index_in_line..].chars().peekable();

            let word_kind = std::cmp::max(
                prev_chars.peek().map(|&c| classifier.kind(c)),
                next_chars.peek().map(|&c| classifier.kind(c)),
            );

            let mut start = rendered_index_in_line;
            for c in prev_chars {
                if Some(classifier.kind(c)) == word_kind {
                    start -= c.len_utf8();
                } else {
                    break;
                }
            }

            let mut end = rendered_index_in_line;
            for c in next_chars {
                if Some(classifier.kind(c)) == word_kind {
                    end += c.len_utf8();
                } else {
                    break;
                }
            }

            return line.source_index_for_rendered_index(line_rendered_start + start)
                ..line.source_index_for_exclusive_rendered_end(line_rendered_start + end);
        }

        source_index..source_index
    }

    pub(crate) fn surrounding_line_range(&self, source_index: usize) -> Range<usize> {
        for line in self.lines.iter() {
            if source_index > line.source_end {
                continue;
            }
            let line_source_start = line.source_mappings.first().unwrap().source_index;
            return line_source_start..line.source_end;
        }

        source_index..source_index
    }

    pub(crate) fn text_for_range(&self, range: Range<usize>) -> String {
        let mut accumulator = String::new();

        for line in self.lines.iter() {
            if range.start > line.source_end {
                continue;
            }
            let line_source_start = line.source_mappings.first().unwrap().source_index;
            if range.end < line_source_start {
                break;
            }

            let text = line.layout.text();

            let start =
                if range.start < line_source_start { 0 } else { line.rendered_index_for_source_index(range.start) };
            let end = if range.end > line.source_end {
                line.rendered_index_for_source_index(line.source_end)
            } else {
                line.rendered_index_for_source_index(range.end)
            }
            .min(text.len());

            accumulator.push_str(&text[start..end]);
            accumulator.push('\n');
        }
        // Remove trailing newline
        accumulator.pop();
        accumulator
    }

    pub(crate) fn link_for_source_index(&self, source_index: usize) -> Option<&RenderedLink> {
        self.links.iter().find(|link| link.source_range.contains(&source_index))
    }

    pub(crate) fn image_link_for_position(&self, position: Point<Pixels>) -> Option<&RenderedImageLink> {
        self.image_links.iter().find(|image| image.bounds.get().is_some_and(|bounds| bounds.contains(&position)))
    }

    pub(crate) fn footnote_ref_for_source_index(&self, source_index: usize) -> Option<&RenderedFootnoteRef> {
        self.footnote_refs.iter().find(|fref| fref.source_range.contains(&source_index))
    }
}
