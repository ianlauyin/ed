# Platform support

## Decision

macOS is the only supported editor and CLI host. Both Apple Silicon and Intel
Macs remain supported, with a minimum macOS version of 10.15.7. The headless
remote server supports macOS and Linux.

## Behaviour

- The editor uses native macOS windows, Metal rendering, system menus, and window tabs.
- The native menu bar has no Help menu.
- Ship only macOS default bindings and macOS variants of alternate keymaps.
- The Linux remote server needs no display server, GPU, desktop integration, or fonts.
  It retains background execution, timers, and a foreground event loop for remote projects.
- Linux remote data, configuration, and logs follow XDG directory settings.
- Linux desktop, FreeBSD, Windows, and browser backends, packaging, and associated
  build/test/release tooling are removed.
- Unsupported editor and CLI targets fail with a macOS-only diagnostic.
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
