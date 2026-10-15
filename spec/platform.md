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
