#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "$0")" && pwd)/common.sh"

if [[ "${MAPLE_NATIVE_NIX_SHELL:-}" != "1" ]] && command -v nix >/dev/null 2>&1; then
  exec env PATH="$HOME/.cargo/bin:$PATH" \
    nix develop --no-write-lock-file "$native_root" --command bash -c \
    "set -euo pipefail; exec bash '$native_root/scripts/ci/package-linux.sh'"
fi

require_command cargo curl dpkg-deb dpkg-shlibdeps magick

bundle_script="$native_root/scripts/bundle-gtk-linux.sh"

host_arch="$(uname -m)"
case "$host_arch" in
  x86_64)
    deb_arch="amd64"
    appimage_arch="x86_64"
    ;;
  aarch64|arm64)
    deb_arch="arm64"
    appimage_arch="aarch64"
    ;;
  *)
    printf 'Unsupported Linux architecture: %s\n' "$host_arch" >&2
    exit 1
    ;;
esac

linux_build_root="$ci_build_root/desktop-linux"
artifact_version="$(maple_artifact_version)"
deb_version="$(maple_deb_version)"
binary_path="$native_root/target/release/maple_desktop_gtk"
bundle_root="$linux_build_root/Maple-bundle"
icon_source="$native_root/ios/Sources/Assets.xcassets/AppIcon.appiconset/icon_1024.png"
resized_icon="$linux_build_root/maple-512.png"
deb_desktop_file="$linux_build_root/Maple-deb.desktop"
appimage_desktop_file="$linux_build_root/Maple-appimage.desktop"
deb_root="$linux_build_root/deb-root"
deb_output="$linux_build_root/Maple-${artifact_version}-${deb_arch}.deb"
appdir="$linux_build_root/AppDir"
appimage_entry_executable="$appdir/usr/lib/maple/maple_desktop_gtk"
appimage_output="$linux_build_root/Maple-${artifact_version}-${appimage_arch}.AppImage"
linuxdeploy_path="$linux_build_root/linuxdeploy-${appimage_arch}.AppImage"

ensure_dir "$linux_build_root"
rm -rf "$bundle_root" "$deb_root" "$appdir" "$deb_desktop_file" "$appimage_desktop_file" "$deb_output" "$appimage_output" "$resized_icon"

export OPEN_SECRET_API_URL="$open_secret_api_url"

magick "$icon_source" -resize 512x512 "$resized_icon"

cargo build --manifest-path "$native_root/Cargo.toml" -p maple_desktop_gtk --release
"$bundle_script" "$binary_path" "$bundle_root"

cat > "$deb_desktop_file" <<EOF
[Desktop Entry]
Name=Maple
Comment=Maple native desktop beta
Exec=maple
Icon=maple
Terminal=false
Type=Application
Categories=Network;Utility;
StartupWMClass=Maple
EOF

cat > "$appimage_desktop_file" <<EOF
[Desktop Entry]
Name=Maple
Comment=Maple native desktop beta
Exec=Maple
Icon=maple
Terminal=false
Type=Application
Categories=Network;Utility;
StartupWMClass=Maple
EOF

mkdir -p "$deb_root/DEBIAN" "$deb_root/opt/Maple" "$deb_root/usr/bin" "$deb_root/usr/share/applications" "$deb_root/usr/share/icons/hicolor/512x512/apps"
cp -a "$bundle_root"/. "$deb_root/opt/Maple/"
chmod 755 "$deb_root/opt/Maple/Maple"
cat > "$deb_root/usr/bin/maple" <<'EOF'
#!/bin/sh
exec /opt/Maple/Maple "$@"
EOF
chmod 755 "$deb_root/usr/bin/maple"
cp "$deb_desktop_file" "$deb_root/usr/share/applications/cloud.opensecret.maple.desktop"
cp "$resized_icon" "$deb_root/usr/share/icons/hicolor/512x512/apps/maple.png"

deb_dependencies=""
if ! deb_dependencies="$(dpkg-shlibdeps -l"$deb_root/opt/Maple/lib" -O "$deb_root/opt/Maple/maple_desktop_gtk" 2>/dev/null | sed -n 's/^shlibs:Depends=//p')"; then
  deb_dependencies=""
fi
if [[ -z "$deb_dependencies" ]]; then
  deb_dependencies="libc6"
fi

cat > "$deb_root/DEBIAN/control" <<EOF
Package: maple
Version: ${deb_version}
Section: utils
Priority: optional
Architecture: ${deb_arch}
Maintainer: OpenSecret
Depends: ${deb_dependencies}
Description: Maple native desktop beta
 Maple native desktop beta build.
EOF

dpkg-deb --build --root-owner-group "$deb_root" "$deb_output"

mkdir -p "$appdir/usr/bin" "$appdir/usr/lib/maple" "$appdir/usr/share/applications" "$appdir/usr/share/icons/hicolor/512x512/apps"
cp -a "$bundle_root"/. "$appdir/usr/lib/maple/"
cat > "$appdir/usr/bin/Maple" <<'EOF'
#!/bin/sh
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec "$HERE/../lib/maple/Maple" "$@"
EOF
chmod 755 "$appdir/usr/bin/Maple"
cat > "$appdir/AppRun" <<'EOF'
#!/bin/sh
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec "$HERE/usr/lib/maple/Maple" "$@"
EOF
chmod 755 "$appdir/AppRun"
cp "$appimage_desktop_file" "$appdir/usr/share/applications/cloud.opensecret.maple.desktop"
cp "$resized_icon" "$appdir/usr/share/icons/hicolor/512x512/apps/maple.png"

curl -fsSL "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-${appimage_arch}.AppImage" -o "$linuxdeploy_path"
chmod +x "$linuxdeploy_path"

(
  cd "$linux_build_root"
  APPIMAGE_EXTRACT_AND_RUN=1 "$linuxdeploy_path" \
    --appdir "$appdir" \
    -e "$appimage_entry_executable" \
    -d "$appimage_desktop_file" \
    -i "$resized_icon" \
    --output appimage
)

generated_appimage="$(find "$linux_build_root" -maxdepth 1 -name '*.AppImage' ! -name 'linuxdeploy-*.AppImage' -print -quit)"

if [[ -z "$generated_appimage" ]]; then
  echo "Failed to locate generated AppImage" >&2
  exit 1
fi

mv "$generated_appimage" "$appimage_output"

tauri_sign_artifact "$deb_output"
tauri_sign_artifact "$appimage_output"

printf 'Linux DEB: %s\n' "$deb_output"
printf 'Linux AppImage: %s\n' "$appimage_output"
