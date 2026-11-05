//! **libxkbcommon, declared as the extern lines it is**: the keymap, state
//! and Compose calls the xkb oracle makes, every handle opaque, so no
//! struct has to be laid out; the `abi` tests hold the enum values passed
//! to `xkbcommon.h` and `xkbcommon-compose.h`.
//!
//! The library has no version call, so the version is the one `pkg-config`
//! named when the crate was built (`build.rs`), and [`pinned`] refuses any
//! but [`LIBXKBCOMMON`] before a line is written.
//!
//! What the Python harness did through ctypes is reproduced call for call:
//! a context with no flags, a keymap compiled from the file's text, twelve
//! modifier and group scenarios fed as key events, and a Compose table read
//! from a buffer, so no file search reaches it beyond the includes its text
//! names.

#![allow(unsafe_code)]

use core::ffi::{c_char, c_int};
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// The libxkbcommon baud-xkb's fixtures were made with.
pub const LIBXKBCOMMON: &str = "1.13.2";

/// The version `pkg-config` named for the linked library at build time.
pub const LINKED: &str = env!("BAUD_ORACLE_XKBCOMMON");

/// `XKB_KEY_UP`, `XKB_KEY_DOWN`.
pub const KEY_UP: c_int = 0;
/// `XKB_KEY_DOWN`.
pub const KEY_DOWN: c_int = 1;
/// `XKB_STATE_MODS_DEPRESSED`, `_LATCHED`, `_LOCKED`, `_EFFECTIVE`.
pub const MODS: [c_int; 4] = [1, 2, 4, 8];
/// `XKB_STATE_LAYOUT_DEPRESSED`, `_LATCHED`, `_LOCKED`, `_EFFECTIVE`.
pub const LAYOUTS: [c_int; 4] = [16, 32, 64, 128];
/// `XKB_KEYMAP_FORMAT_TEXT_V1`, and `XKB_COMPOSE_FORMAT_TEXT_V1`.
pub const FORMAT_TEXT_V1: c_int = 1;

#[repr(C)]
struct Opaque {
    _private: [u8; 0],
}

#[link(name = "xkbcommon")]
unsafe extern "C" {
    fn xkb_context_new(flags: c_int) -> *mut Opaque;
    fn xkb_keymap_new_from_string(
        ctx: *mut Opaque,
        text: *const c_char,
        format: c_int,
        flags: c_int,
    ) -> *mut Opaque;
    fn xkb_keymap_min_keycode(km: *mut Opaque) -> u32;
    fn xkb_keymap_max_keycode(km: *mut Opaque) -> u32;
    fn xkb_keymap_key_get_name(km: *mut Opaque, key: u32) -> *const c_char;
    fn xkb_keymap_key_repeats(km: *mut Opaque, key: u32) -> c_int;
    fn xkb_keymap_num_layouts(km: *mut Opaque) -> u32;
    fn xkb_keymap_num_mods(km: *mut Opaque) -> u32;
    fn xkb_keymap_mod_get_name(km: *mut Opaque, idx: u32) -> *const c_char;
    fn xkb_state_new(km: *mut Opaque) -> *mut Opaque;
    fn xkb_state_unref(st: *mut Opaque);
    fn xkb_state_update_key(st: *mut Opaque, key: u32, direction: c_int) -> c_int;
    fn xkb_state_key_get_one_sym(st: *mut Opaque, key: u32) -> u32;
    fn xkb_state_key_get_utf8(st: *mut Opaque, key: u32, buf: *mut c_char, size: usize) -> c_int;
    fn xkb_state_key_get_layout(st: *mut Opaque, key: u32) -> u32;
    fn xkb_state_key_get_level(st: *mut Opaque, key: u32, layout: u32) -> u32;
    fn xkb_state_serialize_mods(st: *mut Opaque, components: c_int) -> u32;
    fn xkb_state_serialize_layout(st: *mut Opaque, components: c_int) -> u32;

    fn xkb_compose_table_new_from_buffer(
        ctx: *mut Opaque,
        buffer: *const c_char,
        length: usize,
        locale: *const c_char,
        format: c_int,
        flags: c_int,
    ) -> *mut Opaque;
    fn xkb_compose_table_unref(t: *mut Opaque);
    fn xkb_compose_table_iterator_new(t: *mut Opaque) -> *mut Opaque;
    fn xkb_compose_table_iterator_next(it: *mut Opaque) -> *mut Opaque;
    fn xkb_compose_table_iterator_free(it: *mut Opaque);
    fn xkb_compose_table_entry_sequence(e: *mut Opaque, n: *mut usize) -> *const u32;
    fn xkb_compose_table_entry_keysym(e: *mut Opaque) -> u32;
    fn xkb_compose_table_entry_utf8(e: *mut Opaque) -> *const c_char;
    fn xkb_compose_state_new(t: *mut Opaque, flags: c_int) -> *mut Opaque;
    fn xkb_compose_state_unref(st: *mut Opaque);
    fn xkb_compose_state_reset(st: *mut Opaque);
    fn xkb_compose_state_feed(st: *mut Opaque, keysym: u32) -> c_int;
    fn xkb_compose_state_get_status(st: *mut Opaque) -> c_int;
    fn xkb_compose_state_get_utf8(st: *mut Opaque, buf: *mut c_char, size: usize) -> c_int;
    fn xkb_compose_state_get_one_sym(st: *mut Opaque) -> u32;
}

/// Refuse to write a line unless the linked library is the pinned one.
pub fn pinned() {
    assert_eq!(
        LINKED, LIBXKBCOMMON,
        "linked libxkbcommon {LINKED}, not the pinned {LIBXKBCOMMON}"
    );
}

/// Python's `buf.raw[:n]` on a buffer of `buf.len()` bytes: a negative
/// `n` counts from the end, and either end is clamped.
fn raw_prefix(buf: &[u8], n: c_int) -> &[u8] {
    let len = buf.len();
    let end = match usize::try_from(n) {
        Ok(n) => n.min(len),
        Err(_) => len.saturating_sub(n.unsigned_abs() as usize),
    };
    &buf[..end]
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A C string a call returned, or `None` for a null pointer.
///
/// # Safety
/// `p` is null or a NUL-terminated string that outlives the borrow.
unsafe fn text<'a>(p: *const c_char) -> Option<&'a [u8]> {
    // SAFETY: the caller's contract.
    (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_bytes())
}

/// The basename the Python printed, `os.path.basename(path)`.
fn basename(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// One scenario: its label, the keys held, and the keys (or chords,
/// pressed in order and released in reverse) tapped before them.
struct Scenario {
    label: &'static str,
    held: &'static [u32],
    toggles: &'static [&'static [u32]],
}

// evdev + 8
const LSHIFT: u32 = 50;
const RALT: u32 = 108;
const LCTL: u32 = 37;
const LALT: u32 = 64;
const CAPS: u32 = 66;
const NUMLOCK: u32 = 77;
const LWIN: u32 = 133;
const SPACE: u32 = 65;

const SCENARIOS: [Scenario; 12] = [
    Scenario {
        label: "plain",
        held: &[],
        toggles: &[],
    },
    Scenario {
        label: "shift",
        held: &[LSHIFT],
        toggles: &[],
    },
    Scenario {
        label: "ralt",
        held: &[RALT],
        toggles: &[],
    },
    Scenario {
        label: "shift+ralt",
        held: &[LSHIFT, RALT],
        toggles: &[],
    },
    Scenario {
        label: "ctrl",
        held: &[LCTL],
        toggles: &[],
    },
    Scenario {
        label: "caps",
        held: &[],
        toggles: &[&[CAPS]],
    },
    Scenario {
        label: "caps+shift",
        held: &[LSHIFT],
        toggles: &[&[CAPS]],
    },
    Scenario {
        label: "numlock",
        held: &[],
        toggles: &[&[NUMLOCK]],
    },
    Scenario {
        label: "lalt",
        held: &[LALT],
        toggles: &[],
    },
    Scenario {
        label: "group-next",
        held: &[],
        toggles: &[&[LWIN, SPACE]],
    },
    Scenario {
        label: "group-next-shift",
        held: &[LSHIFT],
        toggles: &[&[LWIN, SPACE]],
    },
    Scenario {
        label: "altshift-toggle",
        held: &[],
        toggles: &[&[LALT, LSHIFT]],
    },
];

/// The keymap oracle's file for one `keymap-*.xkb`: the keycode range, the
/// modifiers, every named key below 256, then each scenario's serialized
/// state and every key's keysym, UTF-8, layout and level. `Err` is the
/// Python's `assert km, path`.
pub fn keymap(path: &Path) -> Result<String, String> {
    let mut data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    // ctypes passed the bytes as `c_char_p`: C reads to the first NUL.
    if let Some(i) = data.iter().position(|&b| b == 0) {
        data.truncate(i);
    }
    let source = CString::new(data).expect("no NUL is left");
    let mut out = String::new();
    // SAFETY: every handle is the library's own and used while it lives;
    // the strings it returns are read before the next call on the keymap.
    unsafe {
        let ctx = xkb_context_new(0);
        let km = xkb_keymap_new_from_string(ctx, source.as_ptr(), FORMAT_TEXT_V1, 0);
        if km.is_null() {
            return Err(format!("AssertionError: {}", path.display()));
        }
        let (lo, hi) = (xkb_keymap_min_keycode(km), xkb_keymap_max_keycode(km));
        out += &format!(
            "# keymap {} keycodes {lo}..{hi} layouts {}\n",
            basename(path),
            xkb_keymap_num_layouts(km)
        );
        let mods: Vec<String> = (0..xkb_keymap_num_mods(km))
            .map(|i| {
                let n = text_of(xkb_keymap_mod_get_name(km, i));
                String::from_utf8(n.to_vec()).expect("a modifier name in UTF-8")
            })
            .collect();
        out += &format!("# mods {}\n", mods.join(" "));
        let top = hi.min(255);
        for kc in lo..=top {
            let Some(name) = text(xkb_keymap_key_get_name(km, kc)) else {
                continue;
            };
            out += &format!(
                "key {kc} {} repeats={}\n",
                std::str::from_utf8(name).expect("a key name in UTF-8"),
                xkb_keymap_key_repeats(km, kc)
            );
        }
        let mut buf = [0u8; 64];
        for s in &SCENARIOS {
            let st = xkb_state_new(km);
            for t in s.toggles {
                for &k in *t {
                    xkb_state_update_key(st, k, KEY_DOWN);
                }
                if let [a, b] = t {
                    xkb_state_update_key(st, *b, KEY_UP);
                    xkb_state_update_key(st, *a, KEY_UP);
                } else {
                    xkb_state_update_key(st, t[0], KEY_UP);
                }
            }
            for &h in s.held {
                xkb_state_update_key(st, h, KEY_DOWN);
            }
            let m = MODS.map(|w| xkb_state_serialize_mods(st, w));
            let l = LAYOUTS.map(|w| xkb_state_serialize_layout(st, w));
            out += &format!(
                "scenario {} mods={:x}/{:x}/{:x}/{:x} layout={}/{}/{}/{}\n",
                s.label, m[0], m[1], m[2], m[3], l[0], l[1], l[2], l[3]
            );
            for kc in lo..=top {
                if xkb_keymap_key_get_name(km, kc).is_null() {
                    continue;
                }
                let sym = xkb_state_key_get_one_sym(st, kc);
                let n = xkb_state_key_get_utf8(st, kc, buf.as_mut_ptr().cast(), buf.len());
                let layout = xkb_state_key_get_layout(st, kc);
                let level = xkb_state_key_get_level(st, kc, layout);
                out += &format!(
                    "{kc} {sym:x} {} {layout} {level}\n",
                    hex(raw_prefix(&buf, n))
                );
            }
            xkb_state_unref(st);
        }
    }
    Ok(out)
}

/// `c_char_p`'s restype: a null pointer read as an empty name, which the
/// Python's `.decode()` would have refused; no modifier index below
/// `num_mods` has one.
///
/// # Safety
/// As [`text`].
unsafe fn text_of<'a>(p: *const c_char) -> &'a [u8] {
    // SAFETY: the caller's contract.
    unsafe { text(p) }.expect("a modifier name")
}

const SHIFT_L: u32 = 0xffe1;
const F35: u32 = 0xffe0;
const KEY_A: u32 = 0x61;
/// `XKB_COMPOSE_NOTHING`, `_COMPOSING`, `_COMPOSED`, `_CANCELLED`.
const STATUS: [char; 4] = ['n', 'g', 'd', 'x'];
/// `XKB_COMPOSE_COMPOSED`.
pub const COMPOSED: c_int = 2;
/// `XKB_COMPOSE_FEED_IGNORED`.
pub const FEED_IGNORED: c_int = 0;

/// The Compose oracle's file for one table: every entry of the table, then
/// traces -- each sequence and a keysym after it; all but its last keysym,
/// then F35, which continues nothing here, then `a`; and for every
/// sixteenth entry, Shift_L fed after its first keysym. The environment's
/// `XLOCALEDIR` and `HOME` are what `%S`, `%L` and `%H` expand to.
pub fn compose(path: &Path, locale: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let loc = CString::new(locale).map_err(|e| format!("the locale: {e}"))?;
    let mut out = format!("# compose {} locale {locale}\n", basename(path));
    // SAFETY: the buffer is passed with its length, NULs and all, as ctypes
    // passed the bytes; every handle is used while it lives and the
    // sequence and string an entry returns are read before the iterator
    // moves.
    unsafe {
        let ctx = xkb_context_new(0);
        let t = xkb_compose_table_new_from_buffer(
            ctx,
            data.as_ptr().cast(),
            data.len(),
            loc.as_ptr(),
            FORMAT_TEXT_V1,
            0,
        );
        if t.is_null() {
            out += "table none\n";
            return Ok(out);
        }
        let it = xkb_compose_table_iterator_new(t);
        let mut entries: Vec<(Vec<u32>, u32, Vec<u8>)> = Vec::new();
        loop {
            let e = xkb_compose_table_iterator_next(it);
            if e.is_null() {
                break;
            }
            let mut n = 0usize;
            let seq = xkb_compose_table_entry_sequence(e, &mut n);
            let seq = core::slice::from_raw_parts(seq, n).to_vec();
            let u8 = text(xkb_compose_table_entry_utf8(e))
                .expect("an entry's text")
                .to_vec();
            entries.push((seq, xkb_compose_table_entry_keysym(e), u8));
        }
        xkb_compose_table_iterator_free(it);
        let keys = |s: &[u32]| {
            s.iter()
                .map(|k| format!("{k:x}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        for (seq, sym, u8) in &entries {
            out += &format!("e {} {sym:x} {}\n", keys(seq), hex(u8));
        }
        let st = xkb_compose_state_new(t, 0);
        let mut buf = [0u8; 512];
        let mut trace = |feeds: &[u32], out: &mut String| {
            xkb_compose_state_reset(st);
            let mut res = Vec::new();
            for &k in feeds {
                let fed = xkb_compose_state_feed(st, k);
                let status = xkb_compose_state_get_status(st);
                let s = STATUS[usize::try_from(status).expect("a status")];
                let mut r = format!("{}{s}", if fed == FEED_IGNORED { 'I' } else { 'A' });
                if status == COMPOSED {
                    let n = xkb_compose_state_get_utf8(st, buf.as_mut_ptr().cast(), buf.len());
                    r += &format!(
                        ":{:x}:{}",
                        xkb_compose_state_get_one_sym(st),
                        hex(raw_prefix(&buf, n))
                    );
                }
                res.push(r);
            }
            *out += &format!("t {} = {}\n", keys(feeds), res.join(" "));
        };
        for (i, (seq, _, _)) in entries.iter().enumerate() {
            trace(&[seq.as_slice(), &[KEY_A]].concat(), &mut out);
            if seq.len() >= 2 {
                trace(&[&seq[..seq.len() - 1], &[F35, KEY_A]].concat(), &mut out);
                if i % 16 == 0 {
                    trace(&[&seq[..1], &[SHIFT_L], &seq[1..]].concat(), &mut out);
                }
            }
        }
        xkb_compose_state_unref(st);
        xkb_compose_table_unref(t);
    }
    Ok(out)
}

/// The keymap fixtures, relative to the root.
const FIXTURES: &str = "baud-xkb/tests/fixtures";

/// Every Compose fixture and the environment it was recorded under: the
/// file, the locale, and whether `HOME` is set. `XLOCALEDIR` is
/// `compose/xlocale` and `HOME` `compose/home` for every one, but for
/// `home-unset`, which is the case of `HOME` not set.
pub fn compose_fixtures(root: &Path) -> Vec<(PathBuf, String, bool)> {
    let dir = root.join(FIXTURES).join("compose");
    let mut v: Vec<(PathBuf, String, bool)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.expect("an entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "Compose"))
        .map(|p| {
            let home = p.file_stem().is_some_and(|s| s != "home-unset");
            (p, "xx".to_string(), home)
        })
        .collect();
    v.sort();
    for locale in ["en_US.UTF-8", "el_GR.UTF-8"] {
        v.push((
            root.join(format!(
                "docs/research/sources/libx11-1.8.13-compose-{locale}.txt"
            )),
            locale.to_string(),
            true,
        ));
    }
    v
}

/// The oracle file a keymap or Compose fixture is written to.
fn oracle_file(root: &Path, input: &Path, compose: Option<&str>) -> PathBuf {
    let dir = root.join(FIXTURES);
    match compose {
        Some(locale) if !input.starts_with(dir.join("compose")) => {
            dir.join(format!("compose/oracle-{locale}.txt"))
        }
        Some(_) => dir.join(format!(
            "compose/oracle-{}.txt",
            input.file_stem().expect("a stem").to_string_lossy()
        )),
        None => {
            let name = basename(input);
            let name = name
                .strip_prefix("keymap-")
                .and_then(|n| n.strip_suffix(".xkb"))
                .expect("a keymap-<name>.xkb");
            dir.join(format!("oracle-{name}.txt"))
        }
    }
}

/// `baud-oracle xkb`'s arguments, as the Python harness took them:
/// `compose <file> <out> [locale]`, or keymap files, each written beside
/// itself as `oracle-<name>.txt`; or none, or `--check`, which regenerate
/// (or compare) every fixture under `--root`, each Compose table in a
/// child process whose environment is its own.
pub fn main(args: &[String]) -> ExitCode {
    pinned();
    match args {
        [c, file, out, rest @ ..] if c == "compose" && rest.len() <= 1 => {
            let locale = rest.first().map_or("en_US.UTF-8", String::as_str);
            // The Python opened its output before reading anything.
            std::fs::write(out, "").expect("open the oracle");
            match compose(Path::new(file), locale) {
                Ok(text) => {
                    std::fs::write(out, text).expect("write the oracle");
                    println!("wrote {out}");
                    ExitCode::SUCCESS
                }
                Err(e) => fail(&e),
            }
        }
        [c, ..] if c == "compose" => {
            eprintln!("usage: baud-oracle xkb compose <Compose file> <oracle out> [locale]");
            ExitCode::FAILURE
        }
        [] => all(&crate::repo(), false),
        [c] if c == "--check" => all(&crate::repo(), true),
        [r, root] if r == "--root" => all(Path::new(root), false),
        [c, r, root] | [r, root, c] if c == "--check" && r == "--root" => {
            all(Path::new(root), true)
        }
        files => {
            for f in files {
                let path = Path::new(f);
                let name = basename(path);
                let name = name
                    .get("keymap-".len()..name.len().saturating_sub(".xkb".len()))
                    .unwrap_or("")
                    .to_string();
                let dir = path.parent().unwrap_or(Path::new(""));
                let out = dir.join(format!("oracle-{name}.txt"));
                std::fs::write(&out, "").expect("open the oracle");
                match keymap(path) {
                    Ok(text) => {
                        std::fs::write(&out, text).expect("write the oracle");
                        println!("wrote {name}");
                    }
                    Err(e) => return fail(&e),
                }
            }
            ExitCode::SUCCESS
        }
    }
}

fn fail(e: &str) -> ExitCode {
    eprintln!("baud-oracle xkb: {e}");
    ExitCode::FAILURE
}

/// Every fixture's text, regenerated: the keymaps in this process, each
/// Compose table in a child with only its own environment.
pub fn regenerate(root: &Path) -> Result<Vec<(PathBuf, String)>, String> {
    let dir = root.join(FIXTURES);
    let mut keymaps: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .map(|e| e.expect("an entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "xkb"))
        .collect();
    keymaps.sort();
    let mut v = Vec::new();
    for k in &keymaps {
        v.push((oracle_file(root, k, None), keymap(k)?));
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let compose_dir = dir.join("compose");
    let scratch = std::env::temp_dir().join(format!("baud-oracle-xkb-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
    for (file, locale, home) in compose_fixtures(root) {
        let out = scratch.join("oracle.txt");
        let mut cmd = Command::new(&exe);
        cmd.env_clear()
            .env("XLOCALEDIR", compose_dir.join("xlocale"))
            .args(["xkb", "compose"])
            .arg(&file)
            .arg(&out)
            .arg(&locale);
        if home {
            cmd.env("HOME", compose_dir.join("home"));
        }
        let run = cmd.output().map_err(|e| e.to_string())?;
        if !run.status.success() {
            return Err(format!(
                "{}: {}",
                file.display(),
                String::from_utf8_lossy(&run.stderr)
            ));
        }
        let text = std::fs::read_to_string(&out).map_err(|e| e.to_string())?;
        v.push((oracle_file(root, &file, Some(&locale)), text));
    }
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(v)
}

fn all(root: &Path, check: bool) -> ExitCode {
    let files = match regenerate(root) {
        Ok(f) => f,
        Err(e) => return fail(&e),
    };
    let mut bad = 0;
    for (path, text) in &files {
        if check {
            let committed = std::fs::read_to_string(path).unwrap_or_default();
            if committed != *text {
                eprintln!("{}: differs from the committed file", path.display());
                bad += 1;
            }
        } else {
            std::fs::write(path, text).expect("write the oracle");
            println!("wrote {}", path.display());
        }
    }
    if bad > 0 {
        return ExitCode::FAILURE;
    }
    if check {
        println!("{} xkb fixtures, as committed", files.len());
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::ask_the_headers;

    #[test]
    fn the_enums_are_the_header_s() {
        let h = ask_the_headers();
        assert_eq!(h["XKB_KEY_UP"], KEY_UP as i128);
        assert_eq!(h["XKB_KEY_DOWN"], KEY_DOWN as i128);
        let mods = [
            "XKB_STATE_MODS_DEPRESSED",
            "XKB_STATE_MODS_LATCHED",
            "XKB_STATE_MODS_LOCKED",
            "XKB_STATE_MODS_EFFECTIVE",
        ];
        for (name, v) in mods.iter().zip(MODS) {
            assert_eq!(h[*name], v as i128, "{name}");
        }
        let layouts = [
            "XKB_STATE_LAYOUT_DEPRESSED",
            "XKB_STATE_LAYOUT_LATCHED",
            "XKB_STATE_LAYOUT_LOCKED",
            "XKB_STATE_LAYOUT_EFFECTIVE",
        ];
        for (name, v) in layouts.iter().zip(LAYOUTS) {
            assert_eq!(h[*name], v as i128, "{name}");
        }
        assert_eq!(h["XKB_KEYMAP_FORMAT_TEXT_V1"], FORMAT_TEXT_V1 as i128);
        assert_eq!(h["XKB_COMPOSE_FORMAT_TEXT_V1"], FORMAT_TEXT_V1 as i128);
        assert_eq!(h["XKB_CONTEXT_NO_FLAGS"], 0);
        assert_eq!(h["XKB_KEYMAP_COMPILE_NO_FLAGS"], 0);
        assert_eq!(h["XKB_COMPOSE_COMPILE_NO_FLAGS"], 0);
        assert_eq!(h["XKB_COMPOSE_STATE_NO_FLAGS"], 0);
        let status = [
            "XKB_COMPOSE_NOTHING",
            "XKB_COMPOSE_COMPOSING",
            "XKB_COMPOSE_COMPOSED",
            "XKB_COMPOSE_CANCELLED",
        ];
        for (i, name) in status.iter().enumerate() {
            assert_eq!(h[*name], i as i128, "{name}");
        }
        assert_eq!(h["XKB_COMPOSE_COMPOSED"], COMPOSED as i128);
        assert_eq!(h["XKB_COMPOSE_FEED_IGNORED"], FEED_IGNORED as i128);
        assert_eq!(h["XKB_COMPOSE_FEED_ACCEPTED"], 1);
    }

    #[test]
    fn the_linked_libxkbcommon_is_the_pinned_one() {
        assert_eq!(LINKED, LIBXKBCOMMON);
    }

    /// Python's slice of the raw buffer, past either end.
    #[test]
    fn a_raw_prefix_is_python_s_slice() {
        let b = [1u8, 2, 3, 4];
        assert_eq!(raw_prefix(&b, 2), [1, 2]);
        assert_eq!(raw_prefix(&b, 9), b);
        assert_eq!(raw_prefix(&b, -1), [1, 2, 3]);
        assert_eq!(raw_prefix(&b, -9), [0u8; 0]);
    }
}
