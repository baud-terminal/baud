#!/usr/bin/env python3
"""libxkbcommon as the oracle: what the library answers for every key of a
keymap under a set of modifier scenarios, written as a fixture baud-xkb has
to reproduce. Run once; the fixtures are committed.

The library is found by `$LIBXKBCOMMON`, the path of the shared library
itself, or else by `pkg-config --variable=libdir xkbcommon`; with neither the
script stops and says so. The committed fixtures came from libxkbcommon
1.13.2; set LIBXKBCOMMON to its libxkbcommon.0.dylib or .so."""
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
# The Compose API, `[[source: xkbcommon-compose-h]]`: a table from a
# buffer -- no file search, so the answer is the text's alone -- its
# entries through the iterator, and the state machine fed traces.
x.xkb_compose_table_new_from_buffer.restype = ctypes.c_void_p
x.xkb_compose_table_new_from_buffer.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_size_t,
                                                ctypes.c_char_p, ctypes.c_int, ctypes.c_int]
x.xkb_compose_table_iterator_new.restype = ctypes.c_void_p
x.xkb_compose_table_iterator_new.argtypes = [ctypes.c_void_p]
x.xkb_compose_table_iterator_next.restype = ctypes.c_void_p
x.xkb_compose_table_iterator_next.argtypes = [ctypes.c_void_p]
x.xkb_compose_table_iterator_free.argtypes = [ctypes.c_void_p]
x.xkb_compose_table_entry_sequence.restype = ctypes.POINTER(ctypes.c_uint32)
x.xkb_compose_table_entry_sequence.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t)]
x.xkb_compose_table_entry_keysym.restype = ctypes.c_uint32
x.xkb_compose_table_entry_keysym.argtypes = [ctypes.c_void_p]
x.xkb_compose_table_entry_utf8.restype = ctypes.c_char_p
x.xkb_compose_table_entry_utf8.argtypes = [ctypes.c_void_p]
x.xkb_compose_state_new.restype = ctypes.c_void_p
x.xkb_compose_state_new.argtypes = [ctypes.c_void_p, ctypes.c_int]
x.xkb_compose_state_feed.restype = ctypes.c_int
x.xkb_compose_state_feed.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
x.xkb_compose_state_get_status.restype = ctypes.c_int
x.xkb_compose_state_get_status.argtypes = [ctypes.c_void_p]
x.xkb_compose_state_get_utf8.restype = ctypes.c_int
x.xkb_compose_state_get_utf8.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_size_t]
x.xkb_compose_state_get_one_sym.restype = ctypes.c_uint32
x.xkb_compose_state_get_one_sym.argtypes = [ctypes.c_void_p]
x.xkb_compose_state_reset.argtypes = [ctypes.c_void_p]
x.xkb_compose_state_unref.argtypes = [ctypes.c_void_p]
x.xkb_compose_table_unref.argtypes = [ctypes.c_void_p]
SHIFT_L, F35, KEY_A = 0xffe1, 0xffe0, 0x61
STATUS = "ngdx"  # NOTHING, COMPOSING, COMPOSED, CANCELLED


def compose(path, out, locale):
    """Every entry of the table, then traces: each sequence and a keysym
    after it; all but its last keysym, then F35, which continues nothing
    here, then `a`; and for every sixteenth entry, Shift_L fed after its
    first keysym."""
    ctx = x.xkb_context_new(0)
    text = open(path, "rb").read()
    t = x.xkb_compose_table_new_from_buffer(ctx, text, len(text), locale.encode(), 1, 0)
    out.write(f"# compose {os.path.basename(path)} locale {locale}\n")
    if not t:
        out.write("table none\n")
        return
    it = x.xkb_compose_table_iterator_new(t)
    entries = []
    while True:
        e = x.xkb_compose_table_iterator_next(it)
        if not e:
            break
        n = ctypes.c_size_t()
        seq = x.xkb_compose_table_entry_sequence(e, ctypes.byref(n))
        entries.append(([seq[i] for i in range(n.value)], x.xkb_compose_table_entry_keysym(e),
                        x.xkb_compose_table_entry_utf8(e)))
    x.xkb_compose_table_iterator_free(it)
    for seq, sym, u8 in entries:
        out.write(f"e {','.join(f'{k:x}' for k in seq)} {sym:x} {u8.hex()}\n")
    st = x.xkb_compose_state_new(t, 0)
    buf = ctypes.create_string_buffer(512)
    def trace(feeds):
        x.xkb_compose_state_reset(st)
        res = []
        for k in feeds:
            fed = x.xkb_compose_state_feed(st, k)
            s = STATUS[x.xkb_compose_state_get_status(st)]
            r = ("I" if fed == 0 else "A") + s
            if s == "d":
                n = x.xkb_compose_state_get_utf8(st, buf, 512)
                r += f":{x.xkb_compose_state_get_one_sym(st):x}:{buf.raw[:n].hex()}"
            res.append(r)
        out.write(f"t {','.join(f'{k:x}' for k in feeds)} = {' '.join(res)}\n")
    for i, (seq, _, _) in enumerate(entries):
        trace(seq + [KEY_A])
        if len(seq) >= 2:
            trace(seq[:-1] + [F35, KEY_A])
            if i % 16 == 0:
                trace(seq[:1] + [SHIFT_L] + seq[1:])
    x.xkb_compose_state_unref(st)
    x.xkb_compose_table_unref(t)


if sys.argv[1:2] == ["compose"]:
    # xkb-oracle.py compose <Compose file> <oracle out> [locale]; the
    # environment's XLOCALEDIR and HOME are what %S, %L and %H expand to.
    with open(sys.argv[3], "w") as out:
        compose(sys.argv[2], out, sys.argv[4] if len(sys.argv) > 4 else "en_US.UTF-8")
    print("wrote", sys.argv[3])
    sys.exit(0)
for path in sys.argv[1:]:
    name = os.path.basename(path)[len("keymap-"):-len(".xkb")]
    with open(os.path.join(os.path.dirname(path), f"oracle-{name}.txt"), "w") as out:
        run(path, out)
    print("wrote", name)
