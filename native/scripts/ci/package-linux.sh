#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "$0")" && pwd)/common.sh"

require_command cargo cargo-tauri curl dpkg-deb dpkg-shlibdeps

linux_build_root="$ci_build_root/desktop-linux"
artifact_version="${maple_version_value}-beta.${maple_build_number}"
deb_version="${maple_version_value}~beta.${maple_build_number}"
binary_path="$native_root/target/release/maple_desktop_iced"
icon_source="$native_root/ios/Sources/Assets.xcassets/AppIcon.appiconset/icon_1024.png"
deb_desktop_file="$linux_build_root/Maple-deb.desktop"
appimage_desktop_file="$linux_build_root/Maple-appimage.desktop"
deb_root="$linux_build_root/deb-root"
deb_output="$linux_build_root/Maple-${artifact_version}-amd64.deb"
appdir="$linux_build_root/AppDir"
appimage_output="$linux_build_root/Maple-${artifact_version}-x86_64.AppImage"
linuxdeploy_path="$linux_build_root/linuxdeploy-x86_64.AppImage"

ensure_dir "$linux_build_root"
rm -rf "$deb_root" "$appdir" "$deb_desktop_file" "$appimage_desktop_file" "$deb_output" "$appimage_output"

export OPEN_SECRET_API_URL="$open_secret_api_url"

cargo build --manifest-path "$native_root/Cargo.toml" -p maple_desktop_iced --release

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

mkdir -p "$deb_root/DEBIAN" "$deb_root/opt/Maple" "$deb_root/usr/bin" "$deb_root/usr/share/applications" "$deb_root/usr/share/icons/hicolor/1024x1024/apps"
cp "$binary_path" "$deb_root/opt/Maple/Maple"
chmod 755 "$deb_root/opt/Maple/Maple"
ln -sf /opt/Maple/Maple "$deb_root/usr/bin/maple"
cp "$deb_desktop_file" "$deb_root/usr/share/applications/cloud.opensecret.maple.desktop"
cp "$icon_source" "$deb_root/usr/share/icons/hicolor/1024x1024/apps/maple.png"

deb_dependencies="$(dpkg-shlibdeps -O "$deb_root/opt/Maple/Maple" 2>/dev/null | sed -n 's/^shlibs:Depends=//p')"
if [[ -z "$deb_dependencies" ]]; then
  deb_dependencies="libasound2, libdbus-1-3, libfontconfig1, libfreetype6, libgl1, libwayland-client0, libx11-6, libxkbcommon0"
fi

cat > "$deb_root/DEBIAN/control" <<EOF
Package: maple
Version: ${deb_version}
Section: utils
Priority: optional
Architecture: amd64
Maintainer: OpenSecret
Depends: ${deb_dependencies}
Description: Maple native desktop beta
 Maple native desktop beta build.
EOF

dpkg-deb --build --root-owner-group "$deb_root" "$deb_output"

mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" "$appdir/usr/share/icons/hicolor/1024x1024/apps"
cp "$binary_path" "$appdir/usr/bin/Maple"
chmod 755 "$appdir/usr/bin/Maple"
cp "$appimage_desktop_file" "$appdir/usr/share/applications/cloud.opensecret.maple.desktop"
cp "$icon_source" "$appdir/usr/share/icons/hicolor/1024x1024/apps/maple.png"

curl -fsSL "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage" -o "$linuxdeploy_path"
chmod +x "$linuxdeploy_path"

(
  cd "$linux_build_root"
  APPIMAGE_EXTRACT_AND_RUN=1 "$linuxdeploy_path" \
    --appdir "$appdir" \
    -e "$appdir/usr/bin/Maple" \
    -d "$appimage_desktop_file" \
    -i "$icon_source" \
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
