# Testing and benchmarks

## Behaviour

- No `test-support`, `bench-support`, `bench`, or `visual-tests` Cargo
  features, and no `cfg(test)`-gated code. Test doubles and harnesses (fake
  filesystem, fake language servers, test platform/scheduler/clock,
  `TestAppContext`, visual test runner) are removed.
- GUI testing goes through the opt-in automation socket and `edctl`
  (see `automation.md`); it is a runtime switch, not a Cargo feature.
- GPUI exposes no `test`, `bench`, or `property_test` macros, no
  `bench_group!`/`bench_main!`, and no `debug_selector` element hook.
- The `perf` tooling, `util_macros`, and the dylint UI test fixtures are removed.
- GPUI and component-preview examples and their development-only dependencies
  are removed. GPUI keeps the development dependency needed by its documentation examples.
- GPUI entity leak detection is removed: no `leak-detection` feature,
  `GPUI_LEAK_DETECTION`/`LEAK_BACKTRACE` handling, handle tracking, or
  `assert_released`/leak-snapshot APIs, and no `track-project-leak` feature in zed.
- The component preview page and its `workspace::OpenComponentPreview` action are removed.
- The GPUI element inspector is removed from all builds, including its UI,
  command, keybinding, style schema, reflection macro, and Cargo features.
  Rendering carries no inspector state or IDs. Debug source locations remain
  available for source navigation.

## Dependencies

Development and release builds, with default or all features enabled, must be
warning-free, including all Cargo targets and the configured Clippy lints.
This applies to the macOS workspace and Linux remote-server targets.

Keep Cargo dependencies limited to the code, build steps, documentation, and
optional behaviour that remain in this fork. Remove unused shared workspace
dependency definitions too. Platform-specific dependencies belong to their
target sections; dependencies used by optional features, release builds, or
another target in the same package must remain available.
Default and all-feature workspace checks must have no unused-dependency warnings.
Keep the workspace lint enabled; exceptions for dependencies shared between a
library and its binary are confined to the affected target.
SQLite query macros validate syntax on every supported build host, including
Linux hosts building the remote server.
Avoid duplicate major versions in `Cargo.lock` (e.g. only `bitflags` 2.x and
one `png`); prefer bumping the dependent over accepting a second copy.
LSP protocol types are vendored in `crates/lsp_types` (from the Zed fork of
`lsp-types` 0.95.1, MIT) instead of a git dependency, so their dependencies follow
the workspace. Upstream's newer `fluent_uri`-based releases are not adopted.
Unused parts (LSIF, `proposed` extensions) are removed.
