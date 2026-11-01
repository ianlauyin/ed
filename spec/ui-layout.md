# UI layout

## Decision

The editor area and the terminal float as opaque rounded cards on a single
opaque chrome colour. It is always on, with no setting, and is done the simplest
way: theme colour tweaks plus a shared card style, with no changes to GPUI's
clipping. A translucent, blurred liquid-glass variant was tried and dropped,
because it hurt readability; an opaque window is also cheaper to composite.

## Theme

- The theme is constant: Nord (dark), a fully resolved Rust struct in
  `theme::built_in` (like `settings::built_in`). Every colour is spelled out,
  including the floating-card chrome, so there is no theme JSON, no
  light/dark default palette, no refinement or fallback derivation, and no
  `theme_settings` crate. To change colours, edit `theme::built_in` and rebuild.
- There are no Vim/Helix theme colours (`vim_*`).
- There is no theme registry, theme picker, light/dark mode toggle, `Settings`
  app-menu submenu, or theme settings (`theme`, `markdown_preview.theme`,
  `theme_overrides`, `experimental.theme_overrides`).
- The theme never changes at runtime, so nothing observes theme changes and the
  OS light/dark appearance is not tracked. Native window chrome is forced to the
  theme's appearance.

## Fonts

- Fonts are constants in `ui::font`, not settings: UI font `.SystemUIFont`
  16px with ligatures off, buffer font `JetBrainsMono Nerd Font` 15px, both
  weight 400. Mermaid diagrams use the buffer font family. To change them, edit
  `ui::font` and rebuild.
- Font families never change at runtime; only sizes change, through in-memory
  zoom that resets on restart. `ed::{Increase,Decrease,Reset}UiFontSize`,
  `ed::{Increase,Decrease,Reset}BufferFontSize`, and `ed::ResetAllZoom` take
  no arguments and never write settings. Sizes clamp to 6–100px.
- The markdown preview (default: UI font size) and the git commit editor
  (default: 12px) zoom independently of the buffer when focused.
- `ui` reads fonts directly; `theme` provides no font settings.

## Behaviour

- The chrome is the theme's `panel_background`; `background`, the title bar
  colours and `status_bar_background` are set to it.
- Card content is derived from the chrome colour instead of a new theme key:
  the editor, gutter, toolbar and terminal backgrounds are shifted together so
  the editor sits 5 points of HSL lightness below the chrome (above it in light
  themes). The shift only ever adds contrast; themes that already have more are
  left alone. Opaque active-line colours shift too; translucent ones already
  follow the background.
- Each pane is its own card: its tab bar sits on the chrome, and everything
  below the tab bar is the card (`editor_background` fill, 10px radius, no
  border). Tab bars and inactive tabs take the chrome colour, so only the active
  tab stands out: it takes the card colour and joins the card below it, like a
  browser tab. This replaced cards that wrapped the tab bar, where inactive tabs
  shared the card colour and looked continuous with the active one.
- GPUI clips to rectangles only, so the card's content sits inside a 3px inset
  and the card paints a card-coloured ring over its content that fills the
  inset with an inner edge concentric with the card (10px − 3px). Anything
  painted into a content corner (active line, minimap thumb and markers, diff
  backgrounds) is trimmed to the card's curve by that one mask, so content
  never does its own corner rounding. The mask sits below the drag-target
  overlay, whose border runs along the card edge.
- There is no title bar row; the window's top row is the sidebar and the tab
  bars, to give the content the most room. The window header is one group:
  empty room for the macOS traffic lights, the project name and alerts (such as
  restricted mode), Go Back / Go Forward, then an icon-only, segmented
  Workspace / Git History switch (shown only for a project in a git
  repository). It is outlined like the branch picker's Branches / Stashes
  switch: a thin border, a divider between segments, and the shown mode's icon
  in the accent colour rather than a filled segment. It sits on a strip above the
  sidebar (the left dock, or the Git History sidebar), so sidebar content lines
  up with the cards and tab bars start straight with tabs. With no sidebar
  shown, or while something is zoomed, the same group moves to the start of the
  top-left pane's tab bar, at the same x, so nothing jumps when the sidebar is
  toggled. A zoomed dock covers the panes, so it gets a header row of its own.
- Tab bars are 36px high. Top-row tab bars, the sidebar header, an open right
  dock and a zoomed dock's header keep the 8px card gap from the window's top
  edge (the gap moves the window). Panel header rows one tab high (the outline
  panel's search bar) thus line up with the tab row, their bottom line level
  with the tabs' bottom edge, so the tabs
  sit as far from the edge as the cards do from the sides. macOS doesn't fix
  the titlebar height for a transparent, full-size-content window: the row is
  ours. The traffic lights are placed at (16, 19) (`ui::utils::TRAFFIC_LIGHT_POSITION`):
  centred in the tab row below the gap, and far enough from the left edge to
  clear the window's rounded corner. At (9, 9) they looked jammed into it.
- The header strip, the traffic light room and the empty tab bar space of every
  pane in the top row (columns of side-by-side splits, the first row of
  stacked ones) act as a titlebar: dragging moves the window and a double click
  zooms or minimises it as macOS is set up to. The gap above a top-row tab
  bar does the same. Lower panes' empty tab bar space
  keeps its double-click action. An empty top-row pane still draws its tab bar
  for this, instead of the blank strip.
- The status bar starts, in every mode, with the worktree and branch pickers
  and a divider; they show only for a project in a git repository.
- Modes are strictly isolated for rendering. Only the window header and status bar
  (plus notifications, toasts and modals) are shared; everything between them
  belongs to one mode, rendered by that mode's own cached view, and only the
  shown mode's view is drawn. Changes in a hidden mode redraw nothing (it
  re-renders when shown again), and redraws of the shared chrome reuse the
  shown mode's layout. Status items hidden in the current mode get no active
  item, so they don't track it. A new mode (e.g. a terminal-only mode) adds its
  own view and module rather than branching inside another mode's layout.
- In Git History mode (`git.md`) the history sidebar and panes fill the area of
  the docks and center panes. The status bar stays, so the card's bottom edge
  doesn't move when switching modes. The sidebar shows the active history
  item's `Item::side_card` and is styled like a left dock (flat, resizable, 8px
  gap before the panes), so the tab bars start after it rather than above it.
- Docks have no border and blend into the chrome. Panes inside them (the
  terminal) draw their own cards; a non-pane panel docked at the bottom just
  sits on the chrome.
- Split panes are sections of one set of tabs, not separate editors: a tab
  (a file, a commit, a revision) lives in exactly one pane, and the same file is
  never open in two panes. Opening something already open reveals and focuses
  its tab wherever it is; the pane passed by the caller (or the active pane)
  only decides where new tabs go. "Open to the side" on an open file reveals it
  rather than splitting. Splitting (`pane::Split*`, dragging
  a tab to an edge) moves the active tab into the new pane, so it needs another
  tab left behind; nothing is cloned. Dragging a pane's only tab over that
  same pane offers no split zones (every pane, terminal included), since the
  drop would just move the pane.
- One back/forward history covers every center and Git History pane. Going back
  or forward reveals the entry's tab in whichever pane holds it, switching mode
  if needed; a closed file reopens in the last active center pane. Moving focus
  from one pane to another records where the user was in the pane they left,
  so going back returns there. `workspace::GoBack` / `workspace::GoForward`
  are workspace actions, so they work wherever focus is in the window (the
  terminal still keeps `ctrl-_` for itself). The Go Back / Go Forward buttons sit
  only in the window header (see above), not on tab bars.
  The `tab_bar.show_nav_history_buttons` and `reveal_if_open`
  settings are removed. The terminal panel keeps its own panes and history.
- Split panes are separate cards with an 8px gap between them (the chrome shows
  through) instead of a divider line. The resize handle covers the gap.
  Finding the neighbouring pane (pane navigation, "open to the side") looks
  across the gap, so it reuses an existing pane rather than splitting again.
- Inactive-pane dimming (`active_pane_modifiers.inactive_opacity`) and the
  active-pane border cover only the rounded card, never the tab bar, so tab
  bars stay the plain chrome colour in every pane.
- A zoomed view (editor or any dock) covers exactly the area the cards occupy,
  on the chrome, overlaying the others, so every zoomed view has the same size.
  The `zoomed_padding` setting is removed. The status bar is hidden while
  something is zoomed, so the zoomed view reaches down to the bottom gap. The
  window header moves into the zoomed view's top row, since the macOS window
  controls sit there.
- A zoomed dock (e.g. an expanded terminal) belongs to Workspace mode: Git
  History mode hides it and shows the status bar, and switching back to
  Workspace focuses it again, so it is still open and zoomed rather than
  closed.
- An 8px gap separates the cards from the window edges and from open docks.
  There is no gap at the top: tab bars and the sidebar header start at the
  window's top edge. When the status bar is shown, it replaces
  the bottom gap and pads its content evenly above and below, so the icons sit
  centred between the card and the window edge.
- Status bar items have no "Hide Button" right-click entry; they are hidden only
  through their settings. Items without other menu entries (search, cursor
  position, language, diagnostics, …) have no right-click menu at all; dock
  panel buttons keep theirs for dock position only.
- Status bar buttons that open a pane item (project search, project
  diagnostics, Git History's graph) turn accent while that item is the active
  tab (`status_item_color`). Diagnostics keeps its error/warning icon colours;
  its check icon and counts take the accent.
- Docks always have a fixed size in pixels (width for left/right, height for
  bottom). There is no proportional "flex width" mode and no
  `terminal.flexible` setting. A side dock may shrink when space is tight but
  springs back to its stored width.
- Tabs fill the tab bar's full height with rounded top corners and square
  bottoms. The tab bar has no bottom line and no borders between tabs. It is
  inset 10px on the left (the card radius), so the first tab meets the card's
  straight top edge rather than its rounded corner.
- A tab's close button (editor and terminal tabs alike) is quiet: no fill on
  hover or press and no tooltip; only the muted × brightens to full contrast
  on hover. A filled hover chip plus a "Close" tooltip outweighed the tab
  title.
- The terminal panel's panes use pill tabs instead: the whole pane, tab bar
  included, is one card, and tabs are rounded buttons inside its top edge.
  Pills have no fill: the active pill gets full-contrast text, inactive ones are muted, and
  any pill shows `ghost_element_hover` on hover. A plain terminal icon is
  accented only on the active pill of the focused pane, and the active label
  goes muted when the pane loses focus, so the terminal draws attention only
  while you are typing in it. Task status icons keep their own colours. A
  filled active pill drew too much attention, especially with a single
  terminal, where there is nothing to tell it apart from. Split drop zones
  cover only the content below the pills, and inactive-pane dimming covers the
  whole card.
- Drag-and-drop targets everywhere (panes, project panel) take their colours
  from the theme's `text_accent`: `ThemeColors::drop_target_border()` is the
  accent and `drop_target_background()` a faint wash of it. Themes cannot set
  them separately; the old `drop_target.background` / `drop_target.border`
  theme keys are gone, since the bundled themes set them to a flat grey. A position in the tab bar (onto a tab, or the
  empty strip after the tabs or after the pinned tabs) shows a thin rounded
  accent bar in the gap where the tab will land: after the target when a tab
  moves right within its pane, before it otherwise, nothing when it would not
  move. A content split zone is an area, so it shows a rounded box (card
  radius) with a 1px accent outline and a faint accent fill. The project
  panel draws the same box around a dragged-over folder and its visible rows
  (`project-panel.md`).
- Pane tab bars (center panes, Git History and the terminal panel) have no
  split buttons or split menu: panes are split by dragging a tab to a pane
  edge, or with the `pane::Split*` actions (keys, app menu). The
  `SplitRight` / `SplitDown` icons stay in the icon set. The remaining
  buttons come from one shared `pane::render_tab_bar_pane_buttons`, so every
  tab bar behaves the same. There is no search button: search is opened with
  `cmd-f`, the menu or the action. Left of Zoom, an eye button opens the Markdown/HTML, SVG or
  tabular data preview of the active file (Option-click opens it in a split),
  shown only for files that have one. Center (editor) panes and the terminal
  panel put a "+" before them: `workspace::NewFile` (an untitled file) and
  `NewTerminal`. A top-row tab bar's empty space is a titlebar, so its double
  click can't create one.
- Previews go through `workspace::preview`: each preview crate registers a
  `PreviewProvider` (a tooltip for items it can preview, and how to open
  one); the first that matches an item wins. One pair of actions,
  `preview::OpenPreview` (`cmd-shift-v` in any editor) and
  `preview::OpenPreviewToTheSide`, serves every file type, as do the tab bar
  button and the "Open Preview" entries in the editor and tab context menus; a
  new preview type only registers a provider. A pane handles the actions only
  while its active item has a preview, so the shortcut falls through elsewhere
  and menus just check whether the action is available. An editor's language
  change updates its tab, so the button appears once a new file's language is
  detected. Following previews (`markdown::OpenFollowingPreview`,
  `svg::OpenFollowingPreview`) stay type-specific.
  Zoom stays rightmost. Multibuffers have no collapse/expand all files button:
  that is `editor::ToggleFoldAll` (cmd-shift-enter in search bars) or View >
  Fold All / Unfold All, so a multibuffer with nothing else to show (e.g. find
  references) gets no toolbar row. While a buffer or project search input has
  focus, these fold actions go to the searched editor or results, not the input.
  Only the terminal panel has a `+`: a plain button that opens a terminal in
  that pane. Center and Git History panes have no `+` or new-item menu; new
  files, project/symbol search and terminals come from their keybindings and
  the command palette. Every terminal
  pane gets these buttons in `new_terminal_pane`, so panes restored from a
  session or created by moving a tab into a split get them too.
- Pane toolbars (search bars, diff controls) have no bottom line either; they
  sit flat inside the card. Right-side toolbar items (e.g. image viewer
  controls) stay right-aligned even when nothing is on the left. Editor panes have no breadcrumbs or quick action
  bar (see editor.md).
- An empty pane leaves a blank strip of tab-bar height
  above its card, so the card does not jump when the first tab opens or the
  last one closes.
- An empty pane keeps its card and shows centred keybinding hints (Open File,
  Go to File, Command Palette), styled like the project search landing page.
  Bindings are read from the keymap. This applies to every empty pane, whether
  or not a project is open (there is no welcome page).
- GPUI only clips to rectangles, so the card insets its content by 3px on every
  side, keeping square content corners (editor background, scrollbars) inside
  the rounded edge. If the radius grows, the inset must grow so that
  inset >= radius × (1 − 1/√2).
- Symbols in the outline panel render in the UI
  font at the project panel's label size, not the buffer font. Syntax highlight
  colours are kept, but theme font weight and style are dropped, so every entry
  (e.g. bold markdown headings) uses the same weight and style.
- In multi-buffer views (search results, diffs) the outline panel always shows
  excerpts, symbols and search matches under each file. The upstream
  `ToggleSymbols` action, its filter-bar button and the
  `outline_panel.multi_buffer_hide_symbols` setting are removed.
- There is a single UI density. The upstream `unstable.ui_density` setting
  (compact/comfortable) is removed, and `DynamicSpacing` is a plain enum whose
  `BaseXX` variant is XX px at a 16px rem; `px()` scales it with the UI font
  size.
