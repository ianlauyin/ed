# Platform support

## Decision

macOS is the only supported editor host. Both Apple Silicon and Intel
Macs remain supported. Only the latest macOS is targeted; the minimum is 27.2
and code does not gate on OS version at runtime. The minimum lives in source:
`crates/ed/build.rs` (`MACOSX_DEPLOYMENT_TARGET` for the binary), `Info.plist`
(`LSMinimumSystemVersion`), and the Metal shader build in `gpui_apple/build.rs`. `gpui_apple` builds only for
macOS; iOS is not supported. There is no other build target: the headless
remote server is removed (see `collaboration-and-telemetry.md`).

Paths are Unix-only: `/`-separated with no drive
or UNC prefixes. There is no path-style abstraction; code uses
`std::path::Path` semantics directly, plus the `/`-string helpers in the
`path` crate (`join`, `join_path`, `normalize`, `split`).

## Behaviour

- The editor uses native macOS windows, Metal rendering, and system menus.
- Window output is always tagged Display P3 (no setting). Theme hex values are
  interpreted as P3, not sRGB: a deliberate appearance choice matching the
  untagged look on Mac panels. Grays are unchanged; saturated colors look more
  vivid than their sRGB hex. macOS color-manages to each monitor, so the look
  is consistent across displays and screenshots (clipped on sRGB monitors).
- Confirmation and error dialogs always use native macOS prompts. The custom
  prompt renderer and `use_system_prompts` setting are removed.
- No native window tabs: every window stands alone (one window per project
  group). Automatic tabbing is disabled app-wide at launch, so AppKit adds no
  "Show Tab Bar" / "Merge All Windows" style menu items, and there is no
  `use_system_window_tabs` setting or in-app window-tab strip.
- Closing the last window quits the app (it never lingers windowless in the
  Dock); the last workspace stays flagged open for restore. There is no
  `on_last_window_closed` setting.
- The native menu bar has no Help menu.
- No `ed` CLI binary (`edctl` is only for automation, see `automation.md`). Files and folders open through macOS (Finder, `open -a
  ed`, Apple Events) or as arguments to the app binary itself. There is no
  `--wait`, environment/cwd forwarding, open-mode flags (`-a`/`-n`/`-e`), or
  `cli_default_open_behavior` setting; open requests, Open, and Open Recent
  add to the existing window unless the action asks for a new one. New
  windows show the Launchpad.
- Quit never asks for confirmation, except for unsaved changes and running
  terminals.
- Closing a terminal tab, closing or replacing the window, and quitting ask
  first when a terminal still runs a process: an unfinished task, or a
  foreground job of the shell (the PTY's foreground process group is not the
  shell). Jobs in the background of the shell (`cmd &`) don't count.
- Open and Save As use the system dialogs.
- Terminals, tasks, and environment capture only understand
  POSIX shells (sh/bash/zsh) and fish. Any other shell is treated as POSIX;
  csh/tcsh, rc, PowerShell, cmd, Nushell, Xonsh, and Elvish quoting and syntax
  are not supported.
- The terminal uses upstream `alacritty/alacritty` (`alacritty_terminal`, pinned
  git rev), not Zed's fork. PTYs are spawned off the main thread; the spawning
  thread temporarily takes the main thread's signal mask so the shell doesn't
  inherit blocked signals (Ctrl-C).
- "Allow in the Background": macOS lists the app in Login Items when a
  process it spawned outlives it. git `push`/`pull`/`fetch` are spawned
  disclaimed (`Command::disclaim`), so the ssh `ControlPersist` master they
  may leave behind is charged to git, not the app. Trade-offs: those spawns
  fork (no `posix_spawn`), and git/ssh face macOS privacy checks (e.g.
  Local Network) as themselves. Terminal shells are not disclaimed, so
  `nohup` jobs left by a terminal are still charged to the app.
- Async IO lives in the `async_macos` crate and runs on GCD dispatch sources, with no reactor,
  reaper, or blocking-pool threads and no smol / async-io / tokio:
  - Pipes and Unix sockets are non-blocking fds; a read/write source is
    created on the first `WouldBlock` and stays suspended except while a task
    waits on it.
  - Each child gets a process source that reaps it on exit, so dropping a
    `Child` never leaves a zombie and signals are never sent to a reaped pid.
  - Blocking work (`async_macos::unblock`, `async_macos::fs`) runs on GCD's
    default global queue, shared with background tasks. Trade-off: no
    dedicated blocking pool, and queued work is not cancelled when its future
    is dropped.
- Subprocesses (`async_macos::command::Command`) wrap `std::process::Command` with std
  semantics (unset stdio is inherited). There is no custom `posix_spawn` path:
  it existed upstream only to reset Mach exception ports inherited from Zed's
  crash handler, and there is no crash handler.
- Git operations (including clones and commit signing) require
  authentication that works without user input, such as an SSH agent or cached
  credentials. Password/passphrase requests fail instead of opening a prompt.
- The askpass crate, password dialogs, and askpass helper modes are removed.
- GPUI has no credential storage API; the platform layer never touches the
  macOS Keychain.
- No accessibility support: GPUI has no AccessKit integration, never registers
  with the macOS accessibility API, and builds no accessibility tree. There are
  no role/aria element APIs, no accessibility dev actions, and no
  `ED_EXPERIMENTAL_A11Y` switch. Region navigation (`FocusNextPart`) and
  keyboard toolbar navigation still work and focus the interactive content.
- Audio input and speech recognition are unsupported; the app requests no
  microphone or speech-recognition access. Background tasks use high, medium,
  or low priority, with no realtime audio scheduling.
- No HTTP client and no Tokio: the app runs on gpui's executors and GCD only.
  Images load only from disk or embedded assets; gpui has no URI image source,
  and markdown preview skips `http`/`https` images like `data:` ones.
- Image formats are PNG, JPEG, GIF, WebP and SVG only, for the image viewer,
  markdown and the clipboard; `image` is built with just those decoders.
- No proxy support: there is no `proxy` setting, environment proxy variables are
  ignored, and JSON language server proxy settings are not forwarded.
- Ship one macOS keymap constructed in Rust; alternate keymaps are removed.
- No bundled fonts: the UI defaults to the macOS system font (`.SystemUIFont`),
  the buffer font is Menlo (ships with macOS; a missing font would fall back to
  proportional Helvetica and break the terminal's cell grid), the minimap
  inherits the buffer font, and SVG/Mermaid text uses system fonts.
- No reduce-motion mode: GPUI has no `reduce_motion` flag and there is no
  `reduce_motion` setting; animations (spinners, GIFs, scrollbar fades,
  notification fades, cursor animation, smooth scrolling, dock open/close) always run.
- There is no platform-selection crate: the editor constructs `gpui_macos::MacPlatform`
  directly.
- GPUI core has no direct `libc` dependency; native OS integration belongs in
  platform-specific crates.
- No external drag and drop: GPUI has no file-drop events and windows are not
  drag sources or destinations for other apps. Drags stay inside the app; files
  cross to and from other apps only through the clipboard (`ExternalPaths`).
- No touch or mobile support in GPUI: no raw touch events, tap/long-press/touch-drag
  recognition, touch click events, or touch hover suppression. Input modality is
  mouse or keyboard only. Trackpad input stays: scroll phases (`TouchPhase`),
  per-gesture scroll axis locking (`OngoingScroll`), and pinch (`PinchEvent`).
- No mobile platform hooks: no app lifecycle or memory-warning callbacks, window
  insets/safe areas, visual viewport, system back action, soft keyboard requests,
  text-input state notifications, or soft-keyboard text configuration
  (autocorrect, autocapitalize, enter-key hint).

## App identity and user directories

- The app is named `ed`: binary `ed`, bundle `ed.app`, bundle identifier
  `dev.ed`, menu/About/window-title name `ed`. It must not share any user
  data, bundle identity, or single-instance state with an installed upstream Zed.
  `paths::APP_NAME` must equal the binary name (compile-time assert).
- The About window is compact: app icon, `ed <version>` headline, and a
  single centered OK button (Enter/Escape close it). No separate version
  label and no Copy button. It opens centered on the display the user is
  working on (`cx.main_display()`), not always the primary one.
- User directories follow the XDG layout; nothing is written under
  `~/Library`. Each base honours its `XDG_*_HOME` variable when absolute:
  - data (`projects.json`) `~/.local/share/ed`
  - state `~/.local/state/ed`, logs in `~/.local/state/ed/logs/ed.log`
  - cache/temp `~/.cache/ed`
  - no config directory: ed reads no config files.
  - `--user-data-dir <dir>` replaces all three bases with one root:
    `<dir>/data`, `<dir>/state` (lock, logs) and `<dir>/cache`, so such an
    instance shares nothing with the default one (e.g. `edctl launch --fresh` beside a running app).
- Single instance (release builds only): an exclusive `flock` on
  `<state>/ed.lock`, held for the process lifetime. No TCP port, so it
  cannot collide with Zed or other apps, and a crash leaves no stale lock.
- Minimal macOS footprint: the bundle declares no privacy usage descriptions
  (camera, contacts, AppleScript, etc.), so such requests from processes it
  spawns are denied without a prompt. It is only an `Alternate` handler for
  documents, never the default opener. AppKit defaults gpui needs are
  registered in memory, never written to `~/Library/Preferences`.
- No `zed://` URL scheme: the bundle registers no URL types, there is no
  open-URL prompt or `OpenZedUrl` action, and terminal links don't match
  `zed://`. The app only accepts paths and `file://` URLs.
  Dock-menu items dispatch their actions directly through AppKit, not via URLs.
- Branding is `ed` everywhere (crates `ed`/`ed_actions`, UI text,
  `TERM_PROGRAM=ed`, pasteboard types).
  Environment variables use the `ED_` prefix (task variables like `$ED_FILE`,
  `ED_WINDOW_SIZE`, ...) and the project folder is `.ed`; upstream
  `ZED_*` variables and `.zed` folders are ignored.
- Linux desktop, FreeBSD, Windows, and browser backends, packaging, and associated
  build/test/release tooling are removed.
- No non-macOS code: crates carry no Linux/Windows `cfg` branches or
  target-specific dependency sections, and GPUI has no display-server APIs
  (no windowing-mode switching, `GraphicalEnvironment`, compositor name, or
  window hover tracking; the cursor follows the key window).
  `gpui` fails to compile for any other target.
- System-specification reports are removed: no `--system-specs` flag or
  copy-system-specs action.
- The filesystem-watcher debug window, recording API, and JSON exports are removed.
  Filesystem watching and rescan error logging remain available.
  - Watch registrations are keyed by path components, so equivalent spellings
    (`/a/b/`, `/a//b`, `/a/./b`) share one backend watch and receive the same
    events; on case-insensitive volumes the key is also case-folded and
    Unicode-normalized.
  - A registration under a recursive watch shares its ancestor's backend watch.
    Removing that ancestor first installs replacement watches for the surviving
    descendants (shallowest first); if a replacement fails, the ancestor's watch
    is kept until the descendants it still serves are removed.
  - The backend is an in-tree FSEvents binding (`fs::fsevents`), not the
    `notify` crate: each backend watch is its own file-level stream on its own
    serial dispatch queue, so adding or removing a watch never restarts another
    watch's stream (no event gap) and needs no run-loop thread. Streams start
    from "now", with no latency and no root watching.
  - Event paths are reported under the watched path as registered, even where
    it traverses symlinks (FSEvents itself reports canonical paths).
  - FSEvents coalesces changes per path; an event maps to created, changed
    (rename, content, metadata) and/or removed, and one pending event per path
    is kept per batch (removed, then created, then changed). A
    `MustScanSubDirs` event is sent to every registration: it rescans that
    subtree where it lies under the registration's root, else the whole root.
  - Only FSEvents is used, always recursive; the upstream poll watcher,
    inotify handling (stale-watch tracking, watch-limit cooldown),
    filesystem-type detection, and `*_FILE_WATCHER_*` env overrides are removed. Mounts whose native watcher misses changes made elsewhere
    (e.g. a host commit on a VM's virtiofs/9p/NFS mount) are not tracked
    automatically.
  - `workspace::ReloadProjectFiles` covers that case: every worktree rescans its root
    and its repositories' git directories as if the watcher lost sync, so
    entries, open buffers and git state catch up.
- File formats and protocol data may describe other operating systems, paths,
  or line endings; these do not imply editor host support.

## Build requirements

Use a Rust toolchain and the native SDK/C toolchain for the intended target.
Metal shaders compile at runtime on macOS. Upstream workflow
generators that depended on removed release and deployment infrastructure are absent.
macOS builds pass `-C force-unwind-tables=no` (`.cargo/config.toml`): rustc's
default async unwind tables are not compact-unwind encodable, which bloats
`__eh_frame` past ld's 16MB compact-unwind limit in dev builds. Panics and
backtraces still work through sync CFI.
Release builds use `panic = "abort"`: panic hooks already log and exit, so
unwinding adds code size and nothing else. Line info lives in
`target/release/ed.dSYM`, which is not bundled into `ed.app`; panic backtraces
from the installed app only resolve the panic location itself.

## Packaging

- `release.fish` builds `target/release/ed.app` for the host Mac: the `ed`
  binary in `Contents/MacOS`, `crates/ed/resources/Info.plist` with the
  Cargo.toml version, an icon generated from `app-icon@2x.png`, and `Document.icns`.
- `release.fish` signs the bundle with a self-signed keychain identity
  (`ed-local`, or `ED_SIGN_IDENTITY`) and stops with setup instructions when it is
  missing. A stable signature keeps macOS privacy grants across rebuilds; ad-hoc
  signatures change every build, so grants such as Local Network access (used
  by `ssh`/git run from the terminal) are lost. `Info.plist` carries
  `NSLocalNetworkUsageDescription`.
- App icon: Nord-dark rounded square with an inset screen showing `</>` in
  `#88C0D0`. Source is `crates/ed/resources/app-icon.svg`; the PNGs are rendered
  from it at 512 and 1024 px.
- The bundle is for local use only: no Developer ID signing, entitlements,
  hardened runtime, provisioning profiles, notarization, DMG, or universal binary. No `cargo-bundle` metadata.
- Build scripts do not query git.

## macOS bindings

- Apple framework access (AppKit, Foundation, Core Foundation, Core Graphics,
  Core Text, Quartz Core) uses the `objc2` crate family only; the deprecated
  `cocoa` crate, the legacy `objc` 0.2 crate, and the servo bindings
  (`core-foundation`, `core-graphics`, `core-text`, `foreign-types`) are not
  dependencies. Carbon text-input-source calls (`TIS*`, `UCKeyTranslate`) have
  no objc2 bindings and are declared locally.
- Frames are paced by a per-window `CAMetalDisplayLink` on the window's
  `CAMetalLayer`, scheduled in the main run loop's common modes so it keeps
  firing during live resize. The renderer draws into the drawable each update
  hands out and never calls `nextDrawable` while a link is attached, because
  `nextDrawable` blocks ~33 ms per frame on base M-series Macs on macOS 26+.
  The OS picks the rate (`preferredFrameRateRange` is left at its default).
- An idle window does no per-refresh work: the link pauses after an update in
  which GPUI neither drew, presented, nor asked for another frame, and GPUI's
  `frame_waker`/`schedule_frame` resume it. Synchronous draws (activation,
  `displayLayer:`) and occlusion detach the link; it is recreated afterwards,
  on screen changes, and invalidated when the window is dropped. Neither
  `CADisplayLink` nor the deprecated `CVDisplayLink` is used.
- Glyphs are always rasterized as grayscale coverage masks (monochrome atlas)
  or colour emoji (polychrome atlas). There is no subpixel/LCD text pipeline
  and no text rendering mode setting.
- GPUI's platform traits carry only what macOS implements: no anchored popup
  window kind, jump list, async clipboard read, missing-glyph reporting, font
  prewarming, GPU specs, or timer-resolution hook.
- The Metal renderer and sprite atlas (`gpui_apple`) use `objc2-metal` and
  `objc2-quartz-core`'s `CAMetalLayer`; the `metal` (metal-rs) crate is not a
  dependency.
- `gpui_macos` declares its `NSApplication`/`NSWindow`/`NSPanel`/`NSView`
  subclasses with `objc2::define_class!`. Window, panel and view share one
  typed ivar holding a strong `Arc` to the window state; it is installed right
  after creation and released when the object deallocates. The application and
  its delegate hold a platform pointer that AppKit-created instances get
  zero-filled, so that ivar must stay valid when zeroed and have no `Drop`.
- Much of the crate still sends `msg_send!` to untyped pointers (`id`, from the
  crate-local `ns` module); new code should prefer typed `objc2-*` APIs.
- objc2 debug builds verify every message's type encodings and panic on
  messages to nil, so raw sends must use exact types (e.g. `bool` for `BOOL`,
  `NSUInteger` not `u32`, `*const c_void` for `void *`, a value type instead of
  `()` for non-void methods) and guard receivers that may be nil.
- Text uses Core Text directly (`gpui_apple`); `font-kit` is not a dependency
  and text rendering is not behind a cargo feature. Fonts come from the system
  font collection, plus any registered with `add_fonts`. Face selection within a
  family follows CSS font matching (stretch, then style, then weight), with
  Core Text's normalized weights mapped to CSS weights as font-kit did, so the
  same face is chosen for a given weight and style.
- Each window owns its `WindowTextSystem` (`Rc`, not `Send`). Its line layout
  cache uses `RefCell`s with no locks, and is keyed by the text itself; there is
  no hash-keyed layout API.
