# Async patches research

Question: do we still need the `[patch.crates-io]` forks of `async-process` and
`async-task` (Cargo.toml), or should we reimplement on top of tokio?

Answer: both patches are still needed. Neither is legacy; tokio replaces neither.

## async-task (zed-industries fork, rev `015845a`, based on 4.7.1)

- Patch (Oct 2026): when polling a task, build the waker from the vtable
  stored in the task header. Upstream builds it from the associated constant,
  whose address can differ between references, so `will_wake()` returns false
  for clones of the same task waker. Effect: redundant wakeups and re-polls
  (performance bug, not correctness).
- Usage: gpui's executor is built on `async_task::Builder::new().metadata(RunnableMeta)`
  (`crates/scheduler/src/executor.rs`, `crates/gpui/src/platform_scheduler.rs`,
  `crates/gpui_apple/src/dispatcher.rs`). gpui needs a bare `Runnable` it can
  hand to its own schedule function: main thread, GCD queues, and the
  deterministic test scheduler.
- Tokio has no public "spawn a future, give me a runnable, I schedule it" API;
  its scheduler is built into the runtime. Moving gpui to tokio would lose
  main-thread dispatch and the deterministic test scheduler.
- Verdict: keep. Drop the patch once upstream smol-rs/async-task merges an
  equivalent fix (not checked yet).

## async-process (zed-industries fork, rev `0b6d671`, based on 2.5.0)

- Patch (Jun 2026): adds macOS-only `Child::adopt_raw_pid`, which registers an
  externally spawned pid with async-process's reaper.
- Only caller: `crates/util/src/command/darwin.rs`. It spawns with its own
  `posix_spawn` to get `POSIX_SPAWN_CLOEXEC_DEFAULT` (no leaked fds), reset
  exception ports, and SIGPIPE reset to default, then adopts the pid.
- `tokio::process` can't adopt an outside pid or set those spawn attributes
  either. Switching would also mix two reactors (smol's async-io and tokio)
  across the ~20 crates that use `smol::process`.
- Verdict: keep, or drop the fork with a small in-house reaper in
  `util::command::darwin`: wait on the pid with kqueue `EVFILT_PROC` /
  `NOTE_EXIT`, then `waitpid` (~100 lines).

## Tokio

Removed. It only backed reqwest for remote images in `img()`; remote image
loading was dropped along with `gpui_tokio`, so the app is smol-only.

## Next steps

- [ ] Check whether upstream async-task has merged the waker vtable fix; drop
      the patch if so.
- [ ] Optional: replace `adopt_raw_pid` with an in-house kqueue reaper and drop
      the async-process patch.
