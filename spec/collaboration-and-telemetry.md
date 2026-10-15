# Collaboration and telemetry

This fork is a single-user editor. Collaboration panels, channels and channel
notes, calls, screen sharing, participant controls, account menus, and sign-in
entry points are removed, together with the collaboration server, UI, client,
and account API crates. Unused account, channel, call, and following protocol
definitions are removed. Local services use HTTP directly; remote editing uses RPC.

The title bar retains project, worktree, Git, and platform window controls.
These controls initialize independently of collaboration. Old collaboration and
account visibility settings no longer expose or enable features.

No usage events, activity metrics, machine identifiers, remote telemetry forwarding,
hang reports, or automatic crash uploads are collected or sent. Telemetry settings,
onboarding switches, and telemetry log actions are removed. Local diagnostic logs
and manually requested performance reports remain available. Local minidumps may
be explicitly enabled with `ZED_GENERATE_MINIDUMPS`.

SSH and WSL editing remain supported; their shared transport and
buffer synchronization primitives remain in the editor.
Reconnection uses separate timeouts for the new connection's handshake and
for the resync that follows; the resync timeout starts only once the handshake
completes.

The application updater, update checks, downloads/installers, helper executable,
release-note actions, and update indicators are removed. Updates are installed
externally. The obsolete extension auto-update setting is also removed.

SSH connections require an executable server installed by the user at
`~/.local/bin/ed_remote_server` on the target host. Connection setup checks that
it runs, then uses it for the remote session and command execution. A missing or
unusable binary fails the connection with an error naming the required path.
SSH never downloads, builds, uploads, or installs a remote server. WSL retains
its remote-server installer, independently of any application updater.

Feature flags and local flag overrides are removed.
Diff-review gutter controls and intentional panic/crash
actions are disabled and cannot be enabled through settings or environment variables.

Feedback actions, bug-report links, and feature-request/email menu entries are
removed. Local database failures still display a diagnostic notification.

The editor has a single application identity and icon, with no release-channel
selection, branding, or channel-specific settings overrides. Build version and
commit metadata remain available. Existing local database state remains in the
same directory. Upstream remote-server release identifiers remain for WSL
compatibility; WSL uses the stable remote-server distribution.

The ztracing instrumentation and Tracy integration are removed. The local GPUI
performance profiler and diagnostic logs remain available.
