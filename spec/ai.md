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
- AI actions and default keybindings are removed. Legacy AI bindings cannot
  shadow normal editor bindings.
- Language-server completions, code actions, diagnostics, Git tools, terminals,
  and debugging remain available.
