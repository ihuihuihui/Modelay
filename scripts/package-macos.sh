#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
app_path="$project_dir/src-tauri/target/release/bundle/macos/Modelay.app"
output_dir="$project_dir/dist/installers"
case "$(uname -m)" in
  arm64) architecture=arm64 ;;
  x86_64) architecture=x64 ;;
  *) echo "Unsupported macOS architecture: $(uname -m)" >&2; exit 1 ;;
esac
zip_path="$output_dir/Modelay-macOS-$architecture.zip"
dmg_path="$output_dir/Modelay-macOS-$architecture.dmg"
checksum_path="$output_dir/Modelay-macOS-$architecture-SHA256.txt"
staging_dir=$(/usr/bin/mktemp -d /tmp/modelay-dmg.XXXXXX)

cleanup() {
  case "$staging_dir" in
    /tmp/modelay-dmg.*|/private/tmp/modelay-dmg.*) /bin/rm -rf "$staging_dir" ;;
  esac
}
trap cleanup EXIT

cd "$project_dir"
export PATH="$HOME/.cargo/bin:$PATH"
npm run tauri build -- --bundles app
mkdir -p "$output_dir"
/usr/bin/codesign --force --deep --sign - "$app_path"
/usr/bin/codesign --verify --deep --strict "$app_path"
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$app_path" "$zip_path"
/usr/bin/ditto "$app_path" "$staging_dir/Modelay.app"
/bin/ln -s /Applications "$staging_dir/Applications"
/usr/bin/hdiutil create -volname Modelay -srcfolder "$staging_dir" -ov -format UDZO "$dmg_path" >/dev/null
/usr/bin/hdiutil verify "$dmg_path" >/dev/null
(
  cd "$output_dir"
  /usr/bin/shasum -a 256 "$(basename "$zip_path")" "$(basename "$dmg_path")" >"$(basename "$checksum_path")"
)
echo "$zip_path"
echo "$dmg_path"
echo "$checksum_path"
