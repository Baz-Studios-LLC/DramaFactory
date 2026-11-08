#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --release
bundle="target/DramaFactory.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp target/release/dramafactory "$bundle/Contents/MacOS/DramaFactory"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>DramaFactory</string>
<key>CFBundleIdentifier</key><string>studio.bazstudios.dramafactory</string>
<key>CFBundleName</key><string>DramaFactory</string>
<key>CFBundleDisplayName</key><string>DramaFactory</string>
<key>CFBundleShortVersionString</key><string>1</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>LSMinimumSystemVersion</key><string>13.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
printf 'Built %s (local unsigned build, v001)\n' "$bundle"
