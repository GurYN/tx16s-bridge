#!/bin/sh
# Builds "TX16S Bridge.app" in target/release. macOS asks once for Input
# Monitoring permission the first time the app reads the radio.
set -eu
cd "$(dirname "$0")/.."

# gpui compiles its Metal shaders at build time, which needs full Xcode and its
# Metal toolchain (xcodebuild -downloadComponent MetalToolchain).
if ! xcrun -f metal >/dev/null 2>&1 && [ -d /Applications/Xcode.app ]; then
  export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
fi

cargo build --release -p tx16s-sender

app="target/release/TX16S Bridge.app"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/tx16s-sender "$app/Contents/MacOS/tx16s-sender"
# Regenerate with: swift scripts/make-icon.swift
cp assets/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>TX16S Bridge</string>
  <key>CFBundleDisplayName</key><string>TX16S Bridge</string>
  <key>CFBundleIdentifier</key><string>fr.cibelli.tx16s-bridge</string>
  <key>CFBundleExecutable</key><string>tx16s-sender</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
</dict>
</plist>
PLIST
codesign --force --sign - "$app"
echo "Built $app"
