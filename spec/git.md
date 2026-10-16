# Git

## Decision

Git hosting providers are used only to build URLs locally (permalinks, commit
links, pull request links). They make no network requests.

## Behaviour

- Commit author avatars are removed everywhere (blame gutter, blame and commit
  tooltips, commit view, git graph, git panel history). Authors are shown by
  name/email only.
- `GitHostingProvider` has no avatar API; the `git` and `git_hosting_providers`
  crates make no HTTP requests.
- The `git.blame` settings group (`show_avatar`) is removed.
