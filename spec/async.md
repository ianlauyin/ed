# Async channels

No `postage`. Pick by shape:

| need | use |
|---|---|
| one reply / completion signal | `futures::channel::oneshot` |
| "done" barrier (fires when the sender drops) | `oneshot::Sender<()>`, never sent, just dropped; waiters ignore `Canceled` |
| bounded queue, one consumer | `futures::channel::mpsc` |
| queue with several competing consumers | `async_channel` |
| latest value / "something changed" | local `watch` crate |

## `watch` semantics

- Values coalesce: receivers see the latest value, not every send. Every
  `send` counts as a change, even an equal value.
- A receiver's initial value counts as seen (`Sender::receiver()`, clones,
  `channel()`). `changed()` only resolves after a later send. A loop that has
  to act on startup runs its body once before the first `changed()`
  (see `maintain_buffer_languages` and `maintain_workspace_config` in `lsp_store`).
- `borrow()` marks the current value as seen, so a send that races between
  `borrow()` and `changed()` is not lost. Wait for a condition with
  `while !*rx.borrow() { if rx.changed().await.is_err() { break } }`.
- The sender can't read the value back. Owners that need to read it keep a
  `(Sender, Receiver)` pair and borrow through the receiver.
- `changed()` returns `Err` once the sender is dropped, but a final unseen
  value is still delivered first. Waiters must treat `Err` as "give up"
  instead of looping.
- `send` with no live receiver returns `Err` but still stores the value;
  callers ignore it (`.ok()`).
- `Receiver::borrow` can clash with `std::borrow::Borrow`; call
  `watch::Receiver::borrow(&mut rx)` when it does.

## Scan barriers

`LocalWorktree::refresh_entries_for_paths` / `add_path_prefix_to_scan` return a
`oneshot::Receiver<()>` that resolves (with `Canceled`) once the scanner has
processed the request. Callers that don't wait `drop` it explicitly.
