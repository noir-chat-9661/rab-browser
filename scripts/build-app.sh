#!/usr/bin/env bash
# Builds crates/browser-app in release mode and packages it as a
# double-clickable rab-browser.app under target/release/.
#
# Ad-hoc signed only (codesign --sign -): no Apple Developer ID is
# configured yet, so this bundle is for local use/testing (e.g. verifying
# the WebAuthn/passkey issue tracked in README under "known issues"), not
# distribution. Gatekeeper will still warn on other machines until the app
# is signed with a real Developer ID and notarized.
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "build-app.sh only supports macOS." >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

version="$(sed -n '/^version = /{s/^version = "\(.*\)"/\1/p;q;}' crates/browser-app/Cargo.toml)"
if [[ -z "$version" ]]; then
  echo "Could not extract version from crates/browser-app/Cargo.toml" >&2
  exit 1
fi
app_name="rab-browser"
bundle_id="com.noir-chat-9661.rab-browser"

echo "==> cargo build --release -p browser-app"
cargo build --release -p browser-app

app_dir="target/release/${app_name}.app"
contents_dir="$app_dir/Contents"
macos_dir="$contents_dir/MacOS"
resources_dir="$contents_dir/Resources"

rm -rf "$app_dir"
mkdir -p "$macos_dir" "$resources_dir"

cp "target/release/browser-app" "$macos_dir/$app_name"

cat > "$contents_dir/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleName</key>
	<string>${app_name}</string>
	<key>CFBundleDisplayName</key>
	<string>rab-browser</string>
	<key>CFBundleIdentifier</key>
	<string>${bundle_id}</string>
	<key>CFBundleExecutable</key>
	<string>${app_name}</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>${version}</string>
	<key>CFBundleVersion</key>
	<string>${version}</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>NSHighResolutionCapable</key>
	<true/>
	<key>LSMinimumSystemVersion</key>
	<string>11.0</string>
	<key>NSHumanReadableCopyright</key>
	<string>Copyright (c) 2026 noir-chat-9661. Dual-licensed under MIT OR Apache-2.0.</string>
</dict>
</plist>
PLIST

# No CFBundleIconFile entry above: no .icns asset exists yet, so the bundle
# intentionally falls back to the generic macOS app icon. Once one is
# designed, add it to Resources/ and set CFBundleIconFile above.

echo "==> codesign --sign - (ad-hoc)"
codesign --force --deep --sign - "$app_dir"

dmg_path="target/release/${app_name}-${version}.dmg"
echo "==> hdiutil create (.dmg)"
rm -f "$dmg_path"
# Stage the .app alongside an /Applications symlink so the mounted volume
# shows the familiar "drag app onto Applications (on the right)" layout
# instead of just the bare .app. The positions below are set by opening a
# writable dmg in Finder and driving it with AppleScript (hdiutil alone has
# no icon-layout option), then that writable image is converted to the
# final compressed one.
dmg_staging_dir="target/release/dmg-staging"
volume_name="$app_name"
rw_dmg_path="target/release/${app_name}-rw.dmg"
mount_dir=""
attached_device=""

cleanup() {
  local exit_status=$?

  if [[ -n "$mount_dir" ]]; then
    hdiutil detach "$mount_dir" -quiet 2>/dev/null || true
  elif [[ -n "$attached_device" ]]; then
    # Fall back to the attached device if parsing the mount point failed.
    hdiutil detach "$attached_device" -quiet 2>/dev/null || true
  fi
  rm -f "$rw_dmg_path" 2>/dev/null || true
  rm -rf "$dmg_staging_dir" 2>/dev/null || true

  exit "$exit_status"
}
trap cleanup EXIT

rm -rf "$dmg_staging_dir"
mkdir -p "$dmg_staging_dir"
# ditto, not cp -R: preserves the ad-hoc code signature's extended
# attributes, which a plain recursive copy isn't guaranteed to.
ditto "$app_dir" "$dmg_staging_dir/$(basename "$app_dir")"
ln -s /Applications "$dmg_staging_dir/Applications"

rm -f "$rw_dmg_path"
hdiutil create -volname "$volume_name" -srcfolder "$dmg_staging_dir" -ov -format UDRW "$rw_dmg_path"

attach_plist="$(hdiutil attach "$rw_dmg_path" -noautoopen -plist)"
entity_index=0
while [[ "$entity_index" -lt 16 ]]; do
  entity_device="$(printf '%s\n' "$attach_plist" | plutil -extract "system-entities.${entity_index}.dev-entry" raw -o - -- - 2>/dev/null || true)"
  if [[ -z "$attached_device" && -n "$entity_device" ]]; then
    attached_device="$entity_device"
  fi

  mount_candidate="$(printf '%s\n' "$attach_plist" | plutil -extract "system-entities.${entity_index}.mount-point" raw -o - -- - 2>/dev/null || true)"
  if [[ -n "$mount_candidate" ]]; then
    mount_dir="$mount_candidate"
    break
  fi
  entity_index=$((entity_index + 1))
done

if [[ -z "$mount_dir" ]]; then
  echo "Could not determine the mounted volume path from hdiutil attach." >&2
  exit 1
fi

osascript <<OSA
tell application "Finder"
  set mountedDisk to disk (POSIX file "${mount_dir}" as alias)
  tell mountedDisk
    open
    delay 2
    set current view of container window to icon view
    set toolbar visible of container window to false
    set statusbar visible of container window to false
    set the bounds of container window to {100, 100, 620, 420}
    set viewOptions to the icon view options of container window
    set arrangement of viewOptions to not arranged
    set icon size of viewOptions to 96
    delay 1
    set position of item "${app_name}.app" of container window to {140, 160}
    set position of item "Applications" of container window to {380, 160}
    delay 1
    update without registering applications
    delay 2
    close
  end tell
end tell
OSA

hdiutil detach "$mount_dir" -quiet
mount_dir=""
attached_device=""
hdiutil convert "$rw_dmg_path" -format UDZO -ov -o "$dmg_path"

echo "==> done: $app_dir, $dmg_path"
open -R "$app_dir" 2>/dev/null || true
