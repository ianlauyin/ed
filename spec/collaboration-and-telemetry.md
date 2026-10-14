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

SSH, WSL, and container editing remain supported; their shared transport and
buffer synchronization primitives remain in the editor.

The application updater, update checks, downloads/installers, helper executable,
release-note actions, and update indicators are removed. Updates are installed
externally. The obsolete extension auto-update setting is also removed. Remote
connections retain only the remote-server installer needed for SSH, WSL, and
container setup; it runs when establishing those connections,
independently of any application updater.

Feature flags and local flag overrides are removed. Notebooks are always enabled.
Diff-review gutter controls, debugger history controls, and intentional panic/crash
actions are disabled and cannot be enabled through settings or environment variables.

Feedback actions, bug-report links, and feature-request/email menu entries are
removed. Local database failures still display a diagnostic notification.

The editor has a single application identity and icon, with no release-channel
selection, branding, or channel-specific settings overrides. Build version and
commit metadata remain available. Existing local database state remains in the
same directory. Upstream remote-server release identifiers remain for SSH, WSL,
and container compatibility; this fork uses the stable remote-server distribution.

The ztracing instrumentation and Tracy integration are removed. The local GPUI
performance profiler and diagnostic logs remain available.
