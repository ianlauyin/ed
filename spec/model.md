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
  - `observe_global::<SettingsStore>` reacts to settings changes;
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
 └─ feature crates: editor, project_panel, outline_panel, terminal_view,
    git_ui, search, file_finder, picker, title_bar, diagnostics, ...
     └─ workspace          (views: window shell, panes, docks, items)
         ├─ project        (models: everything one open project knows)
         │   ├─ worktree, language, lsp, git, fs, remote, task, terminal
         ├─ ui             (stateless components)
         └─ gpui, settings, theme
```

- `project` has no UI. It knows nothing about panes, editors or windows, and
  is what `remote_server` runs headless (`HeadlessProject` reuses the same
  stores).
- `workspace` knows `project` but no concrete item types (no `Editor`, no
  panels). It talks to them through traits: `Item`, `ProjectItem`, `Panel`,
  `StatusItemView`, `ToolbarItemView`, `ModalView`, `Sidebar`.
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
- Local vs remote is decided inside the stores: `Project::local` builds local
  stores, `Project::remote` builds ones that forward to a `RemoteClient`.
  Views never branch on it beyond `is_local()` checks for UI choices.
- A model a view shows (a `Buffer`, an `ImageItem`) implements
  `project::ProjectItem` (`try_open` from a `ProjectPath`). The model is shared:
  one `Buffer` per file however many views read it.

## Views: workspace tree

One OS window has this view tree:

```
MultiWorkspace                window root; holds several Workspaces, shows one
 └─ Workspace                 one per project; owns Entity<Project>
     ├─ center: PaneGroup     split tree of Panes (Workspace mode)
     ├─ git_history_center    separate PaneGroup (Git History mode)
     │   └─ Pane              tabs: Vec<Box<dyn ItemHandle>>, Toolbar
     │       └─ Item          Editor, ImageView, ProjectSearchView, ...
     ├─ left/bottom/right Dock
     │   └─ Panel             ProjectPanel, OutlinePanel, TerminalPanel, GitPanel
     ├─ StatusBar             StatusItemViews
     ├─ ModalLayer            at most one ModalView (pickers, prompts)
     ├─ ToastLayer, notifications
     └─ titlebar_item         TitleBar
```

- `MultiWorkspace` keeps every `Workspace` it holds alive (each with its own
  `Project`), grouped by project; switching swaps which one renders. Only the
  displayed workspace writes the window title / edited state.
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
  workspace handles. `Dock` observes each panel to re-render, and observes
  settings to move a panel when its dock position setting changes.
- Panels are loaded asynchronously (each panel's `load`, usually restoring
  serialized state) and added when ready; dock layout is restored after all
  of them (`finish_dock_restoration`).

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
  whose root is a `MultiWorkspace` holding a new `Workspace`, and restores
  the serialized workspace (panes, items via `SerializableItem`, docks) from
  the DB.
- `observe_new::<Workspace>` in `ed` then adds panels, status bar items and
  workspace actions. `observe_new::<MultiWorkspace>` handles window close.
- Items, panes and docks persist themselves by enqueuing serialization on
  relevant events; the workspace debounces writes to the DB.
