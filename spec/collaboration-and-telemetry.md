# Collaboration and telemetry

This fork is a single-user editor. Collaboration panels, channels and channel
notes, calls, screen sharing, participant controls, account menus, and sign-in
entry points are removed, together with the collaboration server, UI, client,
and account API crates. Unused account, channel, call, and following protocol
definitions are removed. Local services use HTTP directly; remote editing uses RPC
over the SSH channel only. The collab websocket transport (`Peer`, websocket
message framing, `async-tungstenite`) is removed.
GPUI has no screen-capture API or `screen-capture` feature, and the app does not
link ScreenCaptureKit or ReplayKit.
GPUI has no video `surface` element or CoreVideo pixel-buffer render path.

The title bar retains project, worktree, Git, and platform window controls.
These controls initialize independently of collaboration. Old collaboration and
account visibility settings no longer expose or enable features.

There are no collaborator replicas: buffers only ever see the local replica,
the SSH remote server replica, and local branches. Dead sharing/unsharing APIs
and the call-status-icon and public-channel-sharing settings are removed.

The theme has a single local player color (cursor and selection). Themes and
`theme_overrides` keep the `players` array format, but only the first entry is
read. Places that need several distinct colors (git blame, Mermaid diagrams, debug
ranges) use the theme's `accents` palette. The drag-and-drop cursor is the local
cursor at half opacity.

No usage events, activity metrics, machine identifiers, remote telemetry forwarding,
hang reports, or automatic crash uploads are collected or sent. Telemetry settings,
onboarding switches, and telemetry log actions are removed. Local diagnostic logs
and manually requested performance reports remain available. There is no crash
handler or minidump support: a panic logs its message and backtrace to the log
(stdout when attached to a terminal) and exits the process.

SSH editing remains supported, including transport and buffer synchronization.
WSL transport, UI, settings, CLI requests, and saved connection restoration are removed.
Reconnection uses separate timeouts for the new connection's handshake and
for the resync that follows; the resync timeout starts only once the handshake
completes.
The SSH username is passed verbatim with `-l` and the host after `--`
(`ssh … -l <user> -- <host>`), so domain usernames like `DOMAIN\user` work and
neither can be parsed as an ssh option; scp and sftp also put `--` before the
destination.

The application updater, update checks, downloads/installers, helper executable,
release-note actions, and update indicators are removed. Updates are installed
externally. The obsolete extension auto-update setting is also removed.

SSH connections require an executable server installed by the user at
`~/.local/bin/ed_server` on the target host. Connection setup checks that
it runs, then uses it for the remote session and command execution. A missing or
unusable binary fails the connection with an error naming the required path.
The editor never downloads, builds, uploads, or installs a remote server.
Remote-server release fetching and archive caching are removed. The obsolete
`upload_binary_over_ssh` setting is removed.

The remote server keeps no log file. Its log records are streamed to the client
as JSON and re-logged into the editor's own log. The server and proxy log at a
fixed `info` level; `RUST_LOG` is not read. The only file it writes is a
panic report (`server-<id>-panic.log` in the remote logs dir, overwritten per
panic), since a panicking server aborts before the record can be forwarded.

Feature flags and local flag overrides are removed.
Diff-review gutter controls and intentional panic/crash
actions are disabled and cannot be enabled through settings or environment variables.

Feedback actions, bug-report links, and feature-request/email menu entries are
removed. Local database failures still display a diagnostic notification.

The editor has a single application identity and icon. There is no
release-channel concept anywhere (app, remote client, or remote server). The app
version is the `ed` crate's Cargo.toml version; no commit SHA, build ID, or
bundle type is embedded or shown. Existing local database state remains in the
same directory.

The ztracing instrumentation and Tracy integration are removed. The GPUI
`profiler` feature is removed too: no task/frame timing collection, performance
profiler panel or `instrumentation.performance_profiler` setting, FPS/frame debug
overlay, input-latency histogram, hang monitor, or remote profiling RPC. GPUI has no
`profiling` crate instrumentation (no puffin/optick/superluminal/tracy hooks).
Diagnostic logs remain available.

Editor and remote RPC code have no direct `tracing` dependency. Verbose RPC
debug/trace output is removed; warnings and errors use `log` and the existing
local logging pipeline. Third-party dependencies may still use `tracing` internally.
