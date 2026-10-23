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
- Snapshot base: upstream `7dc397b61b` (v1.25.0, 2026-10-14). Find newer
  candidates with `git log 7dc397b61b..upstream/main`, skipping commits already
  ported. Match ports by the upstream PR number in the subject (`(#12345)`), not
  only the `-x` trailer: some earlier ports were made without it.
- Port fixes to crates and behaviour the fork keeps; skip commits that only touch
  removed crates or platforms (agent/ACP, collab, extensions, sidebar,
  Windows/Linux/iOS, debugger).
- When porting, keep the fork's side: drop changes to deleted files, and
  strip re-added tests, benches, `test-support` features, and removed-platform
  dependencies (see `testing-and-benchmarks.md`).
- The app version stays at the snapshot's version unless deliberately bumped.
