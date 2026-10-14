//! What both presentation layers set up the same way: the grid the storage
//! is provisioned for, the cache probe whose answer picks the grid's pitch,
//! and the faces the config names -- read once, parsed once, and leaked,
//! because the renderer borrows them for the life of the process.

use crate::config::Config;
use baud_domain::{CacheGeometry, CacheLevel, ImageBudget, ImagePx, ImageRefusals, WinSize};
use baud_font::bytes::Index;
use baud_font::{Face, FaceIndex, Px, StrikeRefusal, StrikeState};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The largest grid the storage is provisioned for.
pub const MAX_GRID: WinSize = WinSize {
    ws_col: 400,
    ws_row: 150,
};

/// A cell height in device pixels as the core's [`ImagePx`], for
/// `Term::set_cell_size` -- the numbers the core needs to advance the
/// cursor past a Sixel. Saturates rather than truncates a cell taller than
/// 65535 px, which no font produces.
pub fn cell_px(cell_h: Px) -> ImagePx {
    ImagePx::of(u16::try_from(cell_h.value()).unwrap_or(u16::MAX))
}

/// The default image arena, when a config names no `[images]` section: 4 MiB
/// of decoded pixels, a 1024x1024 image or several small ones -- the gap-scan
/// packs them, and an overflow is refused and counted rather than truncated --
/// plus 3 MiB of body scratch for one raw Sixel before it decodes, a separate
/// allocation from the arena. yazi writes 1 to 4.6 bytes a pixel at its
/// 600x900 cap, and at the 256 KiB this was until 2026-10-14 its UI
/// screenshot (334-390 KiB at 600x375), photo (474-680 KiB at 600x800) and
/// text-PDF page (797-820 KiB at 600x776) previews were refused whole though
/// each fit the arena, and the pane went blank. **3 MiB is the interim the
/// owner chose on 2026-10-13**, above the most yazi writes at its cap
/// (2,705,560 bytes, computed from `[[source: yazi-sixel]]` 57-111; the
/// record, "The Sixel body default is 3 MiB until a Sixel streams"). **The
/// decided end state is a Sixel decoded as its body arrives**, with no raw
/// body held, so only the pixel arena limits it: the sections of
/// `[[source: dec-std-070]]` 070-9 read for it (§1.1, §2, §8.1.2, §12) name
/// no limit on a body's bytes, and §8.1.2 makes the raster extent "a summary
/// to avoid a complete scanning of the data" (PDF p. 920). It is not built; until it is, a body past the
/// scratch is still refused whole and counted as a body refusal. The single
/// source of the default; `Config::defaults` reads it, and
/// `[images] pixels_mb`/`body_kb` override it.
pub const IMAGE_BUDGET: ImageBudget = ImageBudget {
    pixels: 1 << 20, // 1,048,576 ColorWords, 4 MiB
    body: 3 << 20,   // 3 MiB
};

/// One startup line naming the resolved image budget, at `info` -- so it shows
/// only under `BAUD_LOG=info` or below. A refusal names its own limit and
/// sizes as it happens ([`describe_refusals`]); this line is the budget a
/// reader checks a changed `[images]` against. A `ColorWord` is four bytes,
/// so a MiB is `1 << 18` of them -- `>> 18` gives MiB with no multiply -- and
/// a KiB is `1 << 10` bytes.
pub fn describe_images(b: &ImageBudget) -> String {
    format!(
        "images: {} MiB pixels, {} KiB body",
        b.pixels >> 18,
        b.body >> 10,
    )
}

/// One `warn` line for the images one pump refused against a limit, naming
/// the limit, the sizes that hit it and the `[images]` key that raises it --
/// or `None` when none did. `before` and `after` are the store's
/// [`ImageRefusals`] either side of the pump's feeds, compared once a pump and
/// never per byte, so a flood of refusals is one line a read rather than one
/// an image.
///
/// It exists because the refusal was invisible: yazi's previews went blank
/// against the body default, 256 KiB on 2026-10-09, counted in a total that
/// also counted the arena and the slots and that nothing in the app read. An
/// image with no pixels, or one wider or taller than the 65535 pixels an
/// image coordinate holds, is counted and not said -- no limit refused it and
/// none raised would draw it. A `ColorWord` is four bytes, so a pixel count
/// shifted right 18 is MiB, as in [`describe_images`].
pub fn describe_refusals(
    before: &ImageRefusals,
    after: &ImageRefusals,
    budget: &ImageBudget,
) -> Option<String> {
    let mut said = Vec::new();
    if after.body > before.body {
        let n = after.body - before.body;
        let len = after.body_len.index();
        said.push(format!(
            "{} refused for its body, the last {len} bytes ({} KiB) against a \
             {}-byte ({} KiB) body scratch; [images] body_kb raises it",
            if n == 1 {
                "a Sixel".to_string()
            } else {
                format!("{n} Sixels")
            },
            len >> 10,
            budget.body,
            budget.body >> 10,
        ));
    }
    if after.arena > before.arena {
        let n = after.arena - before.arena;
        let (w, h) = (after.arena_width.index(), after.arena_height.index());
        let area = w * h;
        let why = if area > budget.pixels {
            "larger than the whole arena"
        } else {
            "no free gap held it, the live images hold the rest"
        };
        said.push(format!(
            "{} refused for its pixels, the last {w}x{h} = {area} pixels ({} KiB \
             decoded) against a {} MiB arena of {} pixels -- {why}; [images] \
             pixels_mb raises it",
            if n == 1 {
                "an image".to_string()
            } else {
                format!("{n} images")
            },
            area >> 8,
            budget.pixels >> 18,
            budget.pixels,
        ));
    }
    if after.slot > before.slot {
        let n = after.slot - before.slot;
        said.push(format!(
            "{} refused, every image slot holding a live image",
            if n == 1 {
                "an image".to_string()
            } else {
                format!("{n} images")
            },
        ));
    }
    (!said.is_empty()).then(|| format!("images: {}", said.join("; ")))
}

/// The cache geometry of the machine this runs on, from the per-arch probe:
/// `UNKNOWN` where there is no probe, which still resolves to a pitch.
pub fn host_geometry() -> CacheGeometry {
    #[cfg(target_arch = "aarch64")]
    {
        <baud_isa_aarch64::Host as baud_isa::CacheProbe>::detect()
    }
    #[cfg(target_arch = "x86_64")]
    {
        <baud_isa_x86_64::Host as baud_isa::CacheProbe>::detect()
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        CacheGeometry::UNKNOWN
    }
}

/// One line for the log: what the probe found, level by level, and what
/// was assumed where it found nothing.
pub fn describe(geom: &CacheGeometry) -> String {
    let level = |l: Option<CacheLevel>| match l {
        Some(l) => match l.way() {
            Some(w) => format!(
                "{} KiB {}-way, {} KiB a way",
                l.size.bytes() / 1024,
                l.ways.count(),
                w.bytes() / 1024
            ),
            None => format!("{} KiB", l.size.bytes() / 1024),
        },
        None => "unknown".to_string(),
    };
    let line = match geom.line_reported() {
        Some(l) => format!("{} B", l.bytes()),
        None => format!("unknown, {} B assumed", geom.line().bytes()),
    };
    format!(
        "line {line}; L1d {}; L2 {}; L3 {}",
        level(geom.l1d()),
        level(geom.l2()),
        level(geom.l3())
    )
}

/// The renderer's stack over `faces`, which `load_faces` read from `cfg`
/// in its order: each face with the role its spec gives it, so the styled
/// faces are drawn from and never fallen back through.
pub fn stack<'f, 'a>(cfg: &Config, faces: &'f [Face<'a>]) -> baud_render::Stack<'f, 'a> {
    let roles: Vec<baud_render::FaceRole> = cfg.faces.iter().map(|f| f.role).collect();
    baud_render::Stack::with_roles(faces, &roles)
        .expect("the config's faces: a fallback first, each style once")
}

/// Every face the config names, mapped and parsed. Each face's bytes are
/// a read-only mapping for the life of the process, because the faces
/// borrow them and a read of the whole file was the launch's largest
/// controllable cost; a face that cannot be mapped or parsed is fatal
/// with its path and the reason (a `CFF2` table that does not read, say),
/// because a terminal with no font draws nothing.
pub fn load_faces(cfg: &Config) -> &'static [Face<'static>] {
    let data: Vec<&'static [u8]> = cfg
        .faces
        .iter()
        .map(|f| match f.from {
            crate::config::FaceFrom::Embedded(bytes) => bytes,
            crate::config::FaceFrom::File => {
                let bytes = crate::pty::map_readonly(&f.path)
                    .unwrap_or_else(|e| panic!("font {}: {e}", f.path));
                startup_mark(&format!("map {}", f.path.rsplit('/').next().unwrap_or("")));
                bytes
            }
        })
        .collect();
    Box::leak(
        data.iter()
            .zip(&cfg.faces)
            .map(|(d, f)| {
                let face = Face::parse_index(d, FaceIndex::of(f.index))
                    .unwrap_or_else(|e| panic!("font {}: {e}", f.path));
                for line in bitmap_refusals(&f.path, &face) {
                    crate::warn!("{line}");
                }
                startup_mark(&format!(
                    "parse {}",
                    f.path.rsplit('/').next().unwrap_or("")
                ));
                face
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
}

/// One line for a face whose `CBLC` sizes are not all read: how many, the
/// first and why. A font that loses sizes to the specification's sorting rule
/// says so once, at launch, rather than drawing from fewer sizes in silence;
/// a face whose sizes all read, or that has none, says nothing.
pub fn bitmap_refusals(path: &str, face: &Face<'_>) -> Vec<String> {
    let strikes = face.strikes();
    let mut first = None;
    let mut refused = 0u32;
    let mut i = Index::ZERO;
    while strikes.holds(i) {
        if let Some(StrikeState::Refused(why)) = face.strike(i) {
            refused += 1;
            first.get_or_insert((i, why));
        }
        i = i.next();
    }
    let Some((at, why)) = first else {
        return Vec::new();
    };
    let ppem = face.strike_ppem(at).map_or(0, |p| p.value());
    let reason = match why {
        StrikeRefusal::Unsorted { at: j } => format!(
            "lists its index subtable records out of order: record {} starts before record {}",
            j.value(),
            j.prev().map_or(0, |p| p.value())
        ),
        StrikeRefusal::PastTable => {
            "points its index subtable records past the end of its table".to_string()
        }
    };
    let n = strikes.value();
    let head = if n == 1 {
        "its one CBLC size is not read".to_string()
    } else {
        let verb = if refused == 1 { "is" } else { "are" };
        format!("{refused} of its {n} CBLC sizes {verb} not read")
    };
    let tail = if n == 1 {
        ""
    } else if refused == n {
        "; no size draws"
    } else {
        "; the other sizes still draw"
    };
    vec![format!(
        "font {path}: {head}: size {} ({ppem} ppem) {reason}{tail}",
        at.value()
    )]
}

/// The launch clock: `startup_begin` is the first thing `main` does, and
/// `startup_mark` prints how long after it a stage was reached -- when
/// `BAUD_STARTUP` is set, so a normal launch prints nothing. What it cannot
/// see is the time before `main`, the loader's; `scripts/startup-bench.py`
/// measures that from outside and adds the two.
static START: OnceLock<Instant> = OnceLock::new();
static WANTED: OnceLock<Wanted> = OnceLock::new();

#[derive(Clone, Copy, PartialEq, Eq)]
enum Wanted {
    Print,
    Quiet,
}

/// Tell the core what the theme draws the default foreground and background
/// as, so `OSC 10 ; ?` and `OSC 11 ; ?` answer on a fresh terminal -- the
/// query every theme-aware program sends to learn whether it is on a light
/// or a dark background. One function for both drivers, called wherever a
/// theme is set, so a renderer and its core cannot disagree about the
/// defaults: the renderer draws the theme's colour and the core reports it.
pub fn tell_defaults(
    term: &mut baud_core::Term<'_>,
    theme: &baud_render::Theme,
    overrides: baud_core::Overrides,
) {
    let rgb = |c: baud_render::Argb| (c.r().get(), c.g().get(), c.b().get());
    // One call: the defaults and the table, so an `OSC 4 ; c ; ?` for an entry
    // nothing overrode is answered with the colour drawn, and a `?2031`
    // subscriber gets at most one `CSI ? 997 ; 1|2 n` for the whole change.
    // Every caller that changes the theme comes through here, so the core's
    // copy cannot fall out of step. `Drop` is an OS light/dark flip under
    // `auto`, and only that (`palette_switch` in `app.rs`).
    let palette = theme.palette.map(rgb);
    term.set_theme(
        &baud_core::ThemeColors {
            fg: rgb(theme.fg),
            bg: rgb(theme.bg),
            palette: &palette,
        },
        overrides,
    );
}

pub fn startup_begin() {
    let _ = START.set(Instant::now());
    let _ = WANTED.set(match std::env::var_os("BAUD_STARTUP") {
        Some(_) => Wanted::Print,
        None => Wanted::Quiet,
    });
}

pub fn startup_mark(stage: &str) {
    if WANTED.get() != Some(&Wanted::Print) {
        return;
    }
    let since = START.get().map_or(Duration::ZERO, Instant::elapsed);
    eprintln!("startup: {stage:<18} {:9.3} ms", since.as_secs_f64() * 1e3);
}

/// A stage that happens many times and is worth printing once: the first
/// frame committed, the first completion, the first present.
pub fn startup_first(stage: &str, seen: &AtomicBool) {
    if !seen.swap(true, Ordering::Relaxed) {
        startup_mark(stage);
    }
}

/// A free-form line under the same flag, for a count the stages cannot
/// carry -- the first frame's map misses, a predicted inset that missed.
#[cfg(target_os = "macos")]
pub fn startup_note(text: &str) {
    if WANTED.get() != Some(&Wanted::Print) {
        return;
    }
    eprintln!("startup: {text}");
}

#[cfg(target_os = "macos")]
pub static FIRST_COMMIT: AtomicBool = AtomicBool::new(false);
/// The first bytes the child wrote, and the first frame committed after
/// them: what a user waits for, where `first commit` is an empty screen.
#[cfg(target_os = "macos")]
pub static FIRST_BYTES: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
pub static FIRST_CONTENT: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
pub static FIRST_DONE: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
pub static FIRST_TICK: AtomicBool = AtomicBool::new(false);
/// The first display-link tick returned: what the tick's own work cost
/// the main thread, which is where the Dock icon used to be set.
#[cfg(target_os = "macos")]
pub static FIRST_TICK_DONE: AtomicBool = AtomicBool::new(false);
pub static FIRST_PRESENT: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
mod tests {
    use baud_core::{Chan, Owned, Term};
    use baud_domain::{BytePos, ImagePx, ReadLen, WinSize};
    use baud_render::Theme;

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    /// The default stack loads with the embedded faces read from the binary
    /// rather than looked for as files -- their names are no paths, so a
    /// loader that mapped them would panic here -- and draws bold, italic
    /// and bold-italic text from the embedded styles, an icon of any style
    /// from the icon face, and regular text from the regular face.
    #[test]
    fn the_default_stack_loads_the_embedded_faces() {
        use baud_render::FaceStyle;
        let cfg = crate::config::Config::defaults();
        let faces = super::load_faces(&cfg);
        assert_eq!(faces.len(), cfg.faces.len());
        let stack = super::stack(&cfg, faces);
        let a = baud_domain::Codepoint::new(u32::from('a')).unwrap();
        let folder = baud_domain::Codepoint::new(0xF07B).unwrap();
        let n = faces.len();
        for (style, face) in [
            (FaceStyle::Regular, 0),
            (FaceStyle::Bold, n - 3),
            (FaceStyle::Italic, n - 2),
            (FaceStyle::BoldItalic, n - 1),
        ] {
            assert_eq!(stack.select_styled(a, style).index(), face, "{style:?}");
            assert_eq!(stack.select_styled(folder, style).index(), 1, "{style:?}");
        }
    }

    /// **The default stack draws Hebrew from the embedded font** -- the
    /// owner's boxes (2026-10-08), on the path a launch takes: `Config::defaults`,
    /// `load_faces`, the renderer's stack. Alef, shin and sheva -- two letters
    /// and a point -- map in the embedded Regular, which regular text draws
    /// from, and bold text draws them from the embedded Bold. Linux loads its
    /// faces through the same two calls.
    #[test]
    fn the_default_stack_draws_hebrew_from_the_embedded_font() {
        use baud_render::FaceStyle;
        let cfg = crate::config::Config::defaults();
        let faces = super::load_faces(&cfg);
        let stack = super::stack(&cfg, faces);
        let n = faces.len();
        for cp in [0x05D0, 0x05E9, 0x05B0] {
            let c = baud_domain::Codepoint::new(cp).unwrap();
            assert!(
                faces[0].glyph(c).is_some(),
                "U+{cp:04X}: the embedded Regular, faces[0], does not map it, so it draws as \
                 the primary's .notdef box"
            );
            assert_eq!(stack.select_styled(c, FaceStyle::Regular).index(), 0);
            assert!(faces[n - 3].glyph(c).is_some(), "U+{cp:04X}: the Bold face");
            assert_eq!(
                stack.select_styled(c, FaceStyle::Bold).index(),
                n - 3,
                "U+{cp:04X}: bold Hebrew draws from the embedded Bold"
            );
            // Italic and Bold Italic are stock JetBrains Mono for now
            // (STOCK_ITALIC, interim): index 0 here is the embedded Regular
            // drawing it, which the first assert proves maps it -- not the
            // primary's .notdef, which is also index 0.
            for (style, ix) in [(FaceStyle::Italic, n - 2), (FaceStyle::BoldItalic, n - 1)] {
                assert!(
                    faces[ix].glyph(c).is_none(),
                    "U+{cp:04X}, {style:?}: the premise of the interim -- its face is stock \
                     JetBrains Mono, with no Hebrew; a face that maps it is an italic Hebrew \
                     build, and this test moves with it"
                );
                assert_eq!(
                    stack.select_styled(c, style).index(),
                    0,
                    "U+{cp:04X}, {style:?}: the italic slots stay stock until the owner answers \
                     the lean (1c) and has seen that the built italics are a plain shear, so \
                     the chain answers: the embedded Regular, upright and regular weight"
                );
            }
        }
    }

    /// **A configured stack draws Hebrew from the embedded Regular behind
    /// it**, the owner's order: the configured faces, then baud's embedded
    /// font, then the system, which is not built for a configured stack yet
    /// (note AC decision 9). FiraCode, named alone in `fonts`, maps no
    /// Hebrew; the embedded Regular follows it, ahead of the icon face and
    /// STIX, and draws alef, while Latin stays with FiraCode. With
    /// `font_fallback = false` the stack is FiraCode alone and alef is its
    /// `.notdef`.
    #[test]
    fn a_configured_stack_draws_hebrew_from_the_embedded_regular() {
        let font = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../baud-font/tests/fonts/FiraCode-Regular.ttf"
        );
        let alef = baud_domain::Codepoint::new(0x05D0).unwrap();
        let a = baud_domain::Codepoint::new(u32::from('a')).unwrap();
        for (extra, faces, alef_in) in [("", 4, 1), ("font_fallback = false\n", 1, 0)] {
            let mut cfg = crate::config::Config::defaults();
            let said = cfg.apply(&format!("{extra}fonts = [\"{font}\"]\n"));
            assert!(said.is_empty(), "{said:?}");
            let loaded = super::load_faces(&cfg);
            assert_eq!(loaded.len(), faces, "{extra:?}");
            assert!(
                loaded[0].glyph(alef).is_none(),
                "the control: FiraCode has no Hebrew"
            );
            let stack = super::stack(&cfg, loaded);
            assert_eq!(stack.select(a).index(), 0, "{extra:?}: Latin stays");
            assert_eq!(stack.select(alef).index(), alef_in, "{extra:?}");
            if alef_in != 0 {
                assert!(
                    matches!(cfg.faces[alef_in].from,
                        crate::config::FaceFrom::Embedded(b) if b == crate::embedded::REGULAR.bytes),
                    "alef is drawn by the embedded Regular, not by {}",
                    cfg.faces[alef_in].path
                );
            }
        }
    }

    /// **An icon the configured font lacks draws from the embedded one.**
    /// The committed plain JetBrains Mono carries the Powerline arrows but
    /// not the Nerd Fonts icons; named alone in `fonts`, the renderer's stack
    /// still finds Font Awesome's folder, U+F07B, in the embedded icon face
    /// behind it, and text stays with the named face. Four faces, the icon
    /// face third: the embedded Regular follows a configured stack ahead of
    /// it, and has no icons either. With `font_fallback = false` the stack is
    /// the named face alone and the folder falls to its `.notdef`.
    #[test]
    fn an_icon_the_configured_font_lacks_draws_from_the_embedded_one() {
        let font = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf"
        );
        let folder = baud_domain::Codepoint::new(0xF07B).unwrap();
        let a = baud_domain::Codepoint::new(u32::from('a')).unwrap();
        for (extra, faces, folder_in) in [("", 4, 2), ("font_fallback = false\n", 1, 0)] {
            let mut cfg = crate::config::Config::defaults();
            let said = cfg.apply(&format!("{extra}fonts = [\"{font}\"]\n"));
            assert!(said.is_empty(), "{said:?}");
            let loaded = super::load_faces(&cfg);
            assert_eq!(loaded.len(), faces, "{extra:?}");
            assert!(
                loaded[0].glyph(folder).is_none(),
                "the control: no icon in it"
            );
            let stack = super::stack(&cfg, loaded);
            assert_eq!(stack.select(a).index(), 0, "{extra:?}");
            assert_eq!(stack.select(folder).index(), folder_in, "{extra:?}");
        }
    }

    /// **A refused `CBLC` size is said once, at launch.** The Noto subset
    /// with its first two index subtable records swapped lists them out of
    /// order, and with 100 records claimed they run past its table: either
    /// way its one size is refused and the launch log says which and why. The
    /// cases fixture refuses two of six sizes and names the first. The clean
    /// subset, the `sbix` fixture and JetBrains Mono say nothing.
    #[test]
    fn a_refused_size_is_reported_and_a_clean_face_is_not() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../baud-font/tests/fonts/");
        let noto = std::fs::read(format!("{dir}NotoColorEmoji-subset.ttf")).unwrap();
        let n = usize::from(u16::from_be_bytes([noto[4], noto[5]]));
        let r = (0..n)
            .map(|i| 12 + 16 * i)
            .find(|&r| &noto[r..r + 4] == b"CBLC")
            .unwrap();
        let at =
            usize::try_from(u32::from_be_bytes(noto[r + 8..r + 12].try_into().unwrap())).unwrap();
        let mut swapped = noto.clone();
        swapped[at + 56..at + 64].copy_from_slice(&noto[at + 64..at + 72]);
        swapped[at + 64..at + 72].copy_from_slice(&noto[at + 56..at + 64]);
        let face = baud_font::Face::parse(&swapped).unwrap();
        assert_eq!(
            super::bitmap_refusals("noto.ttf", &face),
            vec![
                "font noto.ttf: its one CBLC size is not read: size 0 (109 ppem) lists its index subtable \
                 records out of order: record 1 starts before record 0"
                    .to_string()
            ]
        );
        // The records claimed past the table: 100 of them, at CBLC+16.
        let mut past = noto.clone();
        past[at + 16..at + 20].copy_from_slice(&100u32.to_be_bytes());
        let face = baud_font::Face::parse(&past).unwrap();
        assert_eq!(
            super::bitmap_refusals("noto.ttf", &face),
            vec![
                "font noto.ttf: its one CBLC size is not read: size 0 (109 ppem) points its index \
                 subtable records past the end of its table"
                    .to_string()
            ]
        );
        // Several sizes, two refused: the first is named and the rest still draw.
        let cases = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../baud-font/tests/bitmap/cblc-cases.ttf"
        ))
        .unwrap();
        let face = baud_font::Face::parse(&cases).unwrap();
        assert_eq!(
            super::bitmap_refusals("cases.ttf", &face),
            vec![
                "font cases.ttf: 2 of its 6 CBLC sizes are not read: size 1 (30 ppem) lists its index \
                 subtable records out of order: record 1 starts before record 0; the other sizes still draw"
                    .to_string()
            ]
        );
        for clean in [
            "NotoColorEmoji-subset.ttf",
            "sbix-strikes.ttf",
            "JetBrainsMono-Regular.ttf",
        ] {
            let data = std::fs::read(format!("{dir}{clean}")).unwrap();
            let face = baud_font::Face::parse(&data).unwrap();
            assert!(super::bitmap_refusals(clean, &face).is_empty(), "{clean}");
        }
    }

    /// **Claude Code's arrow and bullet draw from STIX behind a configured
    /// font** (the owner's call of 2026-10-03). The committed plain JetBrains
    /// Mono has neither U+23F5 nor U+23FA, and neither has the embedded
    /// Regular behind it nor the icon face, so the renderer's stack selects
    /// STIX, fourth; with `font_fallback = false` both fall to the named
    /// face's `.notdef`.
    #[test]
    fn claude_codes_symbols_draw_from_stix_behind_a_configured_font() {
        let font = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf"
        );
        let arrow = baud_domain::Codepoint::new(0x23F5).unwrap();
        let bullet = baud_domain::Codepoint::new(0x23FA).unwrap();
        for (extra, faces, drawn_by) in [("", 4, 3), ("font_fallback = false\n", 1, 0)] {
            let mut cfg = crate::config::Config::defaults();
            let said = cfg.apply(&format!("{extra}fonts = [\"{font}\"]\n"));
            assert!(said.is_empty(), "{said:?}");
            let loaded = super::load_faces(&cfg);
            assert_eq!(loaded.len(), faces, "{extra:?}");
            for c in [arrow, bullet] {
                assert!(
                    loaded[0].glyph(c).is_none(),
                    "the control: not in the named face"
                );
            }
            let stack = super::stack(&cfg, loaded);
            assert_eq!(stack.select(arrow).index(), drawn_by, "{extra:?}");
            assert_eq!(stack.select(bullet).index(), drawn_by, "{extra:?}");
        }
    }

    /// **The core's copy of the palette is the renderer's.** After
    /// `tell_defaults`, an `OSC 4 ; c ; ?` for every entry answers exactly the
    /// colour the theme draws -- the invariant `set_default_colors` already
    /// holds for the foreground and background, over all 256 entries and both
    /// themes, so a table pushed from the wrong place or not at all fails here.
    #[test]
    fn every_osc_4_answer_is_the_colour_the_theme_draws() {
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        for theme in [Theme::dark(), Theme::light()] {
            let mut s = Owned::new(&ws);
            let mut t = Term::new(s.storage(), &ws);
            super::tell_defaults(&mut t, &theme, baud_core::Overrides::Keep);
            for i in 0..=255u8 {
                feed(&mut t, format!("\x1b]4;{i};?\x07").as_bytes());
                let c = theme.palette[usize::from(i)];
                let (r, g, b) = (c.r().get(), c.g().get(), c.b().get());
                let want =
                    format!("\x1b]4;{i};rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}\x07");
                assert_eq!(t.reply(), want.as_bytes(), "entry {i}");
                t.clear_reply();
            }
            assert_eq!(
                t.palette(Chan::from_u8(1)),
                None,
                "the table became an override"
            );
        }
    }

    /// The light theme's entry 2 is Modus Operandi's green, and the core
    /// answers it: the table the renderer draws is the table the query reads.
    #[test]
    fn osc_4_answers_the_light_themes_modus_green() {
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        super::tell_defaults(&mut t, &Theme::light(), baud_core::Overrides::Keep);
        feed(&mut t, b"\x1b]4;2;?\x07");
        assert_eq!(t.reply(), b"\x1b]4;2;rgb:0000/6868/0000\x07");
    }

    /// **An OS flip under `auto` drops the application's colours and reports
    /// once** -- note Y's decision 5, through the call the app makes. A
    /// program subscribed with `?2031` has set palette entry 1 and the
    /// background; the flip to the light theme clears both and sends one
    /// `CSI ? 997 ; 2 n`. The same flip with `Keep` clears nothing, the
    /// control that the drop is the flag's.
    #[test]
    fn a_flip_drops_the_applications_colours_and_reports_once() {
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        for (overrides, dropped) in [
            (baud_core::Overrides::Drop, true),
            (baud_core::Overrides::Keep, false),
        ] {
            let mut s = Owned::new(&ws);
            let mut t = Term::new(s.storage(), &ws);
            super::tell_defaults(&mut t, &Theme::dark(), baud_core::Overrides::Keep);
            feed(
                &mut t,
                b"\x1b[?2031h\x1b]4;1;rgb:12/34/56\x07\x1b]11;rgb:20/20/20\x07",
            );
            assert_eq!(t.reply(), b"", "an application's own colours send nothing");
            super::tell_defaults(&mut t, &Theme::light(), overrides);
            assert_eq!(
                t.palette(Chan::from_u8(1)).is_none(),
                dropped,
                "{overrides:?}"
            );
            let report: &[u8] = if dropped { b"\x1b[?997;2n" } else { b"" };
            assert_eq!(t.reply(), report, "{overrides:?}");
        }
    }

    /// **A refusal is said by the limit it hit, with the sizes and the key
    /// that raises it** -- and only when a counter moved across the pump, so
    /// a snapshot whose last sizes are set says nothing again on the next
    /// pump. An image with no pixels is no limit and is not said. The body
    /// is one byte past the 3 MiB default; the arena cases are an image over
    /// the whole 4 MiB arena and one inside it that found no gap, which are
    /// different fixes.
    #[test]
    fn a_refusal_is_said_by_the_limit_it_hit() {
        use baud_domain::ImageRefusals;
        let say = |a: &ImageRefusals, b: &ImageRefusals| {
            super::describe_refusals(a, b, &super::IMAGE_BUDGET)
        };
        let none = ImageRefusals::NONE;
        assert_eq!(say(&none, &none), None, "nothing moved");
        let empty = ImageRefusals { empty: 1, ..none };
        assert_eq!(say(&none, &empty), None, "no pixels is no limit");

        let body = ImageRefusals {
            body: 1,
            body_len: ReadLen::from_index(3_145_729),
            ..none
        };
        let line = say(&none, &body).expect("a body refusal is said");
        for want in [
            "a Sixel",
            "3145729 bytes",
            "(3072 KiB)",
            "3145728-byte",
            "body_kb",
        ] {
            assert!(line.contains(want), "{want:?} in {line:?}");
        }
        assert!(!line.contains("pixels_mb"), "not the arena: {line:?}");
        assert_eq!(say(&body, &body), None, "said once, not every pump after");
        let more = ImageRefusals { body: 3, ..body };
        assert!(say(&body, &more).unwrap().contains("2 Sixels"), "a count");

        let over = ImageRefusals {
            arena: 1,
            arena_width: ImagePx::of(2048),
            arena_height: ImagePx::of(1024),
            ..none
        };
        let line = say(&none, &over).expect("an arena refusal is said");
        for want in [
            "2048x1024",
            "2097152 pixels",
            "4 MiB arena",
            "larger than the whole",
            "pixels_mb",
        ] {
            assert!(line.contains(want), "{want:?} in {line:?}");
        }
        assert!(!line.contains("body_kb"), "not the body: {line:?}");
        let gap = ImageRefusals {
            arena_width: ImagePx::of(600),
            arena_height: ImagePx::of(800),
            ..over
        };
        let line = say(&none, &gap).unwrap();
        assert!(
            line.contains("600x800 = 480000 pixels (1875 KiB"),
            "{line:?}"
        );
        assert!(line.contains("no free gap"), "inside the arena: {line:?}");

        let slot = ImageRefusals { slot: 1, ..none };
        assert!(say(&none, &slot).unwrap().contains("every image slot"));

        let both = ImageRefusals { body: 1, ..gap };
        let line = say(&none, &both).unwrap();
        assert!(
            line.contains("body_kb") && line.contains("pixels_mb") && !line.contains('\n'),
            "two limits in one pump are one line: {line:?}"
        );
    }

    /// Feed `bytes` the way the pump does, a `READ_CHUNK` read at a time:
    /// a body past the 1 MiB read arena cannot be written in one.
    fn pump(t: &mut Term<'_>, bytes: &[u8]) {
        for read in bytes.chunks(baud_core::READ_CHUNK) {
            feed(t, read);
        }
    }

    /// A terminal provisioned the way the app provisions one, at the default
    /// image budget, 200x60. The caller gives it the walk's cell, 17x34, the
    /// size the capture's reply named. A cell size of zero, a headless
    /// terminal's default, frees no image text is written over
    /// (`Term::set_cell_size`), so without it the walk's previews are never
    /// freed and nine of them find the arena full.
    fn at_default() -> (Owned, WinSize) {
        let ws = WinSize {
            ws_row: 60,
            ws_col: 200,
        };
        (Owned::new(&ws).with_images(super::IMAGE_BUDGET), ws)
    }

    /// A Sixel whose body is `len` bytes the decoder reads, every one of them
    /// `~` or `$`: `$` returns to the band's left edge, so the image is one
    /// column by one band, 1x6 pixels at `P1` 9's 1:1, at any length --
    /// inside the arena and a coordinate, so the body is the only limit it
    /// can meet. (A body of `~` alone is `len` columns wide, past 65535 at
    /// these lengths.)
    fn narrow_sixel(len: ReadLen) -> Vec<u8> {
        let mut s = b"\x1bP9q".to_vec();
        s.extend((0..len.index()).map(|i| if i % 2 == 0 { b'~' } else { b'$' }));
        s.extend_from_slice(b"\x1b\\");
        s
    }

    /// **A body of exactly the default scratch stores**: one column by one
    /// band, refused for nothing. The scratch is 3 MiB since 2026-10-14, the
    /// owner's interim of 2026-10-13 until a Sixel is decoded as it streams.
    #[test]
    fn a_body_of_exactly_the_default_scratch_stores() {
        let body = super::IMAGE_BUDGET.body;
        assert_eq!(body, 3_145_728, "the default is 3 MiB");
        let (mut s, ws) = at_default();
        let mut t = Term::new(s.storage(), &ws);
        pump(&mut t, &narrow_sixel(ReadLen::from_index(body)));
        let imgs = t.images();
        assert_eq!(imgs.refusals, baud_domain::ImageRefusals::NONE);
        let placed: Vec<_> = imgs.placements().map(|(_, p)| p.image).collect();
        assert_eq!(placed.len(), 1, "a body of exactly the scratch stores");
        let v = imgs.image(placed[0]).expect("placed means live");
        assert_eq!((v.width.raw(), v.height.raw()), (1, 6), "one column");
    }

    /// **A body one byte past the default scratch is refused against the
    /// body and said with its length**, through a real terminal fed the way
    /// the pump feeds: counted there with its true length and against no
    /// other limit, nothing placed, and the line names the limit it hit.
    /// Until 2026-10-14 this fed a 341,669-byte body of `~` against the
    /// 256 KiB default; that body is an image 341,669 pixels wide, past the
    /// 65535 a coordinate holds, so at 3 MiB it would be refused as too
    /// large and the body would not be what bites.
    #[test]
    fn a_body_past_the_default_scratch_is_said_with_its_length() {
        let body = super::IMAGE_BUDGET.body;
        let (mut s, ws) = at_default();
        let mut t = Term::new(s.storage(), &ws);
        let before = t.images().refusals;
        pump(&mut t, &narrow_sixel(ReadLen::from_index(body + 1)));
        let after = t.images().refusals;
        assert_eq!(after.body, 1, "refused against the body");
        assert_eq!(after.body_len, ReadLen::from_index(body + 1));
        assert_eq!(after.total(), 1, "and against nothing else: {after:?}");
        assert_eq!(t.images().placements().count(), 0, "nothing placed");
        let line = super::describe_refusals(&before, &after, &super::IMAGE_BUDGET).unwrap();
        assert_eq!(
            line,
            "images: a Sixel refused for its body, the last 3145729 bytes (3072 KiB) \
             against a 3145728-byte (3072 KiB) body scratch; [images] body_kb raises it"
        );
    }

    /// The committed yazi recording (`tests/recordings/README.md`): yazi
    /// 26.5.6 walking seven files and back, thirteen previews.
    const WALK: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/recordings/sixel/yazi-walk.raw"
    ));

    /// Each Sixel in `data`, `ESC P` through `ESC \`, with the length of the
    /// body its decoder reads: what follows the `q`, less the C0 controls,
    /// `SPACE` and `DEL` the handler drops. A Sixel is a DCS whose prefix is
    /// digits and `;` before the `q`; the walk's one other DCS is not one.
    fn sixels(data: &[u8]) -> Vec<(&[u8], usize)> {
        let find = |from: usize, needle: &[u8]| {
            data[from..]
                .windows(needle.len())
                .position(|w| w == needle)
                .map(|p| p + from)
        };
        let mut out = Vec::new();
        let mut i = 0;
        while let Some(s) = find(i, b"\x1bP") {
            let e = find(s, b"\x1b\\").expect("every Sixel is terminated") + 2;
            let q = s
                + 2
                + data[s + 2..e]
                    .iter()
                    .position(|&b| !(b.is_ascii_digit() || b == b';'))
                    .expect("a final");
            if data[q] != b'q' {
                i = e;
                continue;
            }
            let body = data[q + 1..e - 2]
                .iter()
                .filter(|&&b| b > 0x20 && b != 0x7f)
                .count();
            out.push((&data[s..e], body));
            i = e;
        }
        out
    }

    /// **Every preview of the yazi walk stores at the default budget**,
    /// through a real terminal fed a read at a time: each Sixel alone into a
    /// fresh store places one image, and the walk fed whole refuses none.
    /// At 256 KiB, the default until 2026-10-14, five of the thirteen were
    /// refused for the body -- the screenshot (399,396 bytes), the photo
    /// (485,395), each twice, and the text PDF's page (815,682) -- and the
    /// pane went blank. The positive control is in the fixture: those five
    /// bodies are past 256 KiB, so a recording without them, or a reader
    /// that measured them short, fails here rather than passes.
    #[test]
    fn every_yazi_preview_of_the_walk_stores_at_the_default_budget() {
        let all = sixels(WALK);
        assert_eq!(all.len(), 13, "thirteen previews");
        let large: Vec<usize> = all
            .iter()
            .map(|&(_, n)| n)
            .filter(|&n| n > 1 << 18)
            .collect();
        assert_eq!(
            large,
            [485_395, 399_396, 815_682, 399_396, 485_395],
            "the five bodies the 256 KiB default refused"
        );
        let cell = (ImagePx::of(17), ImagePx::of(34));
        for (k, &(sixel, n)) in all.iter().enumerate() {
            let (mut s, ws) = at_default();
            let mut t = Term::new(s.storage(), &ws);
            t.set_cell_size(cell.0, cell.1);
            pump(&mut t, sixel);
            let imgs = t.images();
            assert_eq!(
                imgs.refusals,
                baud_domain::ImageRefusals::NONE,
                "preview {k}, a {n}-byte body"
            );
            assert_eq!(imgs.placements().count(), 1, "preview {k} placed");
        }
        let (mut s, ws) = at_default();
        let mut t = Term::new(s.storage(), &ws);
        t.set_cell_size(cell.0, cell.1);
        pump(&mut t, WALK);
        assert_eq!(t.images().refusals, baud_domain::ImageRefusals::NONE);
        assert_eq!(t.images().placements().count(), 1, "the last preview");
    }
}
