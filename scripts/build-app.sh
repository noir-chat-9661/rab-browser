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
# shows the familiar "drag app onto Applications" layout instead of just
# the bare .app.
dmg_staging_dir="target/release/dmg-staging"
rm -rf "$dmg_staging_dir"
mkdir -p "$dmg_staging_dir"
trap 'rm -rf "$dmg_staging_dir"' EXIT
# ditto, not cp -R: preserves the ad-hoc code signature's extended
# attributes, which a plain recursive copy isn't guaranteed to.
ditto "$app_dir" "$dmg_staging_dir/$(basename "$app_dir")"
ln -s /Applications "$dmg_staging_dir/Applications"
hdiutil create -volname "$app_name" -srcfolder "$dmg_staging_dir" -ov -format UDZO "$dmg_path"

echo "==> done: $app_dir, $dmg_path"
open -R "$app_dir" 2>/dev/null || true
