//! **libxkbcommon is the oracle.** Four keymaps compiled by `xkbcli` from
//! xkeyboard-config 2.48 -- what a compositor would send -- and, for each,
//! the library's answer for every key under twelve modifier and group
//! scenarios: the serialized state the compositor would report, and per
//! key the keysym, the UTF-8 and the level. `scripts/xkb-oracle.py` is the
//! harness that wrote them. A disagreement is a bug here unless the fixture
//! says why.

use baud_xkb::*;
use std::fs;
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let dir = std::env::var("BAUD_XKB_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"));
    let p = dir.join(name);
    fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

struct Scenario {
    label: String,
    mods: Mods,
    group: usize,
    /// `(keycode, keysym, utf8, layout, level)`
    keys: Vec<(u32, u32, Vec<u8>, usize, usize)>,
}

fn scenarios(oracle: &str) -> (Vec<(u32, String, bool)>, Vec<Scenario>) {
    let mut keys = Vec::new();
    let mut out: Vec<Scenario> = Vec::new();
    for line in oracle.lines() {
        if line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("key ") {
            let mut it = rest.split(' ');
            let kc: u32 = it.next().unwrap().parse().unwrap();
            let name = it.next().unwrap().to_string();
            let repeats = it.next().unwrap() == "repeats=1";
            keys.push((kc, name, repeats));
            continue;
        }
        if let Some(rest) = line.strip_prefix("scenario ") {
            let mut it = rest.split(' ');
            let label = it.next().unwrap().to_string();
            let mods = it.next().unwrap().strip_prefix("mods=").unwrap();
            let m: Vec<u32> = mods
                .split('/')
                .map(|h| u32::from_str_radix(h, 16).unwrap())
                .collect();
            let lay = it.next().unwrap().strip_prefix("layout=").unwrap();
            let l: Vec<usize> = lay.split('/').map(|d| d.parse().unwrap()).collect();
            out.push(Scenario {
                label,
                mods: Mods {
                    depressed: m[0],
                    latched: m[1],
                    locked: m[2],
                },
                group: l[2],
                keys: Vec::new(),
            });
            continue;
        }
        let mut it = line.split(' ');
        let kc: u32 = it.next().unwrap().parse().unwrap();
        let sym = u32::from_str_radix(it.next().unwrap(), 16).unwrap();
        let hex = it.next().unwrap();
        let utf8: Vec<u8> = (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
            .collect();
        let layout: usize = it.next().unwrap().parse().unwrap();
        let level: usize = it.next().unwrap().parse().unwrap();
        out.last_mut()
            .unwrap()
            .keys
            .push((kc, sym, utf8, layout, level));
    }
    (keys, out)
}

fn check(name: &str) {
    let km = Keymap::from_text(&fixture(&format!("keymap-{name}.xkb")))
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    let (keys, scenarios) = scenarios(&fixture(&format!("oracle-{name}.txt")));
    assert!(
        keys.len() > 200 && scenarios.len() == 12,
        "{name}: the fixture is whole"
    );
    let mut st = State::new(&km);
    for (kc, kname, repeats) in &keys {
        assert_eq!(
            km.key(Keycode(*kc)).map(|k| k.name.as_str()),
            Some(kname.as_str()),
            "{name}: key {kc}"
        );
        assert_eq!(
            st.repeats(Keycode(*kc)),
            *repeats,
            "{name}: {kname} repeats"
        );
    }
    let mut wrong = Vec::new();
    let mut total = 0;
    for sc in &scenarios {
        st.update(sc.mods, sc.group);
        for (kc, sym, utf8, layout, level) in &sc.keys {
            total += 1;
            let Some(l) = st.lookup(Keycode(*kc)) else {
                // An unbound keycode: the library reports an invalid layout
                // and level, no keysym and no text.
                if !(*sym == 0
                    && utf8.is_empty()
                    && *layout == 0xffff_ffff
                    && *level == 0xffff_ffff)
                {
                    wrong.push(format!(
                        "{name}: {} key {kc}: unbound here, bound there",
                        sc.label
                    ));
                }
                continue;
            };
            let got_sym = st.sym(Keycode(*kc));
            let got_text: Vec<u8> = st
                .text(Keycode(*kc))
                .map(|c| c.to_string().into_bytes())
                .unwrap_or_default();
            if got_sym.0 != *sym || got_text != *utf8 || l.layout != *layout || l.level != *level {
                wrong.push(format!(
                    "{}: {} key {kc} {}: want sym {sym:x} text {} layout {layout} level {level}; got sym {:x} text {} layout {} level {}",
                    name,
                    sc.label,
                    km.key(Keycode(*kc)).map(|k| k.name.as_str()).unwrap_or("?"),
                    String::from_utf8_lossy(utf8).escape_debug(),
                    got_sym.0,
                    String::from_utf8_lossy(&got_text).escape_debug(),
                    l.layout,
                    l.level
                ));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{name}: {} of {total} disagree with the library:\n{}",
        wrong.len(),
        wrong[..wrong.len().min(40)].join("\n")
    );
}

#[test]
fn us_agrees_with_the_library() {
    check("us");
}

#[test]
fn de_agrees_with_the_library() {
    check("de");
}

#[test]
fn us_intl_agrees_with_the_library() {
    check("us_intl");
}

#[test]
fn three_layouts_with_a_group_toggle_agree_with_the_library() {
    check("us_de_ru__grp_win_space_toggle");
}

/// The virtual modifiers resolve to the real encodings the library's
/// `# mods` line orders: LevelThree is Mod5 on these keymaps, NumLock Mod2.
#[test]
fn the_virtual_modifiers_encode_as_the_modmap_says() {
    let km = Keymap::from_text(&fixture("keymap-us.xkb")).unwrap();
    assert_eq!(
        km.mod_names[8..].join(" "),
        "NumLock Alt LevelThree Super LevelFive Meta Hyper ScrollLock"
    );
    let real = |n: &str| km.effective(1 << km.mod_index(n).unwrap());
    assert_eq!(real("NumLock"), 1 << 4, "Mod2");
    assert_eq!(real("Alt"), 1 << 3, "Mod1");
    assert_eq!(real("Super"), 1 << 6, "Mod4");
    assert_eq!(real("LevelThree"), 1 << 7, "Mod5");
}

/// Any further keymap pair in `$BAUD_XKB_FIXTURES`, named in
/// `$BAUD_XKB_EXTRA` as a comma-separated list: the six other layouts the
/// harness compiled are checked this way without being committed.
#[test]
fn extra_keymaps_named_in_the_environment_agree_with_the_library() {
    let Ok(names) = std::env::var("BAUD_XKB_EXTRA") else {
        return;
    };
    for n in names.split(',').filter(|n| !n.is_empty()) {
        check(n);
    }
}
