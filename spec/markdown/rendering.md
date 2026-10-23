# Markdown rendering

- `markdown` is the shared renderer (pulldown-cmark, selection, code
  highlighting via the language registry, optional raw HTML) used by hover
  popovers, completions docs, diagnostics, notifications, git and the preview.
  It knows nothing about diagrams: callers can take over closed fenced code
  blocks with `MarkdownElement::render_code_block_with`.
- Code blocks show a copy-on-hover button unless the caller hides it; there is
  no wrap button and no custom code-block renderer beyond that hook.
- Preview-only behaviour lives in `markdown_preview`: its style, and Mermaid
  diagrams (cache, Preview/Code tabs), plugged in through that hook.
- The preview also opens HTML files (toolbar button, project panel, Open
  Preview), rendered through the same HTML tag subset below as one HTML
  document, not as markdown. It is not a browser: no CSS, scripts or forms.
  Only markdown files auto-open in the preview.
- HTML in markdown (preview only; elsewhere it shows as raw text): a small
  tag lexer maps a whitelist of tags onto the same events markdown syntax
  produces, so there is one renderer and no HTML parser dependency. Tags may
  span markdown blocks (`<div align="center">`, markdown, `</div>`); markdown
  containers close HTML left open inside them. Supported: `p h1-6 div/center
  (align) br hr img (width/height) a b strong i em s del sub sup u ins mark
  code kbd blockquote ul ol li pre table thead tr th td details summary`.
  `<details>` is always expanded; `style`/`class`, comments, scripts and
  `<caption>` are dropped; unknown tags render their children; table cells
  have no `colspan`/`rowspan`.
- The preview is virtualized (`MarkdownElement::virtualized`): the element is
  its own scroll container (a `gpui::list`) whose items are the top-level
  blocks, so only visible blocks are built and laid out each frame. A reparse
  keeps the measured heights of blocks whose source (and footnote position)
  didn't change; edited blocks are remeasured in place so the scroll position
  holds. Selection, links and hit testing work on the visible blocks' text;
  Copy and select-all cover the whole document, with Copy's text derived
  from the parse instead of the layout. Other callers (popovers, docs) lay
  out the whole document in one element as before.
- Smoke-test documents for the preview live in `test/`.
