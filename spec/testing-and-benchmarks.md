# Testing and benchmarks

## Decision

The fork carries no automated tests or benchmarks. Correctness is checked by
building (`cargo check` / `cargo clippy`) and running the app.

## Behaviour

- No `#[test]`, `#[gpui::test]`, property tests, `tests/` integration targets,
  `benches/` targets, or benchmark crates. `cargo test` has nothing to run.
- No `test-support`, `bench-support`, `bench`, or `visual-tests` Cargo
  features, and no `cfg(test)`-gated code. Test doubles and harnesses (fake
  filesystem, fake language servers, test platform/scheduler/clock,
  `TestAppContext`, visual test runner) are removed.
- GPUI exposes no `test`, `bench`, or `property_test` macros, no
  `bench_group!`/`bench_main!`, and no `debug_selector` element hook.
- The `perf` tooling, `util_macros`, and the dylint UI test fixtures are removed.
- GPUI and component-preview examples and their development-only dependencies
  are removed. GPUI keeps the development dependency needed by its documentation examples.
- GPUI leak detection (`GPUI_LEAK_DETECTION` / `leak-detection` feature) stays as
  a runtime debugging aid.
- The GPUI element inspector is removed from all builds, including its UI,
  command, keybinding, style schema, reflection macro, and Cargo features.
  Rendering carries no inspector state or IDs. Debug source locations remain
  available for accessibility diagnostics and source navigation.

## Dependencies

Keep Cargo dependencies limited to the code, build steps, documentation, and
optional behaviour that remain in this fork. Remove unused shared workspace
dependency definitions too. Platform-specific dependencies belong to their
target sections; dependencies used by optional features, release builds, or
another target in the same package must remain available.
Default and all-feature workspace checks must have no unused-dependency warnings.
Keep the workspace lint enabled; exceptions for dependencies shared between a
library and its binary are confined to the affected target.
