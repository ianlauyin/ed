# UI layout

## Decision

The editor area and the terminal float as opaque rounded cards on a single
opaque chrome colour. It is always on, with no setting, and is done the simplest
way: theme colour tweaks plus a shared card style, with no changes to GPUI's
clipping. A translucent, blurred liquid-glass variant was tried and dropped,
because it hurt readability; an opaque window is also cheaper to composite.

## Theme

- The theme is constant: One Dark, built in Rust in `theme::built_in`, so
  there is no theme JSON, no light/dark default palette, no refinement or
  fallback derivation, and no `theme_settings` crate. To change colours, edit
  `theme::built_in` and rebuild.
- `theme::built_in` is the single origin of every colour, in three layers:
  the One Dark palette (raw hex values, plus a few syntax-only shades), role constants derived from it
  by formula (`shade` scales RGB channels, `mix` flattens a translucent colour
  onto a surface, alpha via `Hsla::alpha`), and theme fields that reference
  roles only. No chrome or terminal field holds a colour from outside the palette (except
  the macOS traffic lights).
- The theme is one lazily built `static`: `built_in::theme()` and `cx.theme()`
  return `&'static Theme`. There is no theme global, no `theme::init`, and no
  `Arc` around the theme, the syntax theme or the accents (a fixed array);
  holders keep a `&'static SyntaxTheme`. Tests cannot install another theme.
- Terminal bright / dim ANSI colours are the normal colour shaded +20% / -30%.
- `StatusColors` only has the fields the UI reads; unused `_background` /
  `_border` variants and `predictive` are gone.
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
- Cards are darker than the chrome: the editor, gutter, toolbar, terminal and
  active-tab backgrounds are one role, the chrome shaded -33% (about 5 points
  of HSL lightness lower). The active line is translucent, so it follows the
  card.
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
  bars, to give the content the most room. The window header is one group,
  read left to right as "where, then how": empty room for the macOS traffic
  lights, the project name (the active folder, see `git.md`), the
  worktree and branch pickers as `worktree / branch` (shown only for a project
  in a git repository), then an icon-only, segmented mode switch, then Go
  Back / Go Forward. The switch offers Editor (`cmd-1`, `mode::Editor`), Git
  History (`cmd-2`, `mode::GitHistory`, git only) and Terminal (`cmd-3`,
  `mode::Terminal`), in that order, and is
  shown whenever more than one mode is available; the keys work in any mode. The pickers moved here from
  the status bar so everything that names the project sits together; their
  menus open downward. The switch groups its icon buttons on a faint, borderless
  track (2px inset) so the modes read as one control as more are added; the
  shown mode's button is filled with its icon in the accent colour. The
  buttons keep the top-row width and 16px icon but are 22px high, so the 26px
  track sits on the row's centre line with clear space above and below; a
  track of full-height buttons filled the row and nearly touched the card
  below. An outlined switch looked heavier.
  Every header text button (project, worktree, branch) shares one
  style (`title_bar::worktree_branch::header_text_button`): 28px high like the
  icon buttons, the tab label's size and regular weight (semibold was too
  loud), with a 14px leading icon. The project name is full contrast; worktree
  and branch are muted, so the row reads as a breadcrumb. The branch icon keeps
  its change-status colour. While its menu is open, each gets the accent tint
  and an accent label.
  Every top-row icon button (Go Back / Go Forward, the switch, and the tab bar
  ends' New File, preview and Zoom) is a 16px icon in a 28px square (the
  switch's are shorter, above)
  (`workspace::pane::top_row_icon_button`), 4px apart, so both ends of the row
  match; the header's controls are 8px from the project name and from each
  other's group. It sits on a strip above the
  sidebar (the left dock, or the Git History sidebar), so sidebar content lines
  up with the cards and tab bars start straight with tabs. The header is drawn
  over the mode's content, never clipped by the sidebar: when the sidebar is
  narrower than the header, the header overhangs it and the top-left tab bar
  keeps room before its tabs (which moves the window), so the tabs stop where
  they sit with the sidebar hidden and hold still when it is toggled; only the
  cards below follow the sidebar's edge. With no sidebar
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
  ours. The top row is laid out like every other pane row: the card gap, then
  the tab row, then the card. Everything in it (the traffic lights, the window
  header, tab labels, the tab bars' buttons) sits on the tab row's centre line,
  with no per-element offsets, the browser-tab layout. Centring the controls
  in the whole strip from the window's edge instead (like native toolbars) was
  tried: with tabs joined to the card, tab labels can't share that line
  (they're half the gap lower), so either the labels sat high in their tabs or
  the controls and labels split onto two lines. Getting the native look
  properly would need pill tabs that don't join the card. `CARD_GAP` is in
  `ui::utils`, and the traffic light position is derived from it and the tab
  row height: (16, 19), far enough from the left edge to clear the window's
  rounded corner (at (9, 9) they looked jammed into it).
- Only the red close button of the traffic lights is shown; minimize and zoom
  are hidden (also in fullscreen) so the header starts right after it, its
  first text as far from the button as the button is from the window's left
  edge (16px) for visual balance. They
  stay reachable through `cmd-m`, `ctrl-cmd-f` and a titlebar
  double click. This applies to every window with a custom traffic light
  position.
- The header strip, the traffic light room and the empty tab bar space of every
  pane in the top row (columns of side-by-side splits, the first row of
  stacked ones) act as a titlebar: dragging moves the window and a double click
  zooms or minimises it as macOS is set up to. The gap above a top-row tab
  bar does the same. Lower panes' empty tab bar space
  keeps its double-click action. An empty top-row pane still draws its tab bar
  for this, instead of the blank strip.
- The status bar has no worktree or branch pickers; they are in the window
  header.
- A window can have a solid 2px border in one of the theme's accent colours
  (`built_in::accent_colors`), to tell windows apart. It is picked from a
  row of 16px colour circles, close together and starting on the menu's
  text edge like Finder's tags (centred or spread over the width, they
  ignored the edge everything else hangs off), on the last row of the project
  menu (the header's project name), below a separator; the picked circle is
  ringed and clicking it again clears the border. The menu stays open, so
  colours can be tried. The colour is per project (`.ed/workspace.json`); a
  window without a root folder keeps it only until it closes. The border is
  painted over the chrome's edge rather than laid out, so picking one moves
  nothing, and it follows the macOS window corner (16px radius, square in
  fullscreen). Tinting the whole chrome would be more visible but needs a
  per-window theme, which the single static theme rules out.
- Each pane's tab bar (with the window header inside it) is its own cached
  view, apart from the pane, which re-renders with every frame of its active
  item. The tab bar re-renders when the pane notifies, when something inside
  it notifies, and when git statuses change (tab labels are coloured by
  them). Anything else a tab shows must reach it through a pane notification
  (`ItemEvent::UpdateTab`), as it already had to for inactive panes.
- The status bar is a cached view, so frames that only change the editor
  don't re-render it. A cached view isn't measured from its content, so its
  height is fixed: a default-sized button plus the even gap above and below.
  Status items must not be taller than a default button. Items that follow
  the active editor observe every editor notify, so they (and anything else
  fed by frequent events, like diagnostics summaries and scrollbar markers)
  notify only when what they show changed.
- Modes are strictly isolated for rendering. Only the window header and status bar
  (plus notifications, toasts and modals) are drawn outside the modes; everything between them
  belongs to one mode, rendered by that mode's own cached view, and only the
  shown mode's view is drawn. Changes in a hidden mode redraw nothing (it
  re-renders when shown again), and redraws of the shared chrome reuse the
  shown mode's layout. A new mode adds its own view and module rather than
  branching inside another mode's layout.
- In Git History mode (`git.md`) the history sidebar and panes fill the area of
  the docks and center panes. The status bar is hidden there (it offers too
  little in history to be worth the space), leaving the bottom gap. The sidebar shows the active history
  item's `Item::side_card` and is styled like a left dock (flat, resizable, 8px
  gap before the panes), so the tab bars start after it rather than above it.
- Terminal mode (`cmd-3`) fills the area of the docks and center panes with
  terminal panes, split like editor panes; the status bar is hidden, as in Git
  History. The workspace owns these panes (like Git History's), apart from
  the terminal panel's; the terminal crate plugs in through
  `TerminalModeDelegate` (pane setup, opening shells, reading a terminal's
  directory), since it depends on `workspace`.
  - The two terminal sets have separate jobs: terminal mode for work in
    shells, the bottom dock (`cmd-j`) for a quick shell and task output beside
    the code. Tasks always run in the dock.
  - Showing the mode with no terminals starts a shell, except that a repeated
    `cmd-3` while it is shown does not. Closing the last terminal keeps one
    empty pane offering "New Terminal" (`EmptyPaneHints::Terminal`) instead of
    leaving the mode. The first time the mode is shown in a window it restores
    the saved layout instead (`configuration.md`).
  - The top row holds only the window header, on a strip like the sidebar
    header (it moves the window), so the header sits exactly where it does in
    the other modes and the cards start at the same height. Below it, panes
    use the terminal panel's pill tabs inside their cards; their tab bars are
    not titlebar space. Folder tabs looked like editor tabs, and pills in a
    top-row tab bar shifted the header by the card inset.
  - Tab bars are the exception to
    "no split buttons": `+` (new shell in the project's default directory),
    Split Right (`cmd-d`), Split Down (`cmd-shift-d`), then Zoom. A split
    (button, `pane::Split*`) opens
    a new pane with a shell in the active terminal's working directory,
    focused, on the side asked for; only a move split of a pane with another
    tab left behind moves the tab instead. Dragging a tab to an edge moves it,
    as elsewhere.
  - `workspace::NewTerminal` opens in terminal mode's active pane while that
    mode is shown, else in the terminal panel. "Open in Terminal"
    (`workspace::OpenTerminal`, from the project panel, outline panel, tab and
    file header menus) opens a shell in that directory in terminal mode and
    switches to it.
  - Pane navigation (`ActivatePane*`, next/previous pane) stays among the
    terminal panes; Zoom expands one terminal pane over the others.
- There are no terminals in editor panes: the `NewCenterTerminal` action is
  gone, and editor panes refuse dropped terminal tabs, so a terminal never
  lands among files.
- Docks have no border and blend into the chrome. Panes inside them (the
  terminal) draw their own cards; a non-pane panel docked at the bottom just
  sits on the chrome.
- Split panes are sections of one set of tabs, not separate editors: a tab
  (a file, a commit, a revision) lives in exactly one pane, and the same file is
  never open in two panes. Opening something already open reveals and focuses
  its tab wherever it is; the pane passed by the caller (or the active pane)
  only decides where new tabs go. "Open to the side" on an open file reveals it
  rather than splitting. Splitting (`pane::Split*`, dragging
  a tab to an edge) moves the active tab into the new pane; nothing is cloned,
  and there is no clone split mode (`SplitMode` is only `MovePane`, the
  default, and `EmptyPane`). A pane's only tab can't leave it, so splitting it
  opens an empty pane on the other side instead, leaving the tab, still
  focused, on the side asked for. Dragging a pane's only tab over that
  same pane offers no split zones (every pane, terminal included), since the
  drop would just move the pane.
- One back/forward history covers every center and Git History pane. Going back
  or forward reveals the entry's tab in whichever pane holds it, switching mode
  if needed; a closed file reopens in the last active center pane. Moving focus
  from one pane to another records where the user was in the pane they left,
  so going back returns there. `workspace::GoBack` / `workspace::GoForward`
  are workspace actions bound only to `cmd-[` / `cmd-]`, so they work wherever
  focus is in the window. The Go Back / Go Forward buttons sit
  only in the window header (see above), not on tab bars.
  The `tab_bar.show_nav_history_buttons` and `reveal_if_open`
  settings are removed. The terminal panel keeps its own panes and history.
- Every pane always shows its tab bar (pill tabs inside the card, otherwise on
  the chrome above it). Tab bars always carry their buttons and keep pinned
  tabs in the same row as the others. There are no `tab_bar` settings and no
  Toggle Tab Bar action.
- Tabs are hardcoded too; there are no `tabs` settings. Every tab shows its
  file icon and git status colour, the close button sits on the right and
  appears on hover, closing the active tab activates the previously active one
  (else its left neighbour), and tabs carry no diagnostic badges. The tab
  switcher and file headers follow the same icon and git colouring.
- Split panes are separate cards with an 8px gap between them (the chrome shows
  through) instead of a divider line. The resize handle covers the gap.
  Finding the neighbouring pane (pane navigation, "open to the side") looks
  across the gap, so it reuses an existing pane rather than splitting again.
- Inactive split panes are dimmed (70% overlay opacity) over the rounded card
  only, never the tab bar, so tab bars stay the plain chrome colour in every
  pane. The active pane has no border.
- The workspace has no settings (`WorkspaceSettingsContent`,
  `StatusBarSettings` and `PreviewTabsSettings` are gone). Fixed behaviour:
  - Layout: the bottom dock sits between the left and right docks. Splitting
    goes right or down. Split drop targets are 20% of the pane's smaller
    side. Resizing the left dock resizes all its panels; other docks resize
    only the active panel. No max tab count.
  - Closing the active item in an empty pane closes the window. Files deleted
    on disk keep their tab. A buffer without unsaved edits follows its file's
    deletion and becomes empty (undo brings the text back as unsaved edits),
    so git diffs show the file as deleted; unsaved edits are kept.
  - Focus never follows the mouse.
  - Window: native macOS fullscreen. Text uses the platform's default
    rendering mode.
  - Status bar: shown only in Editor mode (and not while zoomed), with the language and
    cursor position buttons; the encoding button only for non-UTF-8 or
    UTF-8-with-BOM buffers.
  - Preview tabs are always on (`TogglePreviewTab` always works). A single
    click in the project panel, opening from a multibuffer, and single-file
    code navigation open preview tabs, and code navigation replaces the
    current preview rather than keeping it. The file finder and multibuffer
    results of code navigation (references, multiple definitions, selections)
    open permanent tabs.
- A zoomed view (editor or any dock) covers exactly the area the cards occupy,
  on the chrome, overlaying the others, so every zoomed view has the same size.
  The `zoomed_padding` setting is removed, as is centered layout
  (`workspace::ToggleCenteredLayout` and the `centered_layout` setting). The status bar is hidden while
  something is zoomed, so the zoomed view reaches down to the bottom gap. The
  window header moves into the zoomed view's top row, since the macOS window
  controls sit there.
- A zoomed dock (e.g. an expanded terminal) belongs to Editor mode: Git
  History mode hides it, and switching back to
  Editor mode focuses it again, so it is still open and zoomed rather than
  closed.
- Actions act only in the mode they belong to; in other modes they are ignored
  rather than switching modes, so switching is always explicit (`cmd-1`,
  `cmd-2`, `cmd-3`). Docks belong to Editor mode: toggling, opening, closing, focusing
  or resizing docks and panels (`cmd-b`, `cmd-r`, `cmd-j`, …)
  does nothing in Git History.
- One key per dock, no per-panel shortcuts (the panel `ToggleFocus` actions
  stay in the command palette and View menu):
  - `cmd-b` / `cmd-r` (`workspace::CycleLeftDock` / `CycleRightDock`) rotate
    the dock: hidden → first enabled panel → next enabled panel → … → hidden
    (left: project → git → hidden; right: outline → hidden). The rotation
    follows what is visible, not focus: a hidden dock always starts at its
    first panel, and a press on an open dock advances even when focus is in
    the editor. The shown panel takes focus; closing returns focus to the
    center only if the dock had it. A dock hidden behind another zoomed view
    counts as hidden.
  - `cmd-j` (`terminal_panel::ToggleShell`) is about shell terminals, not
    task output (tasks run as terminal tabs in the same panel). With a shell
    in front of a visible panel it hides the panel; otherwise (hidden, or task
    output in front) it shows and focuses a shell: the active tab if it is
    one, else the first shell in the active terminal pane, then in the other
    panes, else a new shell.
- Zoom (`workspace::ToggleZoom`, `` cmd-` ``, also `shift-escape`) expands the
  focused pane. In the terminal dock that is the focused terminal pane alone,
  not every split, as zooming an editor pane hides the other panes.
- `cmd-1`…`cmd-9` don't activate panes, and the image viewer and branch/stash
  picker have no `cmd-1`/`cmd-2`/`cmd-3`, so the mode shortcuts work everywhere.
- An 8px gap separates the cards from the window edges and from open docks.
  There is no gap at the top: tab bars and the sidebar header start at the
  window's top edge. When the status bar is shown, it replaces
  the bottom gap and pads its content evenly above and below, so the icons sit
  centred between the card and the window edge.
- Status bar items have no "Hide Button" right-click entry and can't be hidden. Items without other menu entries (search, cursor
  position, language, diagnostics, …) have no right-click menu at all; dock
  panel buttons have none either.
- Status bar buttons that open a pane item (project search, project
  diagnostics, Git History's graph) turn accent while that item is the active
  tab (`status_item_color`). Diagnostics keeps its error/warning icon colours
  and its counts stay default (the icons already carry colour); only its check
  icon takes the accent.
- Docks always have a fixed size in pixels (width for left/right, height for
  bottom). There is no proportional "flex width" mode and no
  `terminal.flexible` setting. A side dock may shrink when space is tight but
  springs back to its stored width.
- The project, git (commit) and outline panels, and the Git History sidebar,
  share a 240px minimum width (`dock::SIDEBAR_MIN_WIDTH`): dragging or the
  dock-size actions stop there instead of storing a smaller width.
- Tabs fill the tab bar's full height with rounded top corners. The active tab's
  base flares into the card with concave curves. Tab tops, flares and card
  corners share one 10px radius, so tab and card read as one shape rather than
  two stacked rectangles (convex bottom corners would detach the tab like a
  button). Inactive tabs paint no fill (they are the bar's colour), so a
  neighbour never covers a flare. The tab bar has no bottom line and no borders
  between tabs. It is inset 10px on the left (the card radius), and the first
  tab keeps another 10px before it for its flare, so its left side makes an
  S-curve with the card's corner: up the card's side, round its corner, flare
  into the tab. A flare is clipped where a tab strip starts with another tab
  (the unpinned tabs after pinned ones), leaving that base square. Without end
  buttons (an unfocused pane shows none), the tab strip also stops 10px short of
  the right edge, so a tab cut off by a narrow pane ends over the card's
  straight top edge instead of hanging past its rounded corner. A dragged tab
  has no flares.
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
  `cmd-f`, the menu or the action. Left of Zoom, an eye button opens the Markdown or SVG
  preview of the active file (Option-click opens it in a split),
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
  pane gets these buttons in `new_terminal_pane`, so panes created by moving
  a tab into a split get them too.
- Pane toolbars (search bars, diff controls) have no bottom line either; they
  sit flat inside the card. Every toolbar-like row (pane toolbars, the Git
  History header and its search row) shares one set of metrics so they line up
  when switching tabs or modes: 6px vertical and 8px horizontal padding around
  a 32px row, square icon buttons with 16px (`IconSize::Medium`) icons, and
  14px (default label size) control text; counts and stats stay small. Right-side toolbar items (e.g. image viewer
  controls) stay right-aligned even when nothing is on the left. Editor panes have no breadcrumbs or quick action
  bar (see editor.md).
- An empty pane leaves a blank strip of tab-bar height
  above its card, so the card does not jump when the first tab opens or the
  last one closes.
- An empty pane keeps its card and shows centred keybinding hints (New File,
  Open File, Go to File, Command Palette), styled like the project search
  landing page. New File leads because a double click on a tab bar that acts
  as the titlebar zooms the window instead of creating a file.
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
