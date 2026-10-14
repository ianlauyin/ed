# Extensions

## Decision

No extension system. The fork does not load, install, or run extensions, and does
not depend on wasmtime.

## Behaviour

- Languages, grammars, themes, icon themes, language servers, debug adapters,
  context servers, and LLM providers come only from built-in code and user
  settings/files.
- Tree-sitter grammars are native only (`LanguageRegistry::register_native_grammars`);
  WASM grammars cannot be loaded, and tree-sitter is built without its `wasm` feature.
- The remote server does not sync or install extensions.
- No Extensions page, menu item, keybinding, or install/update status in the
  activity indicator.

## What remains

- The `extension` crate (`ExtensionHostProxy` and proxy traits) is still threaded
  through project/remote/language init, along with `language_extension`,
  `theme_extension`, and `debug_adapter_extension`. Nothing registers extensions
  through them, so they are inert and can be removed later.

## Build requirements

- `cargo build` needs only a Rust toolchain (plus the platform SDK).
- macOS: `gpui_apple` enables `runtime_shaders` by default, so Metal shaders are
  compiled at app startup and the Xcode Metal toolchain (`xcrun metal`) is not needed.
- `cli` enables `no-bundled-uninstall` by default because `script/` was removed.
