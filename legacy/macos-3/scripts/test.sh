#!/bin/zsh
set -euo pipefail

PROJECT_DIR="${0:A:h:h}"
SDK_PATH="/Library/Developer/CommandLineTools/SDKs/MacOSX15.4.sdk"
mkdir -p "$PROJECT_DIR/work/clang-cache"

CLANG_MODULE_CACHE_PATH="$PROJECT_DIR/work/clang-cache" swiftc \
  -sdk "$SDK_PATH" \
  -target arm64-apple-macosx14.0 \
  -D CODEX_SWITCH_TESTING \
  -parse-as-library \
  "$PROJECT_DIR/Sources/CodexSwitch/main.swift" \
  "$PROJECT_DIR/Tests/TestMain.swift" \
  -o "$PROJECT_DIR/work/CodexSwitchTests" \
  -framework SwiftUI -framework Security

"$PROJECT_DIR/work/CodexSwitchTests"
