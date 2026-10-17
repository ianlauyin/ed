#!/usr/bin/env fish
# Builds target/release/ed.app for this Mac. Unsigned; for local use only.

set -l root (path dirname (status filename))
cd $root; or exit 1

set -l app_version (string match -rg '^version = "(.+)"' < crates/zed/Cargo.toml)[1]
set -l resources crates/zed/resources
set -l app target/release/ed.app

echo "Building ed $app_version"
cargo build --release --package zed --package cli; or exit 1

rm -rf $app
mkdir -p $app/Contents/MacOS $app/Contents/Resources

cp target/release/ed target/release/cli $app/Contents/MacOS/
cp $resources/Info.plist $app/Contents/Info.plist
plutil -replace CFBundleShortVersionString -string $app_version $app/Contents/Info.plist; or exit 1
plutil -replace CFBundleVersion -string $app_version $app/Contents/Info.plist; or exit 1

set -l iconset (mktemp -d)/ed.iconset
mkdir -p $iconset
for size in 16 32 128 256 512
    set -l double (math $size \* 2)
    sips -z $size $size $resources/app-icon@2x.png --out $iconset/icon_{$size}x{$size}.png >/dev/null; or exit 1
    sips -z $double $double $resources/app-icon@2x.png --out $iconset/icon_{$size}x{$size}@2x.png >/dev/null; or exit 1
end
iconutil -c icns $iconset -o $app/Contents/Resources/ed.icns; or exit 1
rm -rf (path dirname $iconset)
cp $resources/Document.icns $app/Contents/Resources/

echo "Packaged $app"
