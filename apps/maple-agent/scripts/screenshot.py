#!/usr/bin/env python3
"""Take a screenshot on GNOME Wayland through the xdg desktop portal.

Usage: scripts/screenshot.py /tmp/shot.png

GNOME blocks the Shell screenshot D-Bus method for unlisted callers, and
grim needs wlr-screencopy, which GNOME lacks. The portal is the one path
that works from a shell. The first request opens a permission dialog on
the desktop; click Share.
"""
import shutil
import sys
import urllib.parse

import gi

gi.require_version("Gio", "2.0")
gi.require_version("GLib", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

WAIT_SECONDS = 60


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    out = sys.argv[1]
    bus = Gio.bus_get_sync(Gio.BusType.SESSION)
    loop = GLib.MainLoop()
    status = {"code": None}

    def on_response(_conn, _sender, _path, _iface, _signal, params):
        code, results = params.unpack()
        status["code"] = code
        if code == 0:
            uri = results.get("uri", "")
            shutil.copy(urllib.parse.urlparse(uri).path, out)
            print(f"saved {out}")
        else:
            print(f"portal response code {code} (1 = cancelled, 2 = failed)")
        loop.quit()

    bus.signal_subscribe(
        "org.freedesktop.portal.Desktop",
        "org.freedesktop.portal.Request",
        "Response",
        None,
        None,
        0,
        on_response,
    )
    opts = {
        "interactive": GLib.Variant("b", False),
        "modal": GLib.Variant("b", False),
    }
    bus.call_sync(
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Screenshot",
        "Screenshot",
        GLib.Variant("(sa{sv})", ("", opts)),
        None,
        0,
        30000,
        None,
    )
    GLib.timeout_add_seconds(WAIT_SECONDS, loop.quit)
    loop.run()
    if status["code"] is None:
        print(f"no portal response in {WAIT_SECONDS}s; a permission dialog may be open")
        return 1
    return 0 if status["code"] == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
