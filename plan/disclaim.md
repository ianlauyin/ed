# Disclaiming macOS responsibility for child processes

Question: can we stop macOS listing `ed` under System Settings → Login Items →
"Allow in the Background"?

Status: implemented on `feature/disclaim` (commit `9900469014`). Not merged,
pending review of the overhead.

## Finding

- `ed` registers no login item, launch agent, `SMAppService` or background-task
  API. The bundle `Info.plist` and the linked frameworks are clean.
- `sfltool dumpbtm` shows a record "ed - background tasks" (type `0x2000`,
  parent `dev.ed`, `/Applications/ed.app`). macOS creates it when a process the
  app spawned keeps running, detached or after the app quits. Each process is
  charged to a "responsible" process, which is normally the app that spawned it.
- The likely source here is `~/.ssh/config` with `Host *`, `ControlMaster auto`
  and `ControlPersist 600`. The first `ssh` that ed runs (git fetch/push/pull,
  or a remote connection) forks a detached `[mux]` master that lives on for 10
  minutes, charged to ed. The ed log shows a git remote operation at 10:55:03
  and the background record was updated at 10:56:15.
- Jobs left running in the integrated terminal (`cmd &`, `nohup`) are charged
  the same way.
- Not the cause: `edctl` / UI tests. They launch the unbundled
  `target/debug/ed`, and `edctl` exits right after launching.
- The existing record won't go away on its own. Clear it with the Settings
  toggle or `sudo sfltool resetbtm` (which resets the list for every app).

## Mechanism

- `responsibility_spawnattrs_setdisclaim(attrs, 1)` (libSystem SPI, used by
  Chromium, iTerm2 and VS Code) makes a `posix_spawn`ed process responsible for
  itself. Disclaiming applies only at exec time. A process can't disclaim
  itself: `responsibility_set_pid_responsible_for_pid` returns -1 when
  unprivileged.
- Verified on this machine:
  - fork + exec: the child stays charged to the parent's responsible process
  - `posix_spawn` with disclaim: the child is responsible for itself
  - fork, then `posix_spawn(POSIX_SPAWN_SETEXEC)` with disclaim: the child is
    responsible for itself, which lets the hook run from `pre_exec`
- Descendants inherit responsibility, so disclaiming the top process (shell,
  ssh, git) also covers what it starts, such as the ssh mux.

## Design (as implemented on the branch)

- `util::disclaim`:
  - `DisclaimedExec`: path, argv, envp and spawn attributes (`SETEXEC` and
    disclaim) are all built in the parent, so the child between fork and exec
    only makes async-signal-safe calls. The child sets `environ` and calls
    `posix_spawn`. If that fails, it falls back to the plain exec.
  - `child_env` rebuilds the final environment (inherited environment, unless
    cleared, plus the command's explicit changes), because std installs the
    command's environment only after the `pre_exec` hooks run.
  - `resolve_program` does the `execvp` PATH lookup in the parent.
- `util::command::new_disclaimed_command`: opt-in. It installs the `pre_exec`
  hook when the command is built (`async-process` has no `pre_exec`), and fills
  the shared exec plan just before spawn. `new_command` is unchanged.
- Uses of the disclaimed command: `ssh`/`scp`/`sftp` (remote transport and the
  `path_exists` probe), and git `push`/`pull`/`fetch` via
  `GitBinary::build_remote_command`.
- Terminal: alacritty owns the PTY spawn and its `pre_exec`, so the shell runs
  through the ed binary as `ed --exec-disclaimed <program> <args…>`. `main`
  handles that argument first and re-execs with all signals reset to default,
  because Rust's runtime ignores `SIGPIPE`. With no explicit shell, ed builds
  alacritty's macOS default `login -flp user /bin/zsh -fc "exec -a -<shell>
  <shell>"` itself. The trampoline is registered only by the macOS app, so
  tests and Linux pass the shell through unchanged.
- Spec: `spec/platform.md` gains an "Allow in the Background" bullet.

## Overhead

None of this has been measured yet.

- Plain commands (`git status`, LSPs, …): none. They stay on std's
  `posix_spawn` fast path.
- Disclaimed commands (ssh, git network operations): std must fork, not
  `posix_spawn`, because a `pre_exec` hook exists. Forking a large app
  copies its page tables, so expect roughly sub-millisecond to a few ms.
  The second exec in the child is cheap. These operations already take
  network time.
- Terminal open: one extra exec of the full `ed` binary (dyld load of AppKit,
  Metal, etc., plus Rust runtime init) before `login`. This is the most
  visible cost, probably tens of ms per new terminal.
- Behaviour change: disclaimed processes face macOS privacy checks as
  themselves, not as ed. A shell touching `~/Documents` may prompt for or be
  denied access, rather than being denied silently under ed.

## Options to weigh

1. Merge as is: covers ssh, git and the terminal.
2. Cheaper terminal: replace the ed-binary trampoline with a tiny helper
   executable in the bundle (`Contents/MacOS/ed-exec`, just argv →
   `posix_spawn`), which avoids loading AppKit for each terminal.
3. ssh and git only: drop the terminal part. That removes the per-terminal
   cost but leaves terminal background jobs charged to ed.
4. No code change: pass `-o ControlPersist=no` to the ssh that ed runs. That
   fixes the remote-connection mux, but not git (which uses the user's ssh
   config) or the terminal.
5. Do nothing: switch the Settings toggle off. Background jobs keep running
   either way, and the toggle only controls the listing and notification.

## To measure before deciding

- Terminal open latency, `main` vs `feature/disclaim`: time from
  `terminal_panel::ToggleFocus` until the first prompt via `edctl wait`.
- `git fetch` wall time with and without disclaim. Expect it to be lost in
  network noise.
