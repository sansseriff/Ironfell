#!/bin/sh
# Minimal macOS application metadata around the ordinary cargo-built executable.
set -eu
probe_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
probe_bundle="$probe_root/.agents/latency/standalone/Iron Bevy Control.app"
probe_binary="$probe_root/target/aarch64-apple-darwin/dev-opt/iron-latency-probe"
mkdir -p "$probe_bundle/Contents/MacOS"
if ! cp -c "$probe_binary" "$probe_bundle/Contents/MacOS/iron-latency-probe"; then
    cp "$probe_binary" "$probe_bundle/Contents/MacOS/iron-latency-probe"
fi
cat > "$probe_bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>iron-latency-probe</string>
<key>CFBundleIdentifier</key><string>org.ironfell.bevy-latency-probe</string>
<key>CFBundleName</key><string>Iron Bevy Control</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.0.1</string>
<key>CFBundleVersion</key><string>1</string>
<key>LSMinimumSystemVersion</key><string>14.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
printf '%s\n' "$probe_bundle"
