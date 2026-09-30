#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
profile=${1:-release}
case "$profile" in debug|release) ;; *) echo 'usage: prepare-macos-app.sh [debug|release]' >&2; exit 1;; esac
output="$root/target/$profile/Network Lantern.app"
if [[ -e "$output" ]]; then
  echo "Destination already exists: $output" >&2
  exit 1
fi
for executable in network-lantern-desktop network-lantern-helper network-lantern; do
  [[ -x "$root/target/$profile/$executable" ]] || { echo "Build $executable first" >&2; exit 1; }
done
mkdir -p "$output/Contents/MacOS" "$output/Contents/Library/LaunchDaemons" "$output/Contents/Library/HelperTools"
cp "$root/target/$profile/network-lantern-desktop" "$output/Contents/MacOS/"
cp "$root/target/$profile/network-lantern" "$output/Contents/MacOS/"
cp "$root/target/$profile/network-lantern-helper" "$output/Contents/Library/HelperTools/"
cp "$root/deployment/helper/macos/Info.plist" "$output/Contents/Info.plist"
cp "$root/deployment/helper/macos/dev.network-lantern.helper.plist" "$output/Contents/Library/LaunchDaemons/"
/usr/bin/plutil -lint "$output/Contents/Info.plist" "$output/Contents/Library/LaunchDaemons/dev.network-lantern.helper.plist"
echo "Prepared unsigned local bundle: $output"
echo 'Helper registration requires Developer ID signing and explicit system approval; no registration was performed.'
