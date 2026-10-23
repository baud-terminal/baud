//! **What a program's `OSC 50` does to the font**: the faces a name can
//! reach, the stack it gives, and the size a step lands on. Pure but for
//! the parse of a face already in memory, so its tests run on every
//! platform, and both drivers perform through it.
//!
//! **A name resolves to a face baud has loaded** (Jev's lean, OWN_FACES,
//! 0.79/0.85 after a re-ask), through the lookup a config's `fonts` names
//! take (`Catalogue::lookup_at`, kitty's order): a face's PostScript name
//! (nameID 6), else its full name (nameID 4) where that is none of its own
//! family names, else a family name (nameID 16 or 1, any language,
//! `[[source: ms-opentype-name]]`) of a face in the fallback chain -- the
//! embedded Regular, Symbols Nerd Font Mono and STIX Two Math, and the
//! faces a config names -- each by the default caseless match CSS Fonts
//! gives a family name (`[[source: css-fonts-3]]` 5.1). A face found by its
//! PostScript or full name may be a styled one, the embedded Bold among
//! them; it leads the chain as the regular face, as a config's face named
//! alone does. The embedded Regular's table names it "Proto Hebrew Mono
//! C", the built font's name while its family name is open
//! (`fonts/built/FONTLOG.txt`). The face named leads the chain, the rest
//! follow in the config's order, and the other styled faces of its family
//! come with it; the embedded faces stay behind it as they stay behind a
//! configured stack. A name baud has not loaded leaves the font as it
//! was: a program's name is not looked up among the installed fonts, only
//! a config's is.
//!
//! **The query answers the name a program set** (Jev, Q_AS_SET,
//! 0.94/0.94 after reading mintty): the last name baud took, as the program
//! sent it, as xterm answers the face name `setFaceName` stored
//! (`[[source: xterm-misc]]` 3798-3846, `getFaceName` in its
//! `fontutils.c`) and mintty the name `win_change_font` stored
//! (`[[source: mintty-wintext-0aea388]]` 1120-1154), so the
//! answer sent back finds the same face: each face's last name while it
//! leads, and the family of a face no program has named.
//!
//! **A size step is a point of the View menu's Bigger and Smaller**,
//! bounded as they are (Jev's lean, SIZE_STEPS, 0.22/0.18): `#+n` and `#-n`
//! step `n` points, `#0` returns to the configured size, and a step past
//! the bounds is refused whole, as xterm's `lookupRelativeFontSize` finds
//! no entry.

use crate::config::Config;
use crate::discover::{Catalogue, NamedAt};
use baud_core::{FontChange, FontSize, SizeStep};
use baud_font::var::Variation;
use baud_font::{Face, FaceIndex};
use baud_render::FaceRole;

/// The smallest and largest size, in points, a step or the View menu may
/// reach.
pub const SIZE_MIN: f32 = 6.0;
pub const SIZE_MAX: f32 = 72.0;

/// A face of the config's list, by its place in it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot(usize);

#[cfg(all(test, target_os = "macos"))]
impl Slot {
    /// A face's slot, for a test that needs one without a stack.
    pub const TEST: Slot = Slot(3);
    /// The slot that leads at start, for a test that needs it.
    pub const FIRST: Slot = Slot(0);
}

/// The faces a program can name, and the stacks built for them.
pub struct Fonts {
    data: Vec<&'static [u8]>,
    /// Each face's index in its file, its role, and the axis values a
    /// variable one is drawn at, so a stack made again draws it as the
    /// config's did.
    specs: Vec<(FaceIndex, FaceRole, Vec<Variation>)>,
    families: Vec<Option<String>>,
    built: Vec<Option<Built>>,
    current: Slot,
    /// The faces as the config's lookup reads them, and each one's slot.
    lookup: Catalogue,
    slots: Vec<usize>,
    /// Each face's last name a program's change was taken with, as it
    /// sent it: what a query answers while that face leads.
    asked: Vec<Option<String>>,
    /// The mappings under `data` and every parsed slice in `built`, held
    /// so a font reload can give them back (`host::refont`).
    held: crate::pty::Held,
}

/// A stack's faces, in its order, and each one's role.
#[derive(Clone)]
pub struct Built {
    pub faces: &'static [Face<'static>],
    pub roles: Vec<FaceRole>,
}

/// What a change does, decided before any of it is done.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Plan {
    /// The face that leads the chain after, `None` when the change names
    /// no family.
    pub face: Option<Slot>,
    /// The size after, in points.
    pub points: f32,
}

/// Why a change changes nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refused {
    /// No face baud has loaded is this family.
    Unknown,
    /// The step lands outside [`SIZE_MIN`] to [`SIZE_MAX`].
    Bounds,
}

impl Fonts {
    /// The config's faces, `data` their bytes and `faces` them parsed, in
    /// the config's order: the stack the app starts with, led by the first.
    /// `held` holds the mappings under `data` and the slice `faces` is.
    pub fn new(
        cfg: &Config,
        data: Vec<&'static [u8]>,
        faces: &'static [Face<'static>],
        held: crate::pty::Held,
    ) -> Fonts {
        let families = faces
            .iter()
            .map(|f| {
                let mut out = [0u8; baud_core::FONT_NAME_MAX];
                let name = f.family()?.utf8(&mut out)?.to_vec();
                String::from_utf8(name).ok()
            })
            .collect();
        let specs: Vec<_> = cfg
            .faces
            .iter()
            .map(|f| (f.index, f.role, f.variations.clone()))
            .collect();
        // The lookup holds every face but the badge's, which draws no text.
        let named: Vec<usize> = (0..specs.len())
            .filter(|&i| specs[i].1 != FaceRole::Badge)
            .collect();
        let loaded: Vec<_> = named.iter().map(|&i| (data[i], specs[i].0)).collect();
        let (lookup, at) = Catalogue::of_loaded(&loaded);
        let slots = at.into_iter().map(|k| named[k]).collect();
        let mut built = vec![None; specs.len()];
        if let Some(first) = built.first_mut() {
            *first = Some(Built {
                faces,
                roles: specs.iter().map(|s| s.1).collect(),
            });
        }
        let asked = vec![None; specs.len()];
        Fonts {
            data,
            specs,
            families,
            built,
            current: Slot(0),
            lookup,
            slots,
            asked,
            held,
        }
    }

    /// **What these fonts hold, for a font reload to give back**: every
    /// other borrow of their faces and bytes -- the stacks built, the
    /// lookup -- dropped here.
    pub fn into_held(self) -> crate::pty::Held {
        self.held
    }

    /// The family the chain is led by, as the core tells a query; empty
    /// for a face whose `name` table names none.
    pub fn family(&self) -> &str {
        self.families
            .get(self.current.0)
            .and_then(Option::as_deref)
            .unwrap_or("")
    }

    /// What a query answers: the name a program last set while the face
    /// it named leads, else the family ([`Fonts::family`]).
    pub fn answer(&self) -> &str {
        match self.asked.get(self.current.0) {
            Some(Some(name)) => name,
            _ => self.family(),
        }
    }

    /// **The face `name` names**, by the config's lookup: the first face
    /// whose PostScript name or full name it is, styled or not, else the
    /// first face of the chain that is of its family, in the config's
    /// order. Never the badge's face, which draws no text.
    pub fn resolve(&self, name: &[u8]) -> Option<Slot> {
        let name = core::str::from_utf8(name).ok()?;
        match self.lookup.lookup_at(name) {
            NamedAt::Face(i) => Some(Slot(self.slots[i])),
            NamedAt::Family(f) => f
                .into_iter()
                .map(|i| self.slots[i])
                .find(|&i| self.specs[i].1 == FaceRole::Fallback)
                .map(Slot),
            NamedAt::Nothing => None,
        }
    }

    /// What `change` does at `points`, with `configured` the config's size;
    /// refused whole when its name is none baud has or its step leaves the
    /// bounds. A change taken with a name keeps it for the query, which
    /// answers it once that face leads.
    pub fn plan(
        &mut self,
        change: &FontChange,
        points: f32,
        configured: f32,
    ) -> Result<Plan, Refused> {
        let face = match change.name() {
            Some(n) => Some(self.resolve(n).ok_or(Refused::Unknown)?),
            None => None,
        };
        let points = match change.size {
            SizeStep::Keep => points,
            SizeStep::Configured => configured,
            SizeStep::By(n) => {
                let to = points + n as f32;
                if !(SIZE_MIN..=SIZE_MAX).contains(&to) {
                    return Err(Refused::Bounds);
                }
                to
            }
        };
        if let (Some(slot), Some(name)) = (face, change.name()) {
            self.asked[slot.0] = Some(String::from_utf8_lossy(name).into_owned());
        }
        Ok(Plan { face, points })
    }

    /// Whether `slot` already leads the chain.
    pub fn leads(&self, slot: Slot) -> bool {
        self.current == slot
    }

    /// **The stack `slot` leads**: it first, as the regular face, the rest
    /// of the chain in the config's order, the other styled faces of its
    /// family, so a styled face named alone has no style beside itself,
    /// and the badge's face, whatever the family. Parsed from
    /// bytes already in memory and kept, so a face is parsed once per
    /// stack it leads and a program asking again and again makes nothing
    /// new. `None` if a face does not parse again, which it did once.
    pub fn stack(&mut self, slot: Slot) -> Option<Built> {
        if let Some(b) = self.built.get(slot.0)?.clone() {
            return Some(b);
        }
        let family = self.families.get(slot.0)?.clone();
        let mut order = vec![slot.0];
        for (i, (_, role, _)) in self.specs.iter().enumerate() {
            let same = family.is_some() && self.families[i] == family;
            match role {
                _ if i == slot.0 => {}
                FaceRole::Fallback | FaceRole::Badge => order.push(i),
                FaceRole::Style(_) if same => order.push(i),
                FaceRole::Style(_) => {}
            }
        }
        let faces = order
            .iter()
            .map(|&i| {
                let mut face = Face::parse_index(self.data[i], self.specs[i].0).ok()?;
                face.set_variations(&self.specs[i].2);
                Some(face)
            })
            .collect::<Option<Vec<_>>>()?;
        let built = Built {
            faces: self.held.leak(faces),
            roles: order
                .iter()
                .enumerate()
                .map(|(n, &i)| match n {
                    0 => FaceRole::Fallback,
                    _ => self.specs[i].1,
                })
                .collect(),
        };
        self.built[slot.0] = Some(built.clone());
        Some(built)
    }

    /// `slot` leads the chain now.
    pub fn set(&mut self, slot: Slot) {
        self.current = slot;
    }
}

/// A zoom key's step: Cmd + / Cmd - / Cmd 0 on macOS, Ctrl on Linux.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zoom {
    In,
    Out,
    Reset,
}

/// How far one zoom step moves the size: one of the configured size's
/// units on both systems -- a point on macOS, a pixel on Linux, which has
/// no backing scale yet -- so the same presses reach the same `font_size`
/// (Jev, ONE_POINT, 0.94/0.92, over foot's half point).
pub const ZOOM_STEP: f32 = 1.0;

/// **The size a zoom step from `at` lands on**: a point up or down, within
/// [`SIZE_MIN`] and [`SIZE_MAX`], or back to the configured `base`.
pub fn zoomed(step: Zoom, at: f32, base: f32) -> f32 {
    match step {
        Zoom::In => (at + ZOOM_STEP).clamp(SIZE_MIN, SIZE_MAX),
        Zoom::Out => (at - ZOOM_STEP).clamp(SIZE_MIN, SIZE_MAX),
        Zoom::Reset => base,
    }
}

/// Whether `points` is the configured size, as the core says it to a query
/// of the current entry.
pub fn size_of(points: f32, configured: f32) -> FontSize {
    match points == configured {
        true => FontSize::Configured,
        false => FontSize::Stepped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A zoom step is a point, held inside the sizes, and a reset is the
    /// configured size.** Perturbation: a half-point step -- the first
    /// assertion fails.
    #[test]
    fn a_zoom_step_is_a_point_within_the_sizes() {
        assert_eq!(zoomed(Zoom::In, 13.0, 13.0), 14.0);
        assert_eq!(zoomed(Zoom::Out, 13.0, 13.0), 12.0);
        assert_eq!(zoomed(Zoom::In, SIZE_MAX, 13.0), SIZE_MAX);
        assert_eq!(zoomed(Zoom::Out, SIZE_MIN, 13.0), SIZE_MIN);
        assert_eq!(zoomed(Zoom::Reset, 30.0, 13.0), 13.0);
    }

    fn fonts(text: &str) -> Fonts {
        let mut cfg = Config::defaults();
        let said = cfg.apply(text);
        assert!(said.is_empty(), "{said:?}");
        crate::host::resolved(&mut cfg);
        let (data, mut held) = crate::host::map_faces(&cfg);
        let faces = crate::host::parse_faces(&cfg, &data, &mut held);
        Fonts::new(&cfg, data, faces, held)
    }

    /// **A program cannot name the badge's face for the text**: with Fira
    /// Code configured alone, the embedded Bold is loaded only as the
    /// badge's face, and `OSC 50` naming it by its PostScript or full name
    /// finds no face, as it found none before the badge had one. The
    /// control: on the default stack, where the embedded Bold is the bold
    /// face too, the same names resolve to that styled face.
    #[test]
    fn a_program_cannot_name_the_badges_face() {
        let fira = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../baud-font/tests/fonts/FiraCode-Regular.ttf"
        );
        let names: [&[u8]; 2] = [b"ProtoHebrewMonoC-Bold", b"Proto Hebrew Mono C Bold"];
        let f = fonts(&format!("font_fallback = false\nfonts = [\"{fira}\"]\n"));
        assert!(
            f.specs.iter().any(|s| s.1 == FaceRole::Badge),
            "the control: the badge's face is loaded"
        );
        for name in names {
            assert_eq!(f.resolve(name), None, "{}", String::from_utf8_lossy(name));
        }
        let d = fonts("");
        for name in names {
            let slot = d.resolve(name).expect("the default stack's bold");
            assert!(
                matches!(d.specs[slot.0].1, FaceRole::Style(_)),
                "{}",
                String::from_utf8_lossy(name)
            );
        }
    }

    fn change(t: &mut baud_core::Term<'_>, pt: &[u8]) -> FontChange {
        let mut b = b"\x1b]50;".to_vec();
        b.extend_from_slice(pt);
        b.push(0x07);
        t.write_at(baud_core::BytePos::ZERO, &b);
        t.feed(
            baud_core::BytePos::ZERO,
            baud_core::ReadLen::from_index(b.len()),
        );
        let c = t.font_requests()[0];
        t.clear_font_requests();
        c
    }

    fn with_term(f: impl FnOnce(&mut baud_core::Term<'_>)) {
        let ws = baud_core::WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        let mut s = baud_core::Owned::new(&ws);
        let mut t = baud_core::Term::new(s.storage(), &ws);
        f(&mut t);
    }

    /// **The default stack names its families, and each resolves to its
    /// own face**, in any ASCII case; a family baud has not loaded does
    /// not resolve, and neither does "JetBrains Mono", which no loaded
    /// face's nameID 1, 4, 6 or 16 carries. The four embedded text faces
    /// are the built font, whose `name` table says "Proto Hebrew Mono C"
    /// (`fonts/built/FONTLOG.txt`), the stock Italic and Bold Italic too.
    #[test]
    fn the_default_stack_names_its_families() {
        let f = fonts("");
        assert_eq!(f.family(), "Proto Hebrew Mono C");
        let jb = f
            .resolve(b"proto hebrew mono c")
            .expect("the embedded Regular");
        assert!(f.leads(jb));
        let nerd = f.resolve(b"Symbols Nerd Font Mono").expect("the icon face");
        let stix = f.resolve(b"STIX Two Math").expect("the math face");
        assert!(!f.leads(nerd) && !f.leads(stix) && nerd != stix);
        assert_eq!(f.resolve(b"No Such Family"), None, "not loaded");
        assert_eq!(
            f.resolve(b"JetBrains Mono"),
            None,
            "no loaded face carries it"
        );
    }

    /// **A program names a face by its PostScript name or its full name**,
    /// as a config's `fonts` does (`Catalogue::lookup_at`), in any case:
    /// each embedded face's nameID 6 and US English nameID 4 find it, read
    /// from the faces themselves. Symbols Nerd Font Mono's full name is its
    /// family name, so it finds the face as the family. A styled face
    /// named alone, the embedded Bold, is found by its own name, though
    /// its family name finds the Regular, and leads its stack as the
    /// regular face, with its family's Italic and Bold Italic and no Bold
    /// beside itself.
    #[test]
    fn a_program_names_a_face_by_its_postscript_or_full_name() {
        let mut f = fonts("");
        let names = |data: &[u8]| {
            let d = baud_font::Described::of(data, FaceIndex::FIRST).expect("a face");
            let text = |n: Option<baud_font::Family<'_>>| {
                let mut out = [0u8; 256];
                String::from_utf8(
                    n.and_then(|n| n.utf8(&mut out).map(<[u8]>::to_vec))
                        .unwrap(),
                )
                .unwrap()
            };
            (text(d.postscript_name()), text(d.full_name()))
        };
        use crate::embedded::{BOLD, REGULAR, STIX, SYMBOLS};
        for (e, family) in [
            (REGULAR, "Proto Hebrew Mono C"),
            (STIX, "STIX Two Math"),
            (SYMBOLS, "Symbols Nerd Font Mono"),
        ] {
            let want = f.resolve(family.as_bytes()).expect("loaded");
            let (ps, full) = names(e.bytes);
            assert_eq!(f.resolve(ps.as_bytes()), Some(want), "{ps}");
            assert_eq!(f.resolve(ps.to_lowercase().as_bytes()), Some(want), "{ps}");
            assert_eq!(
                f.resolve(full.to_uppercase().as_bytes()),
                Some(want),
                "{full}"
            );
        }
        let (ps, full) = names(BOLD.bytes);
        let bold = f.resolve(ps.as_bytes()).expect("the embedded Bold");
        assert_eq!(f.resolve(full.as_bytes()), Some(bold), "{full}");
        let regular = f.resolve(b"Proto Hebrew Mono C").unwrap();
        assert!(bold != regular && !f.leads(bold));
        let b = f.stack(bold).expect("its stack");
        assert_eq!(b.roles[0], FaceRole::Fallback, "it leads as the regular");
        let mut out = [0u8; 64];
        let first = b.faces[0].family().unwrap();
        assert_eq!(first.utf8(&mut out), Some(&b"Proto Hebrew Mono C"[..]));
        use baud_render::FaceStyle::{Bold, BoldItalic, Italic};
        let styles: Vec<_> = b
            .roles
            .iter()
            .filter_map(|r| match r {
                FaceRole::Style(s) => Some(*s),
                FaceRole::Fallback | FaceRole::Badge => None,
            })
            .collect();
        assert_eq!(styles, [Italic, BoldItalic], "no {Bold:?} beside itself");
        assert!(baud_render::Stack::with_roles(b.faces, &b.roles).is_some());
        assert_eq!(f.resolve(b"ProtoHebrewMonoCBold"), None, "no name of it");
    }

    /// **The query answers the name a program set, as it sent it**, while
    /// the face it named leads (Jev, Q_AS_SET), as xterm's `getFaceName`
    /// and mintty's `win_get_font` answer the stored string, so the answer
    /// sent back finds the same face; the family before any, a size step
    /// keeping the name, a refused name changing nothing, and each face's
    /// own name while it leads. A name for the face in force is told the
    /// core at once, as nothing restacks (`host::plan_font`).
    #[test]
    fn the_query_answers_the_name_a_program_set() {
        let mut f = fonts("");
        with_term(|t| {
            crate::host::tell_font(t, &f, 13.0, 13.0);
            let query = |t: &mut baud_core::Term<'_>| {
                t.clear_reply();
                let q = b"\x1b]50;?\x07";
                t.write_at(baud_core::BytePos::ZERO, q);
                t.feed(
                    baud_core::BytePos::ZERO,
                    baud_core::ReadLen::from_index(q.len()),
                );
                String::from_utf8(t.reply().to_vec()).unwrap()
            };
            assert_eq!(query(t), "\x1b]50;Proto Hebrew Mono C\x07");
            let c = change(t, b"protohebrewmonoc-regular");
            let plan = crate::host::plan_font(t, &mut f, &c, 13.0, 13.0, 13.0);
            assert!(plan.is_some_and(|p| p.face.is_some_and(|s| f.leads(s))));
            assert_eq!(query(t), "\x1b]50;protohebrewmonoc-regular\x07");
            let c = change(t, b"stixtwomath-regular");
            let stix = f.plan(&c, 13.0, 13.0).unwrap().face.unwrap();
            assert_eq!(f.answer(), "protohebrewmonoc-regular", "not yet led");
            f.set(stix);
            assert_eq!(f.answer(), "stixtwomath-regular");
            assert_eq!(f.resolve(f.answer().as_bytes()), Some(stix), "sent back");
            let c = change(t, b"#+1");
            assert!(f.plan(&c, 13.0, 13.0).is_ok());
            let c = change(t, b"Nowhere-Mono");
            assert!(f.plan(&c, 13.0, 13.0).is_err());
            assert_eq!(f.answer(), "stixtwomath-regular", "kept");
            let c = change(t, b"Symbols Nerd Font Mono");
            let nerd = f.plan(&c, 13.0, 13.0).unwrap().face.unwrap();
            f.set(nerd);
            assert_eq!(f.answer(), "Symbols Nerd Font Mono");
            f.set(stix);
            assert_eq!(f.answer(), "stixtwomath-regular", "its own name again");
            let regular = f.resolve(b"Proto Hebrew Mono C").unwrap();
            f.set(regular);
            assert_eq!(f.answer(), "protohebrewmonoc-regular");
        });
    }

    /// **With changes off, a PostScript name changes nothing and the query
    /// is answered**, as a family name: the core queues no change.
    #[test]
    fn with_changes_off_a_postscript_name_changes_nothing() {
        let f = fonts("");
        with_term(|t| {
            t.set_font_changes(baud_domain::FontChanges::Off);
            crate::host::tell_font(t, &f, 13.0, 13.0);
            let b = b"\x1b]50;ProtoHebrewMonoC-Bold\x07\x1b]50;?\x07";
            t.write_at(baud_core::BytePos::ZERO, b);
            t.feed(
                baud_core::BytePos::ZERO,
                baud_core::ReadLen::from_index(b.len()),
            );
            assert!(t.font_requests().is_empty());
            assert_eq!(t.reply(), b"\x1b]50;Proto Hebrew Mono C\x07");
        });
    }

    /// **A stack led by another face keeps the rest of the chain behind it,
    /// only its own family's styles, and the badge's face, last**; asked
    /// again, it is the same faces, made once.
    #[test]
    fn a_stack_is_led_by_its_face_and_made_once() {
        let mut f = fonts("");
        let stix = f.resolve(b"STIX Two Math").unwrap();
        let b = f.stack(stix).expect("the stack");
        assert_eq!(b.roles[0], FaceRole::Fallback);
        let mut out = [0u8; 64];
        let first = b.faces[0].family().unwrap();
        assert_eq!(first.utf8(&mut out), Some(&b"STIX Two Math"[..]));
        let (badge, text) = b.roles.split_last().unwrap();
        assert_eq!(
            *badge,
            FaceRole::Badge,
            "the badge's face, whatever the family"
        );
        assert!(
            text.iter().all(|r| *r == FaceRole::Fallback),
            "JetBrains Mono's styles stay"
        );
        let chain = f.specs.iter().filter(|s| s.1 == FaceRole::Fallback).count();
        assert_eq!(
            b.faces.len(),
            chain + 1,
            "the chain and the badge's face, and no styled face"
        );
        assert!(baud_render::Stack::with_roles(b.faces, &b.roles).is_some());
        let again = f.stack(stix).unwrap();
        assert!(core::ptr::eq(again.faces, b.faces), "made once");
        let jb = f.resolve(b"Proto Hebrew Mono C").unwrap();
        let back = f.stack(jb).unwrap();
        assert_eq!(
            back.faces.len(),
            chain + 4,
            "the chain, the three styles and the badge's face"
        );
        f.set(stix);
        assert_eq!(f.family(), "STIX Two Math");
    }

    /// **A program names a loaded family by any of its family records,
    /// by default caseless match** (`[[source: css-fonts-3]]` 5.1), on the
    /// default stack's system faces (macOS only): Hiragino Sans GB by its
    /// Simplified Chinese record, Apple SD Gothic Neo by its Korean one in
    /// another case.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_program_names_a_family_by_any_of_its_names() {
        let f = fonts("");
        let hiragino = f.resolve(b"Hiragino Sans GB").expect("loaded");
        assert_eq!(f.resolve("冬青黑体简体中文".as_bytes()), Some(hiragino));
        let gothic = f.resolve(b"Apple SD Gothic Neo").expect("loaded");
        assert_eq!(f.resolve("APPLE SD 산돌고딕 NEO".as_bytes()), Some(gothic));
    }

    /// **A stack made again draws a variable face at the axis values the
    /// config's did**: Fira Code named alone, from a directory holding only
    /// its variable face, is drawn at wght 400 (its default is 300), and so
    /// it still is in the stack STIX Two Math leads after an `OSC 50`.
    #[test]
    fn a_restack_keeps_a_variable_faces_values() {
        let dir = std::env::temp_dir().join(format!("baud-b141-vf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../baud-font/tests/fonts/FiraCode-VF.ttf"
            ),
            dir.join("FiraCode-VF.ttf"),
        )
        .unwrap();
        let mut cfg = Config::defaults();
        assert!(cfg.apply("fonts = [\"Fira Code\"]\n").is_empty());
        let cat = crate::discover::Catalogue::scan(std::slice::from_ref(&dir));
        assert!(cfg.resolve(&cat).is_empty());
        assert!(
            !cfg.faces[0].variations.is_empty(),
            "drawn at 400, not its default"
        );
        let (data, mut held) = crate::host::map_faces(&cfg);
        let faces = crate::host::parse_faces(&cfg, &data, &mut held);
        let first = *faces[0].coords();
        assert_ne!(first, baud_font::var::Coords::default());
        let mut f = Fonts::new(&cfg, data, faces, held);
        let stix = f.resolve(b"STIX Two Math").expect("the math face");
        let b = f.stack(stix).expect("its stack");
        let mut out = [0u8; 64];
        let at = b
            .faces
            .iter()
            .position(|x| {
                x.family()
                    .and_then(|n| n.utf8(&mut out).map(<[u8]>::to_vec))
                    .as_deref()
                    == Some(&b"Fira Code"[..])
            })
            .expect("Fira Code in the chain");
        assert_eq!(*b.faces[at].coords(), first);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// **A change is planned whole**: a family baud has not and a step past
    /// the bounds refuse all of it; entry 0 is the configured size.
    #[test]
    fn a_change_is_planned_whole() {
        let mut f = fonts("");
        with_term(|t| {
            let stix = f.resolve(b"STIX Two Math");
            let c = change(t, b"#+2 STIX Two Math");
            assert_eq!(
                f.plan(&c, 13.0, 13.0),
                Ok(Plan {
                    face: stix,
                    points: 15.0
                })
            );
            let c = change(t, b"#-1");
            assert_eq!(
                f.plan(&c, 13.0, 13.0),
                Ok(Plan {
                    face: None,
                    points: 12.0
                })
            );
            let c = change(t, b"#");
            assert_eq!(
                f.plan(&c, 20.0, 13.0),
                Ok(Plan {
                    face: None,
                    points: 13.0
                })
            );
            let c = change(t, b"#+2 No Such Family");
            assert_eq!(f.plan(&c, 13.0, 13.0), Err(Refused::Unknown));
            let c = change(t, b"#+60");
            assert_eq!(f.plan(&c, 13.0, 13.0), Err(Refused::Bounds));
            let c = change(t, b"#-7");
            assert_eq!(
                f.plan(&c, 13.0, 13.0),
                Ok(Plan {
                    face: None,
                    points: 6.0
                })
            );
            assert_eq!(f.plan(&c, 12.0, 13.0), Err(Refused::Bounds));
        });
        assert_eq!(size_of(13.0, 13.0), FontSize::Configured);
        assert_eq!(size_of(14.0, 13.0), FontSize::Stepped);
    }
}
