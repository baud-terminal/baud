#!/usr/bin/env python3
"""A pointer and a keyboard for a headless wlroots compositor, from a script
(scripts/linux-pointer.sh drives it).

Standard library only: it speaks the Wayland wire format itself, as
wayland.xml's "Wire Format" describes it -- a header of the object id and
of the size and opcode, then the arguments, each four-byte aligned -- and
binds two globals a wlroots compositor offers to clients:

  zwlr_virtual_pointer_manager_v1  (wlr-virtual-pointer-unstable-v1.xml)
  zwp_virtual_keyboard_manager_v1  (virtual-keyboard-unstable-v1.xml)

Both XMLs are pinned in docs/research/sources. One process holds one
pointer and one keyboard for the whole script, so a drag is one press, its
moves and one release on the same device, and the modifiers a click is
read with are the seat's while it happens.

  linux-pointer.py <keymap.xkb> < script

One command a line:

  move X Y           motion_absolute to output pixel (X, Y), then frame
  press B | release B | click B
                     B is left, right, middle or side; click is a press
                     and a release
  wheel N            N detents down (negative up): axis_discrete, value
                     15 a detent, as libinput reports a wheel, then frame
  across N           the same on the horizontal axis
  swipe PX           a touchpad's scroll, PX surface pixels down (negative
                     up): axis_source finger and one axis, no detents
  mods M             the modifiers held: none, or shift, ctrl, alt joined
                     by '+'; sent as the keymap's bits, Shift 0, Control 2,
                     Mod1 3
  key K              an evdev key code pressed and released
  tick MS            advance the event clock; every event advances it 10
  sleep MS           wait in real time, and advance the event clock as much
  sh CMD             run CMD with bash, waiting for it
  waitfor CMD        run CMD every 50 ms until it succeeds, 10 s at most
  again              destroy both devices and make two new ones: the seat
                     loses its pointer and keyboard and gains new ones

The event clock is the time every request carries: it starts at 1000 ms
and moves only by the script, so a double click is the same double click
however long the machine took between its two halves, and two clicks the
script keeps apart are never one.
"""

import array
import os
import socket
import struct
import subprocess
import sys
import time

BUTTONS = {"left": 0x110, "right": 0x111, "middle": 0x112, "side": 0x113}
MODS = {"shift": 1 << 0, "ctrl": 1 << 2, "alt": 1 << 3}

# Opcodes, in each interface's order in its XML.
DISPLAY_SYNC, DISPLAY_GET_REGISTRY = 0, 1
REGISTRY_BIND = 0
VPM_CREATE = 0  # zwlr_virtual_pointer_manager_v1.create_virtual_pointer
VP_MOTION_ABSOLUTE, VP_BUTTON, VP_AXIS, VP_FRAME = 1, 2, 3, 4
VP_AXIS_SOURCE, VP_AXIS_DISCRETE, VP_DESTROY = 5, 7, 8
VKM_CREATE = 0  # zwp_virtual_keyboard_manager_v1.create_virtual_keyboard
VK_KEYMAP, VK_KEY, VK_MODIFIERS, VK_DESTROY = 0, 1, 2, 3
# wayland.xml's enums.
AXIS_VERTICAL, AXIS_HORIZONTAL = 0, 1
AXIS_SOURCE_WHEEL, AXIS_SOURCE_FINGER = 0, 1
KEYMAP_XKB_V1 = 1


def u32(v):
    return struct.pack("=I", v & 0xFFFFFFFF)


def i32(v):
    return struct.pack("=i", v)


def fixed(v):
    return struct.pack("=i", round(v * 256))


def string(s):
    b = s.encode() + b"\0"
    return u32(len(b)) + b + b"\0" * (-len(b) % 4)


class Wire:
    def __init__(self):
        path = os.path.join(
            os.environ["XDG_RUNTIME_DIR"], os.environ.get("WAYLAND_DISPLAY", "wayland-0")
        )
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.connect(path)
        self.next = 2
        self.inbox = b""
        self.globals = {}
        self.registry = None
        self.waiting = set()
        self.done = set()

    def new(self):
        n = self.next
        self.next += 1
        return n

    def send(self, obj, op, args=b"", fds=()):
        msg = u32(obj) + u32(((8 + len(args)) << 16) | op) + args
        anc = [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array("i", fds))] if fds else []
        self.sock.sendmsg([msg], anc)

    def read(self):
        data = self.sock.recv(65536)
        if not data:
            sys.exit("linux-pointer: the compositor closed the connection")
        self.inbox += data
        while len(self.inbox) >= 8:
            obj, word = struct.unpack_from("=II", self.inbox)
            size, op = word >> 16, word & 0xFFFF
            if len(self.inbox) < size:
                break
            body, self.inbox = self.inbox[8:size], self.inbox[size:]
            self.event(obj, op, body)

    def event(self, obj, op, body):
        if obj == 1 and op == 0:  # wl_display.error
            oid, code, n = struct.unpack_from("=III", body)
            sys.exit(f"linux-pointer: error on object {oid}, code {code}: {body[12:12 + n - 1].decode()}")
        if obj == self.registry and op == 0:  # wl_registry.global
            name, n = struct.unpack_from("=II", body)
            iface = body[8 : 8 + n - 1].decode()
            (version,) = struct.unpack_from("=I", body, 8 + n + (-n % 4))
            self.globals[iface] = (name, version)
        elif op == 0 and obj in self.waiting:  # wl_callback.done
            self.done.add(obj)

    def roundtrip(self):
        cb = self.new()
        self.waiting.add(cb)
        self.send(1, DISPLAY_SYNC, u32(cb))
        while cb not in self.done:
            self.read()

    def bind(self, iface, version):
        if iface not in self.globals:
            sys.exit(f"linux-pointer: the compositor offers no {iface}")
        name, offered = self.globals[iface]
        obj = self.new()
        v = min(version, offered)
        self.send(self.registry, REGISTRY_BIND, u32(name) + string(iface) + u32(v) + u32(obj))
        return obj


def main():
    keymap = open(sys.argv[1], "rb").read() + b"\0"
    w = Wire()
    w.registry = w.new()
    w.send(1, DISPLAY_GET_REGISTRY, u32(w.registry))
    w.roundtrip()
    seat = w.bind("wl_seat", 1)
    vpm = w.bind("zwlr_virtual_pointer_manager_v1", 1)
    vkm = w.bind("zwp_virtual_keyboard_manager_v1", 1)

    def devices():
        pointer, keyboard = w.new(), w.new()
        w.send(vpm, VPM_CREATE, u32(seat) + u32(pointer))
        w.send(vkm, VKM_CREATE, u32(seat) + u32(keyboard))
        fd = os.memfd_create("keymap")
        os.write(fd, keymap)
        w.send(keyboard, VK_KEYMAP, u32(KEYMAP_XKB_V1) + u32(len(keymap)), fds=[fd])
        os.close(fd)
        w.roundtrip()
        return pointer, keyboard

    pointer, keyboard = devices()
    out = os.environ.get("POINTER_W", "1024"), os.environ.get("POINTER_H", "768")
    extent = int(out[0]), int(out[1])
    clock = 1000

    def at():
        nonlocal clock
        clock += 10
        return u32(clock)

    def scroll(axis, n):
        w.send(pointer, VP_AXIS_SOURCE, u32(AXIS_SOURCE_WHEEL))
        w.send(pointer, VP_AXIS_DISCRETE, at() + u32(axis) + fixed(15.0 * n) + i32(n))
        w.send(pointer, VP_FRAME)

    for line in sys.stdin:
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        cmd, _, rest = line.partition(" ")
        if cmd == "move":
            x, y = (int(v) for v in rest.split())
            w.send(pointer, VP_MOTION_ABSOLUTE, at() + u32(x) + u32(y) + u32(extent[0]) + u32(extent[1]))
            w.send(pointer, VP_FRAME)
        elif cmd in ("press", "release", "click"):
            b = BUTTONS[rest]
            if cmd in ("press", "click"):
                w.send(pointer, VP_BUTTON, at() + u32(b) + u32(1))
                w.send(pointer, VP_FRAME)
            if cmd in ("release", "click"):
                w.send(pointer, VP_BUTTON, at() + u32(b) + u32(0))
                w.send(pointer, VP_FRAME)
        elif cmd == "wheel":
            scroll(AXIS_VERTICAL, int(rest))
        elif cmd == "swipe":
            w.send(pointer, VP_AXIS_SOURCE, u32(AXIS_SOURCE_FINGER))
            w.send(pointer, VP_AXIS, at() + u32(AXIS_VERTICAL) + fixed(float(rest)))
            w.send(pointer, VP_FRAME)
        elif cmd == "across":
            scroll(AXIS_HORIZONTAL, int(rest))
        elif cmd == "mods":
            bits = 0
            if rest != "none":
                for m in rest.split("+"):
                    bits |= MODS[m]
            w.send(keyboard, VK_MODIFIERS, u32(bits) + u32(0) + u32(0) + u32(0))
        elif cmd == "key":
            k = int(rest)
            w.send(keyboard, VK_KEY, at() + u32(k) + u32(1))
            w.send(keyboard, VK_KEY, at() + u32(k) + u32(0))
        elif cmd == "tick":
            clock += int(rest)
        elif cmd == "sleep":
            time.sleep(int(rest) / 1000)
            clock += int(rest)
        elif cmd == "sh":
            subprocess.run(["bash", "-c", rest])
        elif cmd == "waitfor":
            for _ in range(200):
                if subprocess.run(["bash", "-c", rest]).returncode == 0:
                    break
                time.sleep(0.05)
            else:
                print(f"linux-pointer: timed out waiting for: {rest}", flush=True)
        elif cmd == "again":
            w.send(pointer, VP_DESTROY)
            w.send(keyboard, VK_DESTROY)
            w.roundtrip()
            time.sleep(0.3)
            pointer, keyboard = devices()
        else:
            sys.exit(f"linux-pointer: unknown command: {line}")
        w.roundtrip()


if __name__ == "__main__":
    main()
