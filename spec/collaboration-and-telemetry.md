# Collaboration and telemetry

This fork is a single-user editor. Collaboration panels, channels and channel
notes, calls, screen sharing, participant controls, account menus, and sign-in
entry points are removed, together with the collaboration server, UI, client,
and account API crates. Unused account, channel, call, and following protocol
definitions are removed. Local services use HTTP directly; remote editing uses RPC.
GPUI has no screen-capture API or `screen-capture` feature, and the app does not
link ScreenCaptureKit or ReplayKit.
GPUI has no video `surface` element or CoreVideo pixel-buffer render path.

The title bar retains project, worktree, Git, and platform window controls.
These controls initialize independently of collaboration. Old collaboration and
account visibility settings no longer expose or enable features.

No usage events, activity metrics, machine identifiers, remote telemetry forwarding,
hang reports, or automatic crash uploads are collected or sent. Telemetry settings,
onboarding switches, and telemetry log actions are removed. Local diagnostic logs
and manually requested performance reports remain available. Local minidumps may
be explicitly enabled with `ZED_GENERATE_MINIDUMPS`.

SSH editing remains supported, including transport and buffer synchronization.
WSL transport, UI, settings, CLI requests, and saved connection restoration are removed.
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
The editor never downloads, builds, uploads, or installs a remote server.
Remote-server release fetching and archive caching are removed. The obsolete
`upload_binary_over_ssh` setting is removed.

Feature flags and local flag overrides are removed.
Diff-review gutter controls and intentional panic/crash
actions are disabled and cannot be enabled through settings or environment variables.

Feedback actions, bug-report links, and feature-request/email menu entries are
removed. Local database failures still display a diagnostic notification.

The editor has a single application identity and icon, with no release-channel
selection, branding, or channel-specific settings overrides. Build version and
commit metadata remain available. Existing local database state remains in the
same directory.

The ztracing instrumentation and Tracy integration are removed. The GPUI
`profiler` feature is removed too: no task/frame timing collection, performance
profiler panel or `instrumentation.performance_profiler` setting, FPS/frame debug
overlay, input-latency histogram, hang monitor, or remote profiling RPC. Diagnostic logs remain available.
