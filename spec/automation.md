# GUI automation (`edctl`)

## Decision

Agents test the editor through a control socket and a CLI, not macOS
accessibility, MCP, or screen scraping. GPUI paints the whole window with
Metal, so the OS sees no UI structure; the app reports what it painted itself.

- `crates/control` holds the protocol (newline-delimited JSON over a Unix
  socket, one response per request) and the `edctl` binary. It has no GPUI
  dependency, so any agent or shell script can drive the app.
- The server lives in the app (`ed::control_server`). It only runs when the
  app starts with `ED_CONTROL` (socket `$TMPDIR/ed-control-<pid>.sock`) or
  `ED_CONTROL_SOCKET=<path>`; otherwise nothing is recorded or opened.

## Development workflow

- Verify new features and UI changes in the running app with `edctl`
  (`launch --fresh`, drive, then check `tree`/`state`/`screenshot`), not only
  by building.
- When a feature can't be tested this way (missing command, element or state
  not exposed, unreliable settling, etc.), extend the control protocol, server,
  or GPUI recording as part of that work rather than skipping the check, and
  update this spec.

## Behaviour

- `edctl launch [--fresh]` starts the `ed` next to it with automation enabled
  and returns once a window exists. `--fresh` uses a new data directory so
  tests never touch the user's session; it also holds the instance's state
  (single-instance lock and log, `platform.md`), so a fresh release build runs
  beside the user's own release build. `launch` prints the pid, socket, data
  directory and log file. Other
  commands find the socket via `--socket`, `$ED_CONTROL_SOCKET`, or the only
  live `ed-control-*.sock` (stale ones are removed).
- `tree` lists, from the last frame, every element that has an id and takes
  mouse input or focus, with its visible bounds (logical px, window-relative),
  whether it is clickable, focusable, or focused, and the text lines painted
  inside it. Ids are the `/`-joined element id path without view and
  focus-handle ids, and component type paths are shortened to their last
  segment. Text too small to read (e.g. the minimap) is left out.
- `state` reports the window's bounds and its display's visible area,
  workspace roots, panes and items, and the active editor's text and
  selections. `screenshot` renders the last frame offscreen at device
  resolution, so the window may be hidden or covered. It waits for the window
  to settle first (no redraw and no animation frame pending); `--no-settle` captures the current frame, to see an
  animation mid-flight.
- `action`, `keys` (keymap syntax), `type`, `click`, `mouse`, and `scroll`
  inject input as the platform would, except that injected keystrokes don't
  hide the cursor: only a real mouse move shows it again, and hover features
  (e.g. cmd+hover links) ignore a hidden cursor. Each event is dispatched in its own
  update and followed by a draw, so the next event sees the new focus and key
  contexts. Mutating commands return once the window has stayed clean for a
  few frames (at most 1s). `click`/`scroll` target `--id`, `--text`, or `--at`;
  ambiguous matches fail and list candidates for `--nth`.
- `wait` polls `tree` for an element or text to appear or disappear, or for
  focus to move to an id.
- `state` lists the editor panes and, separately, the Git History and
  Terminal mode panes, each
  with whether it is the workspace's focused pane, the active pane of its
  group, and whether it holds keyboard focus, plus the current mode.
- GPUI only sends focus-in/out events while the window is active (frontmost),
  so checks that depend on a pane becoming active on focus need the app in
  front; a background window keeps keyboard focus but no pane activates.
- `state` lists the left and bottom docks: open or not, the visible
  panel's persistent name, and whether the dock holds keyboard focus.
- `state` also lists the project's language servers with the progress work
  each reports in flight. `wait-lsp`
  polls it until a (named) server exists and has reported no work for 1s,
  since servers go briefly quiet between phases (e.g. rust-analyzer's fetch
  then index). Use it instead of fixed sleeps before LSP-driven steps.
- `perf-reset` / `perf` measure CPU cost between two points, to compare
  builds on a scripted workload. They report the process's CPU time,
  retired instructions and cycles (`proc_pid_rusage`; instructions are the
  low-noise number for A/B comparisons), the main thread's CPU time, and the
  window's drawn frames: count and total/mean/p50/p95/max of draw, build
  (render, layout, prepaint), paint, and present, plus lines shaped by the
  platform text system (line layout cache misses). Compare per-frame costs,
  since animations decide how many frames a workload draws. Measure release
  builds with the window visible: macOS throttles hidden windows.

## GPUI support

- `App::enable_ui_snapshots` turns on recording. Divs record themselves in
  paint; `ShapedLine`/`WrappedLine` record painted text with the current id
  path; custom elements that take input (the editor, id `editor`) call
  `Window::record_ui_element`. Recording is part of the paint range, so cached
  views replay their entries.
- `Window::reset_frame_stats` starts per-window frame timing; nothing is
  timed or stored before it is called.
- `PlatformWindow::render_to_image` (Metal readback on macOS) backs
  `Window::capture_image`.
