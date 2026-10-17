# Project panel

## Clipboard

- Copy / Cut put the selected entries on the system clipboard as file URLs
  (like Finder's Copy), plus their absolute paths as text. So files can be
  pasted as real files into another window's project panel or Finder, and text
  targets (editor, terminal) still get the paths.
- Which entries count as files: every local entry, plus remote (SSH) entries
  whose absolute path also exists on this machine. This covers a VM that
  shares the host's folders at the same path. Other remote entries only put
  path text on the clipboard.
- Paste prefers the panel's own clipboard when the system clipboard still holds
  exactly the files this panel copied, so cut-moves and "copy" renaming on name
  clashes keep working. Otherwise, system-clipboard files (from another window
  or Finder) are copied into the selected folder, asking before replacing.
  Into a remote project they are uploaded.
- Cut followed by paste in another window copies rather than moves; only the
  window that cut can move.
