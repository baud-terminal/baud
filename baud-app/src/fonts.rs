//! **What a program's `OSC 50` does to the font**: the faces a name can
//! reach, the stack it gives, and the size a step lands on. Pure but for
//! the parse of a face already in memory, so its tests run on every
//! platform, and both drivers perform through it.
//!
//! **A name resolves to a face baud has loaded** (Jev's lean, OWN_FACES,
//! 0.79/0.85 after a re-ask): a family name (`name` table, nameID 16 or 1,
//! any language, `[[source: ms-opentype-name]]`) of a face in the fallback
//! chain -- the embedded Regular, Symbols Nerd Font Mono and STIX Two Math,
//! and the faces a config names -- by the default caseless match CSS Fonts
//! gives a family name (`[[source: css-fonts-3]]` 5.1), as the config's
//! own names are matched (`discover.rs`). The embedded
//! Regular's table names it "Proto Hebrew Mono C", the built font's name
//! while its family name is open (`fonts/built/FONTLOG.txt`), and a query
//! answers that. That face leads
//! the chain, the rest follow in the config's order, and the styled faces
//! of its family come with it; the embedded faces stay behind it as they
//! stay behind a configured stack. A family baud has not loaded leaves the
//! font as it was: a program's name is not looked up among the installed
//! fonts, only a config's is.
//!
//! **A size step is a point of the View menu's Bigger and Smaller**,
//! bounded as they are (Jev's lean, SIZE_STEPS, 0.22/0.18): `#+n` and `#-n`
//! step `n` points, `#0` returns to the configured size, and a step past
//! the bounds is refused whole, as xterm's `lookupRelativeFontSize` finds
//! no entry.

use crate::config::Config;
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
    /// The config's faces as the app starts with them, each answering to
    /// its own family names.
    named: &'static [Face<'static>],
    /// Each face's index in its file, its role, and the axis values a
    /// variable one is drawn at, so a stack made again draws it as the
    /// config's did.
    specs: Vec<(FaceIndex, FaceRole, Vec<Variation>)>,
    families: Vec<Option<String>>,
    built: Vec<Option<Built>>,
    current: Slot,
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
    pub fn new(cfg: &Config, data: Vec<&'static [u8]>, faces: &'static [Face<'static>]) -> Fonts {
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
        let mut built = vec![None; specs.len()];
        if let Some(first) = built.first_mut() {
            *first = Some(Built {
                faces,
                roles: specs.iter().map(|s| s.1).collect(),
            });
        }
        Fonts {
            data,
            named: faces,
            specs,
            families,
            built,
            current: Slot(0),
        }
    }

    /// The family the chain is led by, as the core tells a query; empty
    /// for a face whose `name` table names none.
    pub fn family(&self) -> &str {
        self.families
            .get(self.current.0)
            .and_then(Option::as_deref)
            .unwrap_or("")
    }

    /// The face of the chain that is `name`'s family, the first in the
    /// config's order.
    pub fn resolve(&self, name: &[u8]) -> Option<Slot> {
        self.specs
            .iter()
            .zip(self.named)
            .position(|((_, role, _), face)| *role == FaceRole::Fallback && face.answers_to(name))
            .map(Slot)
    }

    /// What `change` does at `points`, with `configured` the config's size;
    /// refused whole when its family is none baud has or its step leaves the
    /// bounds.
    pub fn plan(&self, change: &FontChange, points: f32, configured: f32) -> Result<Plan, Refused> {
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
        Ok(Plan { face, points })
    }

    /// Whether `slot` already leads the chain.
    pub fn leads(&self, slot: Slot) -> bool {
        self.current == slot
    }

    /// **The stack `slot` leads**: it first, the rest of the chain in the
    /// config's order, and the styled faces of its family. Parsed from
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
                FaceRole::Fallback if i != slot.0 => order.push(i),
                FaceRole::Style(_) if same => order.push(i),
                _ => {}
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
            faces: Box::leak(faces.into_boxed_slice()),
            roles: order.iter().map(|&i| self.specs[i].1).collect(),
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
        let data = crate::host::map_faces(&cfg);
        let faces = crate::host::parse_faces(&cfg, &data);
        Fonts::new(&cfg, data, faces)
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
    /// not resolve, and neither does a styled face's. The embedded Regular
    /// is the built font, whose `name` table says "Proto Hebrew Mono C"
    /// (`fonts/built/FONTLOG.txt`); the stock Italic, a styled face, says
    /// "JetBrains Mono".
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
        assert_eq!(f.resolve(b"JetBrains Mono"), None, "a styled face's");
    }

    /// **A stack led by another face keeps the rest of the chain behind it,
    /// and only its own family's styles**; asked again, it is the same
    /// faces, made once.
    #[test]
    fn a_stack_is_led_by_its_face_and_made_once() {
        let mut f = fonts("");
        let stix = f.resolve(b"STIX Two Math").unwrap();
        let b = f.stack(stix).expect("the stack");
        assert_eq!(b.roles[0], FaceRole::Fallback);
        let mut out = [0u8; 64];
        let first = b.faces[0].family().unwrap();
        assert_eq!(first.utf8(&mut out), Some(&b"STIX Two Math"[..]));
        assert!(
            b.roles.iter().all(|r| *r == FaceRole::Fallback),
            "JetBrains Mono's styles stay"
        );
        let chain = f.specs.iter().filter(|s| s.1 == FaceRole::Fallback).count();
        assert_eq!(b.faces.len(), chain, "the chain, and no styled face");
        assert!(baud_render::Stack::with_roles(b.faces, &b.roles).is_some());
        let again = f.stack(stix).unwrap();
        assert!(core::ptr::eq(again.faces, b.faces), "made once");
        let jb = f.resolve(b"Proto Hebrew Mono C").unwrap();
        let back = f.stack(jb).unwrap();
        assert_eq!(
            back.faces.len(),
            chain + 3,
            "the chain and the three styles"
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
        let data = crate::host::map_faces(&cfg);
        let faces = crate::host::parse_faces(&cfg, &data);
        let first = *faces[0].coords();
        assert_ne!(first, baud_font::var::Coords::default());
        let mut f = Fonts::new(&cfg, data, faces);
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
        let f = fonts("");
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
