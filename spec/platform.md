# Platform support

## Decision

macOS is the only supported editor and CLI host. Both Apple Silicon and Intel
Macs remain supported, with a minimum macOS version of 10.15.7. The headless
remote server supports macOS and Linux.

## Behaviour

- The editor uses native macOS windows, Metal rendering, and system menus.
- Confirmation and error dialogs always use native macOS prompts. The custom
  prompt renderer and `use_system_prompts` setting are removed.
- No native window tabs: every window stands alone (one window per project
  group). Automatic tabbing is disabled app-wide at launch, so AppKit adds no
  "Show Tab Bar" / "Merge All Windows" style menu items, and there is no
  `use_system_window_tabs` setting or in-app window-tab strip.
- The native menu bar has no Help menu.
- CLI installation is manual. The editor has no CLI installer, installation menu
  item, or commands for installing the CLI or registering its URL scheme.
- Git operations (including clones and commit signing) and SSH connections require
  authentication that works without user input, such as an SSH agent or cached
  credentials. Password/passphrase requests fail instead of opening a prompt.
  SSH host verification must also succeed without interactive confirmation.
- SSH remotes run plain `ssh <args> <host> ...` for every command. The editor
  starts no master process and passes no ControlMaster/ControlPath options;
  connection reuse is left to the user's ssh config (e.g. `ControlMaster auto`
  + `ControlPersist`). The initial connect times out after 17s.
- The askpass crate, password dialogs, credential forwarding over RPC, and
  askpass helper modes are removed. Remote connection progress and cancellation
  remain available.
- No accessibility support: GPUI has no AccessKit integration, never registers
  with the macOS accessibility API, and builds no accessibility tree. There are
  no role/aria element APIs, no accessibility dev actions, and no
  `ZED_EXPERIMENTAL_A11Y` switch. Region navigation (`FocusNextPart`) and
  keyboard toolbar navigation still work and focus the interactive content.
- Audio input and speech recognition are unsupported; the app requests no
  microphone or speech-recognition access. Background tasks use high, medium,
  or low priority, with no realtime audio scheduling.
- HTTP uses the upstream `reqwest` crate directly (rustls with the platform
  certificate verifier), run on the app's Tokio runtime. There is no HTTP client
  abstraction crate; gpui holds an optional client used only to load remote images.
- No proxy support: there is no `proxy` setting, environment proxy variables are
  ignored, and JSON language server proxy settings are not forwarded.
- Ship one macOS keymap constructed in Rust; alternate keymaps are removed.
- No bundled fonts: the UI defaults to the macOS system font (`.SystemUIFont`),
  the minimap inherits the buffer font, and SVG/Mermaid text uses system fonts.
- The Linux remote server needs no display server, GPU, desktop integration, or fonts.
  It retains background execution, timers, and a foreground event loop for remote projects.
- GPUI core has no direct `libc` dependency; native OS integration belongs in
  platform-specific crates.
- Linux remote data, configuration, and logs follow XDG directory settings.

## App identity and user directories

- The app is named `ed`: binary `ed`, bundle `ed.app`, bundle identifier
  `dev.ed`, menu/About/window-title name `ed`. It must not share any user
  data, bundle identity, or single-instance state with an installed upstream Zed.
  `paths::APP_NAME` must equal the binary name (compile-time assert).
- The About window is compact: app icon, `ed <version>` headline, and a
  single centered OK button (Enter/Escape close it). No separate version
  label and no Copy button.
- macOS and Linux use the same XDG layout; nothing is written under
  `~/Library`. Each base honours its `XDG_*_HOME` variable when absolute:
  - config `~/.config/ed`
  - data (db, remote server state) `~/.local/share/ed`
  - state `~/.local/state/ed`, logs in `~/.local/state/ed/logs/ed.log`
  - cache/temp `~/.cache/ed`
  - `--user-data-dir` still overrides data (and config under it).
- Single instance (release builds only): an exclusive `flock` on
  `~/.local/state/ed/ed.lock`, held for the process lifetime. No TCP port, so it
  cannot collide with Zed or other apps, and a crash leaves no stale lock.
- Minimal macOS footprint: the bundle declares no privacy usage descriptions
  (camera, contacts, AppleScript, etc.), so such requests from processes it
  spawns are denied without a prompt. It is only an `Alternate` handler for
  documents, never the default opener. AppKit defaults gpui needs are
  registered in memory, never written to `~/Library/Preferences`.
- Kept from upstream: `zed://`/`zed-cli://` URL schemes, `ZED_*` environment
  variables, and the `.zed` project folder name.
- Linux desktop, FreeBSD, Windows, and browser backends, packaging, and associated
  build/test/release tooling are removed.
- Unsupported editor and CLI targets fail with a macOS-only diagnostic.
- System-specification reports are removed: no `--system-specs` flag or
  copy-system-specs action.
- The filesystem-watcher debug window, recording API, and JSON exports are removed.
  Filesystem watching and rescan error logging remain available.
- File formats and protocol data may describe other operating systems, paths,
  line endings, or remote hosts; these do not imply editor host support.

## Build requirements

Use a Rust toolchain and the native SDK/C toolchain for the intended target.
Metal shaders compile at runtime on macOS. Linux remote-server builds require a
Linux host or a configured Linux cross-compilation toolchain.
Crates in the remote server's dependency tree must gate macOS-only build steps
(e.g. `xcrun`, SDK probes) and macOS paths behind `target_os = "macos"`, keeping
Linux branches for paths, crash handling, and LSP download assets. Upstream workflow
generators that depended on removed release and deployment infrastructure are absent.
macOS builds pass `-C force-unwind-tables=no` (`.cargo/config.toml`): rustc's
default async unwind tables are not compact-unwind encodable, which bloats
`__eh_frame` past ld's 16MB compact-unwind limit in dev builds. Panics and
backtraces still work through sync CFI.

## Packaging

- `install_remote_server.fish` runs on a Linux host with the build prerequisites
  installed, builds the remote server in release mode, and installs it as
  `~/.local/bin/ed_server` for SSH editing.
- `release.fish` builds `target/release/ed.app` for the host Mac: `ed` and
  `cli` binaries in `Contents/MacOS`, `crates/zed/resources/Info.plist` with the
  Cargo.toml version, an icon generated from `app-icon@2x.png`, and `Document.icns`.
- App icon: Nord-dark rounded square with an inset screen showing `</>` in
  `#88C0D0`. Source is `crates/zed/resources/app-icon.svg`; the PNGs are rendered
  from it at 512 and 1024 px.
- The bundle is for local use only: no codesigning, entitlements, provisioning
  profiles, notarization, DMG, or universal binary. No `cargo-bundle` metadata.
- Build scripts do not query git.

## macOS bindings

- AppKit/Foundation access uses the `objc2` crate family only; neither the
  deprecated `cocoa` crate nor the legacy `objc` 0.2 crate is a dependency.
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
