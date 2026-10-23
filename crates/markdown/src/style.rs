use super::*;

/// A callback function that can be used to customize the style of links based on the destination URL.
/// If the callback returns `None`, the default link style will be used.
type LinkStyleCallback = Rc<dyn Fn(&str, &App) -> Option<TextStyleRefinement>>;

#[derive(Clone, Copy, Default)]
pub struct BlockQuoteKindColors {
    pub note: Hsla,
    pub tip: Hsla,
    pub important: Hsla,
    pub warning: Hsla,
    pub caution: Hsla,
}

impl BlockQuoteKindColors {
    pub(crate) const fn for_kind(&self, kind: Option<BlockQuoteKind>, default: Hsla) -> Hsla {
        match kind {
            Some(BlockQuoteKind::Note) => self.note,
            Some(BlockQuoteKind::Tip) => self.tip,
            Some(BlockQuoteKind::Important) => self.important,
            Some(BlockQuoteKind::Warning) => self.warning,
            Some(BlockQuoteKind::Caution) => self.caution,
            None => default,
        }
    }
}

#[derive(Clone, Default)]
pub struct HeadingLevelStyles {
    pub h1: Option<TextStyleRefinement>,
    pub h2: Option<TextStyleRefinement>,
    pub h3: Option<TextStyleRefinement>,
    pub h4: Option<TextStyleRefinement>,
    pub h5: Option<TextStyleRefinement>,
    pub h6: Option<TextStyleRefinement>,
}

#[derive(Clone)]
pub struct MarkdownStyle {
    pub base_text_style: TextStyle,
    pub container_style: StyleRefinement,
    pub code_block: StyleRefinement,
    pub code_block_overflow_x_scroll: bool,
    pub inline_code: TextStyleRefinement,
    pub block_quote: TextStyleRefinement,
    pub link: TextStyleRefinement,
    pub link_callback: Option<LinkStyleCallback>,
    pub rule_color: Hsla,
    pub block_quote_border_color: Hsla,
    pub block_quote_kind_colors: BlockQuoteKindColors,
    pub syntax: Arc<SyntaxTheme>,
    pub selection_background_color: Hsla,
    /// Background behind the rendered markdown, used to blend translucent highlights.
    pub highlight_base_background: Hsla,
    /// Minimum APCA contrast for text inside search/selection highlights; 0 disables it.
    pub minimum_highlight_contrast: f32,
    pub heading: StyleRefinement,
    pub heading_level_styles: Option<HeadingLevelStyles>,
    pub heading_border_color: Option<Hsla>,
    pub paragraph_spacing: Pixels,
    pub paragraph_line_height: DefiniteLength,
    /// Bottom margin of top-level lists only
    pub list_spacing: Pixels,
    /// Horizontal (`x`) and vertical (`y`) padding of table cells
    pub table_cell_padding: Point<Pixels>,
    pub height_is_multiple_of_line_height: bool,
    pub prevent_mouse_interaction: bool,
    pub table_columns_min_size: bool,
    pub soft_break_as_hard_break: bool,
}

impl Default for MarkdownStyle {
    fn default() -> Self {
        Self {
            base_text_style: Default::default(),
            container_style: Default::default(),
            code_block: Default::default(),
            code_block_overflow_x_scroll: false,
            inline_code: Default::default(),
            block_quote: Default::default(),
            link: Default::default(),
            link_callback: None,
            rule_color: Default::default(),
            block_quote_border_color: Default::default(),
            block_quote_kind_colors: Default::default(),
            syntax: Arc::new(SyntaxTheme::default()),
            selection_background_color: Default::default(),
            highlight_base_background: Default::default(),
            minimum_highlight_contrast: 0.,
            heading: Default::default(),
            heading_level_styles: None,
            heading_border_color: None,
            paragraph_spacing: px(8.),
            paragraph_line_height: rems(1.3).into(),
            list_spacing: px(0.),
            table_cell_padding: point(px(4.), px(2.)),
            height_is_multiple_of_line_height: false,
            prevent_mouse_interaction: false,
            table_columns_min_size: false,
            soft_break_as_hard_break: false,
        }
    }
}
