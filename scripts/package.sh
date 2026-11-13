#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
cargo build --release --locked --offline --target-dir "$root/target"
stage=$(mktemp -d "$root/target/bundle.XXXXXX")
trap 'rm -rf "$stage"' EXIT
app="$stage/Launcher.app"
mkdir -p "$app/Contents/MacOS"
/usr/bin/install -m 755 target/release/launcher "$app/Contents/MacOS/launcher"
/usr/bin/install -m 644 packaging/Info.plist "$app/Contents/Info.plist"
/usr/bin/plutil -lint "$app/Contents/Info.plist"
/usr/bin/codesign --force --sign - "$app"
/usr/bin/codesign --verify --strict "$app"
mkdir -p target/bundle
# Only replace our generated bundle, never an installed application.
rm -rf target/bundle/Launcher.app
mv "$app" target/bundle/Launcher.app
printf 'Built %s/target/bundle/Launcher.app\n' "$root"
