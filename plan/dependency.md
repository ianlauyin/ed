# Dependency size

Question: how big is each local crate, and how much external code does it pull
in? Used to guide cleanup.

Snapshot: Oct 2026 (commit `c411ccc1`), macOS aarch64 build of `zed`, `cli` and
`remote_server`, normal + build deps (no dev deps).

## Method

- Graph: `cargo tree --workspace -e normal,build --target aarch64-apple-darwin --prefix depth -f '{p}'`.
  Don't use `cargo metadata`'s resolve: it over-approximates (e.g. lists `rav1e`,
  512 external crates instead of 460).
- Lines: non-blank `.rs` lines per crate dir, skipping `tests/`, `benches/`,
  `examples/`, `fuzz/`. C/ObjC (`.c .h .cc .cpp .m .mm`) counted separately.
- "Exclusive" = external crates that leave the build if that local crate is
  removed (reachability from the three binaries with the crate banned).
- Rough numbers: `libc` and `objc2-*` contain lots of code for other platforms
  or features that never compiles; proc-macro/build deps (`syn`, `cbindgen`,
  `prost-build`) cost build time but don't ship.

## Totals

| | crates | Rust lines | C lines |
|---|---|---|---|
| local | 106 | 473k | – |
| external | 460 | 3.4M | 2.2M |

## Largest local crates

| crate | lines |
|---|---|
| mermaid | 82k (no external deps) |
| editor | 59k |
| project | 44k |
| gpui | 40k |
| workspace | 23k |
| git_ui | 22k |
| ui | 20k |
| language | 13k |

## Exclusive external weight per local crate

Most UI/feature crates (search, outline, picker, git_ui, ...) have 0 exclusive
deps: removing them only saves their own code.

| crate | excl crates | excl lines | main culprits |
|---|---|---|---|
| gpui | 48 | 422k | `read-fonts`/`skrifa`/`harfrust` (via `usvg`), `object` (via `stacksafe`→`stacker`→`psm`), `lyon_tessellation`, `image` with all default formats (`moxcms`, `pxfm`, exr, tiff, ...) |
| gpui_macos / gpui_apple | 24 | 302k | `objc2-*` bindings (feature-gated, compiles less than it reads), `cbindgen` at build time |
| proto | 21 | 123k | build-time: `prost-build`, `protox`, `prost-reflect`, `petgraph`, `miette` |
| cli | 13 | 121k | `ipc-channel` (50k), `quick-xml`, `plist` |
| remote_server | 3 | 90k | `jiff` (85k), only via `env_logger` default features |
| project | 10 | 70k | `fancy-regex`, `nom`, `const_format`, `wax` |
| fs | 6 | 63k | `unicode-normalization`, `objc2-core-services`, `notify`, `trash` |
| util | 4 | 38k | `nix` |
| sqlez | 3 | 27k + 514k C | bundled SQLite (`libsqlite3-sys`) |
| language | 6 | 23k + 92k C | `syntect` + `onig` (second highlighter next to tree-sitter) |
| ui | 4 | 18k | `itertools`, `documented` |
| terminal | 6 | 17k | `alacritty_terminal` |
| markdown | 1 | 13k | `pulldown-cmark` |

## Where the C comes from

- Tree-sitter grammars ≈1.5M (generated `parser.c`): typescript 567k, bash
  352k, rust 207k, markdown 138k, python 131k.
- SQLite 514k, `onig_sys` 92k, `zstd-sys` 70k.

## Largest external crates (Rust lines)

`encoding_rs` 137k, `libc` 117k, `objc2-app-kit` 101k, `pxfm` 95k,
`read-fonts` 92k, `jiff` 86k, `objc2-foundation` 67k, `rustix` 66k,
`regex-automata` 62k, `regex-syntax` 56k, `objc2-security` 55k,
`objc2-io-kit` 52k, `ipc-channel` 50k, `syn` 3 49k, `syn` 2 48k, `object` 48k.

## Cleanup candidates

- [x] `image`: features trimmed to png/jpeg/gif/webp; `gpui::ImageFormat` lost
  bmp/tiff/ico/pnm (drops `exr`, `tiff`, `qoi`, `half`, `fax`, `zune-inflate`,
  ...). `moxcms`/`pxfm` stay: they are non-optional deps of `image` 0.25.
- [x] `env_logger`: removed; `remote_server` uses a fixed-level JSON logger (drops `jiff`).
- [ ] `syntect`/`onig`: check whether still needed alongside tree-sitter.
- [ ] `proto`: check in generated code to drop the prost/protox build stack.
- [ ] `stacksafe`: pulls `object` (48k) through `psm`'s build script; check need.
- [x] `cli`: removed with `ipc-channel`, `quick-xml`, `plist`.
- [x] Remove dead `streaming_diff`.
- [x] Duplicate versions: bumped `rand` 0.10, `itertools` 0.15, `lru` 0.19,
  `cbindgen` 0.29, `protox` 0.10, `uuid` 1.29. Gone: `rand`/`getrandom` 0.3
  (mac), `toml`, `logos`, `heck`. Still duplicated, blocked upstream (already
  latest):
  - `syn` 2/3: ~30 proc macros still on syn 2 (serde_derive is on 3).
  - `hashbrown` 0.15 (`imara-diff` 0.2, `petgraph`), 0.16 (`libflate_lz77` via
    `include-flate`), 0.17 (`indexmap`).
  - `itertools` 0.14: `prost-build`/`prost-derive`, `wax` 0.7.
  - `unicode-width` 0.1: `miette` (protox, build only).
  - `base64` 0.22: pinned `alacritty_terminal` fork; `convert_case` 0.10:
    `derive_more`.
- [x] `thiserror` 1: dropped `postage` (oneshot/barrier → `futures` oneshot,
  mpsc → `futures` mpsc, dispatch → `async-channel`, watch → local `watch`
  crate); updated `lyon` for its `lyon_extra` copy. Local `watch` treats the
  initial value as seen, unlike postage, so loops that relied on postage's
  immediate first wakeup now run their body once up front.
