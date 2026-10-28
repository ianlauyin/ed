# Tasks

## Decision

User-defined tasks are removed. The only way to run something from the editor
is the gutter run icon (runnables), which spawns the command in a terminal tab
("task output"). Anything else is run from a script in the terminal.

The `tasks_ui` crate is removed. The `task` crate stays only as the shared
template/variable/spawn types the run icon needs. A template is just label,
command, args, env, cwd and runnable tags; there are no per-task knobs.
`TaskTemplate` stays (de)serializable only because LSP runnables reach remote
clients as JSON.

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
- A task always runs in the terminal dock with the default system shell (the
  remote shell on a remote project), saves nothing first, and leaves its tab
  open with no summary lines when it finishes. Running a task whose tab
  already exists waits for that run to finish, then replaces the tab's output.
- A task terminal tab has a rerun button (and `terminal::RerunTask`) that
  respawns that tab's own command in place; there is no global rerun. The tab
  keeps the task as requested; wrapping it in the shell happens on each spawn,
  so a rerun never wraps an already wrapped command.
- Rust `#[cfg(test)] mod <name>` icons run `cargo test -p <pkg> -- <file module>::<name>::`
  (just `<name>::` in a crate root); the file's full module path isn't known,
  which is fine because libtest filters by substring.
- Not supported: `tasks.json` (global, `.ed/`), `.vscode/tasks.json`, the task
  picker/modal, `task::Spawn`, `task::Rerun`, task history, task hooks (including
  worktree-creation setup tasks and the worktree picker's "Automate Setup"),
  git "Custom Commands" (`git-command` tagged tasks) in commit/ref context menus,
  the tasks JSON schema, and the "Run" app menu.
