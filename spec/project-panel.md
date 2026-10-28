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
- Duplicate copies the selected entries next to themselves ("copy" renaming)
  without touching either clipboard: a pending Copy/Cut and the system
  clipboard survive it.

## Moving entries

- When a drag-and-drop move or an undo/redo rename fails because the
  destination already exists, the error names both paths and says that a file
  or folder already exists there. Detection uses the error kind
  (`io::ErrorKind::AlreadyExists` locally, `ErrorCode::AlreadyExists` from a
  remote server), never the message text.

## Deleting

- There is a single `project_panel::Delete` action and a single "Delete"
  context-menu item. It always moves entries to the system trash; the panel
  never permanently deletes. Every successful delete is recorded on the
  panel's undo stack, and undo restores it from the trash.
- Backspace/Delete prompt first; Cmd-Backspace/Cmd-Delete skip the prompt
  (`skip_prompt: true`).
- Undo only works within the session: the mapping from undo entries to trashed
  items is in memory. After a restart, items are restored from the OS trash.
- Trashing uses the vendored `crates/trash` (macOS and Linux only). It merges
  upstream `trash-rs` fixes with Zed's fork, which is needed because upstream
  can't return the trashed item or restore on macOS:
  - macOS: `NSFileManager.trashItemAtURL` (no Finder/AppleScript, so no
    permission prompt and no "Put Back"); restore renames the item back.
  - Linux: freedesktop Trash spec. The home trash is used for the home trash's
    volume; other volumes use `.Trash/$uid` or `.Trash-$uid`, falling back to
    the home trash when the volume's trash isn't writable.
  - Moves never overwrite: the destination is claimed with an empty
    placeholder before renaming, and an occupied restore path is a collision.
    Cross-filesystem moves fall back to copy + remove, both ways.

## External libraries

- A path inside a library (as recognized by a language's `LibraryProvider`)
  joins one worktree rooted at that library instead of getting its own
  single-file worktree. Later files from the same library reuse it. This
  includes visible opens such as a file passed on the command line. Opening
  the library's root folder itself still makes a normal project root.
- A library worktree is invisible: it is not a project root for the title,
  search, tasks or terminals. It reuses the project's running language servers.
  It is returned only after its first scan, so manifest lookups (the crate's
  `Cargo.toml`) never see a half-scanned library. On a remote project the
  client's copy fills in asynchronously, so an active path whose entry isn't
  listed yet becomes the active entry (and is revealed) once it arrives.
  The host creates remote library worktrees itself (e.g. for a go-to-definition
  target), so they and the worktrees the client adds share one entry-id
  counter; the panel finds an entry's worktree by id alone.
- A library is listed in the panel, and held strongly, once one of its files
  becomes the active item. Until then it lives only as long as something
  references it, so crates that only appear in inlay hints or hovers don't
  pile up. Libraries are not persisted; restoring a tab brings its library back.
- The panel groups all listed libraries under one collapsible
  "External Libraries" row after the project roots. Each library is a
  package-icon root, sorted by name and nested one level under that row.
  The group row is a one-entry section with ids no worktree uses, so row
  indexing, selection and scrolling treat it like any other row. Revealing a
  library file expands the group, including the first time its library is
  listed. Auto-reveal skips ignored files, but not library files, which are
  often gitignored on their host.
- Outside the panel, a library file is labeled by its full path (multibuffer
  headers, breadcrumbs), since a library-relative path like `src/lib.rs` reads
  as a project file.
- Library sources are read-only:
  - Buffers open with `Capability::ReadOnly`, including buffers a remote host
    pushes to the client (e.g. a go-to-definition target). It can't be
    toggled: the tab's lock is shown disabled ("Locked Tab"), and
    "Make Tab Editable" and "Toggle Read Only" do nothing.
  - `Project` refuses to create, rename, move into or out of, trash or delete
    library entries. Copying out of a library is allowed.
  - The context menu shows only non-mutating actions, plus "Remove Library" on
    a library root.
