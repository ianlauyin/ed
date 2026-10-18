# AI

## Decision

This fork has no AI features. The agent, ACP, skills, AI onboarding, model
provider, Copilot, edit prediction, prompt, sandbox, and threads sidebar crates
are removed. There is no `disable_ai` configuration option.

## Behaviour

- No agent panels, threads sidebar, AI settings pages, AI onboarding, inline
  assist, commit-message generation, or edit predictions.
- No agent registry, agent servers, MCP context servers, model providers, or AI
  network initialization in local or remote projects.
- No agent-only leftovers: no git checkpoints, diff review comments ("Send
  Review to Agent"), agent font settings (`agent_ui_font_*`,
  `agent_buffer_font_*`), or embedded/read-only terminal views.
- AI actions and default keybindings are removed. Legacy AI bindings cannot
  shadow normal editor bindings.
- Language-server completions, code actions, diagnostics, Git tools, and
  terminals remain available.
