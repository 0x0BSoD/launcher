#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
destination="${1:-/Applications}/Launcher.app"
if [ -e "$destination" ] || [ -L "$destination" ]; then
    printf 'Refusing to overwrite %s; move the old app aside first.\n' "$destination" >&2
    exit 1
fi
"$root/scripts/package.sh"
mkdir -p "$(dirname -- "$destination")"
/usr/bin/ditto "$root/target/bundle/Launcher.app" "$destination"
/usr/bin/codesign --verify --strict "$destination"
printf 'Installed %s\n' "$destination"
