#!/usr/bin/env fish
# Run on the Linux remote host from a checkout of this fork.

if test (uname -s) != Linux
    echo "This installer must run on Linux." >&2
    exit 1
end

set -l root (path dirname (status filename))
cd "$root"; or exit 1

echo "Building remote_server"
cargo build --release --package remote_server --bin remote_server --target-dir target; or exit 1

set -l destination "$HOME/.local/bin/ed_remote_server"
install -d "$HOME/.local/bin"; or exit 1
install -m 755 target/release/remote_server "$destination"; or exit 1

echo "Installed $destination"
