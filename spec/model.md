# Model / view structure

How the app is split into models (state, no rendering) and views (render +
focus), how they depend on each other, and how GPUI wires them together.
Read the code for details; this only records the shape and the rules.

## GPUI primitives

- Everything long-lived is an `Entity<T>` owned by the `App`. A *model* is an
  entity that never renders; a *view* is an entity that implements `Render`.
  Both are created with `cx.new` and reached through `Entity` (strong) or
  `WeakEntity` (weak) handles.
- Two ways to react to another entity:
  - `cx.observe(&e, ..)`: fires on `cx.notify()` from `e`. Means "something
    changed, re-read me". Views call `cx.notify()` to schedule a re-render.
  - `cx.subscribe(&e, ..)` / `subscribe_in` (with `Window`): typed events,
    declared by `impl EventEmitter<E> for T` and sent with `cx.emit`. Means
    "this specific thing happened".
  - `observe_global::<G>` reacts to a global changing;
    `observe_release` / `on_release` to an entity being dropped.
- A `Subscription` lives as long as it is stored (usually a
  `_subscriptions: Vec<Subscription>` field) or until the subscriber is
  dropped when `.detach()`ed. Dropping it unsubscribes.
- `cx.observe_new::<T>(..)` runs a callback for every new `T`. This is how
  crates that sit above `workspace` attach themselves to each `Workspace`
  without `workspace` knowing them.
- Views render independently: a parent embeds a child view (`Entity<V>` /
  `AnyView`), and only notified views re-render.
- Actions dispatch along the focus tree from the focused element up to the
  window root. A view that handles an action registers it on its root element
  (`.on_action`); `Workspace::register_action` adds a handler to the
  workspace root, so it works wherever focus is in the window.

## Crate layering

Dependencies only point downward:

```
ed (binary: wires everything, opens windows)
 └─ feature crates: editor, project_panel, terminal_view,
    git_ui, search, file_finder, picker, title_bar, diagnostics, ...
     └─ workspace          (views: window shell, panes, docks, items)
         ├─ project        (models: everything one open project knows)
         │   ├─ worktree, language, lsp, git, fs, task, terminal
         ├─ ui             (stateless components)
         └─ gpui, settings, theme
```

- `project` has no UI. It knows nothing about panes, editors or windows.
  `Project` and its stores (buffer, worktree, LSP, git, task, image) are
  local-only: no remote mode, wire protocol or headless host.
- `workspace` knows `project` but no concrete item types (no `Editor`, no
  panels). It talks to them through traits: `Item`, `ProjectItem`, `Panel`,
  `StatusItemView`, `ToolbarItemView`, `ModalView`.
- `ui` components are almost all `RenderOnce`: plain values built during a
  parent's `render`, holding no state between frames. Stateful widgets are
  views in feature crates.
- `ed` (`initialize_workspace`) is the only place that knows every piece: it
  adds the panels, status bar items and pane toolbars to each new
  `Workspace` via `observe_new`.

## Models: `Project`

`Project` is one open project (local or remote). It owns a set of stores,
each an entity with its own event enum:

| store | owns |
|---|---|
| `WorktreeStore` | `Worktree`s (root folders, file trees, fs scanning) |
| `BufferStore` | open text `Buffer`s, keyed by path / id |
| `ImageStore` | open `ImageItem`s |
| `LspStore` | language servers, diagnostics, LSP requests |
| `GitStore` | `Repository`s and their status, buffer diffs |
| `TaskStore` | task inventory |
| `ProjectEnvironment` | shell environment per worktree |

- Stores reference each other directly (`LspStore`, `GitStore`,
  `BufferStore`, `ImageStore` all subscribe to `WorktreeStore`; `LspStore` and
  `GitStore` subscribe to `BufferStore`), not through `Project`.
- `Project` subscribes to its stores and re-emits a flattened
  `project::Event` (worktree added/removed/updated, diagnostics, toasts,
  language server prompts, ...). Views subscribe to `Project` for those, and
  to a store directly only for what `Project` doesn't forward (e.g. the git
  panel and project panel subscribe to `GitStore`).
- Everything is local: `Project::local` builds the stores. A buffer's
  `language::File` has a disk path (`as_local`) only for worktree files;
  git-backed files (index text, commit blobs) have none.
- A model a view shows (a `Buffer`, an `ImageItem`) implements
  `project::ProjectItem` (`try_open` from a `ProjectPath`). The model is shared:
  one `Buffer` per file however many views read it.

## Views: workspace tree

One OS window has this view tree:

```
Workspace                 window root, one per window; owns Entity<Project>
 ├─ center: PaneGroup     split tree of Panes (Editor mode)
 ├─ git_history_center    separate PaneGroup (Git History mode)
 │   └─ Pane              tabs: Vec<Box<dyn ItemHandle>>, Toolbar
 │       └─ Item          Editor, ImageView, ProjectSearchView, ...
 ├─ left/bottom Dock
 │   └─ Panel             ProjectPanel, GitPanel, TerminalPanel
 ├─ StatusBar             StatusItemViews
 ├─ ModalLayer            at most one ModalView (pickers, prompts)
 ├─ ToastLayer, notifications
 └─ titlebar_item         TitleBar (project name, in the window header)
```

- Zoom is stored per mode as that mode needs it. Editor mode can zoom a dock
  panel as well as a pane, so `Workspace::zoomed` (plus `zoomed_position`)
  records it. Git History and Terminal modes zoom only panes, so their panes'
  own zoom flags are the only record. Code shared by the modes reads zoom through
  `Workspace::zoomed_pane(mode)` / `has_zoomed(mode)`, never the storage.
- A window holds exactly one `Workspace`. Opening another project in the same
  window (recent projects, Close Project, switching or creating a git worktree)
  first runs the current workspace's close prompts, then replaces the window's
  root with a new `Workspace` (`replace_window_workspace`); the old one leaves
  the session and is released, staying in recent projects. Its language
  servers are stopped explicitly at that point, and when its window closes,
  rather than waiting for the project to be released: a leaked project must
  not keep servers (gigabytes each) running. Nothing
  keeps a second workspace alive in the background, and there are no
  per-window project groups or workspace sidebar.
- Only the window's root workspace writes the window title / edited state.
- `WindowHandle<Workspace>::update` leases the root workspace, so code inside it
  uses the root it is given rather than updating the same `Entity<Workspace>`
  again (a double lease panics). Callbacks that may run for a non-root
  workspace update through the untyped `AnyWindowHandle`.
- Session restore reopens one window per project open at the last quit.
- `Workspace` holds the only strong `Project` handle that matters; panes,
  items and panels keep `Entity<Project>` or `WeakEntity` copies. When the
  workspace goes, the project goes.
- Children point back up with weak handles (`Pane::workspace`,
  panels' `WeakEntity<Workspace>`) so there are no ownership cycles.
- An item (tab) lives in exactly one pane; `Workspace::panes_by_item` tracks
  which (see `ui-layout.md`).

## Wiring

### Model → view

- A view that shows project state subscribes to `Project` (or a store) and
  updates its own state or `cx.notify()`s. Examples: `Workspace` reacts to
  worktree added/removed, toasts, LSP prompts, disconnects; `ProjectPanel`
  to worktree entry changes; `Editor` to project and `GitStore` events and to
  its `MultiBuffer` (`observe` for re-render, `subscribe` for edits).
- Views read models during `render` with `entity.read(cx)` and never cache
  copies they can't invalidate. Only notify when something the view shows
  actually changed (`gpui/best_practice.md`).

### View → model

- Views mutate models with `entity.update(cx, ..)` (e.g. `project.open_buffer`,
  `buffer.edit`). The model emits events / notifies; every other view showing
  it updates through its own subscription, never because the writer told it.

### Item → pane → workspace

- `Item` has its own event type and maps it to generic `ItemEvent`s
  (`to_item_events`: `CloseItem`, `UpdateTab`, `UpdateActivePath`, `Edit`).
  When an item first joins a pane, the workspace subscribes to it once and
  turns those into pane actions (close, retitle tab, autosave, active path).
  Moving an item between panes keeps that subscription.
- `Pane` emits `pane::Event` (`AddItem`, `ActivateItem`, `Split`, `Remove`,
  `Focus`, ...). `Workspace` subscribes to every pane it creates
  (`handle_pane_event`) and owns the cross-pane decisions: splitting, active
  pane, nav history, status bar / title updates.
- `Workspace` emits `workspace::Event` (`PaneAdded`, `ItemAdded`,
  `ActiveItemChanged`, ...). Outer crates subscribe for setup they own, e.g.
  `ed` adds toolbar items to each new pane on `PaneAdded`.

### Active item fan-out

Status bar items and toolbar items don't subscribe to items themselves. When
the active pane or its active item changes, the workspace calls
`StatusBar::set_active_pane` and the pane calls `Toolbar` with the new item;
each `StatusItemView` / `ToolbarItemView` gets `set_active_pane_item`,
downcasts to the item types it cares about, and (re)subscribes to that item.

### Docks and panels

- A `Panel` emits `PanelEvent` (`Activate`, `Close`, `ZoomIn/Out`), which the
  workspace handles. `Dock` observes each panel to re-render.
- There are two docks, left and bottom; there is no right dock and no outline
  panel, so the editor keeps the full width beside the sidebar. Every panel
  has one fixed dock (project, git: left; terminal: bottom). Panels can't be
  moved: there is no dock position setting, "Dock Left/Bottom" menu, or
  move-panel action.
- Panels are loaded asynchronously (each panel's `load`) and added when ready; dock layout is restored after all
  of them (`finish_dock_restoration`).
- Opening or closing a dock is animated visually only: layout jumps to the
  final state at once and a single overlay quad glides over it (`motion::GLIDE`,
  decelerating, no overshoot; open: a background curtain wipes off the panel
  toward the center; close: a panel-coloured ghost shrinks into the window
  edge). Its state (`DockMotion`: phase, last laid-out extent) lives on the
  workspace and nothing plays on first paint. Panes keep their cached
  frames and the terminal is resized once. Switching panels in an open dock
  and resizing by drag are not animated.
- Only local layout changes animate. Switching or closing tabs leaves the pane
  layout alone, and a mode switch swaps the whole view: both are instant (a mode
  switch snaps the incoming group's motion, so changes made while it was hidden
  don't play).
- Pane layout changes (split, close, move/swap, maximize, zoom) animate the same way
  in every pane group (`pane_motion`, state owned by `PaneGroup`: editor
  center, Terminal and Git History modes, the terminal panel; the group's
  `PaneBackdrop` says what shows between its cards): each pane records its
  laid-out bounds, gets a presented rect that decelerates toward them
  (`motion::GLIDE`, friction-style exponential decay: fastest at the start, no
  overshoot, ~95% there in `motion::BASE`), and one
  overlay paints background where a card hasn't arrived and the card's
  silhouette where it hangs past its bounds: the tab strip in backdrop and the
  body below it as a rounded card, since a flat full-rect fill reads as a dark
  block over the strip. Raised cards also cast a shadow outside themselves
  (fading as they land, so it doesn't pop off), because neighbouring cards
  share the card colour and an edgeless ghost moving over them is invisible
  (e.g. zoom-out). Existing panes slide from their old rect; a
  new pane grows from 85% about its center; a pane appearing as another
  disappears (dragging a pane's last tab into a split) travels from the
  removed pane's rect. New and moved cards are drawn above the rest. A moving
  card shows its content only where it overlaps its final bounds. The first
  frame, a group region resize (window resize, dock toggle) and anything
  while the pointer is down (divider drags) snap.
- The pane overlay is `deferred`: it paints above neighbours and escapes
  ancestor clipping, so an unzooming pane visibly shrinks from the zoomed area
  (over docks) back into the clipped center column.
- Zoom goes through the same motion: the zoomed pane renders via
  `PaneGroup::render_zoomed`, recording its zoomed bounds into its group's
  motion, so zooming in grows the card from its rect (other panes vanish to
  backdrop) and zooming out shrinks it back (other panes reappear in place
  beneath it). The region change that comes with a zoom doesn't snap. A
  zoomed center pane in Editor mode replaces the center group's render; the
  zoom-out ghost is clipped to the center region, so docks reappear at once.
  Zooming a dock panel is not animated.
- Pane motion costs nothing once settled: an unchanged layout with no moving
  card returns before any decay or overlay work. Dock overlays size from the
  dock's laid-out extent (docks flex-shrink and clamp), kept while closed.

## Opening a file

1. `Workspace::open_path(ProjectPath)` asks the `ProjectItemRegistry`
   (a global filled by `register_project_item::<V>()`). Registrations are
   tried newest first; each `project::ProjectItem::try_open` returns `None`
   if it can't open that path (so images go to `ImageView`, everything else
   falls through to `Editor`).
2. The model is loaded through `Project` → store (`BufferStore` /
   `ImageStore`), which dedupes by path.
3. `workspace::ProjectItem::for_project_item` builds the view for that model,
   and the pane adds it as an item (or reveals the existing tab for it).

## Lifecycle

- `Workspace::new_local` / `open_paths` creates a `Project`, opens a window
  (or replaces the requesting window's root) with a new `Workspace`, and restores
  the serialized workspace (panes, items via `SerializableItem`, docks) from
  the project's `.ed/workspace.json`.
- `observe_new::<Workspace>` in `ed` then adds panels, status bar items and
  workspace actions, and routes the window's close request to its current root
  `Workspace`.
- An item persists by returning its state from `SerializableItem::serialize`;
  events it marks with `should_serialize` (and pane, dock and window changes)
  schedule a debounced rewrite of the whole `.ed/workspace.json`.
