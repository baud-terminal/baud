#!/usr/bin/env python3
"""libxkbcommon as the oracle: what the library answers for every key of a
keymap under a set of modifier scenarios, written as a fixture baud-xkb has
to reproduce. Run once; the fixtures are committed.

The library is found by `$LIBXKBCOMMON`, the path of the shared library
itself, or else by `pkg-config --variable=libdir xkbcommon`; with neither the
script stops and says so. The committed fixtures came from libxkbcommon
1.13.2, whose nix output is /nix/store/q68bncwdcfknxhswvhkk6jq3aq6k2pa7-
libxkbcommon-1.13.2 -- a path, not a dependency, and one the store
collects."""
import ctypes, sys, os, subprocess


def find_lib():
    env = os.environ.get("LIBXKBCOMMON")
    if env:
        if not os.path.isfile(env):
            sys.exit(f"xkb-oracle: LIBXKBCOMMON={env} is not a file")
        return env
    try:
        libdir = subprocess.run(["pkg-config", "--variable=libdir", "xkbcommon"],
                                capture_output=True, text=True, check=True).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        libdir = ""
    names = ("libxkbcommon.dylib", "libxkbcommon.so", "libxkbcommon.so.0")
    for name in names if libdir else ():
        path = os.path.join(libdir, name)
        if os.path.isfile(path):
            return path
    sys.exit("xkb-oracle: libxkbcommon not found. Set LIBXKBCOMMON to the shared library "
             "(e.g. .../lib/libxkbcommon.dylib), or put xkbcommon.pc on PKG_CONFIG_PATH"
             + (f" (pkg-config's libdir {libdir} holds none of {', '.join(names)})" if libdir else ""))


x = ctypes.CDLL(find_lib())
x.xkb_context_new.restype = ctypes.c_void_p
x.xkb_keymap_new_from_string.restype = ctypes.c_void_p
x.xkb_keymap_new_from_string.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int, ctypes.c_int]
x.xkb_state_new.restype = ctypes.c_void_p
x.xkb_state_new.argtypes = [ctypes.c_void_p]
x.xkb_state_update_key.argtypes = [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_int]
x.xkb_state_update_key.restype = ctypes.c_int
x.xkb_state_key_get_one_sym.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
x.xkb_state_key_get_one_sym.restype = ctypes.c_uint32
x.xkb_state_key_get_utf8.argtypes = [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_char_p, ctypes.c_size_t]
x.xkb_state_key_get_utf8.restype = ctypes.c_int
x.xkb_state_key_get_layout.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
x.xkb_state_key_get_layout.restype = ctypes.c_uint32
x.xkb_state_key_get_level.argtypes = [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint32]
x.xkb_state_key_get_level.restype = ctypes.c_uint32
x.xkb_state_serialize_mods.argtypes = [ctypes.c_void_p, ctypes.c_int]
x.xkb_state_serialize_mods.restype = ctypes.c_uint32
x.xkb_state_serialize_layout.argtypes = [ctypes.c_void_p, ctypes.c_int]
x.xkb_state_serialize_layout.restype = ctypes.c_uint32
x.xkb_keymap_min_keycode.argtypes = [ctypes.c_void_p]; x.xkb_keymap_min_keycode.restype = ctypes.c_uint32
x.xkb_keymap_max_keycode.argtypes = [ctypes.c_void_p]; x.xkb_keymap_max_keycode.restype = ctypes.c_uint32
x.xkb_keymap_key_get_name.argtypes = [ctypes.c_void_p, ctypes.c_uint32]; x.xkb_keymap_key_get_name.restype = ctypes.c_char_p
x.xkb_keymap_key_repeats.argtypes = [ctypes.c_void_p, ctypes.c_uint32]; x.xkb_keymap_key_repeats.restype = ctypes.c_int
x.xkb_keymap_num_layouts.argtypes = [ctypes.c_void_p]; x.xkb_keymap_num_layouts.restype = ctypes.c_uint32
x.xkb_keymap_num_mods.argtypes = [ctypes.c_void_p]; x.xkb_keymap_num_mods.restype = ctypes.c_uint32
x.xkb_keymap_mod_get_name.argtypes = [ctypes.c_void_p, ctypes.c_uint32]; x.xkb_keymap_mod_get_name.restype = ctypes.c_char_p
x.xkb_state_unref.argtypes = [ctypes.c_void_p]
DOWN, UP = 1, 0
DEPRESSED, LATCHED, LOCKED, EFFECTIVE = 1, 2, 4, 8
# evdev + 8
LSHIFT, RSHIFT, LCTL, LALT, RALT, CAPS, NUMLOCK, LWIN, SPACE, LEVEL5 = 50, 62, 37, 64, 108, 66, 77, 133, 65, 0
SCENARIOS = [
    ("plain", [], []),
    ("shift", [LSHIFT], []),
    ("ralt", [RALT], []),
    ("shift+ralt", [LSHIFT, RALT], []),
    ("ctrl", [LCTL], []),
    ("caps", [], [CAPS]),
    ("caps+shift", [LSHIFT], [CAPS]),
    ("numlock", [], [NUMLOCK]),
    ("lalt", [LALT], []),
    ("group-next", [], [(LWIN, SPACE)]),
    ("group-next-shift", [LSHIFT], [(LWIN, SPACE)]),
    ("altshift-toggle", [], [(LALT, LSHIFT)]),
]
def run(path, out):
    ctx = x.xkb_context_new(0)
    text = open(path, "rb").read()
    km = x.xkb_keymap_new_from_string(ctx, text, 1, 0)
    assert km, path
    lo, hi = x.xkb_keymap_min_keycode(km), x.xkb_keymap_max_keycode(km)
    out.write(f"# keymap {os.path.basename(path)} keycodes {lo}..{hi} layouts {x.xkb_keymap_num_layouts(km)}\n")
    out.write("# mods " + " ".join(x.xkb_keymap_mod_get_name(km, i).decode() for i in range(x.xkb_keymap_num_mods(km))) + "\n")
    for kc in range(lo, min(hi, 255) + 1):
        name = x.xkb_keymap_key_get_name(km, kc)
        if not name:
            continue
        out.write(f"key {kc} {name.decode()} repeats={x.xkb_keymap_key_repeats(km, kc)}\n")
    buf = ctypes.create_string_buffer(64)
    for label, held, toggles in SCENARIOS:
        st = x.xkb_state_new(km)
        for t in toggles:
            if isinstance(t, tuple):
                x.xkb_state_update_key(st, t[0], DOWN); x.xkb_state_update_key(st, t[1], DOWN)
                x.xkb_state_update_key(st, t[1], UP); x.xkb_state_update_key(st, t[0], UP)
            else:
                x.xkb_state_update_key(st, t, DOWN); x.xkb_state_update_key(st, t, UP)
        for h in held:
            x.xkb_state_update_key(st, h, DOWN)
        mods = tuple(x.xkb_state_serialize_mods(st, w) for w in (DEPRESSED, LATCHED, LOCKED, EFFECTIVE))
        lay = tuple(x.xkb_state_serialize_layout(st, w) for w in (16, 32, 64, 128))
        out.write(f"scenario {label} mods={mods[0]:x}/{mods[1]:x}/{mods[2]:x}/{mods[3]:x} layout={lay[0]}/{lay[1]}/{lay[2]}/{lay[3]}\n")
        for kc in range(lo, min(hi, 255) + 1):
            if not x.xkb_keymap_key_get_name(km, kc):
                continue
            sym = x.xkb_state_key_get_one_sym(st, kc)
            n = x.xkb_state_key_get_utf8(st, kc, buf, 64)
            u8 = buf.raw[:n].hex()
            layout = x.xkb_state_key_get_layout(st, kc)
            level = x.xkb_state_key_get_level(st, kc, layout)
            out.write(f"{kc} {sym:x} {u8} {layout} {level}\n")
        x.xkb_state_unref(st)
for path in sys.argv[1:]:
    name = os.path.basename(path)[len("keymap-"):-len(".xkb")]
    with open(os.path.join(os.path.dirname(path), f"oracle-{name}.txt"), "w") as out:
        run(path, out)
    print("wrote", name)
