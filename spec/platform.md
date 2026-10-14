# Platform support

## Decision

macOS is the only supported host platform for the editor, CLI, and remote server.
Both Apple Silicon and Intel Macs remain supported. The minimum macOS version is
10.15.7.

## Behaviour

- Use the native macOS window system, Metal renderer, system menus, and window tabs.
- Ship only macOS default bindings and macOS variants of alternate keymaps.
- Linux, FreeBSD, Windows, and browser backends, native integrations, packaging,
  and platform-specific build/test/release tooling are removed.
- Unsupported editor and CLI build targets fail with a macOS-only diagnostic.
- File formats and protocol data may still describe other operating systems,
  paths, line endings, or remote hosts; these do not imply host-platform support.

## Build requirements

Use a macOS host with a Rust toolchain and the macOS SDK. Metal shaders continue
to compile at runtime. Upstream workflow generators that depended on removed
release scripts and deployment infrastructure are not part of this fork.
