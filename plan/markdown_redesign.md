# Markdown redesign: plan and status

Goal: fewer dependencies, simpler code, better performance for markdown
rendering and the markdown preview. "Worse is better": accept trade-offs for a
smaller, ownable codebase. Durable decisions go to `spec/markdown/`
(`rendering.md`, `mermaid.md`); this file is the working plan.

## Decisions

- **Keep our own pulldown-cmark renderer** (`crates/markdown`). Vendoring
  gpui-kit's text module (`longbridge/gpui-kit`, `crates/base/src/text`) was
  rejected: ~35k lines coupled to gpui-kit's selection/motion/input
  infrastructure, its own highlighter (not our `language` registry), pinned
  to a crates.io gpui snapshot, still html5ever, no mermaid. Its ideas are
  worth taking: top-level block virtualization with `gpui::list`, per-block
  render caches, tail-only reparse when streaming.
- **Mermaid = `crates/mermaid`**, started from Ondine
  (https://github.com/wevm/ondine, commit `0b589ec`), a zero-dependency Rust
  port of Obsidian's Pele (https://github.com/obsidianmd/pele). It is *owned*
  code: Ondine/Pele are references to port fixes from, not upstreams to
  merge. Separate crate (not folded into `markdown_preview`) because it is a
  pure text→SVG function, ~125k lines that should compile once, in parallel,
  and keep its own pedantic lint config.
- **Diagram rendering path**: Ondine SVG → gpui `SvgRenderer` (resvg) →
  raster at the fence scale. Per-diagram zoom (ctrl/cmd+scroll, debounced
  re-raster, fit-to-width, reset) removed 2026-10-23: trackpad-only use.
  Diagrams instead re-render at the preview's cmd +/- font zoom. Native GPUI
  painting (crisp, selectable text) is a possible later step, not planned
  yet.
- **HTML in markdown**: a tiny tag lexer over pulldown-cmark's
  `Html`/`InlineHtml` events maps a whitelist of tags onto the existing
  markdown events, so there is one renderer (done, phase 2).

## Done

Committed as `c70a28c` (2026-10-23): phase 1 and the Mermaid swap below.

Phase 1 – trim and split `markdown`:
- Removed dead APIs: wrap button, `CodeBlockRenderer`/`CopyButtonVisibility`
  (now `MarkdownElement::hide_code_block_copy_button()`), code-span link
  callback, `on_mermaid_zoom`, links-only parse mode/`new_text`, `append`,
  `first_code_block_language`, `with_buffer_font`, `with_muted_text`,
  `copied_code_blocks`.
- Mermaid moved out of `markdown`: new hook
  `MarkdownElement::render_code_block_with(..)` + `ParsedMarkdown::code_blocks()`;
  the preview owns diagrams in `markdown_preview/src/mermaid_diagrams.rs`
  (`MermaidDiagrams` entity: cache keyed by contents, per-block view state,
  Preview/Code tabs; synced from the preview's markdown observer).
- Preview-only style moved to `markdown_preview/src/markdown_preview_style.rs`
  (`MarkdownFont`/`MarkdownStyle::themed` removed). `markdown` no longer
  depends on `settings`, `theme_settings`, `anyhow`, `mermaid_render`.
- `markdown.rs` (5.1k lines) split into `markdown.rs` (entity, parse,
  `ParsedMarkdown`), `element.rs`, `builder.rs`, `rendered_text.rs`,
  `style.rs`.

Mermaid swap:
- `crates/mermaid_render` + merman deleted (lockfile 836 → 801 packages);
  `crates/mermaid` added.
- `mermaid::render(source, options_json, now, &Theme)`: theme is a typed
  struct (`mermaid::Color`, `mermaid::Theme`, `Theme::monochrome`). Diagram
  modules still write internal `var(--_x)` placeholders;
  `svg/theme.rs::resolve` replaces them at the end of `render`, so output has
  no CSS variables or `color-mix()` (usvg supports neither, and stops
  reading a `style` attribute at a custom property). Edited vs Ondine:
  `lib.rs`, `svg/theme.rs`, `svg/root.rs`, `svg/cluster.rs`.
- All Mermaid diagram types render (the old allow-list is gone).
- `mermaid_font_family` now defaults to the buffer font (layout assumes
  monospace, 0.6 em per glyph).

Phase 2 – HTML (uncommitted as of 2026-10-23):
- `markdown/src/html.rs`: lexer + `HtmlConverter` (frame stack per open
  element, closed by markdown container ends; table column count by
  lookahead, short rows padded; `<pre>` → indented code block). New events
  `StartHtmlStyle`/`EndHtmlStyle` (underline, mark, code);
  `MarkdownTag::HtmlBlock { align }`; `Image { width, height }`. Paragraphs
  and headings inherit alignment from the text style stack.
- Deleted `markdown/src/html/` (html5ever, markup5ever_rcdom, stacksafe dep;
  lockfile 801 → 791 packages). `cargo test -p markdown` covers the lexer and
  conversion.

Phase 3 – virtualized preview (uncommitted as of 2026-10-23):
- `ParsedMarkdown::blocks`: events split at root block starts, each with a
  hash of its source; `MarkdownListState::sync` diffs hashes (common prefix /
  suffix) and remeasures or splices only the changed middle of the
  `ListState`.
- `MarkdownElement` split into `BlockRenderer` (renders an event range, shared
  with the list's item closure) and the element (input, mouse, actions). In
  virtualized mode each item is a `MarkdownBlock` that sets its cursor, takes
  autoscroll requests inside it, and reports its `RenderedText` as it paints;
  the element joins them for hit testing. Mouse listeners read that text
  lazily, so they still register before children (copy buttons, checkboxes
  rely on bubble order and `default_prevented`).
- Copy in the preview uses `ParsedMarkdown::text_for_range` (events only).
- Code block scroll handles are pruned per parse, not per frame.
- `RightClickMenu::size_full()` so the list gets a definite height.
- Preview scroll actions use the `ListState`; up/down by item now step by
  block.
- Not done: per-block render caching (elements are rebuilt every frame for
  the visible blocks), and avoiding `rope.to_string()` per edit.

Verified: `cargo check -p zed`, clippy clean on touched crates,
`cargo test -p markdown_preview` (1 test renders a themed flowchart), sample
diagrams (flowchart, sequence, pie, class, state, gantt) rasterized with resvg
and inspected. **Not verified in the running app** (preview, tabs,
copy buttons).

## Next

1. **Smoke-test in the app**: open a markdown file with several diagrams,
   check rendering, theme switching (re-render), wide diagrams scroll
   horizontally, Preview/Code tabs, copy buttons, editing a diagram (previous raster
   shown while re-rendering), plain code-block copy button in preview and
   hidden in hover popovers. Also a README with HTML (centered logo and
   badges, `<details>`, HTML tables, `<kbd>`).
2. **Phase 3 follow-ups**: smoke-test the virtualized preview (scrolling,
   drag-select across blocks with autoscroll, search jumps, heading links,
   editor cursor sync, Copy of an off-screen selection, page/item scroll
   keys, font size changes); profile a large document before adding
   per-block caches; avoid the preview's `rope.to_string()` copy per edit if
   it shows up in profiles.
3. **`crates/mermaid` cleanup** (owned code now):
   - typed options struct instead of the JSON string (`options_from_json`,
     `yaml` JSON path);
   - drop browser-only features (link/image schemes, `idPrefix`,
     accessibility title/desc, `responsive`);
   - drop the `model` feature's serializers (~80 `cfg(feature = "model")`
     sites), which only Ondine's parity tests read; those tests were deleted
     2026-10-23 (they needed fixtures that were never copied);
   - optionally a text-measure callback so layout can use real font widths.
4. **Tidy `markdown`**: the split widened many fields/methods to
   `pub(crate)`; tighten where possible. `element.rs::request_layout` is still
   ~700 lines; break it into per-tag functions.

## Gotchas

- Format only touched files (`rustfmt --edition 2024 <files>`), never
  workspace `cargo fmt`. For `crates/mermaid` files pipe through stdin
  (`rustfmt --edition 2024 --emit stdout < f`): rustfmt follows `mod`
  declarations, and the crate keeps Ondine's wider formatting.
- usvg ignores CSS custom properties and `color-mix()`, and stops parsing a
  `style` attribute at the first `--x:` declaration. It also rejects
  `letter-spacing`/`word-spacing: normal` (warns per diagram); the root font
  style writes `0` instead.
- The rasterizer sees only system fonts, hence the monospace fallback stack
  in the preview's `mermaid_theme`.
