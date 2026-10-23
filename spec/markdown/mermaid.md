# Markdown diagrams

- Mermaid is compiled to SVG by `crates/mermaid`, which has no dependencies.
  It started as a copy of Ondine (a Rust port of Obsidian's Pele) and is owned
  here: Ondine and Pele are references for porting fixes, not upstreams to
  merge. It has no Cargo features: every diagram type is always compiled and
  renders; a diagram that fails to compile shows its source.
- `mermaid::render` takes a `mermaid::Theme` (opaque colors, chart series,
  font, stroke width, corner radius) and emits concrete values, so the SVG has
  no CSS variables or `color-mix()` and rasterizes with usvg as is. The preview
  fills the theme from the editor theme (colors flattened onto the editor
  background, accents as chart series).
- Layout measures text as monospace (0.6 em per glyph), so diagrams use
  `mermaid_font_family`, defaulting to the buffer font, with common system
  monospace fonts as fallbacks.
- Diagrams render at their natural size times the fence scale
  (```` ```mermaid 150 ````); wider ones scroll horizontally. There is no
  per-diagram zoom (ctrl/cmd+scroll was removed). Diagrams follow the
  preview's temporary font zoom (cmd +/-/0): they scale by current ÷
  configured preview font size, re-rendered (not stretched) on change, so at
  the configured size they are 1:1. Theme and zoom changes keep showing the
  old raster until the new one is ready.
