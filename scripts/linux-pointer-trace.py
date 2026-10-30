#!/usr/bin/env python3
"""The app's pointer traffic from a compositor's WAYLAND_DEBUG=server log
(scripts/linux-pointer.sh drives it): the wl_pointer events it was sent
and the pointer requests it sent, one a line, without timestamps, serials
or object numbers, so two runs compare line for line.

  linux-pointer-trace.py < sway.log > trace.txt

libwayland prints a message as `[ms] -> iface@id.name(args)` for an event
and without the arrow for a request (`#` in place of `@` from 1.22). The
app is the one client that asks for a wl_pointer here (the harness runs
sway with no bar), so its seat is the one whose get_pointer comes first,
and only that seat's capabilities and the pointers it made are kept. set_cursor and set_shape carry the serial of an enter, which they
must match -- "The serial parameter must match the latest
wl_pointer.enter serial number sent to the client. Otherwise the request
will be ignored" (wp-cursor-shape-v1.xml; wayland.xml says the same of
set_cursor) -- and each is marked when it does not.

What the app's surfaces are drawn at is kept too, as lines that begin
"surface": the window's surface (the one given an xdg_surface) and the
pointer's (the one set_cursor names), each buffer scale the app sets on
them, the size of each buffer it attaches to the pointer's and of the
window's when it changes, and the
preferred_buffer_scale the compositor sends the window. The app is the one
client here that makes surfaces: grim, wl-paste and the virtual devices
make none.
"""

import re
import sys

LINE = re.compile(r"^\[\s*[\d.]+\]\s+(?:\{[^}]*\}\s+)?(->\s+)?([\w]+)[@#](\d+)\.(\w+)\((.*)\)\s*$")
BUTTONS = {
    "272": "left",
    "273": "right",
    "274": "middle",
    "275": "side",
    "276": "extra",
    "277": "forward",
    "278": "back",
    "279": "task",
}
# wp_cursor_shape_device_v1.shape, version 1's entries, from the pinned XML.
SHAPES = """default context_menu help pointer progress wait cell crosshair text
vertical_text alias copy move no_drop not_allowed grab grabbing e_resize
n_resize ne_resize nw_resize s_resize se_resize sw_resize w_resize ew_resize
ns_resize nesw_resize nwse_resize col_resize row_resize all_scroll zoom_in
zoom_out""".split()
# The three images the app draws without cursor-shape-v1, by their
# hotspot (baud-present-wayland's cursor.rs).
DRAWN = {("12", "12"): "the I-beam", ("4", "3"): "the arrow", ("9", "2"): "the hand"}


def num(s):
    return s.rstrip("0").rstrip(".") if "." in s else s


def main():
    seat = None
    pointers = set()
    serial = None
    window = cursor = drawn = None
    buffers = {}
    # Lines for a surface not yet named by set_cursor: the app attaches and
    # scales the pointer's surface before it sets it.
    pending = {}

    def surface(sid, what):
        if sid == window:
            print("surface window", what)
        elif sid == cursor:
            print("surface pointer", what)
        else:
            pending.setdefault(sid, []).append(what)
    for raw in sys.stdin:
        m = LINE.match(raw.strip())
        if not m:
            continue
        event, iface, oid, name, args = m.groups()
        a = [x.strip() for x in args.split(",")] if args else []
        if iface == "xdg_wm_base" and name == "get_xdg_surface" and not event:
            window = re.search(r"wl_surface[@#](\d+)", args).group(1)
        elif iface == "wl_shm_pool" and name == "create_buffer" and not event:
            bid = re.search(r"wl_buffer[@#](\d+)", args).group(1)
            buffers[bid] = f"{a[2]}x{a[3]}"
        elif iface == "wl_surface" and not event and name == "set_buffer_scale":
            surface(oid, f"buffer_scale {a[0]}")
        elif iface == "wl_surface" and not event and name == "attach":
            bid = re.search(r"wl_buffer[@#](\d+)", args)
            size = buffers.get(bid.group(1), "?") if bid else None
            # The window attaches a buffer a frame: its size when it changes.
            if size and (oid != window or size != drawn):
                surface(oid, f"attach {size}")
                if oid == window:
                    drawn = size
        elif iface == "wl_surface" and event and name == "preferred_buffer_scale" and oid == window:
            print("surface window preferred_buffer_scale", a[0])
        if iface == "wl_seat" and name == "get_pointer" and seat in (None, oid):
            seat = oid
            pointers.add(re.search(r"wl_pointer[@#](\d+)", args).group(1))
        if iface == "wl_pointer" and oid not in pointers:
            continue
        if iface == "wl_seat" and name == "capabilities" and oid == seat:
            caps = int(a[0])
            have = [n for b, n in ((1, "pointer"), (2, "keyboard")) if caps & b]
            print("capabilities", " ".join(have) or "none")
        elif iface == "wl_seat" and name == "get_pointer" and oid == seat:
            print("get_pointer")
        elif iface == "wl_pointer" and event:
            if name == "enter":
                serial = a[0]
                print("enter", num(a[2]), num(a[3]))
            elif name == "leave":
                print("leave")
            elif name == "motion":
                print("motion", num(a[1]), num(a[2]))
            elif name == "button":
                print("button", BUTTONS.get(a[2], a[2]), "pressed" if a[3] == "1" else "released")
            elif name in ("axis", "axis_discrete", "axis_value120", "axis_source", "axis_stop"):
                axis = {"0": "vertical", "1": "horizontal"}
                if name == "axis":
                    print("axis", axis.get(a[1], a[1]), num(a[2]))
                elif name == "axis_source":
                    print("axis_source", a[0])
                elif name == "axis_stop":
                    print("axis_stop", axis.get(a[1], a[1]))
                else:
                    print(name, axis.get(a[0], a[0]), a[1])
        elif iface == "wl_pointer" and name == "set_cursor":
            mark = "" if a[0] == serial else "  (NOT the last enter's serial)"
            named = re.search(r"wl_surface[@#](\d+)", args)
            if named and named.group(1) != cursor:
                cursor = named.group(1)
                for what in pending.pop(cursor, []):
                    print("surface pointer", what)
            if a[1] == "nil":
                print("set_cursor hidden" + mark)
            else:
                print("set_cursor", DRAWN.get((a[2], a[3]), f"hotspot {a[2]} {a[3]}") + mark)
        elif iface == "wl_pointer" and name == "release":
            print("release")
        elif iface == "wp_cursor_shape_manager_v1" and name == "get_pointer":
            print("cursor-shape get_pointer")
        elif iface == "wp_cursor_shape_device_v1" and name == "set_shape":
            mark = "" if a[0] == serial else "  (NOT the last enter's serial)"
            v = int(a[1])
            print("set_shape", (SHAPES[v - 1] if 1 <= v <= len(SHAPES) else str(v)) + mark)
        elif iface == "wp_cursor_shape_device_v1" and name == "destroy":
            print("cursor-shape destroy")


if __name__ == "__main__":
    main()
