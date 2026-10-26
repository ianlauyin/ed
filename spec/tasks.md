# Tasks

## Decision

User-defined tasks are removed. The only way to run something from the editor
is the gutter run icon (runnables), which spawns the command in a terminal tab
("task output"). Anything else is run from a script in the terminal.

The `tasks_ui` crate is removed. The `task` crate stays only as the shared
template/variable/spawn types the run icon needs.

## Behaviour

- Run icons come from two sources:
  - tree-sitter `runnables.scm` tags matched against the task templates the
    buffer's language provides (`ContextProvider::associated_tasks`), e.g.
    `cargo run` for a Rust `main` or example;
  - LSP runnables (e.g. rust-analyzer), controlled by `lsp.<server>.enable_lsp_tasks`
    and `tasks.prefer_lsp` per language.
- `languages.<lang>.tasks.enabled = false` hides a language's template-based icons.
- Clicking the icon (or `editor::SpawnNearestTask`) resolves the template with
  `$ED_*` variables and spawns it in the terminal panel. The icon shows
  running / passed / failed state.
- A task terminal tab has a rerun button (and `terminal::RerunTask`) that
  respawns that tab's own command in place; there is no global rerun.
- Not supported: `tasks.json` (global, `.ed/`), `.vscode/tasks.json`, the task
  picker/modal, `task::Spawn`, `task::Rerun`, task history, task hooks (including
  worktree-creation setup tasks and the worktree picker's "Automate Setup"),
  git "Custom Commands" (`git-command` tagged tasks) in commit/ref context menus,
  the tasks JSON schema, and the "Run" app menu.
