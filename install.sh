#!/usr/bin/env fish
# Installs target/release/ed.app (built by release.fish) to /Applications.

set -l root (path dirname (status filename))
cd $root; or exit 1

set -l app target/release/ed.app
set -l dest /Applications/ed.app

if not test -d $app
    echo "$app not found, run ./release.fish first" >&2
    exit 1
end

rm -rf $dest
mv $app $dest; or exit 1

echo "Installed $dest"
