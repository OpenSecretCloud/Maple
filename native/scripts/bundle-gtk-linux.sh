#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "bundle-gtk-linux.sh only supports Linux" >&2
  exit 2
fi

if (( $# < 2 || $# > 3 )); then
  echo "usage: $0 <binary-path> <bundle-dir> [launcher-name]" >&2
  exit 2
fi

binary_path="$(readlink -f "$1")"
bundle_dir="$2"
launcher_name="${3:-Maple}"

if [[ ! -x "$binary_path" ]]; then
  echo "Missing executable binary: $binary_path" >&2
  exit 2
fi

require_command() {
  local missing=()
  for cmd in "$@"; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
      missing+=("$cmd")
    fi
  done

  if (( ${#missing[@]} > 0 )); then
    printf 'Missing required commands: %s\n' "${missing[*]}" >&2
    exit 2
  fi
}

require_command awk chmod cp file grep ldd mkdir patchelf readelf sed sort strings tar

shopt -s nullglob

binary_name="$(basename "$binary_path")"
bundle_binary="$bundle_dir/$binary_name"
bundle_lib_dir="$bundle_dir/lib"
bundle_bin_dir="$bundle_dir/bin"
bundle_share_dir="$bundle_dir/share"

declare -A copied_libs=()
declare -A copied_gio_modules=()

is_elf() {
  readelf -h "$1" >/dev/null 2>&1
}

first_glob() {
  local entries=( $1 )
  if (( ${#entries[@]} > 0 )); then
    printf '%s\n' "${entries[0]}"
  fi
}

copy_tree() {
  local src="$1"
  local dest="$2"

  if [[ ! -d "$src" ]]; then
    return 0
  fi

  mkdir -p "$(dirname "$dest")"
  rm -rf "$dest"
  cp -aL "$src" "$dest"
  chmod -R u+w "$dest" 2>/dev/null || true
}

copy_tree_contents() {
  local src="$1"
  local dest="$2"

  if [[ ! -d "$src" ]]; then
    return 0
  fi

  mkdir -p "$dest"
  cp -aL "$src"/. "$dest"/
  chmod -R u+w "$dest" 2>/dev/null || true
}

patch_absolute_needed() {
  local target="$1"
  local needed=()
  local dep

  while IFS= read -r dep; do
    needed+=("$dep")
  done < <(readelf -d "$target" 2>/dev/null | sed -n 's/.*Shared library: \[\(\/[^]]*\)\].*/\1/p')

  for dep in "${needed[@]}"; do
    patchelf --replace-needed "$dep" "$(basename "$dep")" "$target"
  done
}

patch_elf() {
  local target="$1"
  local runpath="$2"

  if ! is_elf "$target"; then
    return 0
  fi

  chmod u+w "$target"
  patch_absolute_needed "$target"
  patchelf --set-rpath "$runpath" "$target"
}

should_bundle_dep() {
  local dep="$1"

  if [[ -z "$dep" || ! -e "$dep" ]]; then
    return 1
  fi

  case "$dep" in
    */ld-linux-*.so.*|*/libc.so.6|*/libm.so.6|*/libpthread.so.0|*/libdl.so.2|*/librt.so.1|*/libutil.so.1|*/libresolv.so.2|*/libnss_*.so.*)
      return 1
      ;;
  esac

  return 0
}

list_deps() {
  {
    ldd "$1" 2>/dev/null | awk '
      /=> \/[^ ]+/ { print $3 }
      /^[[:space:]]*\/[[:graph:]]+ \(/ { print $1 }
    '
    readelf -d "$1" 2>/dev/null | sed -n 's/.*Shared library: \[\(\/[^]]*\)\].*/\1/p'
  } | sort -u
}

copy_library() {
  local src="$1"
  local base="$(basename "$src")"
  local dest="$bundle_lib_dir/$base"

  if [[ -n "${copied_libs[$base]:-}" ]]; then
    return 0
  fi

  copied_libs[$base]="$src"
  mkdir -p "$bundle_lib_dir"
  cp -L "$src" "$dest"
  patch_elf "$dest" '$ORIGIN'
  copy_dependency_closure "$src"
}

copy_dependency_closure() {
  local source="$1"
  local dep

  while IFS= read -r dep; do
    if should_bundle_dep "$dep"; then
      copy_library "$dep"
    fi
  done < <(list_deps "$source")
}

copy_helper_binary() {
  local src="$1"
  local dest="$bundle_bin_dir/$(basename "$src")"

  mkdir -p "$bundle_bin_dir"
  cp -L "$src" "$dest"
  patch_elf "$dest" '$ORIGIN/../lib'
  copy_dependency_closure "$src"
}

copy_gio_module() {
  local src="$1"
  local base="$(basename "$src")"
  local dest="$bundle_lib_dir/gio/modules/$base"

  if [[ -n "${copied_gio_modules[$base]:-}" ]]; then
    return 0
  fi

  copied_gio_modules[$base]="$src"
  mkdir -p "$bundle_lib_dir/gio/modules"
  cp -L "$src" "$dest"
  patch_elf "$dest" '$ORIGIN/../../..'
  copy_dependency_closure "$src"
}

extract_store_string() {
  local target="$1"
  local pattern="$2"

  strings "$target" | grep -E -m1 "$pattern" || true
}

version_from_store_path() {
  local store_path="$1"
  local prefix="$2"
  local store_name="$(basename "$store_path")"

  printf '%s\n' "${store_name##*-${prefix}-}"
}

bundle_runtime_data() {
  local xkb_src
  local xlocale_src
  local gtk_share_root
  local iso_codes_root
  local gdk_lib_root
  local gdk_store_root
  local gdk_version
  local gdk_query_bin
  local gdk_loaders_src
  local glib_modules_root
  local glib_store_root
  local glib_version
  local glib_dev_root
  local glib_compile_schemas_bin
  local gsettings_schema_root
  local adwaita_icons_root
  local hicolor_icons_root
  local mime_root
  local gio_module

  xkb_src="$(extract_store_string "$bundle_lib_dir/libxkbcommon.so.0" '^/nix/store/[^ ]+-xkeyboard-config-[^/]+/etc/X11/xkb$')"
  xlocale_src="$(extract_store_string "$bundle_lib_dir/libX11.so.6" '^/nix/store/[^ ]+-libx11-[^/]+/share/X11/locale$')"
  gtk_share_root="$(extract_store_string "$bundle_lib_dir/libgtk-4.so.1" '^/nix/store/[^ ]+-gtk4-[^/]+/share$')"
  iso_codes_root="$(extract_store_string "$bundle_lib_dir/libgtk-4.so.1" '^/nix/store/[^ ]+-iso-codes-[^/]+/share/xml/iso-codes$')"
  gdk_lib_root="$(extract_store_string "$bundle_lib_dir/libgdk_pixbuf-2.0.so.0" '^/nix/store/[^ ]+-gdk-pixbuf-[^/]+/lib$')"
  glib_modules_root="$(extract_store_string "$bundle_lib_dir/libgio-2.0.so.0" '^/nix/store/[^ ]+-glib-[^/]+/lib/gio/modules$')"

  if [[ -n "$xkb_src" ]]; then
    copy_tree "$xkb_src" "$bundle_share_dir/X11/xkb"
  fi

  if [[ -n "$xlocale_src" ]]; then
    copy_tree "$xlocale_src" "$bundle_share_dir/X11/locale"
  fi

  if [[ -n "$gtk_share_root" ]]; then
    copy_tree "$gtk_share_root/gtk-4.0" "$bundle_share_dir/gtk-4.0"
  fi

  if [[ -n "$iso_codes_root" ]]; then
    copy_tree "$iso_codes_root" "$bundle_share_dir/xml/iso-codes"
  fi

  if [[ -n "$gdk_lib_root" ]]; then
    gdk_store_root="$(dirname "$gdk_lib_root")"
    gdk_version="$(version_from_store_path "$gdk_store_root" 'gdk-pixbuf')"
    gdk_loaders_src="$gdk_lib_root/gdk-pixbuf-2.0/2.10.0/loaders"
    gdk_query_bin="$(first_glob "/nix/store/*-gdk-pixbuf-${gdk_version}-dev/bin/gdk-pixbuf-query-loaders")"

    copy_tree "$gdk_loaders_src" "$bundle_lib_dir/gdk-pixbuf-2.0/2.10.0/loaders"

    if [[ -n "$gdk_query_bin" ]]; then
      copy_helper_binary "$gdk_query_bin"
    fi

    for gio_module in "$gdk_loaders_src"/*.so; do
      [[ -f "$gio_module" ]] || continue
      copy_dependency_closure "$gio_module"
      patch_elf "$bundle_lib_dir/gdk-pixbuf-2.0/2.10.0/loaders/$(basename "$gio_module")" '$ORIGIN/../../..'
    done
  fi

  if [[ -n "$glib_modules_root" ]]; then
    glib_store_root="$(dirname "$(dirname "$(dirname "$glib_modules_root")")")"
    glib_version="$(version_from_store_path "$glib_store_root" 'glib')"
    glib_dev_root="$(first_glob "/nix/store/*-glib-${glib_version}-dev")"

    if [[ -n "$glib_dev_root" ]]; then
      glib_compile_schemas_bin="$glib_dev_root/bin/glib-compile-schemas"

      copy_tree_contents "$glib_dev_root/share/glib-2.0/schemas" "$bundle_share_dir/glib-2.0/schemas"

      gsettings_schema_root="$(first_glob "/nix/store/*-gsettings-desktop-schemas-*/share/gsettings-schemas/*/glib-2.0/schemas")"
      if [[ -z "$gsettings_schema_root" ]]; then
        gsettings_schema_root="$(first_glob "/nix/store/*-gsettings-desktop-schemas-*/share/glib-2.0/schemas")"
      fi
      if [[ -n "$gsettings_schema_root" ]]; then
        copy_tree_contents "$gsettings_schema_root" "$bundle_share_dir/glib-2.0/schemas"
      fi

      if ls "$bundle_share_dir/glib-2.0/schemas"/*.xml >/dev/null 2>&1; then
        "$glib_compile_schemas_bin" "$bundle_share_dir/glib-2.0/schemas"
      fi
    fi
  fi

  adwaita_icons_root="$(first_glob '/nix/store/*-adwaita-icon-theme-*/share/icons/Adwaita')"
  hicolor_icons_root="$(first_glob '/nix/store/*-hicolor-icon-theme-*/share/icons/hicolor')"
  mime_root="$(first_glob '/nix/store/*-shared-mime-info-*/share/mime')"

  if [[ -n "$adwaita_icons_root" ]]; then
    copy_tree "$adwaita_icons_root" "$bundle_share_dir/icons/Adwaita"
  fi

  if [[ -n "$hicolor_icons_root" ]]; then
    copy_tree "$hicolor_icons_root" "$bundle_share_dir/icons/hicolor"
  fi

  if [[ -n "$mime_root" ]]; then
    copy_tree "$mime_root" "$bundle_share_dir/mime"
  fi
}

write_launcher() {
  local launcher_path="$bundle_dir/$launcher_name"

  cat > "$launcher_path" <<EOF
#!/usr/bin/env sh
set -eu
HERE=\$(CDPATH= cd -- "\$(dirname -- "\$0")" && pwd)
LIB="\$HERE/lib"
DATA="\$HERE/share"
LOADERS_DIR="\$LIB/gdk-pixbuf-2.0/2.10.0/loaders"
LOADERS_CACHE="\$LIB/gdk-pixbuf-2.0/2.10.0/loaders.cache"
GIO_MODULES_DIR="\$LIB/gio/modules"
APP_BINARY="\$HERE/$binary_name"
export XKB_CONFIG_ROOT="\${XKB_CONFIG_ROOT:-\$DATA/X11/xkb}"
export XLOCALEDIR="\${XLOCALEDIR:-\$DATA/X11/locale}"
export GSETTINGS_SCHEMA_DIR="\${GSETTINGS_SCHEMA_DIR:-\$DATA/glib-2.0/schemas}"
export XDG_DATA_DIRS="\$DATA\${XDG_DATA_DIRS:+:\$XDG_DATA_DIRS}"
export GTK_DATA_PREFIX="\${GTK_DATA_PREFIX:-\$HERE}"
export GTK_EXE_PREFIX="\${GTK_EXE_PREFIX:-\$HERE}"
export GDK_PIXBUF_MODULEDIR="\$LOADERS_DIR"
export GDK_PIXBUF_MODULE_FILE="\$LOADERS_CACHE"
export GDK_BACKEND="\${GDK_BACKEND:-wayland,x11}"
export GSK_RENDERER="\${GSK_RENDERER:-cairo}"

if [ -d "\$GIO_MODULES_DIR" ]; then
  export GIO_EXTRA_MODULES="\${GIO_EXTRA_MODULES:-\$GIO_MODULES_DIR}"
fi

for ld in /lib/ld-linux-aarch64.so.1 /lib64/ld-linux-aarch64.so.1 /lib/ld-linux-x86-64.so.2 /lib64/ld-linux-x86-64.so.2; do
  if [ -x "\$ld" ]; then
    if [ -x "\$HERE/bin/gdk-pixbuf-query-loaders" ] && [ -d "\$LOADERS_DIR" ]; then
      if ls "\$LOADERS_DIR"/*.so >/dev/null 2>&1; then
        tmp="\$LOADERS_CACHE.tmp"
        "\$ld" --library-path "\$LIB\${LD_LIBRARY_PATH:+:\$LD_LIBRARY_PATH}" \
          "\$HERE/bin/gdk-pixbuf-query-loaders" "\$LOADERS_DIR"/*.so > "\$tmp" 2>/dev/null || true
        if [ -s "\$tmp" ]; then
          mv "\$tmp" "\$LOADERS_CACHE"
        else
          rm -f "\$tmp"
        fi
      fi
    fi

    exec "\$ld" --library-path "\$LIB\${LD_LIBRARY_PATH:+:\$LD_LIBRARY_PATH}" "\$APP_BINARY" "\$@"
  fi
done

printf '%s\n' "Could not find a supported system dynamic loader." >&2
exit 1
EOF

  chmod 755 "$launcher_path"
}

mkdir -p "$bundle_dir"
chmod -R u+w "$bundle_dir" 2>/dev/null || true
rm -rf "$bundle_dir"
mkdir -p "$bundle_dir" "$bundle_lib_dir" "$bundle_bin_dir" "$bundle_share_dir"

cp -L "$binary_path" "$bundle_binary"
patch_elf "$bundle_binary" '$ORIGIN/lib'
copy_dependency_closure "$binary_path"
bundle_runtime_data
write_launcher

ldd "$binary_path" > "$bundle_dir/deps.txt" || true

printf '%s\n' "$bundle_dir"
