use gpui::AnyElement;
use gpui::Entity;
use ui::ColumnWidthConfig;
use ui::ResizableColumnsState;
use ui::Table;
use ui::UncheckedTableRow;
use ui::div;
use ui::prelude::*;

use crate::TableView;
use crate::types::AnyColumn;
use crate::types::DataCellId;
use crate::types::DisplayCellId;
use crate::types::DisplayRow;

impl TableView {
    pub(crate) fn create_table(
        &self,
        current_widths: &Entity<ResizableColumnsState>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let row_count = self.engine.d2d_mapping().visible_row_count();
        let cols = current_widths.read(cx).cols();
        let mut headers = Vec::with_capacity(cols);

        headers.push(self.create_row_identifier_header(cx));

        for i in 0..(cols - 1) {
            let header_text = self
                .engine
                .contents
                .headers
                .get(AnyColumn(i))
                .and_then(|h| h.display_value().cloned())
                .unwrap_or_else(|| format!("Col {}", i + 1).into());

            headers.push(self.create_header_element_with_sort_button(header_text, cx, AnyColumn::from(i)));
        }

        Table::new(cols)
            .interactable(&self.table_interaction_state)
            .width_config(ColumnWidthConfig::Resizable(current_widths.clone()))
            .header(headers)
            .disable_base_style()
            .pin_cols(1)
            .variable_row_height_list(
                row_count,
                self.list_state.clone(),
                cx.processor(move |this, display_row: usize, _window, cx| {
                    // The mapping may be transiently stale while a filter/sort task is in-flight. Return an empty row rather than panicking
                    Self::render_single_table_row(this, cols, DisplayRow(display_row), cx).unwrap_or_default()
                }),
            )
            .into_any_element()
    }

    /// Render a single table row
    fn render_single_table_row(
        this: &TableView,
        cols: usize,
        display_row: DisplayRow,
        cx: &Context<TableView>,
    ) -> Option<UncheckedTableRow<AnyElement>> {
        // Get the actual row index from our sorted indices
        let data_row = this.engine.d2d_mapping().get_data_row(display_row)?;
        let row = this.engine.contents.get_row(data_row)?;

        let mut elements = Vec::with_capacity(cols);
        elements.push(this.create_row_identifier_cell(display_row, data_row, cx)?);

        // Remaining columns: actual table data
        for col in (0..this.engine.contents.number_of_cols).map(AnyColumn) {
            let table_cell = row.expect_get(col);

            // TODO: Introduce `<null>` cell type
            let cell_content = table_cell.display_value().cloned().unwrap_or_default();

            let display_cell_id = DisplayCellId::new(display_row, col);
            let data_cell_id = DataCellId::new(data_row, col);

            let is_focus_cell = this.selection.as_ref().is_some_and(|selection| selection.focus == data_cell_id);

            let cell_bg = if is_focus_cell { Some(cx.theme().colors().element_selected) } else { None };

            let cell = div()
                .size_full()
                .when(!this.settings.multiline_cells_enabled, |div| {
                    div.whitespace_nowrap().text_ellipsis().h(this.row_height).overflow_hidden()
                })
                .child(TableView::create_selectable_cell(
                    display_cell_id,
                    cell_content,
                    this.settings.vertical_alignment,
                    cx,
                ));

            elements
                .push(div().size_full().when_some(cell_bg, |div, color| div.bg(color)).child(cell).into_any_element());
        }

        Some(elements)
    }
}
