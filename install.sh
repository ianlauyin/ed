#!/usr/bin/env fish
# Installs target/release/ed.app (built by release.fish) to /Applications and
# links its cli into ~/.local/bin/ed. The cli locates the app bundle from its own
# resolved path, so it must be a symlink into the bundle, not a copy.

set -l root (path dirname (status filename))
cd $root; or exit 1

set -l app target/release/ed.app
set -l dest /Applications/ed.app
set -l bin ~/.local/bin

if not test -d $app
    echo "$app not found, run ./release.fish first" >&2
    exit 1
end

rm -rf $dest
mv $app $dest; or exit 1

mkdir -p $bin
ln -sf $dest/Contents/MacOS/cli $bin/ed; or exit 1

echo "Installed $dest, cli linked at $bin/ed"
