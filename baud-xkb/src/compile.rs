//! **From the source sections to a keymap the state can look keys up in.**
//!
//! What a client needs from a keymap is the keysym of a key at a
//! modifier state and group, and which modifiers that lookup consumed. So
//! the compile step resolves what the lookup depends on and nothing else:
//! the real encoding of every virtual modifier (`[[source:
//! xkb-keymap-text-format]]` "Modifiers encoding": the explicit `=` mask
//! or the union of the real modmaps of the keys whose virtual modmap
//! names it, which the compat `interpret` statements populate), each
//! key's type per group (named, or inferred by the four-way heuristic the
//! format states), and the type masks and map entries in real bits. The
//! actions the interprets bind are not carried: on Wayland the compositor
//! runs them and sends the client the resulting masks.

use crate::keysym::Keysym;
use crate::parse::{self, GroupDef, KeyDef, KeyRef, MatchOp, ModExpr, Source};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// A modifier mask: bits 0..8 the real modifiers in the X order, bit
/// `8 + i` the `i`th virtual modifier declared.
pub type ModMask = u32;

/// A keycode as the keymap and the wire number it: evdev's code plus
/// eight, which is what `wl_keyboard.key` carries and `<AE01> = 10` means.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Keycode(pub u32);

pub const SHIFT: ModMask = 1 << 0;
pub const LOCK: ModMask = 1 << 1;
pub const CONTROL: ModMask = 1 << 2;
pub const REAL: ModMask = 0xff;

const REAL_NAMES: [&str; 8] = [
    "Shift", "Lock", "Control", "Mod1", "Mod2", "Mod3", "Mod4", "Mod5",
];

#[derive(Clone, Debug)]
pub struct Type {
    pub name: String,
    /// The modifiers the type considers, in real bits.
    pub mask: ModMask,
    /// `(modifiers, level)`: the level when the masked state equals the mask.
    pub map: Vec<(ModMask, usize)>,
    /// `(modifiers, preserved)`: what a matching entry does not consume.
    pub preserve: Vec<(ModMask, ModMask)>,
    pub levels: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Group {
    pub type_index: usize,
    /// The keysyms per level.
    pub levels: Vec<Vec<Keysym>>,
}

#[derive(Clone, Debug, Default)]
pub struct Key {
    pub name: String,
    pub groups: Vec<Group>,
    pub repeats: bool,
    /// The real modifiers bound to the key by `modifier_map`.
    pub modmap: ModMask,
    /// The virtual modifiers bound to the key, as virtual bits.
    pub vmodmap: ModMask,
}

#[derive(Debug)]
pub struct Keymap {
    pub min_keycode: u32,
    pub max_keycode: u32,
    /// Indexed by `keycode - min_keycode`.
    pub keys: Vec<Option<Key>>,
    pub types: Vec<Type>,
    /// The eight real names, then the virtual ones in declaration order.
    pub mod_names: Vec<String>,
    /// Each virtual modifier's real encoding.
    pub vmod_real: Vec<ModMask>,
    pub num_groups: usize,
    pub group_names: Vec<String>,
}

#[derive(Debug)]
pub enum Error {
    Parse(parse::Error),
    /// A key names a keycode the keycodes section did not declare.
    UnknownKey(String),
    /// A group names a type the types section did not declare.
    UnknownType(String),
    /// The keycodes section declares no range.
    NoKeycodes,
}

impl From<parse::Error> for Error {
    fn from(e: parse::Error) -> Error {
        Error::Parse(e)
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(self, f)
    }
}

impl Keymap {
    /// Compile the keymap text a compositor sends.
    pub fn from_text(text: &str) -> Result<Keymap, Error> {
        compile(parse::parse(text)?)
    }

    pub fn key(&self, keycode: Keycode) -> Option<&Key> {
        self.keys
            .get(usize::try_from(keycode.0.checked_sub(self.min_keycode)?).ok()?)?
            .as_ref()
    }

    pub fn keycode_named(&self, name: &str) -> Option<Keycode> {
        self.keys
            .iter()
            .position(|k| k.as_ref().is_some_and(|k| k.name == name))
            .and_then(|i| u32::try_from(i).ok())
            .map(|i| Keycode(i + self.min_keycode))
    }

    /// A mask with virtual bits, in real bits.
    pub fn effective(&self, mask: ModMask) -> ModMask {
        let mut m = mask & REAL;
        for (i, r) in self.vmod_real.iter().enumerate() {
            if mask & (1 << (8 + i)) != 0 {
                m |= r & REAL;
            }
        }
        m
    }

    pub fn mod_index(&self, name: &str) -> Option<usize> {
        self.mod_names.iter().position(|n| n == name)
    }
}

/// The four-way heuristic the format states for a key without a type.
fn inferred_type(levels: &[Vec<Keysym>]) -> &'static str {
    let sym = |i: usize| levels.get(i).and_then(|l| (l.len() == 1).then(|| l[0]));
    let pair = |a: Option<Keysym>, b: Option<Keysym>| matches!((a, b), (Some(a), Some(b)) if a.is_lower() && b.is_upper());
    let keypad = |i: usize| sym(i).is_some_and(Keysym::is_keypad);
    match levels.len() {
        0 | 1 => "ONE_LEVEL",
        2 => match pair(sym(0), sym(1)) {
            true => "ALPHABETIC",
            false => match keypad(0) || keypad(1) {
                true => "KEYPAD",
                false => "TWO_LEVEL",
            },
        },
        3 | 4 => match pair(sym(0), sym(1)) {
            true => match pair(sym(2), sym(3)) {
                true => "FOUR_LEVEL_ALPHABETIC",
                false => "FOUR_LEVEL_SEMIALPHABETIC",
            },
            false => match keypad(0) || keypad(1) {
                true => "FOUR_LEVEL_KEYPAD",
                false => "FOUR_LEVEL",
            },
        },
        _ => "ONE_LEVEL",
    }
}

fn mask_of(e: &ModExpr, mod_names: &[String]) -> ModMask {
    let mut m = e.mask;
    if e.all {
        m |= REAL;
    }
    for n in &e.names {
        if let Some(i) = mod_names.iter().position(|x| x.eq_ignore_ascii_case(n)) {
            m |= 1 << i;
        }
    }
    m
}

fn predicate(op: MatchOp, wanted: ModMask, modmap: ModMask) -> bool {
    let wanted = wanted & REAL;
    match op {
        MatchOp::AnyOfOrNone => modmap == 0 || modmap & wanted != 0,
        MatchOp::AnyOf => modmap & wanted != 0,
        MatchOp::NoneOf => modmap & wanted == 0,
        MatchOp::AllOf => modmap & wanted == wanted,
        MatchOp::Exactly => modmap == wanted,
    }
}

fn compile(src: Source) -> Result<Keymap, Error> {
    if src.key_names.is_empty() {
        return Err(Error::NoKeycodes);
    }
    let min = src
        .min_keycode
        .min(src.key_names.iter().map(|(_, c)| *c).min().unwrap_or(0));
    let max = src
        .max_keycode
        .max(src.key_names.iter().map(|(_, c)| *c).max().unwrap_or(0));
    let mut mod_names: Vec<String> = REAL_NAMES.iter().map(|s| s.to_string()).collect();
    mod_names.extend(src.vmods.iter().map(|(n, _)| n.clone()));
    let span = usize::try_from(max - min + 1).map_err(|_| Error::NoKeycodes)?;
    let mut keys: Vec<Option<Key>> = (0..span).map(|_| None).collect();
    let code_of = |name: &str| -> Option<u32> {
        let name = src
            .aliases
            .iter()
            .find(|(a, _)| a == name)
            .map_or(name, |(_, b)| b.as_str());
        src.key_names
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, c)| *c)
    };
    // Types by name, with the standard names resolvable for inference.
    let types: Vec<Type> = src
        .types
        .iter()
        .map(|t| Type {
            name: t.name.clone(),
            mask: mask_of(&t.mods, &mod_names),
            map: t
                .map
                .iter()
                .map(|(m, l)| (mask_of(m, &mod_names), *l))
                .collect(),
            preserve: t
                .preserve
                .iter()
                .map(|(m, p)| (mask_of(m, &mod_names), mask_of(p, &mod_names)))
                .collect(),
            levels: t.levels,
        })
        .collect();
    let type_named = |n: &str| types.iter().position(|t| t.name == n);
    // Every declared keycode is a key; one the symbols section leaves
    // unbound has no levels and, as the library answers, does not repeat.
    for (name, code) in &src.key_names {
        let slot = usize::try_from(code - min).map_err(|_| Error::NoKeycodes)?;
        if keys[slot].is_none() {
            keys[slot] = Some(Key {
                name: name.clone(),
                repeats: false,
                modmap: 0,
                vmodmap: 0,
                groups: alloc::vec![Group {
                    type_index: type_named("ONE_LEVEL").unwrap_or(0),
                    levels: Vec::new(),
                }],
            });
        }
    }
    // Keys.
    for kd in &src.keys {
        let code = code_of(&kd.name).ok_or_else(|| Error::UnknownKey(kd.name.clone()))?;
        let slot = usize::try_from(code - min).map_err(|_| Error::NoKeycodes)?;
        let name = keys[slot]
            .as_ref()
            .map_or(kd.name.clone(), |k| k.name.clone());
        let mut key = Key {
            name,
            // The library's rule: a key starts not repeating, and only the
            // interpret its base level matches -- or the default interpret,
            // which repeats, when a keysym there matches none -- makes it
            // repeat. A base level with no keysym matches nothing at all.
            repeats: kd.repeat.unwrap_or(false),
            modmap: 0,
            vmodmap: kd
                .vmods
                .as_ref()
                .map_or(0, |v| mask_of(v, &mod_names) & !REAL),
            groups: Vec::new(),
        };
        for g in &kd.groups {
            key.groups.push(group_of(g, kd, &types, type_named)?);
        }
        if key.groups.is_empty() {
            key.groups.push(Group {
                type_index: type_named("ONE_LEVEL").unwrap_or(0),
                levels: Vec::new(),
            });
        }
        keys[slot] = Some(key);
    }
    // The real modifier map: by keycode, or by the first key with the keysym.
    for (m, refs) in &src.modmap {
        let Some(bit) = REAL_NAMES.iter().position(|n| n.eq_ignore_ascii_case(m)) else {
            continue;
        };
        for r in refs {
            let slot = match r {
                KeyRef::Name(n) => code_of(n).map(|c| c - min),
                KeyRef::Sym(s) => first_key_with(&keys, *s),
            };
            if let Some(k) = slot
                .and_then(|s| keys.get_mut(usize::try_from(s).ok()?))
                .and_then(Option::as_mut)
            {
                k.modmap |= 1 << bit;
            }
        }
    }
    // The interpret step, for the virtual modifier map and the repeat.
    for key in keys.iter_mut().flatten() {
        let explicit_repeat = src
            .keys
            .iter()
            .find(|k| k.name == key.name)
            .and_then(|k| k.repeat);
        for (gi, g) in key.groups.iter().enumerate() {
            for (li, level) in g.levels.iter().enumerate() {
                let Some(&sym) = level.first() else {
                    continue;
                };
                let mut best: Option<&parse::Interpret> = None;
                for i in &src.interprets {
                    if i.level1 && (gi > 0 || li > 0) {
                        continue;
                    }
                    let sym_ok = match i.sym {
                        Some(s) => s == sym,
                        None => true,
                    };
                    if !sym_ok || !predicate(i.op, mask_of(&i.mods, &mod_names), key.modmap) {
                        continue;
                    }
                    let better = match best {
                        None => true,
                        Some(b) => match (b.sym.is_some(), i.sym.is_some()) {
                            (false, true) => true,
                            (true, false) => false,
                            _ => i.op > b.op,
                        },
                    };
                    if better {
                        best = Some(i);
                    }
                }
                let Some(i) = best else {
                    // The default interpret: no action, no modifier, repeats.
                    if gi == 0 && li == 0 && explicit_repeat.is_none() {
                        key.repeats = true;
                    }
                    continue;
                };
                if let Some(v) = &i.vmod {
                    if let Some(idx) = mod_names.iter().position(|n| n.eq_ignore_ascii_case(v)) {
                        key.vmodmap |= 1 << idx;
                    }
                }
                if gi == 0
                    && li == 0
                    && explicit_repeat.is_none()
                    && i.repeat.unwrap_or(src.interpret_repeat_default)
                {
                    key.repeats = true;
                }
            }
        }
    }
    // Virtual modifier encodings: explicit, plus the modmaps of the keys
    // that carry the virtual modifier.
    let mut vmod_real: Vec<ModMask> = src.vmods.iter().map(|(_, m)| m.unwrap_or(0)).collect();
    for key in keys.iter().flatten() {
        for (i, r) in vmod_real.iter_mut().enumerate() {
            if key.vmodmap & (1 << (8 + i)) != 0 {
                *r |= key.modmap;
            }
        }
    }
    let mut km = Keymap {
        min_keycode: min,
        max_keycode: max,
        keys,
        types,
        mod_names,
        vmod_real,
        num_groups: 1,
        group_names: src.group_names.clone(),
    };
    // Types in real bits, now that the encodings are known.
    let eff: Vec<Type> = km
        .types
        .iter()
        .map(|t| Type {
            name: t.name.clone(),
            mask: km.effective(t.mask),
            map: t.map.iter().map(|(m, l)| (km.effective(*m), *l)).collect(),
            preserve: t
                .preserve
                .iter()
                .map(|(m, p)| (km.effective(*m), km.effective(*p)))
                .collect(),
            levels: t.levels,
        })
        .collect();
    km.types = eff;
    km.num_groups = km
        .keys
        .iter()
        .flatten()
        .map(|k| k.groups.len())
        .max()
        .unwrap_or(1)
        .max(1);
    Ok(km)
}

fn group_of(
    g: &GroupDef,
    kd: &KeyDef,
    types: &[Type],
    type_named: impl Fn(&str) -> Option<usize>,
) -> Result<Group, Error> {
    let name = g
        .type_name
        .as_deref()
        .or(kd.type_all.as_deref())
        .map_or_else(|| inferred_type(&g.syms).to_string(), str::to_string);
    let type_index = type_named(&name)
        .or_else(|| type_named(inferred_type(&g.syms)))
        .ok_or(Error::UnknownType(name))?;
    let mut levels = g.syms.clone();
    // A level the type has and the key does not is empty.
    let want = types[type_index].levels;
    if levels.len() < want {
        levels.resize(want, Vec::new());
    }
    Ok(Group { type_index, levels })
}

/// The first key producing `sym`: lowest group, then level, then keycode.
fn first_key_with(keys: &[Option<Key>], sym: Keysym) -> Option<u32> {
    let mut best: Option<(usize, usize, usize)> = None;
    for (slot, key) in keys.iter().enumerate() {
        let Some(key) = key else { continue };
        for (gi, g) in key.groups.iter().enumerate() {
            for (li, l) in g.levels.iter().enumerate() {
                if l.len() == 1 && l[0] == sym && best.is_none_or(|b| (gi, li, slot) < b) {
                    best = Some((gi, li, slot));
                }
            }
        }
    }
    best.and_then(|(_, _, s)| u32::try_from(s).ok())
}
