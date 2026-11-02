# Tabular data preview

`tabular_data_preview` shows CSV-like files as a grid, opened through the shared
preview actions (see ui-layout.md).

## Column widths

- Every data column can be resized by dragging its right edge, from the header or
  from any row. The row-identifier column is pinned and not resizable.
- Double-clicking a column's header or its edge resets that column to its initial width.
- Widths are view state only: they are not persisted, and they reset when the
  number of columns changes (e.g. the file is edited to add a column).
- Built on `ui::Table` with `ColumnWidthConfig::Resizable`; for that mode the
  table draws its resize handles over the header and the body, while
  redistributable tables keep handles over the body only.
