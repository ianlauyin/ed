# Binary size by crate

Question: what takes up space in the release `ed` binary?

Snapshot: Nov 2026, `cargo bloat --release -p ed --crates`, arm64 macOS,
release profile (`lto = "thin"`, `codegen-units = 1`).

## Whole binary

Update: with `panic = "abort"` and the `ed` package override removed (it
silently stopped cargo from passing `-C strip`), the shipped binary went from
117.6 MiB (unstripped) to 64.9 MiB: `.text` 49.4 MiB, unwind / exception
tables under 0.3 MiB, symbol table gone. The table below is the snapshot before
those changes, sized as if stripped (about 83 MiB):

| Part | Size | What it is |
|---|---|---|
| `__TEXT,__text` | 59.3 MiB | machine code (broken down by crate below) |
| `__TEXT,__const` | 10.9 MiB | read-only data: tree-sitter parse tables, embedded assets (icons, themes, spell dictionary ~0.9 MiB), string literals |
| unwind / exception tables | 8.0 MiB | `__gcc_except_tab` 3.8, `__eh_frame` 3.1, `__unwind_info` 1.1. `panic = "abort"` would remove most of `__gcc_except_tab` |
| `__DATA_CONST` | 2.4 MiB | vtables, relocated constants |
| symbol table | ~36 MiB | was shipped: the `ed` package override dropped `-C strip` |

The binary is memory-mapped, so pages that are never touched cost no RAM. The
size mostly matters for disk usage and cold-start page-ins, not for memory
usage while running.

## `.text` by crate

| Crate | Size | % of `.text` | Notes |
|---|---|---|---|
| gpui | 12.4 MiB | 21.0% | UI framework, generic over every view/element type |
| std | 10.5 MiB | 17.8% | includes monomorphized std generics (`Vec`, iterators, `HashMap`) instantiated by every crate |
| editor | 2.8 MiB | 4.7% | |
| project | 2.7 MiB | 4.5% | |
| mermaid | 2.5 MiB | 4.3% | largest optional feature: mermaid diagrams in markdown preview |
| [Unknown] | 2.4 MiB | 4.0% | mostly C code: tree-sitter grammars, sqlite, zstd |
| workspace | 2.0 MiB | 3.3% | |
| sum_tree | 1.4 MiB | 2.3% | generic B-tree, instantiated per item type |
| git_ui | 1.1 MiB | 1.9% | |
| picker | 1.1 MiB | 1.8% | generic over every picker delegate |
| async_task | 957 KiB | 1.6% | one monomorphized task per spawned future type |
| serde_core | 673 KiB | 1.1% | |
| scheduler | 625 KiB | 1.0% | |
| ui | 598 KiB | 1.0% | |
| prost | 596 KiB | 1.0% | protobuf for the remote protocol |
| futures_util | 501 KiB | 0.8% | |
| image_webp | 500 KiB | 0.8% | image viewer, WebP decoding |
| language | 484 KiB | 0.8% | |
| lsp | 479 KiB | 0.8% | |
| serde_json | 468 KiB | 0.8% | |
| multi_buffer | 464 KiB | 0.8% | |
| anyhow | 406 KiB | 0.7% | |
| regex_automata | 393 KiB | 0.6% | |
| harfrust | 377 KiB | 0.6% | text shaping for SVG text, pulled in by gpui through usvg |
| usvg | 350 KiB | 0.6% | SVG parsing |
| everything else | ~13 MiB | ~22% | ~300 crates, each under 0.6% |

Other crates that only exist for one feature: `zune_jpeg` 202 KiB + `image`
177 KiB (image viewer), `spellbook` 219 KiB (spell check), `syntect` 85 KiB
(fallback highlighting), `tabular_data_preview` 138 KiB, `markdown_preview`
137 KiB.

## Takeaways

- There is no single big dependency to cut. About 40% of the code is gpui
  plus std generics, which is the cost of using gpui at all.
- `mermaid` (a local crate used only by `markdown_preview`) is the biggest
  feature you could drop (2.5 MiB) if you don't use mermaid diagrams.
- Generic-heavy crates (`gpui`, `sum_tree`, `picker`, `async_task`) grow with
  every new view or task type. That's expected and doesn't hurt runtime.
- Embedded data (`include_str!` / rust_embed) is a small part of the binary.
  The spell dictionary is ~1% of the file.
- cargo-bloat attributes symbols by name, so numbers are approximate. With LTO,
  inlined code is counted under the caller's crate.
