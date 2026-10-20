# Upstream sync

## Decision

The fork is a squashed snapshot of Zed with no shared git history. Upstream
changes are ported selectively by cherry-picking from the `upstream` remote
(`zed-industries/zed`, branch `main`); there are no merges or rebases.

## Behaviour

- Snapshot base: upstream `7dc397b61b` (v1.25.0, 2026-10-14). Find newer
  candidates with `git log 7dc397b61b..upstream/main`, skipping commits already
  ported. Match ports by the upstream PR number in the subject (`(#12345)`), not
  only the `-x` trailer: some earlier ports were made without it.
- Port fixes to crates and behaviour the fork keeps; skip commits that only touch
  removed crates or platforms (agent/ACP, collab, extensions, sidebar,
  Windows/Linux/iOS, debugger).
- Resolve conflicts with the fork's side: drop changes to deleted files, and
  strip re-added tests, benches, `test-support` features, and removed-platform
  dependencies (see `testing-and-benchmarks.md`). Keep the upstream code changes
  outside those areas.
- The app version stays at the snapshot's version unless deliberately bumped.
