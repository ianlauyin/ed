# File finder

- History (recently opened files, the current file, and selected matches)
  only lists paths that are still files. They are checked when the finder
  opens and again on every worktree update while it is open, so a file deleted
  meanwhile drops out without reopening the finder. Paths outside the worktree
  are checked on disk locally and through the remote server over SSH.
- Path ranking (`fuzzy_nucleo`) adds a small bonus for matches near the end of
  the path, so a path that repeats the query (`x/x.py` for `x`) ranks above one
  matching only once (`y/x.py`).
