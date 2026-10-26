# Upstream sync

## Decision

The fork is a squashed snapshot of Zed with no shared git history and is
intentionally diverging from upstream and from vendored crates. All code is
being reformatted with the fork's own `rustfmt.toml` (`rustfmt +nightly`) and
linted with the owner's standard lint set, so eventually every file differs
from upstream in layout as well as in behaviour.

- Changes may touch more files than the feature strictly needs: reformatting,
  lint fixes, and cleanups in passing are welcome. A noisy diff is acceptable.
- Clippy's style group is enabled workspace-wide along with selected nursery and
  pedantic lints. The workspace stays warning-free; where a lint fights a
  deliberate API shape or macro output, it is allowed at that item with a reason
  rather than disabled globally.
- Warning cleanup must preserve runtime behaviour, including integer truncation,
  signed reinterpretation, overflow handling, branch order, and rendering output.
  It must not add clamping, new panics, or fallback values to silence a lint.
- Upstream changes are ported by reading and understanding them, then
  re-applying the intent against the fork's code. There are no merges, rebases,
  cherry-picks, or `git diff | git apply` — textual patches are not expected to
  apply.
- The same applies to vendored crates: they are owned code, not pristine
  copies, and are not re-synced by replacing files.

## Behaviour

- Upstream remote: `zed-industries/zed`, branch `main`.
- Snapshot base: upstream `7dc397b61b` (v1.25.0, 2026-10-14).
- Ports match upstream by the PR number in the subject (`(#12345)`, one
  parenthesized number per PR), not only the `-x` trailer: some earlier ports
  were made without it.
- Port fixes to crates and behaviour the fork keeps; skip commits that only touch
  removed crates or platforms (agent/ACP, collab, extensions, sidebar,
  Windows/Linux/iOS, debugger).
- When porting, keep the fork's side: drop changes to deleted files, and
  strip re-added tests, benches, `test-support` features, and removed-platform
  dependencies (see `testing-and-benchmarks.md`).
- The app version stays at the snapshot's version unless deliberately bumped.

## Sync status

- Reviewed up to upstream `e96ffb0fd3` (2026-10-23, #66028): every commit in
  `7dc397b61b..e96ffb0fd3` was either ported or declined below. The next review
  starts at `git log e96ffb0fd3..upstream/main`.
- Ported in the 2026-10-23 pass: #65856, #65829, #65995, #65998, #65939,
  #65769, #65956, #65997 (security bumps, lockfile only), #65795, #60894,
  #64344, #65660, #59224, #65189, #64744.

### Declined

Reviewed and deliberately not ported. Don't re-evaluate these unless a later
upstream change depends on them or the need changes.

- UI/behaviour fixes not wanted: #66017 (scrollbar settings not applying to
  open scrollbars), #65852 and #65968 (cache resolved fonts, warn only for
  explicitly requested fonts), #65252 and #65253 (LSP dynamic registrations
  honouring document selectors/patterns; large `lsp_store` change).
- Features not wanted: #65559 (git panel discard keeps staged changes),
  #65776 (project panel sort by extension), #65950 (Taffy 0.13 → 0.15),
  #64394 (Display P3 colour space), #44262 (built-in Pyrefly server),
  #65863 (origin of conflicting app borrows), #61528 (recent commits for AI
  commit messages), #65389 (open README by default), #65531 (welcome-screen
  project action buttons), #65738 (Subsecond hot reload).
- Churn or refactors with no fork benefit: #65715, #65714, #65713, #65932,
  #65933, #65934 (`gpui_macos` objc2 migration), #65792 and #65782 (`path`
  helpers / `PathStyle` normalization ahead of upstream's `join_path`),
  #65805 (`ScrollWheelEvent` → `ScrollEvent` rename, for iOS), #65970
  (web-aware `Modifiers::secondary`), #65821 (proptest bump), #65593 and
  #65929 (version bumps).
- Removed platforms or areas: #63102 (Windows trust paths), #65722 (Go test
  cwd on Windows), #63068, #65806, #65975, #64239, #65658, #65858 (iOS),
  #53198, #66025, #65371 (debugger), #65470 (agent lazy markdown; its
  `gpui::list` and `markdown` changes only serve the agent thread view),
  #65381, #65702 (extensions), #65280 and the Node/Wasmtime parts of #65956
  (managed Node, Wasm extensions), plus all agent/ACP, sidebar,
  language-model, collab, Linux/Windows backend and CI commits.

### Things to do on the next pass

1. `git fetch upstream main`, then list `git log --reverse
   e96ffb0fd3..upstream/main`. Drop commits whose PR number is already in the
   fork's log (`git log --format=%s | grep -oE '\(#[0-9]+\)'`) or in Declined.
2. Drop commits that only touch crates the fork removed (compare
   `git show --name-only` against `ls crates`), then sort the rest into
   correctness fixes, user-visible fixes, and features, and ask which to port.
3. Port each as its own commit: upstream subject, short body, `(cherry picked
   from commit <full hash>)` trailer. Strip tests, test-support helpers and
   removed-platform code; say in the body what was dropped.
4. `rustfmt +nightly` touched files and keep `cargo clippy --workspace`
   warning-free; prefer `&Window`/`&Context` when clippy flags an unused `mut`.
5. Dependency bumps: `cargo update -p <crate> --precise <version>` per crate,
   then revert any unrelated transitive reselections Cargo makes in
   `Cargo.lock` and confirm with `cargo check --locked -p ed`.
6. Update `/spec` where a fix sets a lasting rule, then move the reviewed
   range and the new Declined entries into this file.
