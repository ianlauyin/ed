# File finder

- History (recently opened files, the current file, and selected matches)
  only lists paths that are still files. They are checked when the finder
  opens and again on every worktree update while it is open, so a file deleted
  meanwhile drops out without reopening the finder. Paths outside the worktree
  are checked on disk locally and through the remote server over SSH.
