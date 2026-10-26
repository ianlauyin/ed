#!/usr/bin/env fish
# Builds target/release/ed.app for this Mac, for local use only.
# Signs with a self-signed identity (default `ed-local`, override with
# ED_SIGN_IDENTITY) so macOS privacy grants such as Local Network survive rebuilds.

set -l root (path dirname (status filename))
cd $root; or exit 1

set -l app_version (string match -rg '^version = "(.+)"' < crates/ed/Cargo.toml)[1]
set -l resources crates/ed/resources
set -l app target/release/ed.app
set -l identity ed-local
set -q ED_SIGN_IDENTITY; and set identity $ED_SIGN_IDENTITY

if not security find-certificate -c $identity >/dev/null 2>&1
    echo "Code signing identity '$identity' not found in the keychain. Create it once with:

    set -l dir (mktemp -d)
    /usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj /CN=$identity \\
        -addext keyUsage=critical,digitalSignature -addext extendedKeyUsage=critical,codeSigning \\
        -keyout \$dir/key.pem -out \$dir/cert.pem
    /usr/bin/openssl pkcs12 -export -inkey \$dir/key.pem -in \$dir/cert.pem -out \$dir/id.p12 -passout pass:ed
    security import \$dir/id.p12 -k ~/Library/Keychains/login.keychain-db -P ed -T /usr/bin/codesign
    rm -rf \$dir" >&2
    exit 1
end

echo "Building ed $app_version"
cargo build --release --package ed; or exit 1

rm -rf $app
mkdir -p $app/Contents/MacOS $app/Contents/Resources

cp target/release/ed $app/Contents/MacOS/
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

codesign --force --sign $identity $app; or exit 1

echo "Packaged $app"
