# Debugger and dev containers

## Decision

This fork has no debugger and no dev container support. The `dap`,
`dap_adapters`, `debug_adapter_extension`, `debugger_tools`, `debugger_ui`, and
`dev_container` crates are removed, along with the Docker/Podman remote
transport that only served dev containers.

## Behaviour

- No debug panel, breakpoints (gutter, actions, persistence), inline values,
  active-debug-line highlight, debug code actions/scenarios, debug console, or
  DAP log view. The "Run" menu only spawns and edits tasks.
- `debug.json` (global and `.zed/`) and `.vscode/launch.json` are not read, and
  have no JSON schema. Tasks (`tasks.json`, `.vscode/tasks.json`) are unaffected.
- Settings `dap`, `debugger`, `debuggers` (per language), `gutter.breakpoints`,
  `inlay_hints.show_value_hints`, `dev_container_connections`, `use_podman`, and
  `dev_container_use_buildkit` no longer exist. Theme keys `debugger.accent` and
  `editor.debugger_active_line.background` are ignored.
- Language configs no longer declare `debuggers` and grammars have no
  `debugger.scm` queries.
- No "Connect Dev Container" entry, `.devcontainer` suggestion toast,
  `projects::OpenDevContainer` action, or `--dev-container` CLI flag. Remote
  projects are SSH or WSL only; saved Docker connections are not restored.
- Debugger RPC messages and the `GetProcesses` request are removed from the
  protocol (field numbers reserved). The workspace database drops the
  `breakpoints` table.
- Gutter right-click still offers git blame and run status.
