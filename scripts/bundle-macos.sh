#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$PWD=/DramaFactory --remap-path-prefix=$HOME=/build"
cargo build --release
version=$(target/release/dramafactory --version)
number=$(printf '%s' "$version" | sed 's/^v0*//')
bundle="target/DramaFactory.build.app"
if [ -d "$bundle" ]; then rm -rf "$bundle"; fi
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp target/release/dramafactory "$bundle/Contents/MacOS/DramaFactory"
icon_output="target/icon-compiled"
mkdir -p "$icon_output"
xcrun actool branding/DramaFactory.icon --compile "$icon_output" --platform macosx --minimum-deployment-target 13.0 --app-icon DramaFactory --output-partial-info-plist "$icon_output/partial-info.plist" --output-format human-readable-text
cp "$icon_output/DramaFactory.icns" "$bundle/Contents/Resources/DramaFactory.icns"
cp "$icon_output/Assets.car" "$bundle/Contents/Resources/Assets.car"
cat > "$bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>DramaFactory</string>
<key>CFBundleIdentifier</key><string>studio.bazstudios.dramafactory</string>
<key>CFBundleName</key><string>DramaFactory</string>
<key>CFBundleDisplayName</key><string>DramaFactory</string>
<key>CFBundleShortVersionString</key><string>$number</string>
<key>CFBundleVersion</key><string>$number</string>
<key>CFBundleIconFile</key><string>DramaFactory</string>
<key>CFBundleIconName</key><string>DramaFactory</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>LSMinimumSystemVersion</key><string>13.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
codesign --force --sign - "$bundle"
codesign --verify --deep --strict "$bundle"
"$bundle/Contents/MacOS/DramaFactory" --version
python3 scripts/audit-release.py --bundle "$bundle"
previous="target/DramaFactory.previous-build.app"
if [ -d "$previous" ]; then rm -rf "$previous"; fi
if [ -d target/DramaFactory.app ]; then mv target/DramaFactory.app "$previous"; fi
mv "$bundle" target/DramaFactory.app
printf 'Built target/DramaFactory.app (%s; ad-hoc signed, not notarized)\n' "$version"
