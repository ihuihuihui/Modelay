#!/bin/zsh
set -euo pipefail

PROJECT_DIR="${0:A:h:h}"
OUTPUT_DIR="$PROJECT_DIR/outputs"
APP_DIR="$OUTPUT_DIR/CodexSwitch.app"

cd "$PROJECT_DIR"
mkdir -p "$PROJECT_DIR/work/clang-cache"
"$PROJECT_DIR/scripts/build-icon.sh"
SDK_PATH="/Library/Developer/CommandLineTools/SDKs/MacOSX15.4.sdk"
CLANG_MODULE_CACHE_PATH="$PROJECT_DIR/work/clang-cache" swiftc \
  -sdk "$SDK_PATH" \
  -target arm64-apple-macosx14.0 \
  -O -parse-as-library \
  "$PROJECT_DIR/Sources/CodexSwitch/main.swift" \
  -o "$PROJECT_DIR/work/CodexSwitch" \
  -framework SwiftUI -framework Security
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"
cp "$PROJECT_DIR/work/CodexSwitch" "$APP_DIR/Contents/MacOS/CodexSwitch"
cp "Resources/Info.plist" "$APP_DIR/Contents/Info.plist"
cp "Resources/AppIcon.icns" "$APP_DIR/Contents/Resources/AppIcon.icns"
codesign --force --deep --sign - "$APP_DIR"
ditto -c -k --sequesterRsrc --keepParent "$APP_DIR" "$OUTPUT_DIR/CodexSwitch-macOS.zip"
echo "$APP_DIR"
