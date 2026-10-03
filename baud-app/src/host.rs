//! What both presentation layers set up the same way: the grid the storage
//! is provisioned for, the cache probe whose answer picks the grid's pitch,
//! and the faces the config names -- read once, parsed once, and leaked,
//! because the renderer borrows them for the life of the process.

use crate::config::Config;
use baud_domain::{CacheGeometry, CacheLevel, ImageBudget, ImagePx, WinSize};
use baud_font::{Face, FaceIndex, Px};
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

/// The default image arena, when a config names no `[images]` section: enough
/// for a moderate Sixel or several small ones -- the gap-scan packs them, and
/// an overflow is refused and counted rather than truncated -- plus body
/// scratch for one raw Sixel before it decodes. The single source of the
/// default; `Config::defaults` reads it, and `[images] pixels_mb`/`body_kb`
/// override it for a reader who wants a full-screen image at [`MAX_GRID`].
pub const IMAGE_BUDGET: ImageBudget = ImageBudget {
    pixels: 1 << 20, // 1,048,576 ColorWords, 4 MiB
    body: 1 << 18,   // 256 KiB
};

/// One startup line naming the resolved image budget, so a `[images]`
/// misconfiguration is diagnosable without a rebuild: a refused image against
/// a budget the log names is a config the reader can fix. A `ColorWord` is
/// four bytes, so a MiB is `1 << 18` of them -- `>> 18` gives MiB with no
/// multiply -- and a KiB is `1 << 10` bytes.
pub fn describe_images(b: &ImageBudget) -> String {
    format!(
        "images: {} MiB pixels, {} KiB body",
        b.pixels >> 18,
        b.body >> 10,
    )
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
/// with its path, because a terminal with no font draws nothing.
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
                    .unwrap_or_else(|_| panic!("font {}: not a face", f.path));
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
    use baud_domain::{BytePos, ReadLen, WinSize};
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

    /// **An icon the configured font lacks draws from the embedded one.**
    /// The committed plain JetBrains Mono carries the Powerline arrows but
    /// not the Nerd Fonts icons; named alone in `fonts`, the renderer's stack
    /// still finds Font Awesome's folder, U+F07B, in the embedded face behind
    /// it, and text stays with the named face. With `font_fallback = false`
    /// the stack is the named face alone and the folder falls to its
    /// `.notdef`.
    #[test]
    fn an_icon_the_configured_font_lacks_draws_from_the_embedded_one() {
        let font = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf"
        );
        let folder = baud_domain::Codepoint::new(0xF07B).unwrap();
        let a = baud_domain::Codepoint::new(u32::from('a')).unwrap();
        for (extra, faces, folder_in) in [("", 3, 1), ("font_fallback = false\n", 1, 0)] {
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

    /// **Claude Code's arrow and bullet draw from STIX behind a configured
    /// font** (the owner's call of 2026-10-03). The committed plain JetBrains
    /// Mono has neither U+23F5 nor U+23FA, and neither has the icon face, so
    /// the renderer's stack selects STIX, third; with `font_fallback = false`
    /// both fall to the named face's `.notdef`.
    #[test]
    fn claude_codes_symbols_draw_from_stix_behind_a_configured_font() {
        let font = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf"
        );
        let arrow = baud_domain::Codepoint::new(0x23F5).unwrap();
        let bullet = baud_domain::Codepoint::new(0x23FA).unwrap();
        for (extra, faces, drawn_by) in [("", 3, 2), ("font_fallback = false\n", 1, 0)] {
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

    /// **An OS flip under `auto` drops the application's colours and reports
    /// once** -- note Y's decision 5, through the call the app makes. A
    /// program subscribed with `?2031` has set palette entry 1 and the
    /// background; the flip to the light theme clears both and sends one
    /// `CSI ? 997 ; 2 n`. The same flip with `Keep` clears nothing, the
    /// control that the drop is the flag's.
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
}
