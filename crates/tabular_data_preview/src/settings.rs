#[derive(Default, Clone, Copy)]
pub enum VerticalAlignment {
    /// Align text to the top of cells
    #[default]
    Top,
    /// Center text vertically in cells
    Center,
}

#[derive(Default, Clone, Copy)]
pub enum RowIdentifiers {
    /// Show original line numbers from the source file
    #[default]
    SrcLines,
    /// Show sequential row numbers starting from 1
    RowNum,
}

#[derive(Default, Clone, Copy, PartialEq)]
pub enum FilterSortOrder {
    /// Sort alphabetically (A→Z), then by number of occurrences descending within ties
    #[default]
    AlphaThenCount,
    /// Sort by number of occurrences descending, then alphabetically within ties
    CountThenAlpha,
}

#[derive(Clone, Default)]
pub(crate) struct TableViewSettings {
    pub(crate) vertical_alignment: VerticalAlignment,
    pub(crate) numbering_type: RowIdentifiers,
    pub(crate) filter_sort_order: FilterSortOrder,
    pub(crate) multiline_cells_enabled: bool,
}
