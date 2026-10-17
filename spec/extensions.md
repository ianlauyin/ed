# Extensions and WebAssembly

## Decision

No extension system or WebAssembly build target. The editor runs natively on
macOS; the remote server runs natively on macOS and Linux.

## Behaviour

- Languages, grammars, themes, icon themes, and language servers come only from
  built-in code and user settings/files. Snippets come only from built-in code.
- Tree-sitter grammars are native only; Wasm grammars cannot be loaded.
- Extension crates, host proxies, builders, adapters, settings, and remote
  install/sync messages are removed.
- No Extensions page, menu item, keybinding, or install/update status.
- No browser backend, Wasm worker scheduler, or direct Wasm runtime/tooling
  dependencies. Native scheduling and timing use operating-system threads and clocks.
- Third-party dependencies may retain unused Wasm/WASI target entries in
  `Cargo.lock`; these are not built for supported targets.

## Build requirements

- Native builds need a Rust toolchain and the target platform's SDK/C toolchain.
- macOS Metal shaders compile at app startup; the Xcode Metal toolchain is not required.
- The CLI has no bundled uninstall script, uninstall flag, or compatibility feature.
