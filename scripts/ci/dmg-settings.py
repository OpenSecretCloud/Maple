# dmgbuild settings for the Maple macOS installer window.
#
# Used by build_macos_dmg in scripts/ci/_common.sh:
#   dmgbuild -s scripts/ci/dmg-settings.py -D app=... -D background=... \
#     -D volume_icon=... Maple Maple_<version>_<arch>.dmg
#
# dmgbuild writes .DS_Store directly, so no Finder or AppleScript is involved.
# Positions are icon centers in Finder points.
import os.path

app = defines["app"]  # noqa: F821 (provided by dmgbuild)
app_name = os.path.basename(app)

files = [app]
symlinks = {"Applications": "/Applications"}
hide_extensions = [app_name]

# Two-density TIFF (660x400 and 1320x800). dmgbuild copies it to
# /.background.tiff on the volume and points the icon view at it.
background = defines["background"]  # noqa: F821
icon = defines["volume_icon"]  # noqa: F821

format = "UDZO"
compression_level = 9
filesystem = "HFS+"

window_rect = ((200, 120), (660, 400))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False

icon_size = 128
text_size = 13
icon_locations = {
    app_name: (175, 212),
    "Applications": (485, 212),
}
