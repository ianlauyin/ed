use super::*;

pub(crate) type UrlHoverCallback = Rc<dyn Fn(Option<SharedString>, &mut Window, &mut App)>;
pub(crate) type SourceClickCallback = Box<dyn Fn(usize, usize, &mut Window, &mut App) -> bool>;
pub(crate) type CheckboxToggleCallback = Rc<dyn Fn(Range<usize>, bool, &mut Window, &mut App)>;
pub(crate) type CodeBlockRenderFn =
    Rc<dyn Fn(&CodeBlock, &MarkdownStyle, &mut Window, &mut App) -> Option<AnyElement>>;

pub enum AutoscrollBehavior {
    /// Propagate the request up the element tree for the nearest
    /// scrollable ancestor (e.g. `List`) to handle.
    Propagate,
    /// Directly control a specific scroll handle.
    Controlled(ScrollHandle),
}

pub struct MarkdownElement {
    markdown: Entity<Markdown>,
    pub(crate) style: MarkdownStyle,
    show_code_block_copy_button: bool,
    render_code_block: Option<CodeBlockRenderFn>,
    input_focus_handle: Option<FocusHandle>,
    on_url_click: Option<Rc<dyn Fn(SharedString, &mut Window, &mut App)>>,
    on_url_hover: Option<UrlHoverCallback>,
    on_source_click: Option<SourceClickCallback>,
    on_checkbox_toggle: Option<CheckboxToggleCallback>,
    pub(crate) image_resolver: Option<Box<dyn Fn(&str, &App) -> Option<ImageSource>>>,
    show_root_block_markers: bool,
    autoscroll: AutoscrollBehavior,
}

impl MarkdownElement {
    pub fn new(markdown: Entity<Markdown>, style: MarkdownStyle) -> Self {
        Self {
            markdown,
            style,
            show_code_block_copy_button: true,
            render_code_block: None,
            input_focus_handle: None,
            on_url_click: None,
            on_url_hover: None,
            on_source_click: None,
            on_checkbox_toggle: None,
            image_resolver: None,
            show_root_block_markers: false,
            autoscroll: AutoscrollBehavior::Propagate,
        }
    }

    pub fn hide_code_block_copy_button(mut self) -> Self {
        self.show_code_block_copy_button = false;
        self
    }

    /// Renders fenced code blocks with `render` instead of as highlighted code,
    /// whenever it returns `Some`.
    pub fn render_code_block_with(
        mut self,
        render: impl Fn(&CodeBlock, &MarkdownStyle, &mut Window, &mut App) -> Option<AnyElement>
        + 'static,
    ) -> Self {
        self.render_code_block = Some(Rc::new(render));
        self
    }

    pub fn input_focus_handle(mut self, focus_handle: FocusHandle) -> Self {
        self.input_focus_handle = Some(focus_handle);
        self
    }

    pub fn on_url_click(
        mut self,
        handler: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_url_click = Some(Rc::new(handler));
        self
    }

    pub fn on_url_hover(
        mut self,
        handler: impl Fn(Option<SharedString>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_url_hover = Some(Rc::new(handler));
        self
    }

    pub fn on_source_click(
        mut self,
        handler: impl Fn(usize, usize, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.on_source_click = Some(Box::new(handler));
        self
    }

    pub fn on_checkbox_toggle(
        mut self,
        handler: impl Fn(Range<usize>, bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_checkbox_toggle = Some(Rc::new(handler));
        self
    }

    pub fn image_resolver(
        mut self,
        resolver: impl Fn(&str, &App) -> Option<ImageSource> + 'static,
    ) -> Self {
        self.image_resolver = Some(Box::new(resolver));
        self
    }

    pub fn show_root_block_markers(mut self) -> Self {
        self.show_root_block_markers = true;
        self
    }

    pub fn scroll_handle(mut self, scroll_handle: ScrollHandle) -> Self {
        self.autoscroll = AutoscrollBehavior::Controlled(scroll_handle);
        self
    }

    fn push_markdown_code_span(
        &self,
        builder: &mut MarkdownElementBuilder,
        text: &str,
        range: Range<usize>,
    ) {
        let mut code_style = self.style.inline_code.clone();
        let chip_background = code_style.background_color.take();
        if builder.link_depth > 0 {
            code_style.color = self.style.link.color.or(code_style.color);
        }
        builder.push_text_style(code_style);
        builder.push_code_chip_text(text, range, chip_background);
        builder.pop_text_style();
    }

    pub(crate) fn push_markdown_image(
        &self,
        builder: &mut MarkdownElementBuilder,
        range: &Range<usize>,
        source: ImageSource,
        dest_url: SharedString,
        alt_text: Option<SharedString>,
        width: Option<DefiniteLength>,
        height: Option<DefiniteLength>,
    ) {
        let enclosing_link_url = (builder.link_depth > 0)
            .then(|| builder.rendered_links.last())
            .flatten()
            .map(|link| link.destination_url.clone());
        let fallback_opens_image_url = enclosing_link_url.is_none();

        let image_element = {
            let wrapper = div().id(("markdown-image-link", range.start)).min_w_0();
            let wrapper = if !self.style.prevent_mouse_interaction
                && let Some(url) = enclosing_link_url
            {
                let click_url = url.clone();
                let markdown = self.markdown.clone();
                let url_click = self.on_url_click.clone();
                let bounds = Rc::new(Cell::new(None));
                builder.push_image_link(url.clone(), bounds.clone());
                wrapper
                    .relative()
                    .cursor_pointer()
                    .child(
                        canvas(
                            move |image_bounds, _window, _cx| bounds.set(Some(image_bounds)),
                            |_, _, _, _| {},
                        )
                        .size_full()
                        .absolute()
                        .top_0()
                        .left_0(),
                    )
                    .on_click(move |_, window, cx| {
                        if let Some(ref on_url_click) = url_click {
                            on_url_click(click_url.clone(), window, cx);
                        } else {
                            cx.open_url(&click_url);
                        }
                    })
                    .capture_any_mouse_down(move |event, _window, cx| {
                        if event.button == MouseButton::Right {
                            markdown.update(cx, |md, _| {
                                md.capture_for_context_menu(Some(url.clone()), None)
                            });
                        }
                    })
            } else {
                wrapper
            };
            wrapper.child(
                img(source)
                    .id(("markdown-image", range.start))
                    .min_w_0()
                    .max_w_full()
                    .rounded_md()
                    .mr_1()
                    .mb_1()
                    .when_some(height, |this, height| this.h(height))
                    .when_some(width, |this, width| this.w(width))
                    .with_fallback(move || {
                        image_fallback_element(
                            dest_url.clone(),
                            alt_text.clone(),
                            fallback_opens_image_url,
                        )
                    }),
            )
        };

        builder.push_image_child(image_element);
    }

    pub(crate) fn push_markdown_paragraph(
        &self,
        builder: &mut MarkdownElementBuilder,
        range: &Range<usize>,
        markdown_end: usize,
        text_align_override: Option<TextAlign>,
    ) {
        let align = text_align_override.unwrap_or(self.style.base_text_style.text_align);
        let mut paragraph = div().when(!self.style.height_is_multiple_of_line_height, |el| {
            el.mb(self.style.paragraph_spacing)
                .line_height(self.style.paragraph_line_height)
        });

        paragraph = match align {
            TextAlign::Center => paragraph.text_center(),
            TextAlign::Left => paragraph.text_left(),
            TextAlign::Right => paragraph.text_right(),
        };

        builder.push_text_style(TextStyleRefinement {
            text_align: Some(align),
            ..Default::default()
        });
        builder.push_div(paragraph, range, markdown_end);
    }

    pub(crate) fn pop_markdown_paragraph(&self, builder: &mut MarkdownElementBuilder) {
        builder.pop_div();
        builder.pop_text_style();
    }

    pub(crate) fn push_markdown_heading(
        &self,
        builder: &mut MarkdownElementBuilder,
        level: pulldown_cmark::HeadingLevel,
        range: &Range<usize>,
        markdown_end: usize,
        text_align_override: Option<TextAlign>,
    ) {
        let align = text_align_override.unwrap_or(self.style.base_text_style.text_align);
        let mut heading = div().mt_4().mb_2();
        heading = apply_heading_style(
            heading,
            level,
            self.style.heading_level_styles.as_ref(),
            self.style.heading_border_color,
        );

        heading = match align {
            TextAlign::Center => heading.text_center(),
            TextAlign::Left => heading.text_left(),
            TextAlign::Right => heading.text_right(),
        };

        let mut heading_style = self.style.heading.clone();
        let mut heading_text_style = heading_style.text_style().clone();
        heading.style().refine(&heading_style);

        if let Some(level_style) =
            heading_level_style(level, self.style.heading_level_styles.as_ref())
        {
            heading_text_style.refine(level_style);
        }

        builder.push_text_style(TextStyleRefinement {
            text_align: Some(align),
            ..heading_text_style
        });
        builder.push_div(heading, range, markdown_end);
    }

    pub(crate) fn pop_markdown_heading(&self, builder: &mut MarkdownElementBuilder) {
        builder.pop_div();
        builder.pop_text_style();
    }

    pub(crate) fn push_markdown_block_quote(
        &self,
        builder: &mut MarkdownElementBuilder,
        kind: Option<pulldown_cmark::BlockQuoteKind>,
        range: &Range<usize>,
        markdown_end: usize,
    ) {
        let border_color = self
            .style
            .block_quote_kind_colors
            .for_kind(kind, self.style.block_quote_border_color);

        let header = kind.map(|kind| {
            let (icon_name, label) = match kind {
                BlockQuoteKind::Note => (IconName::Info, "Note"),
                BlockQuoteKind::Tip => (IconName::Sparkle, "Tip"),
                BlockQuoteKind::Important => (IconName::Chat, "Important"),
                BlockQuoteKind::Warning => (IconName::Warning, "Warning"),
                BlockQuoteKind::Caution => (IconName::Stop, "Caution"),
            };
            h_flex()
                .gap_1()
                .items_center()
                .mb_1()
                .child(
                    Icon::new(icon_name)
                        .size(IconSize::Small)
                        .color(Color::Custom(border_color)),
                )
                .child(
                    Label::new(label)
                        .color(Color::Custom(border_color))
                        .weight(FontWeight::BOLD),
                )
                .into_any_element()
        });

        let block_div = div()
            .pl_4()
            .mb(self.style.paragraph_spacing)
            .border_l_4()
            .border_color(border_color);
        let block_div = match header {
            Some(header) => block_div.child(header),
            None => block_div,
        };

        builder.push_text_style(self.style.block_quote.clone());
        builder.push_div(block_div, range, markdown_end);
    }

    pub(crate) fn pop_markdown_block_quote(&self, builder: &mut MarkdownElementBuilder) {
        builder.pop_div();
        builder.pop_text_style();
    }

    fn push_metadata_block(
        &self,
        builder: &mut MarkdownElementBuilder,
        source: &str,
        metadata_block: &ParsedMetadataBlock,
        markdown_end: usize,
        cx: &App,
    ) {
        let content_range = &metadata_block.content_range;
        if let Some(rows) = metadata_block.rows.as_deref() {
            builder.push_div(
                div()
                    .grid()
                    .grid_cols(2)
                    .w_full()
                    .mb_2()
                    .border_1()
                    .border_color(cx.theme().colors().border)
                    .rounded_sm()
                    .overflow_hidden(),
                content_range,
                markdown_end,
            );

            for (row_index, row) in rows.iter().enumerate() {
                self.push_metadata_cell(
                    builder,
                    source,
                    row.key.clone(),
                    content_range,
                    markdown_end,
                    MetadataCellStyle {
                        row_index,
                        is_key: true,
                    },
                    cx,
                );
                self.push_metadata_cell(
                    builder,
                    source,
                    row.value.clone(),
                    content_range,
                    markdown_end,
                    MetadataCellStyle {
                        row_index,
                        is_key: false,
                    },
                    cx,
                );
            }

            builder.pop_div();
        } else {
            let mut metadata_block = div().w_full().rounded_md();
            metadata_block.style().refine(&self.style.code_block);
            builder.push_text_style(self.style.code_block.text.to_owned());
            builder.push_code_block(None);
            builder.push_div(metadata_block, content_range, markdown_end);
            builder.push_text(&source[content_range.clone()], content_range.clone());
            builder.trim_trailing_newline();
            builder.pop_div();
            builder.pop_code_block();
            builder.pop_text_style();
        }
    }

    fn push_metadata_cell(
        &self,
        builder: &mut MarkdownElementBuilder,
        source: &str,
        text_range: Range<usize>,
        block_range: &Range<usize>,
        markdown_end: usize,
        cell_style: MetadataCellStyle,
        cx: &App,
    ) {
        builder.push_div(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .px_2()
                .py_1()
                .border_color(cx.theme().colors().border)
                .when(cell_style.row_index > 0, |this| this.border_t_1())
                .when(!cell_style.is_key, |this| this.border_l_1())
                .when(cell_style.is_key, |this| {
                    this.bg(cx.theme().colors().panel_background)
                }),
            block_range,
            markdown_end,
        );

        let text_style = if cell_style.is_key {
            TextStyleRefinement {
                color: Some(cx.theme().colors().text_muted),
                font_weight: Some(FontWeight::SEMIBOLD),
                ..Default::default()
            }
        } else {
            TextStyleRefinement::default()
        };
        builder.push_text_style(text_style);
        builder.push_text(&source[text_range.clone()], text_range);
        builder.pop_text_style();
        builder.pop_div();
    }

    pub(crate) fn push_markdown_list_item(
        &self,
        builder: &mut MarkdownElementBuilder,
        bullet: AnyElement,
        range: &Range<usize>,
        markdown_end: usize,
    ) {
        builder.push_div(
            div()
                .when(!self.style.height_is_multiple_of_line_height, |el| {
                    el.mb_1()
                        .gap_1()
                        .line_height(self.style.paragraph_line_height)
                })
                .h_flex()
                .items_start()
                .child(bullet),
            range,
            markdown_end,
        );
        // Without `w_0`, text doesn't wrap to the width of the container.
        builder.push_div(div().flex_1().w_0(), range, markdown_end);
    }

    pub(crate) fn pop_markdown_list_item(&self, builder: &mut MarkdownElementBuilder) {
        builder.pop_div();
        builder.pop_div();
    }

    fn paint_mouse_listeners(
        &mut self,
        hitbox: &Hitbox,
        rendered_text: &RenderedText,
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.style.prevent_mouse_interaction {
            return;
        }

        let is_hovering_clickable = hitbox.is_hovered(window)
            && !self.markdown.read(cx).selection.pending
            && (rendered_text
                .image_link_for_position(window.mouse_position())
                .is_some()
                || rendered_text
                    .source_index_for_position(window.mouse_position())
                    .ok()
                    .is_some_and(|source_index| {
                        rendered_text.link_for_source_index(source_index).is_some()
                            || rendered_text
                                .footnote_ref_for_source_index(source_index)
                                .is_some()
                    }));

        if is_hovering_clickable {
            window.set_cursor_style(CursorStyle::PointingHand, hitbox);
        } else {
            window.set_cursor_style(CursorStyle::IBeam, hitbox);
        }

        let on_open_url = self.on_url_click.take();
        let on_url_hover = self.on_url_hover.take();
        let on_source_click = self.on_source_click.take();

        self.on_mouse_event(window, cx, {
            let hitbox = hitbox.clone();
            let rendered_text = rendered_text.clone();
            move |markdown, event: &MouseDownEvent, phase, window, _cx| {
                if phase.capture()
                    && event.button == MouseButton::Right
                    && hitbox.is_hovered(window)
                {
                    let link = rendered_text
                        .source_index_for_position(event.position)
                        .ok()
                        .and_then(|ix| rendered_text.link_for_source_index(ix))
                        .map(|link| link.destination_url.clone());
                    markdown.capture_for_context_menu(link, Some(&rendered_text));
                }
            }
        });

        self.on_mouse_event(window, cx, {
            let rendered_text = rendered_text.clone();
            let hitbox = hitbox.clone();
            move |markdown, event: &MouseDownEvent, phase, window, cx| {
                if hitbox.is_hovered(window) {
                    if phase.bubble() && event.button != MouseButton::Right {
                        let position_result =
                            rendered_text.source_index_for_position(event.position);

                        if let Ok(source_index) = position_result {
                            if let Some(footnote_ref) =
                                rendered_text.footnote_ref_for_source_index(source_index)
                            {
                                markdown.pressed_footnote_ref = Some(footnote_ref.clone());
                            } else if let Some(link) =
                                rendered_text.link_for_source_index(source_index)
                            {
                                markdown.pressed_link = Some(link.clone());
                            }
                        }

                        if markdown.pressed_footnote_ref.is_none()
                            && markdown.pressed_link.is_none()
                        {
                            let source_index = match position_result {
                                Ok(ix) | Err(ix) => ix,
                            };
                            if let Some(handler) = on_source_click.as_ref() {
                                let blocked = handler(source_index, event.click_count, window, cx);
                                if blocked {
                                    markdown.selection = Selection::default();
                                    markdown.pressed_link = None;
                                    window.prevent_default();
                                    cx.notify();
                                    return;
                                }
                            }
                            let (range, mode, reversed) = match event.click_count {
                                1 if event.modifiers.shift => {
                                    let tail = markdown.selection.tail();
                                    let reversed = source_index < tail;
                                    let range = if reversed {
                                        source_index..tail
                                    } else {
                                        tail..source_index
                                    };
                                    (range, SelectMode::Character, reversed)
                                }
                                1 => {
                                    let range = source_index..source_index;
                                    (range, SelectMode::Character, false)
                                }
                                2 => {
                                    let range = rendered_text.surrounding_word_range(source_index);
                                    (range.clone(), SelectMode::Word(range), false)
                                }
                                3 => {
                                    let range = rendered_text.surrounding_line_range(source_index);
                                    (range.clone(), SelectMode::Line(range), false)
                                }
                                _ => {
                                    let range = 0..rendered_text
                                        .lines
                                        .last()
                                        .map(|line| line.source_end)
                                        .unwrap_or(0);
                                    (range, SelectMode::All, false)
                                }
                            };
                            markdown.selection = Selection {
                                start: range.start,
                                end: range.end,
                                reversed,
                                pending: true,
                                mode,
                            };
                            window.focus(&markdown.focus_handle, cx);
                        }

                        window.prevent_default();
                        cx.notify();
                    }
                } else if phase.capture() && event.button == MouseButton::Left {
                    markdown.selection = Selection::default();
                    markdown.pressed_link = None;
                    cx.notify();
                }
            }
        });
        self.on_mouse_event(window, cx, {
            let rendered_text = rendered_text.clone();
            let hitbox = hitbox.clone();
            let was_hovering_clickable = is_hovering_clickable;
            move |markdown, event: &MouseMoveEvent, phase, window, cx| {
                if phase.capture() {
                    return;
                }

                if markdown.selection.pending {
                    let source_index = match rendered_text.source_index_for_position(event.position)
                    {
                        Ok(ix) | Err(ix) => ix,
                    };
                    markdown.selection.set_head(source_index, &rendered_text);
                    markdown.autoscroll_code_block(source_index, event.position);
                    markdown.autoscroll_request = Some(source_index);
                    cx.notify();
                } else {
                    let is_hitbox_hovered = hitbox.is_hovered(window);
                    let source_index = is_hitbox_hovered
                        .then(|| rendered_text.source_index_for_position(event.position).ok())
                        .flatten();
                    let hovered_url = is_hitbox_hovered
                        .then(|| rendered_text.image_link_for_position(event.position))
                        .flatten()
                        .map(|image| image.destination_url.clone())
                        .or_else(|| {
                            source_index
                                .and_then(|source_index| {
                                    rendered_text.link_for_source_index(source_index)
                                })
                                .map(|link| link.destination_url.clone())
                        });
                    let is_hovering_clickable = hovered_url.is_some()
                        || source_index.is_some_and(|source_index| {
                            rendered_text
                                .footnote_ref_for_source_index(source_index)
                                .is_some()
                        });
                    if let Some(on_url_hover) = on_url_hover.as_ref() {
                        on_url_hover(hovered_url, window, cx);
                    }
                    if is_hovering_clickable != was_hovering_clickable {
                        cx.notify();
                    }
                }
            }
        });
        self.on_mouse_event(window, cx, {
            let rendered_text = rendered_text.clone();
            move |markdown, event: &MouseUpEvent, phase, window, cx| {
                if phase.bubble() {
                    let source_index = rendered_text.source_index_for_position(event.position).ok();
                    if let Some(pressed_footnote_ref) = markdown.pressed_footnote_ref.take()
                        && source_index
                            .and_then(|ix| rendered_text.footnote_ref_for_source_index(ix))
                            == Some(&pressed_footnote_ref)
                    {
                        if let Some(source_index) =
                            markdown.footnote_definition_content_start(&pressed_footnote_ref.label)
                        {
                            markdown.autoscroll_request = Some(source_index);
                            cx.notify();
                        }
                    } else if let Some(pressed_link) = markdown.pressed_link.take()
                        && source_index.and_then(|ix| rendered_text.link_for_source_index(ix))
                            == Some(&pressed_link)
                    {
                        if let Some(open_url) = on_open_url.as_ref() {
                            open_url(pressed_link.destination_url, window, cx);
                        } else {
                            cx.open_url(&pressed_link.destination_url);
                        }
                    }
                } else if markdown.selection.pending {
                    markdown.selection.pending = false;

                    cx.notify();
                }
            }
        });
    }

    fn autoscroll(
        &self,
        rendered_text: &RenderedText,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<()> {
        let autoscroll_index = self
            .markdown
            .update(cx, |markdown, _| markdown.autoscroll_request.take())?;
        let (position, line_height) = rendered_text.position_for_source_index(autoscroll_index)?;

        match &self.autoscroll {
            AutoscrollBehavior::Controlled(scroll_handle) => {
                let viewport = scroll_handle.bounds();
                let margin = line_height * 3.;
                let top_goal = viewport.top() + margin;
                let bottom_goal = viewport.bottom() - margin;
                let current_offset = scroll_handle.offset();

                let new_offset_y = if position.y < top_goal {
                    current_offset.y + (top_goal - position.y)
                } else if position.y + line_height > bottom_goal {
                    current_offset.y + (bottom_goal - (position.y + line_height))
                } else {
                    current_offset.y
                };

                scroll_handle.set_offset(point(
                    current_offset.x,
                    new_offset_y.clamp(-scroll_handle.max_offset().y, Pixels::ZERO),
                ));
            }
            AutoscrollBehavior::Propagate => {
                let text_style = self.style.base_text_style.clone();
                let font_id = window.text_system().resolve_font(&text_style.font());
                let font_size = text_style.font_size.to_pixels(window.rem_size());
                let em_width = window.text_system().em_width(font_id, font_size).unwrap();
                window.request_autoscroll(Bounds::from_corners(
                    point(position.x - 3. * em_width, position.y - 3. * line_height),
                    point(position.x + 3. * em_width, position.y + 3. * line_height),
                ));
            }
        }
        Some(())
    }

    fn on_mouse_event<T: MouseEvent>(
        &self,
        window: &mut Window,
        _cx: &mut App,
        mut f: impl 'static
        + FnMut(&mut Markdown, &T, DispatchPhase, &mut Window, &mut Context<Markdown>),
    ) {
        window.on_mouse_event({
            let markdown = self.markdown.downgrade();
            move |event, phase, window, cx| {
                markdown
                    .update(cx, |markdown, cx| f(markdown, event, phase, window, cx))
                    .log_err();
            }
        });
    }
}

impl Styled for MarkdownElement {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style.container_style
    }
}

impl Element for MarkdownElement {
    type RequestLayoutState = RenderedMarkdown;
    type PrepaintState = Hitbox;

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
        let highlights = {
            let markdown = self.markdown.read(cx);
            let colors = cx.theme().colors();
            let selection = &markdown.selection;
            MarkdownHighlights {
                search_highlights: markdown.search_highlights.clone(),
                active_search_highlight: markdown.active_search_highlight,
                search_match_color: colors.search_match_background,
                active_search_match_color: colors.search_active_match_background,
                selection: (selection.start < selection.end).then(|| {
                    (
                        selection.start..selection.end,
                        self.style.selection_background_color,
                    )
                }),
                next_search_highlight_ix: 0,
                base_background: self.style.highlight_base_background,
                minimum_contrast: self.style.minimum_highlight_contrast,
            }
        };

        let (parsed_markdown, images, active_root_block) = {
            let markdown = self.markdown.read(cx);
            (
                markdown.parsed_markdown.clone(),
                markdown.images_by_source_offset.clone(),
                markdown.active_root_block,
            )
        };
        let mut builder = MarkdownElementBuilder::new(
            &self.style.container_style,
            self.style.base_text_style.clone(),
            self.style.syntax.clone(),
            highlights,
        );
        let markdown_end = if let Some(last) = parsed_markdown.events.last() {
            last.0.end
        } else {
            0
        };
        let mut code_block_ids = HashSet::default();
        let mut current_code_block_text = None;

        let mut current_img_block_range: Option<Range<usize>> = None;
        let mut handled_html_block = false;
        let mut rendered_custom_code_block = false;
        let mut rendered_metadata_block = false;
        for (index, (range, event)) in parsed_markdown.events.iter().enumerate() {
            let event_index = MarkdownEventIndex(index);
            // Skip alt text for images that rendered
            if let Some(current_img_block_range) = &current_img_block_range
                && current_img_block_range.end > range.end
            {
                continue;
            }

            if handled_html_block {
                if let MarkdownEvent::End(MarkdownTagEnd::HtmlBlock) = event {
                    handled_html_block = false;
                } else {
                    continue;
                }
            }

            if rendered_custom_code_block {
                if matches!(event, MarkdownEvent::End(MarkdownTagEnd::CodeBlock)) {
                    rendered_custom_code_block = false;
                }
                continue;
            }

            if rendered_metadata_block {
                if matches!(event, MarkdownEvent::End(MarkdownTagEnd::MetadataBlock(_))) {
                    rendered_metadata_block = false;
                }
                continue;
            }

            match event {
                MarkdownEvent::RootStart => {
                    if self.show_root_block_markers {
                        builder.push_root_block(range, markdown_end);
                    }
                }
                MarkdownEvent::RootEnd(root_block_index) => {
                    if self.show_root_block_markers {
                        builder.pop_root_block(
                            active_root_block == Some(*root_block_index),
                            cx.theme().colors().border,
                            cx.theme().colors().border_variant,
                        );
                    }
                }
                MarkdownEvent::Start(tag) => {
                    match tag {
                        MarkdownTag::Image { dest_url, .. } => {
                            let alt_text = collect_image_alt_text(
                                &parsed_markdown.events[index..],
                                &parsed_markdown.source,
                            );
                            if let Some(image) = images.get(&range.start) {
                                current_img_block_range = Some(range.clone());
                                self.push_markdown_image(
                                    &mut builder,
                                    range,
                                    image.clone().into(),
                                    dest_url.clone(),
                                    alt_text,
                                    None,
                                    None,
                                );
                            } else if let Some(source) = self
                                .image_resolver
                                .as_ref()
                                .and_then(|resolve| resolve(dest_url.as_ref(), cx))
                            {
                                current_img_block_range = Some(range.clone());
                                self.push_markdown_image(
                                    &mut builder,
                                    range,
                                    source,
                                    dest_url.clone(),
                                    alt_text,
                                    None,
                                    None,
                                );
                            }
                        }
                        MarkdownTag::Paragraph => {
                            let text_align_override = builder
                                .table
                                .current_cell_alignment()
                                .and_then(alignment_to_text_align);
                            self.push_markdown_paragraph(
                                &mut builder,
                                range,
                                markdown_end,
                                text_align_override,
                            );
                        }
                        MarkdownTag::Heading { level, .. } => {
                            let text_align_override = builder
                                .table
                                .current_cell_alignment()
                                .and_then(alignment_to_text_align);
                            self.push_markdown_heading(
                                &mut builder,
                                *level,
                                range,
                                markdown_end,
                                text_align_override,
                            );
                        }
                        MarkdownTag::BlockQuote(kind) => {
                            self.push_markdown_block_quote(
                                &mut builder,
                                *kind,
                                range,
                                markdown_end,
                            );
                        }
                        MarkdownTag::CodeBlock { kind, metadata } => {
                            if let Some(render) = &self.render_code_block
                                && let Some(code_block) = CodeBlock::new(
                                    range.start,
                                    kind,
                                    metadata,
                                    &parsed_markdown.source,
                                )
                                && let Some(element) = render(&code_block, &self.style, window, cx)
                            {
                                builder
                                    .push_sourced_element(metadata.content_range.clone(), element);
                                rendered_custom_code_block = true;
                                continue;
                            }

                            current_code_block_text =
                                self.show_code_block_copy_button.then(String::new);

                            let language = parsed_markdown.code_block_language(kind);

                            let scroll_handle = if self.style.code_block_overflow_x_scroll {
                                code_block_ids.insert(range.start);
                                Some(self.markdown.update(cx, |markdown, _| {
                                    markdown.code_block_scroll_handle(range.start)
                                }))
                            } else {
                                None
                            };

                            // This is a parent container that we can position the copy button inside.
                            let mut parent_container = div()
                                .group("code_block")
                                .relative()
                                .w_full()
                                .when(scroll_handle.is_some(), |this| this.rounded_lg());

                            // Styles must be final before `custom_scrollbars`, which reads the
                            // corner radii and borders to round the scrollbar track.
                            parent_container.style().refine(&self.style.code_block);

                            let parent_container: AnyDiv =
                                if let Some(scroll_handle) = scroll_handle.as_ref() {
                                    let scrollbars = Scrollbars::new(ScrollAxes::Horizontal)
                                        .id(("markdown-code-block-scrollbar", range.start))
                                        .tracked_scroll_handle(scroll_handle)
                                        .with_track_along(
                                            ScrollAxes::Horizontal,
                                            cx.theme().colors().editor_background,
                                        )
                                        .notify_content();

                                    let mut parent_container =
                                        parent_container.custom_scrollbars(scrollbars, window, cx);
                                    // Keep the code block's padding over the space reserved for the scrollbar track.
                                    parent_container
                                        .style()
                                        .padding
                                        .refine(&self.style.code_block.padding);
                                    parent_container.into()
                                } else {
                                    parent_container.into()
                                };

                            builder.push_div(parent_container, range, markdown_end);

                            let code_block = div()
                                .id(("code-block", range.start))
                                .rounded_lg()
                                .map(|code_block| {
                                    if let Some(scroll_handle) = scroll_handle.as_ref() {
                                        code_block
                                            .flex()
                                            .overflow_x_scroll()
                                            .restrict_scroll_to_axis()
                                            .track_scroll(scroll_handle)
                                    } else {
                                        code_block.w_full()
                                    }
                                });

                            builder.push_text_style(self.style.code_block.text.to_owned());
                            builder.push_code_block(language);
                            builder.push_div(code_block, range, markdown_end);
                        }
                        MarkdownTag::HtmlBlock => {
                            builder.push_div(div(), range, markdown_end);
                            if let Some(block) = parsed_markdown.html_blocks.get(&range.start) {
                                self.render_html_block(block, &mut builder, markdown_end, cx);
                                handled_html_block = true;
                            }
                        }
                        MarkdownTag::List(bullet_index) => {
                            builder.push_list(*bullet_index);
                            let is_top_level = builder.list_stack.len() == 1;
                            builder.push_div(
                                div()
                                    .pl_2p5()
                                    .when(is_top_level, |this| this.mb(self.style.list_spacing)),
                                range,
                                markdown_end,
                            );
                        }
                        MarkdownTag::Item => {
                            let bullet = if let Some((task_range, checked)) =
                                task_list_marker_for_item(&parsed_markdown.events, index)
                            {
                                let source = &parsed_markdown.source()[range.clone()];
                                let checkbox = Checkbox::new(
                                    ElementId::Name(source.to_string().into()),
                                    ToggleState::from(checked),
                                )
                                .fill();

                                let checkbox = if let Some(on_toggle) =
                                    self.on_checkbox_toggle.clone()
                                {
                                    let task_source_range = task_range.clone();
                                    checkbox.on_click(move |_state, window, cx| {
                                        on_toggle(task_source_range.clone(), !checked, window, cx);
                                    })
                                } else {
                                    checkbox.visualization_only(true)
                                };

                                let line_height = self
                                    .style
                                    .paragraph_line_height
                                    .to_pixels(builder.text_style().font_size, window.rem_size());
                                // List items top-align their bullet, which suits text bullets
                                // but leaves the taller checkbox sitting above the text.
                                // Centering it in a slot one line tall aligns it with the
                                // first line, even when the item wraps.
                                div()
                                    .h(line_height)
                                    .flex()
                                    .items_center()
                                    .child(checkbox)
                                    .into_any_element()
                            } else if let Some(bullet_index) = builder.next_bullet_index() {
                                div().child(format!("{}.", bullet_index)).into_any_element()
                            } else {
                                div().child("•").into_any_element()
                            };
                            self.push_markdown_list_item(&mut builder, bullet, range, markdown_end);
                        }
                        MarkdownTag::Emphasis => builder.push_text_style(TextStyleRefinement {
                            font_style: Some(FontStyle::Italic),
                            ..Default::default()
                        }),
                        MarkdownTag::Strong => builder.push_text_style(TextStyleRefinement {
                            font_weight: Some(FontWeight::BOLD),
                            color: Some(cx.theme().colors().text),
                            ..Default::default()
                        }),
                        MarkdownTag::Strikethrough => {
                            builder.push_text_style(TextStyleRefinement {
                                strikethrough: Some(StrikethroughStyle {
                                    thickness: px(1.),
                                    color: None,
                                }),
                                ..Default::default()
                            })
                        }
                        MarkdownTag::Link { dest_url, .. } => {
                            if builder.code_block_stack.is_empty() {
                                builder.link_depth += 1;
                                builder.push_link(dest_url.clone(), range.clone());
                                let style = self
                                    .style
                                    .link_callback
                                    .as_ref()
                                    .and_then(|callback| callback(dest_url, cx))
                                    .unwrap_or_else(|| self.style.link.clone());
                                builder.push_text_style(style)
                            }
                        }
                        MarkdownTag::FootnoteDefinition(label) => {
                            if !builder.rendered_footnote_separator {
                                builder.rendered_footnote_separator = true;
                                builder.push_div(
                                    div()
                                        .border_t_1()
                                        .mt_2()
                                        .border_color(self.style.rule_color),
                                    range,
                                    markdown_end,
                                );
                                builder.pop_div();
                            }
                            builder.push_div(
                                div()
                                    .pt_1()
                                    .mb_1()
                                    .line_height(rems(1.3))
                                    .text_size(rems(0.85))
                                    .h_flex()
                                    .items_start()
                                    .gap_2()
                                    .child(
                                        div().text_size(rems(0.85)).child(format!("{}.", label)),
                                    ),
                                range,
                                markdown_end,
                            );
                            builder.push_div(div().flex_1().w_0(), range, markdown_end);
                        }
                        MarkdownTag::MetadataBlock(_) => {
                            if let Some(metadata_block) =
                                parsed_markdown.metadata_blocks.get(&range.start)
                            {
                                self.push_metadata_block(
                                    &mut builder,
                                    &parsed_markdown.source,
                                    metadata_block,
                                    markdown_end,
                                    cx,
                                );
                                rendered_metadata_block = true;
                            }
                        }
                        MarkdownTag::Table(alignments) => {
                            builder.table.start(alignments.clone());

                            let column_count = alignments.len();
                            builder.push_div(div().flex(), range, markdown_end);
                            builder.push_div(
                                div()
                                    .id(("table", range.start))
                                    .min_w_0()
                                    .grid()
                                    .when(self.style.table_columns_min_size, |this| {
                                        this.w_full().grid_cols_min_content(column_count as u16)
                                    })
                                    .when(!self.style.table_columns_min_size, |this| {
                                        this.grid_cols_max_content(column_count as u16)
                                    })
                                    .mb_2()
                                    .border(px(1.5))
                                    .border_color(cx.theme().colors().border)
                                    .rounded_sm()
                                    .restrict_scroll_to_axis()
                                    .custom_scrollbars(
                                        Scrollbars::new(ScrollAxes::Horizontal)
                                            .id(("markdown-table-scrollbar", range.start))
                                            .notify_content(),
                                        window,
                                        cx,
                                    ),
                                range,
                                markdown_end,
                            );
                        }
                        MarkdownTag::TableHead => {
                            builder.table.start_head();
                            builder.push_text_style(TextStyleRefinement {
                                font_weight: Some(FontWeight::SEMIBOLD),
                                ..Default::default()
                            });
                        }
                        MarkdownTag::TableRow => {
                            builder.table.start_row();
                        }
                        MarkdownTag::TableCell => {
                            builder.table.start_cell();
                            let is_header = builder.table.in_head;
                            let row_index = builder.table.row_index;
                            let col_index = builder.table.col_index;
                            let alignment = builder.table.current_cell_alignment();
                            let text_align = alignment
                                .and_then(alignment_to_text_align)
                                .unwrap_or(self.style.base_text_style.text_align);

                            let mut cell_div = div()
                                .flex()
                                .flex_col()
                                .h_full()
                                .when(col_index > 0, |this| this.border_l_1())
                                .when(row_index > 0, |this| this.border_t_1())
                                .border_color(cx.theme().colors().border)
                                .px(self.style.table_cell_padding.x)
                                .py(self.style.table_cell_padding.y)
                                .when(is_header, |this| {
                                    this.bg(cx.theme().colors().title_bar_background)
                                })
                                .when(!is_header && row_index % 2 == 1, |this| {
                                    this.bg(cx.theme().colors().panel_background)
                                });

                            cell_div = match alignment {
                                Some(Alignment::Center) => cell_div.items_center(),
                                Some(Alignment::Right) => cell_div.items_end(),
                                _ => cell_div,
                            };

                            builder.push_text_style(TextStyleRefinement {
                                text_align: Some(text_align),
                                ..Default::default()
                            });
                            builder.push_div(cell_div, range, markdown_end);
                            builder.push_div(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .w_full()
                                    .justify_center()
                                    .text_align(text_align),
                                range,
                                markdown_end,
                            );
                        }
                        _ => log::debug!("unsupported markdown tag {:?}", tag),
                    }
                }
                MarkdownEvent::End(tag) => match tag {
                    MarkdownTagEnd::Image => {
                        current_img_block_range.take();
                    }
                    MarkdownTagEnd::Paragraph => {
                        self.pop_markdown_paragraph(&mut builder);
                    }
                    MarkdownTagEnd::Heading(_) => {
                        self.pop_markdown_heading(&mut builder);
                    }
                    MarkdownTagEnd::BlockQuote(_kind) => {
                        self.pop_markdown_block_quote(&mut builder);
                    }
                    MarkdownTagEnd::CodeBlock => {
                        builder.trim_trailing_newline();

                        builder.pop_div();
                        builder.pop_code_block();
                        builder.pop_text_style();

                        if let Some(code) = current_code_block_text.take() {
                            builder.modify_current_div(|el| {
                                el.child(
                                    h_flex()
                                        .gap_0p5()
                                        .absolute()
                                        .bg(cx.theme().colors().editor_background)
                                        .top_1()
                                        .right_1()
                                        .visible_on_hover("code_block")
                                        .child(render_copy_code_block_button(
                                            range.end,
                                            code,
                                            self.markdown.entity_id(),
                                        )),
                                )
                            });
                        }

                        // Pop the parent container.
                        builder.pop_div();
                    }
                    MarkdownTagEnd::HtmlBlock => builder.pop_div(),
                    MarkdownTagEnd::List(_) => {
                        builder.pop_list();
                        builder.pop_div();
                    }
                    MarkdownTagEnd::Item => {
                        self.pop_markdown_list_item(&mut builder);
                    }
                    MarkdownTagEnd::Emphasis => builder.pop_text_style(),
                    MarkdownTagEnd::Strong => builder.pop_text_style(),
                    MarkdownTagEnd::Strikethrough => builder.pop_text_style(),
                    MarkdownTagEnd::Link => {
                        if builder.code_block_stack.is_empty() {
                            builder.link_depth = builder.link_depth.saturating_sub(1);
                            builder.pop_text_style()
                        }
                    }
                    MarkdownTagEnd::Table => {
                        builder.pop_div();
                        builder.pop_div();
                        builder.table.end();
                    }
                    MarkdownTagEnd::TableHead => {
                        builder.pop_text_style();
                        builder.table.end_head();
                    }
                    MarkdownTagEnd::TableRow => {
                        builder.table.end_row();
                    }
                    MarkdownTagEnd::TableCell => {
                        builder.replace_pending_checkbox(self.on_checkbox_toggle.clone());
                        builder.pop_div();
                        builder.pop_div();
                        builder.pop_text_style();
                        builder.table.end_cell();
                    }
                    MarkdownTagEnd::FootnoteDefinition => {
                        builder.pop_div();
                        builder.pop_div();
                    }
                    MarkdownTagEnd::MetadataBlock(_) => {}
                    _ => log::debug!("unsupported markdown tag end: {:?}", tag),
                },
                MarkdownEvent::Text => {
                    let text = &parsed_markdown.source[range.clone()];
                    if let Some(current_code_block_text) = &mut current_code_block_text {
                        current_code_block_text.push_str(text);
                    }
                    builder.push_highlighted_text(
                        text,
                        range.clone(),
                        parsed_markdown.code_block_highlights.get(&event_index),
                    );
                }
                MarkdownEvent::SubstitutedText(text) => {
                    if let Some(current_code_block_text) = &mut current_code_block_text {
                        current_code_block_text.push_str(text);
                    }
                    builder.push_highlighted_text(
                        text,
                        range.clone(),
                        parsed_markdown.code_block_highlights.get(&event_index),
                    );
                }
                MarkdownEvent::Code => {
                    self.push_markdown_code_span(
                        &mut builder,
                        &parsed_markdown.source[range.clone()],
                        range.clone(),
                    );
                }
                MarkdownEvent::SubstitutedCode(text) => {
                    self.push_markdown_code_span(&mut builder, text, range.clone());
                }
                MarkdownEvent::Html => {
                    let html = &parsed_markdown.source[range.clone()];
                    if html.starts_with("<!--") {
                        builder.html_comment = true;
                    }
                    if html.trim_end().ends_with("-->") {
                        builder.html_comment = false;
                        continue;
                    }
                    if builder.html_comment {
                        continue;
                    }
                    builder.push_text(html, range.clone());
                }
                MarkdownEvent::InlineHtml => {
                    let html = &parsed_markdown.source[range.clone()];
                    if let Some(code) = html
                        .strip_prefix("<code>")
                        .and_then(|html| html.strip_suffix("</code>"))
                    {
                        let code_start = range.start + "<code>".len();
                        self.push_markdown_code_span(
                            &mut builder,
                            code,
                            code_start..code_start + code.len(),
                        );
                        continue;
                    }
                    if html.starts_with("<code>") {
                        builder.push_text_style(self.style.inline_code.clone());
                        continue;
                    }
                    if html.trim_end().starts_with("</code>") {
                        builder.pop_text_style();
                        continue;
                    }
                    builder.push_text(&parsed_markdown.source[range.clone()], range.clone());
                }
                MarkdownEvent::Rule => {
                    builder.push_div(
                        div()
                            .border_b_1()
                            .my(self.style.paragraph_spacing)
                            .border_color(self.style.rule_color),
                        range,
                        markdown_end,
                    );
                    builder.pop_div()
                }
                MarkdownEvent::SoftBreak if !self.style.soft_break_as_hard_break => {
                    builder.push_soft_break(range.clone());
                }
                MarkdownEvent::SoftBreak | MarkdownEvent::HardBreak => {
                    builder.push_line_break(range.clone());
                }
                MarkdownEvent::TaskListMarker(_) => {
                    // handled inside the `MarkdownTag::Item` case
                }
                MarkdownEvent::FootnoteReference(label) => {
                    builder.push_footnote_ref(label.clone(), range.clone());
                    builder.push_text_style(self.style.link.clone());
                    builder.push_text(&format!("[{label}]"), range.clone());
                    builder.pop_text_style();
                }
            }
        }
        if self.style.code_block_overflow_x_scroll {
            let code_block_ids = code_block_ids;
            self.markdown.update(cx, move |markdown, _| {
                markdown.retain_code_block_scroll_handles(&code_block_ids);
            });
        } else {
            self.markdown
                .update(cx, |markdown, _| markdown.clear_code_block_scroll_handles());
        }
        let mut rendered_markdown = builder.build();
        let child_layout_id = rendered_markdown.element.request_layout(window, cx);
        let layout_id = window.request_layout(gpui::Style::default(), [child_layout_id], cx);
        (layout_id, rendered_markdown)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        bounds: Bounds<Pixels>,
        rendered_markdown: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let focus_handle = self.markdown.read(cx).focus_handle.clone();
        window.set_focus_handle(&focus_handle, cx);
        window.set_view_id(self.markdown.entity_id());

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        rendered_markdown.element.prepaint(window, cx);
        self.autoscroll(&rendered_markdown.text, window, cx);
        hitbox
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _bounds: Bounds<Pixels>,
        rendered_markdown: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let mut context = KeyContext::default();
        context.add("Markdown");
        window.set_key_context(context);

        let markdown_focus_handle = self.markdown.read(cx).focus_handle.clone();
        let input_focus_handle = if markdown_focus_handle.is_focused(window) {
            Some(markdown_focus_handle)
        } else {
            self.input_focus_handle
                .clone()
                .filter(|focus_handle| focus_handle.is_focused(window))
        };
        if let Some(input_focus_handle) = input_focus_handle {
            window.handle_input(
                &input_focus_handle,
                MarkdownInputHandler::new(self.markdown.clone(), rendered_markdown.text.clone()),
                cx,
            );
        }

        window.on_action(std::any::TypeId::of::<crate::Copy>(), {
            let entity = self.markdown.clone();
            let text = rendered_markdown.text.clone();
            move |_, phase, window, cx| {
                let text = text.clone();
                if phase == DispatchPhase::Bubble {
                    entity.update(cx, move |this, cx| this.copy(&text, window, cx))
                }
            }
        });
        window.on_action(std::any::TypeId::of::<crate::CopyAsMarkdown>(), {
            let entity = self.markdown.clone();
            move |_, phase, window, cx| {
                if phase == DispatchPhase::Bubble {
                    entity.update(cx, move |this, cx| this.copy_as_markdown(window, cx))
                }
            }
        });

        self.paint_mouse_listeners(hitbox, &rendered_markdown.text, window, cx);
        rendered_markdown.element.paint(window, cx);
    }
}

pub(crate) fn collect_image_alt_text(
    events_from_image_start: &[(Range<usize>, MarkdownEvent)],
    source: &str,
) -> Option<SharedString> {
    let mut alt_text = String::new();
    for (range, event) in events_from_image_start.iter().skip(1) {
        match event {
            MarkdownEvent::End(MarkdownTagEnd::Image) => break,
            MarkdownEvent::Text => alt_text.push_str(&source[range.clone()]),
            _ => {}
        }
    }
    if alt_text.is_empty() {
        None
    } else {
        Some(alt_text.into())
    }
}

pub(crate) fn image_fallback_element(
    dest_url: SharedString,
    alt_text: Option<SharedString>,
    open_image_url_on_click: bool,
) -> AnyElement {
    let link_label = alt_text
        .filter(|alt| !alt.is_empty())
        .unwrap_or_else(|| dest_url.clone());

    let label = format!("Failed to Load: {link_label}");

    div()
        .id("image-fallback")
        .min_w_0()
        .child(Label::new(label).color(Color::Warning).underline())
        .tooltip(Tooltip::text(
            "Image failed to load. Open `ed: log` for more details.",
        ))
        .when(open_image_url_on_click, |this| {
            this.cursor_pointer()
                .on_click(move |_, _, cx| cx.open_url(&dest_url))
        })
        .into_any_element()
}

pub(crate) fn apply_heading_style(
    mut heading: Div,
    level: pulldown_cmark::HeadingLevel,
    custom_styles: Option<&HeadingLevelStyles>,
    border_color: Option<Hsla>,
) -> Div {
    heading = match level {
        pulldown_cmark::HeadingLevel::H1 => heading.text_3xl(),
        pulldown_cmark::HeadingLevel::H2 => heading.text_2xl(),
        pulldown_cmark::HeadingLevel::H3 => heading.text_xl(),
        pulldown_cmark::HeadingLevel::H4 => heading.text_lg(),
        pulldown_cmark::HeadingLevel::H5 => heading.text_base(),
        pulldown_cmark::HeadingLevel::H6 => heading.text_sm(),
    };

    heading = match level {
        pulldown_cmark::HeadingLevel::H1 => heading,
        _ => heading.mt_6(),
    };

    if let Some(border_color) = border_color {
        heading = match level {
            pulldown_cmark::HeadingLevel::H1 => {
                heading.pb_2().border_b_1().border_color(border_color)
            }
            pulldown_cmark::HeadingLevel::H2 => {
                heading.pb_1().border_b_1().border_color(border_color)
            }
            _ => heading,
        };
    }

    if let Some(style) = heading_level_style(level, custom_styles) {
        heading.style().text = style.clone();
    }

    heading
}

pub(crate) fn heading_level_style(
    level: pulldown_cmark::HeadingLevel,
    custom_styles: Option<&HeadingLevelStyles>,
) -> Option<&TextStyleRefinement> {
    let styles = custom_styles?;
    match level {
        pulldown_cmark::HeadingLevel::H1 => styles.h1.as_ref(),
        pulldown_cmark::HeadingLevel::H2 => styles.h2.as_ref(),
        pulldown_cmark::HeadingLevel::H3 => styles.h3.as_ref(),
        pulldown_cmark::HeadingLevel::H4 => styles.h4.as_ref(),
        pulldown_cmark::HeadingLevel::H5 => styles.h5.as_ref(),
        pulldown_cmark::HeadingLevel::H6 => styles.h6.as_ref(),
    }
}

pub(crate) fn render_copy_code_block_button(
    id: usize,
    code: String,
    markdown_id: EntityId,
) -> impl IntoElement {
    let id = ElementId::NamedChild(
        Arc::new(ElementId::from(("copy-markdown-code", markdown_id))),
        id.to_string().into(),
    );
    CopyButton::new(id, code)
}

impl IntoElement for MarkdownElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

pub enum AnyDiv {
    Div(Div),
    Stateful(Stateful<Div>),
}

impl AnyDiv {
    pub(crate) fn into_any_element(self) -> AnyElement {
        match self {
            Self::Div(div) => div.into_any_element(),
            Self::Stateful(div) => div.into_any_element(),
        }
    }
}

impl From<Div> for AnyDiv {
    fn from(value: Div) -> Self {
        Self::Div(value)
    }
}

impl From<Stateful<Div>> for AnyDiv {
    fn from(value: Stateful<Div>) -> Self {
        Self::Stateful(value)
    }
}

impl Styled for AnyDiv {
    fn style(&mut self) -> &mut StyleRefinement {
        match self {
            Self::Div(div) => div.style(),
            Self::Stateful(div) => div.style(),
        }
    }
}

impl ParentElement for AnyDiv {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        match self {
            Self::Div(div) => div.extend(elements),
            Self::Stateful(div) => div.extend(elements),
        }
    }
}

#[derive(Default)]
pub(crate) struct TableState {
    alignments: Vec<Alignment>,
    in_head: bool,
    pub(crate) in_cell: bool,
    row_index: usize,
    col_index: usize,
}

impl TableState {
    fn start(&mut self, alignments: Vec<Alignment>) {
        self.alignments = alignments;
        self.in_head = false;
        self.in_cell = false;
        self.row_index = 0;
        self.col_index = 0;
    }

    fn end(&mut self) {
        self.alignments.clear();
        self.in_head = false;
        self.in_cell = false;
        self.row_index = 0;
        self.col_index = 0;
    }

    fn start_head(&mut self) {
        self.in_head = true;
    }

    fn end_head(&mut self) {
        self.in_head = false;
    }

    fn start_row(&mut self) {
        self.col_index = 0;
    }

    fn end_row(&mut self) {
        self.row_index += 1;
    }

    fn start_cell(&mut self) {
        self.in_cell = true;
    }

    fn end_cell(&mut self) {
        self.in_cell = false;
        self.col_index += 1;
    }

    pub(crate) fn current_cell_alignment(&self) -> Option<Alignment> {
        if self.alignments.is_empty() {
            return None;
        }
        if self.in_head {
            return Some(Alignment::Center);
        }
        self.alignments.get(self.col_index).copied()
    }
}

pub(crate) fn alignment_to_text_align(alignment: Alignment) -> Option<TextAlign> {
    match alignment {
        Alignment::Left => Some(TextAlign::Left),
        Alignment::Center => Some(TextAlign::Center),
        Alignment::Right => Some(TextAlign::Right),
        Alignment::None => None,
    }
}

// The contents of loose list items are wrapped in a paragraph, so their task
// marker follows `Start(Paragraph)` rather than `Start(Item)`.
pub(crate) fn task_list_marker_for_item(
    events: &[(Range<usize>, MarkdownEvent)],
    item_index: usize,
) -> Option<(Range<usize>, bool)> {
    let next_index = item_index.checked_add(1)?;
    let marker_index = match &events.get(next_index)?.1 {
        MarkdownEvent::Start(MarkdownTag::Paragraph) => next_index.checked_add(1)?,
        MarkdownEvent::TaskListMarker(_) => next_index,
        _ => return None,
    };

    match events.get(marker_index)? {
        (range, MarkdownEvent::TaskListMarker(checked)) => Some((range.clone(), *checked)),
        _ => None,
    }
}

pub(crate) struct MarkdownInputHandler {
    markdown: Entity<Markdown>,
    rendered_text: RenderedText,
}

impl MarkdownInputHandler {
    pub(crate) fn new(markdown: Entity<Markdown>, rendered_text: RenderedText) -> Self {
        Self {
            markdown,
            rendered_text,
        }
    }
}

impl InputHandler for MarkdownInputHandler {
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        cx: &mut App,
    ) -> Option<UTF16Selection> {
        self.markdown.update(cx, |markdown, _cx| {
            let range = markdown.selection.start..markdown.selection.end;
            Some(UTF16Selection {
                range: self.rendered_text.utf16_range_for_source_range(range),
                reversed: markdown.selection.reversed,
            })
        })
    }

    fn marked_text_range(&mut self, _: &mut Window, _: &mut App) -> Option<Range<usize>> {
        None
    }

    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut App,
    ) -> Option<String> {
        if range_utf16.start > range_utf16.end {
            return None;
        }

        Some(
            self.rendered_text
                .text_for_utf16_range(range_utf16, adjusted_range),
        )
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        _: &str,
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        _: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut App) {}

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _: &mut Window,
        _: &mut App,
    ) -> Option<Bounds<Pixels>> {
        let source_range = self.rendered_text.source_range_for_utf16_range(range_utf16);
        self.rendered_text
            .bounds_for_source_range(source_range.clone())
            .into_iter()
            .next()
            .or_else(|| {
                self.rendered_text
                    .position_for_source_index(source_range.start)
                    .map(|(position, line_height)| Bounds {
                        origin: position,
                        size: size(px(0.), line_height),
                    })
            })
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut App,
    ) -> Option<usize> {
        let source_index = self
            .rendered_text
            .source_index_for_visible_position(point)?;
        Some(
            self.rendered_text
                .utf16_index_for_source_index(source_index),
        )
    }

    fn accepts_text_input(&mut self, _: &mut Window, _: &mut App) -> bool {
        false
    }
}
