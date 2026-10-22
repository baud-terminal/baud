//! What both presentation layers set up the same way: the grid the storage
//! is provisioned for, the cache probe whose answer picks the grid's pitch,
//! and the faces the config names -- read once, parsed once, and leaked,
//! because the renderer borrows them for the life of the process.

use crate::config::Config;
use baud_domain::themefile::CellPaint;
use baud_domain::{CacheGeometry, CacheLevel, ImageBudget, ImagePx, ImageRefusals, WinSize};
use baud_font::bytes::Index;
use baud_font::{Face, Px, StrikeRefusal, StrikeState};
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

/// The default image arena, when a config names no `[images]` section:
/// 95 MiB of decoded pixels. That is three 3840x2160 screens (24,883,200
/// pixels) with 20,480 to spare, because `[images] pixels_mb` counts whole
/// MiB and the committed example writes the default. The gap-scan packs
/// smaller images into it, and an overflow is refused and counted rather
/// than truncated.
/// **It is the one limit a config sets on an image**, with the 65535 pixels a
/// coordinate holds each way and the slot table beside it: a Sixel is decoded
/// as its body arrives (`SixelStream`), and no copy of the body is held, so
/// no body is refused for its bytes. That is `[[source: dec-std-070]]` 070-9's
/// limit: §8.1.2's raster extent "does not limit the number of pixels a
/// device may image" (its p. 9, PDF p. 920), §2 makes "'picture too
/// complex'" for an 800x240 raster not permitted and says "The protocol
/// should support any size rasters, although not all devices will be capable
/// of displaying them" (p. 2, PDF p. 913), and no section read names a limit
/// on a body's bytes. Until 2026-10-14 a body was held whole in a scratch,
/// 256 KiB and then the owner's 3 MiB interim; at 256 KiB yazi's UI
/// screenshot (334-390 KiB at 600x375), photo (474-680 KiB at 600x800) and
/// text-PDF page (797-820 KiB at 600x776) previews were refused whole though
/// each fit the arena, and the pane went blank (the record, "A Sixel is
/// decoded as its body arrives"). The single source of the default;
/// `Config::defaults` reads it, and `[images] pixels_mb` overrides it.
///
/// **Sized to the kitty quota's "It should allow at least a few full
/// screen images"** (`[[source: kitty-graphics]]`, "Image persistence and
/// storage quotas"): three screens at 3840x2160, the largest this Mac's
/// displays draw. An idle terminal pays nothing for it, because the arena,
/// the query probe and the PNG scratch come from zeroed memory
/// (`Owned::with_images`, `Pixel::zeroed_vec`) and a page is committed
/// when an image first writes it: a 400x150 terminal built that way and
/// left idle peaked at 11.9 MB resident at this budget and 11.8 MB at the
/// 4 MiB it replaced, where writing the arena at construction had peaked
/// at 111.5 MB and 16.0 MB. A deleted image's pages go back to the system
/// (`pages.rs`), with the PNG scratch its decode wrote: one 1920x1080
/// picture shown and deleted leaves that terminal within 0.3 MB of idle
/// (0.26 MB after a PNG, 0.20 MB after `f=32`), where 21.1 MB (a PNG) and 8.5 MB (`f=32`) stayed resident without it.
/// Until 2026-10-16 this was 4 MiB and refused a 1920x1080 picture whole,
/// an open departure.
pub const IMAGE_BUDGET: ImageBudget = ImageBudget {
    pixels: 95 << 18, // 24,903,680 ColorWords, 95 MiB
};

/// One startup line naming the resolved image budget, at `info` -- so it shows
/// only under `BAUD_LOG=info` or below. A refusal names its own limit and
/// sizes as it happens ([`describe_refusals`]); this line is the budget a
/// reader checks a changed `[images]` against. A `ColorWord` is four bytes,
/// so a MiB is `1 << 18` of them -- `>> 18` gives MiB with no multiply.
pub fn describe_images(b: &ImageBudget) -> String {
    format!("images: {} MiB pixels", b.pixels >> 18)
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
/// also counted the arena and the slots and that nothing in the app read. No
/// body is refused since 2026-10-14, when a Sixel began decoding as it
/// arrived, so the limits said are the arena's and the slots'. An
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
#[cfg(test)]
pub fn load_faces(cfg: &mut Config) -> &'static [Face<'static>] {
    resolved(cfg);
    parse_faces(cfg, &map_faces(cfg))
}

/// `cfg`'s family names resolved against the system's fonts, as
/// `Config::load` resolves them, for a test that builds its config in place.
#[cfg(test)]
pub fn resolved(cfg: &mut Config) {
    let said = cfg.resolve(crate::discover::Catalogue::system());
    assert!(said.is_empty(), "{said:?}");
}

/// The bytes of every face the config names, in its order: compiled in, or
/// a read-only mapping for the life of the process.
pub fn map_faces(cfg: &Config) -> Vec<&'static [u8]> {
    cfg.faces
        .iter()
        .map(|f| match f.from {
            crate::config::FaceFrom::Embedded(bytes) => bytes,
            crate::config::FaceFrom::File => {
                let bytes = crate::pty::map_readonly(&f.path)
                    .unwrap_or_else(|e| panic!("font {}: {e}", f.path));
                startup_mark(&format!("map {}", f.path.rsplit('/').next().unwrap_or("")));
                bytes
            }
            // `Config::load` resolves every name before anything maps.
            crate::config::FaceFrom::Family => {
                panic!("font family {:?}: the config was not resolved", f.path)
            }
        })
        .collect()
}

/// `data`, which [`map_faces`] read from `cfg`, parsed in `cfg`'s order.
pub fn parse_faces(cfg: &Config, data: &[&'static [u8]]) -> &'static [Face<'static>] {
    Box::leak(
        data.iter()
            .zip(&cfg.faces)
            .map(|(d, f)| {
                let mut face = Face::parse_index(d, f.index)
                    .unwrap_or_else(|e| panic!("font {}: {e}", f.path));
                // A variable face is drawn at the values discovery picked
                // for its style (`discover::pick`).
                face.set_variations(&f.variations);
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

/// Tell the core how wide the configured text is: whether a `U+FE0F` widens
/// its base (`vs16_width`) and how many columns an East Asian Ambiguous
/// character takes (`[text] ambiguous_width`). One function for both
/// drivers, called after `Term::new` and before the first feed, so the grid
/// never holds cells measured under two policies.
pub fn set_widths(
    term: &mut baud_core::Term<'_>,
    vs16: baud_domain::Vs16Width,
    ambiguous: baud_domain::AmbiguousWidth,
) {
    term.set_vs16_width(vs16);
    term.set_ambiguous_width(ambiguous);
}

/// Tell the core whether a program's request for a screen checksum
/// (DECRQCRA) is answered, from `[screen] checksum`. One function for both
/// drivers, called after `Term::new` and before the first feed.
pub fn set_checksum(term: &mut baud_core::Term<'_>, checksum: baud_domain::ChecksumReport) {
    term.set_checksum_report(checksum);
}

/// Tell the core whether a program's window operations reach the driver,
/// from `[window] operations`. One function for both drivers, called after
/// `Term::new` and before the first feed.
pub fn set_window_ops(term: &mut baud_core::Term<'_>, ops: baud_domain::WindowOps) {
    term.set_window_ops(ops);
}

/// Tell the core which window manipulations the driver carries out and
/// whether it puts a program's `OSC 52` write on the clipboard, so `OSC
/// 61 ; allowWindowOps` lists what it does not do: `winops::mac_abilities`
/// or `winops::wayland_abilities`, and `[clipboard] osc52`; and the
/// control characters every paste replaces (`paste::FILTER`), for `OSC 61
/// ; allowPasteControls`. One function for both drivers, called before the
/// first feed.
pub fn set_window_abilities(
    term: &mut baud_core::Term<'_>,
    a: baud_core::WindowAbilities,
    osc52: crate::config::Osc52,
) {
    term.set_window_abilities(a);
    // Every paste goes through `paste.rs`, which replaces xterm's default
    // controls on both systems, so `OSC 61 ; allowPasteControls` names them.
    term.set_paste_controls(crate::paste::FILTER);
    term.set_osc52_writes(match osc52 {
        crate::config::Osc52::Write => baud_core::Osc52Writes::On,
        crate::config::Osc52::Off => baud_core::Osc52Writes::Off,
    });
}

/// Tell the core the `TERM` the child is given ([`crate::bell::TERM`]),
/// which a program's XTGETTCAP `TN` asks for. One function for both
/// drivers, called before the first feed.
pub fn set_term_name(term: &mut baud_core::Term<'_>) {
    if let Some(name) = baud_core::TermName::new(crate::bell::TERM.as_bytes()) {
        term.set_term_name(name);
    }
}

/// Tell the core whether there is a pointer for DEC's locator, so `DSR
/// ?55` answers "Locator ready" only where one is: macOS's, always, and
/// the Wayland seat's while its capabilities name one. One function for
/// both drivers, called before the first feed, and on Linux after every
/// event the window gave.
pub fn set_locator_device(term: &mut baud_core::Term<'_>, d: baud_domain::LocatorDevice) {
    term.set_locator_device(d);
}

/// Tell the core whether a program may log what it sends (`?46`, `OSC
/// 46`), from `[screen] logging`. One function for both drivers, called
/// after `Term::new` and before the first feed; each driver writes the log
/// through `hostlog::HostLog`, which its one feed is handed.
pub fn set_logging(term: &mut baud_core::Term<'_>, logging: baud_domain::Logging) {
    term.set_logging(logging);
}

/// Tell the core whether a program's `OSC 50` may change the font, from
/// `[font] changes`. One function for both drivers, called after
/// `Term::new` and before the first feed.
pub fn set_font_changes(term: &mut baud_core::Term<'_>, c: baud_domain::FontChanges) {
    term.set_font_changes(c);
}

/// Tell the core the family the chain is led by and whether `points` is
/// `configured`, the config's size, so `OSC 50 ; ?` answers what is drawn:
/// at start-up and after every change of face or size, a program's or the
/// View menu's.
pub fn tell_font(
    term: &mut baud_core::Term<'_>,
    fonts: &crate::fonts::Fonts,
    points: f32,
    configured: f32,
) {
    term.set_font(
        fonts.family().as_bytes(),
        crate::fonts::size_of(points, configured),
    );
}

/// **What a program's font change does here**, planned against the faces
/// loaded and the size bounds (`fonts.rs`): the plan, or `None` when it
/// changes nothing -- a family baud has not loaded or a step past the
/// bounds -- which is counted in `Term::fonts_refused` with the core's own
/// refusals and said at info. One function for both drivers.
pub fn plan_font(
    term: &mut baud_core::Term<'_>,
    fonts: &crate::fonts::Fonts,
    change: &baud_core::FontChange,
    points: f32,
    configured: f32,
) -> Option<crate::fonts::Plan> {
    match fonts.plan(change, points, configured) {
        Ok(plan) => Some(plan),
        Err(why) => {
            term.refuse_font_request();
            crate::info!("OSC 50: {change:?} changes nothing: {why:?}");
            None
        }
    }
}

/// Tell the core when a key hides the pointer at power-on and after `RIS`,
/// from `[pointer] hide_while_typing`. One function for both drivers,
/// called after `Term::new` and before the first feed.
pub fn set_pointer_mode(term: &mut baud_core::Term<'_>, mode: baud_domain::PointerMode) {
    term.set_initial_pointer_mode(mode);
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
    // A selection colour that names the cell's own text or page answers the
    // default cell's: the text or the page the theme draws.
    let shown = |p: CellPaint<baud_render::Argb>| match p {
        CellPaint::Colour(c) => rgb(c),
        CellPaint::Text => rgb(theme.fg),
        CellPaint::Page => rgb(theme.bg),
    };
    // One call: the defaults, the table and the selection's colours, so an
    // `OSC 4 ; c ; ?` for an entry nothing overrode, or an `OSC 17 ; ?` with
    // no program colour, is answered with the colour drawn, and a `?2031`
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
            selection: theme.selection.map(shown),
            selection_text: theme.selection_text.map(shown),
            // The cursor's colour, when it is one; a cell's own is drawn by
            // the renderer and answers nothing, as no colour did before.
            cursor: theme.cursor.and_then(CellPaint::colour).map(rgb),
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
        let mut cfg = crate::config::Config::defaults();
        let faces = super::load_faces(&mut cfg);
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
        let mut cfg = crate::config::Config::defaults();
        let faces = super::load_faces(&mut cfg);
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
            let loaded = super::load_faces(&mut cfg);
            // The chain; the family's styled faces beside the file follow
            // it, out of the chain.
            let chain = cfg
                .faces
                .iter()
                .filter(|f| f.role == baud_render::FaceRole::Fallback)
                .count();
            assert_eq!(chain, faces, "{extra:?}");
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

    /// **A configured font's own Bold, Italic and Bold Italic are its
    /// styled faces in the renderer's stack, for both drivers**, which
    /// both load through `Config::load`, `load_faces`' path and `stack`:
    /// JetBrains Mono named by its Regular's path brings the three styles
    /// beside it, each style is drawn from its own face, the embedded
    /// styled faces are not among them, and a program's `OSC 50` that
    /// names the family again keeps them (`Fonts::stack`).
    #[test]
    fn a_configured_fonts_styles_reach_the_renderer() {
        use baud_render::{FaceRole, FaceStyle};
        let font = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../baud-app/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf"
        );
        let mut cfg = crate::config::Config::defaults();
        let said = cfg.apply(&format!("fonts = [\"{font}\"]\n"));
        assert!(said.is_empty(), "{said:?}");
        let loaded = super::load_faces(&mut cfg);
        let stack = super::stack(&cfg, loaded);
        for (style, file) in [
            (FaceStyle::Bold, "JetBrainsMono-Bold.ttf"),
            (FaceStyle::Italic, "JetBrainsMono-Italic.ttf"),
            (FaceStyle::BoldItalic, "JetBrainsMono-BoldItalic.ttf"),
        ] {
            assert_eq!(stack.effective(style), style);
            let at = cfg
                .faces
                .iter()
                .position(|f| f.role == FaceRole::Style(style))
                .expect("a styled face");
            assert!(cfg.faces[at].path.ends_with(file), "{}", cfg.faces[at].path);
            assert!(
                !matches!(cfg.faces[at].from, crate::config::FaceFrom::Embedded(_)),
                "the configured family's, not the embedded one's"
            );
        }
        let data = super::map_faces(&cfg);
        let faces = super::parse_faces(&cfg, &data);
        let mut fonts = crate::fonts::Fonts::new(&cfg, data, faces);
        let slot = fonts.resolve(b"JETBRAINS MONO").expect("the family");
        let built = fonts.stack(slot).expect("its stack");
        let styled = built
            .roles
            .iter()
            .filter(|r| matches!(r, FaceRole::Style(_)))
            .count();
        assert_eq!(styled, 3);
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
            let loaded = super::load_faces(&mut cfg);
            // The chain; the family's styled faces beside the file follow
            // it, out of the chain.
            let chain = cfg
                .faces
                .iter()
                .filter(|f| f.role == baud_render::FaceRole::Fallback)
                .count();
            assert_eq!(chain, faces, "{extra:?}");
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
            let loaded = super::load_faces(&mut cfg);
            // The chain; the family's styled faces beside the file follow
            // it, out of the chain.
            let chain = cfg
                .faces
                .iter()
                .filter(|f| f.role == baud_render::FaceRole::Fallback)
                .count();
            assert_eq!(chain, faces, "{extra:?}");
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

    /// **The configured widths reach the core.** After `set_widths`, `±`
    /// (East Asian Ambiguous) takes the configured columns, U+E0B0 (a
    /// Nerd Fonts separator, private use) one under either, and a heart
    /// with `U+FE0F` the configured `vs16_width`; `DECRQM ?1021` says which.
    #[test]
    fn set_widths_reaches_the_core() {
        use baud_domain::{AmbiguousWidth, Vs16Width};
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        for (vs16, ambiguous, cols, rqm) in [
            (Vs16Width::Two, AmbiguousWidth::Narrow, 4, "2"),
            (Vs16Width::Two, AmbiguousWidth::Wide, 5, "1"),
            (Vs16Width::One, AmbiguousWidth::Wide, 4, "1"),
        ] {
            let mut s = Owned::new(&ws);
            let mut t = Term::new(s.storage(), &ws);
            super::set_widths(&mut t, vs16, ambiguous);
            feed(&mut t, "±\u{E0B0}\u{2764}\u{FE0F}".as_bytes());
            assert_eq!(t.cursor().1.index(), cols, "{vs16:?} {ambiguous:?}");
            feed(&mut t, b"\x1b[?1021$p");
            let want = format!("\x1b[?1021;{rqm}$y");
            assert_eq!(t.reply(), want.as_bytes(), "{ambiguous:?}");
        }
    }

    /// **`[screen] checksum` reaches the core, from both drivers.** The
    /// config text read into a `Config`, handed to `set_checksum`, decides
    /// whether DECRQCRA is answered: by default with the checksum report,
    /// with `"off"` with nothing, counted. Both drivers start AppKit or
    /// Wayland, which no test drives, so their call is held in the source:
    /// each calls `set_checksum` once with the configured value.
    #[test]
    fn the_checksum_setting_reaches_the_core() {
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        for (text, answered) in [("", true), ("[screen]\nchecksum = \"off\"\n", false)] {
            let mut cfg = crate::config::Config::defaults();
            let said = cfg.apply(text);
            assert!(said.is_empty(), "{said:?}");
            let mut s = Owned::new(&ws);
            let mut t = Term::new(s.storage(), &ws);
            super::set_checksum(&mut t, cfg.checksum);
            feed(&mut t, b"A\x1b[1;1;1;1;1;1*y");
            let want: &[u8] = if answered { b"\x1bP1!~FFBF\x1b\\" } else { b"" };
            assert_eq!(t.reply(), want, "{text:?}");
            assert_eq!(t.checksums_refused, usize::from(!answered), "{text:?}");
        }
        // Spelled apart, so this test's own text is not what it finds.
        const CALL: &str = concat!("crate::host::", "set_checksum(&mut term, ");
        for (driver, src, arg) in [
            ("macOS", include_str!("app.rs"), "checksum);"),
            ("Linux", include_str!("linux.rs"), "cfg.checksum);"),
        ] {
            assert_eq!(src.matches(CALL).count(), 1, "{driver} calls it once");
            let rest = src.split(CALL).nth(1).expect("the call");
            assert!(
                rest.starts_with(arg),
                "{driver} passes the configured value"
            );
        }
        assert_eq!(
            include_str!("app.rs")
                .matches("checksum: cfg.checksum,")
                .count(),
            1,
            "macOS carries it from the config"
        );
    }

    /// **`[window] operations` reaches the core, from both drivers.** The
    /// config text read into a `Config`, handed to `set_window_ops`,
    /// decides whether `CSI 2 t` reaches the driver: by default as a
    /// request, with `"off"` not at all, counted. Both drivers start AppKit
    /// or Wayland, which no test drives, so their call is held in the
    /// source: each calls `set_window_ops` once with the configured value,
    /// and drains the requests in the per-read callback of its one feed.
    #[test]
    fn the_window_operations_setting_reaches_the_core() {
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        for (text, on) in [("", true), ("[window]\noperations = \"off\"\n", false)] {
            let mut cfg = crate::config::Config::defaults();
            let said = cfg.apply(text);
            assert!(said.is_empty(), "{said:?}");
            let mut s = Owned::new(&ws);
            let mut t = Term::new(s.storage(), &ws);
            let refused = t.window_ops_refused();
            super::set_window_ops(&mut t, cfg.window_ops);
            feed(&mut t, b"\x1b[2t");
            let want: &[baud_core::WindowRequest] = if on {
                &[baud_core::WindowRequest::Iconify]
            } else {
                &[]
            };
            assert_eq!(t.window_requests(), want, "{text:?}");
            assert_eq!(t.window_ops_refused() != refused, !on, "{text:?}");
        }
        // Spelled apart, so this test's own text is not what it finds.
        const CALL: &str = concat!("crate::host::", "set_window_ops(&mut term, ");
        const DRAIN: &str = concat!("t.clear_", "window_requests();");
        for (driver, src, arg, body) in [
            (
                "macOS",
                include_str!("app.rs"),
                "window_ops);",
                "    fn pump(&mut self, trigger: Trigger) {",
            ),
            (
                "Linux",
                include_str!("linux.rs"),
                "cfg.window_ops);",
                "    fn feed(&mut self, bytes: &[u8]) {",
            ),
        ] {
            assert_eq!(src.matches(CALL).count(), 1, "{driver} calls it once");
            let rest = src.split(CALL).nth(1).expect("the call");
            assert!(
                rest.starts_with(arg),
                "{driver} passes the configured value"
            );
            assert_eq!(src.matches(DRAIN).count(), 1, "{driver} drains once");
            let feed = src
                .split_once(body)
                .expect("the driver's feed")
                .1
                .split_once(concat!("crate::reads::", "paced("))
                .expect("the feed")
                .1;
            let call = &feed[..feed.find("\n        );").expect("its end")];
            assert!(call.contains(DRAIN), "{driver} drains inside the feed");
        }
        assert_eq!(
            include_str!("app.rs")
                .matches("window_ops: cfg.window_ops,")
                .count(),
            1,
            "macOS carries it from the config"
        );
    }

    /// **`OSC 61 ; allowWindowOps` says what each driver does**: macOS
    /// every manipulation, Linux all but what xdg-shell lacks, and both name
    /// `SetSelection` under `[clipboard] osc52 = "off"`, which refuses the
    /// copy the core stored. Each driver's call is held in the source, as
    /// the window operations' setting is.
    #[test]
    fn the_window_abilities_reach_the_core_from_both_drivers() {
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        let ask = |a, osc52| {
            let mut s = Owned::new(&ws).with_clipboard(baud_core::ClipboardBudget::of_text(64));
            let mut t = Term::new(s.storage(), &ws);
            super::set_window_abilities(&mut t, a, osc52);
            feed(&mut t, b"\x1b]61;allowWindowOps\x07");
            String::from_utf8_lossy(t.reply()).into_owned()
        };
        let write = crate::config::Osc52::Write;
        let mac = ask(crate::winops::mac_abilities(), write);
        assert!(mac.starts_with("\x1b]61;GetWinState,"), "{mac:?}");
        assert!(mac.ends_with(",GetWinTitle,GetSelection\x07"), "{mac:?}");
        let linux = ask(
            crate::winops::wayland_abilities(crate::winops::BoundsEvent::Offered),
            write,
        );
        assert!(
            linux.starts_with("\x1b]61;RestoreWin,SetWinPosition,RaiseWin,LowerWin,MaximizeWin,"),
            "{linux}"
        );
        let off = ask(crate::winops::mac_abilities(), crate::config::Osc52::Off);
        assert!(off.ends_with(",GetSelection,SetSelection\x07"), "{off}");
        // The paste controls both drivers' pastes replace, named.
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        super::set_window_abilities(&mut t, crate::winops::mac_abilities(), write);
        feed(&mut t, b"\x1b]61;allowPasteControls\x07");
        assert_eq!(t.reply(), b"\x1b]61;NUL,EOT,ENQ,BS,ESC,DEL,STTY\x07");
        // Spelled apart, so this test's own text is not what it finds.
        const CALL: &str = concat!("crate::host::", "set_window_abilities(");
        for (driver, src, policy, osc52) in [
            (
                "macOS",
                include_str!("app.rs"),
                "crate::winops::mac_abilities(),",
                "clipboard.osc52,",
            ),
            (
                "Linux",
                include_str!("linux.rs"),
                "crate::winops::wayland_abilities(crate::winops::BoundsEvent::Never),",
                "cfg.clipboard.osc52,",
            ),
        ] {
            assert_eq!(src.matches(CALL).count(), 1, "{driver} calls it once");
            let rest = src.split(CALL).nth(1).expect("the call");
            let args = &rest[..rest.find(");").expect("its end")];
            assert!(args.contains(policy), "{driver} tells its own policy");
            assert!(args.contains(osc52), "{driver} tells its setting");
        }
        // Linux tells it again once the bound `xdg_wm_base` is known.
        let linux = include_str!("linux.rs");
        let told = linux
            .split("match d.window.configure_bounds()")
            .nth(1)
            .expect("the version read after each turn's events");
        assert!(
            told[..told.find("// The step").expect("the next step")]
                .contains(".set_window_abilities(crate::winops::wayland_abilities(bounds))"),
            "Linux tells the core the abilities the version allows"
        );
    }

    /// **Both drivers tell the core whether there is a pointer**, so `DSR
    /// ?55` says a locator is ready only where one is: macOS once, before
    /// the first feed, and Linux after each turn's events, from the seat.
    /// Untold, the core says there is none. The drivers start AppKit or
    /// Wayland, which no test drives, so their calls are held in the
    /// source.
    #[test]
    fn the_pointer_reaches_the_core_from_both_drivers() {
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        feed(&mut t, b"\x1b[?55n");
        assert_eq!(t.reply(), b"\x1b[?53n", "untold: no locator");
        t.clear_reply();
        super::set_locator_device(&mut t, baud_domain::LocatorDevice::Present);
        feed(&mut t, b"\x1b[?55n");
        assert_eq!(t.reply(), b"\x1b[?50n", "told: ready");
        // Spelled apart, so this test's own text is not what it finds.
        const CALL: &str = concat!("crate::host::", "set_locator_device(");
        for (driver, src, arg) in [
            (
                "macOS",
                include_str!("app.rs"),
                "&mut term, baud_domain::LocatorDevice::Present);",
            ),
            (
                "Linux",
                include_str!("linux.rs"),
                "&mut d.term, d.window.pointer_device());",
            ),
        ] {
            assert_eq!(src.matches(CALL).count(), 1, "{driver} calls it once");
            let rest = src.split(CALL).nth(1).expect("the call");
            assert!(rest.starts_with(arg), "{driver} says what it has");
        }
        let linux = include_str!("linux.rs");
        let events = linux
            .split_once(concat!("while let Some(e) = ", "d.window.next_event()"))
            .expect("the event loop")
            .1;
        let end = events
            .find(concat!("Event::Closed => ", "break 'run,"))
            .expect("its end");
        assert!(
            events[end..].find(CALL).is_some_and(|at| at < 400),
            "Linux tells it straight after the turn's events"
        );
    }

    /// **Both drivers tell the core the `TERM` the child is given**, so a
    /// program's XTGETTCAP `TN` names it; untold, the core refuses `TN`.
    /// The drivers' calls are held in the source, as the pointer's are.
    #[test]
    fn the_term_name_reaches_the_core_from_both_drivers() {
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        // `TN`, hex-encoded.
        feed(&mut t, b"\x1bP+q544E\x1b\\");
        assert_eq!(t.reply(), b"\x1bP0+r\x1b\\", "untold: refused");
        t.clear_reply();
        super::set_term_name(&mut t);
        feed(&mut t, b"\x1bP+q544E\x1b\\");
        let hex: String = crate::bell::TERM
            .bytes()
            .map(|b| format!("{b:02X}"))
            .collect();
        assert_eq!(t.reply(), format!("\x1bP1+r544E={hex}\x1b\\").as_bytes());
        assert_eq!(crate::bell::TERM, "xterm-256color");
        const CALL: &str = concat!("crate::host::", "set_term_name(");
        for (driver, src) in [
            ("macOS", include_str!("app.rs")),
            ("Linux", include_str!("linux.rs")),
        ] {
            assert_eq!(src.matches(CALL).count(), 1, "{driver} calls it once");
        }
    }

    /// **`[screen] logging` reaches the core, from both drivers, and each
    /// hands its log to its one feed.** By default `?46 h` starts a log,
    /// with `"off"` it is refused and counted. The drivers' calls are held
    /// in the source, as the window operations' are.
    #[test]
    fn the_logging_setting_reaches_the_core() {
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        for (text, want) in [
            ("", baud_core::LogMode::Started),
            ("[screen]\nlogging = \"off\"\n", baud_core::LogMode::Stopped),
        ] {
            let mut cfg = crate::config::Config::defaults();
            let said = cfg.apply(text);
            assert!(said.is_empty(), "{said:?}");
            let mut s = Owned::new(&ws);
            let mut t = Term::new(s.storage(), &ws);
            super::set_logging(&mut t, cfg.logging);
            feed(&mut t, b"\x1b[?46h");
            assert_eq!(t.log_mode(), want, "{text:?}");
        }
        const CALL: &str = concat!("crate::host::", "set_logging(&mut term, ");
        for (driver, src, arg, log) in [
            (
                "macOS",
                include_str!("app.rs"),
                "logging);",
                "            hostlog,\n        );",
            ),
            (
                "Linux",
                include_str!("linux.rs"),
                "cfg.logging);",
                "            &mut self.hostlog,\n        );",
            ),
        ] {
            assert_eq!(src.matches(CALL).count(), 1, "{driver} calls it once");
            let rest = src.split(CALL).nth(1).expect("the call");
            assert!(
                rest.starts_with(arg),
                "{driver} passes the configured value"
            );
            assert_eq!(
                src.matches(log).count(),
                1,
                "{driver} hands its feed the log"
            );
        }
        assert_eq!(
            include_str!("app.rs")
                .matches("logging: cfg.logging,")
                .count(),
            1,
            "macOS carries it from the config"
        );
    }

    /// **`[pointer] hide_while_typing` reaches the core, from both drivers,
    /// and each driver asks the policy at a key that sent bytes and after a
    /// read.** The config text read into a `Config`, handed to
    /// `set_pointer_mode`, decides whether a key hides the pointer at
    /// power-on and after `RIS`. Both drivers start AppKit or Wayland, which
    /// no test drives, so their calls are held in the source.
    #[test]
    fn the_pointer_setting_reaches_the_core_and_both_drivers_ask() {
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        for (text, hides) in [
            ("", baud_core::PointerHides::Yes),
            (
                "[pointer]\nhide_while_typing = \"off\"\n",
                baud_core::PointerHides::No,
            ),
        ] {
            let mut cfg = crate::config::Config::defaults();
            let said = cfg.apply(text);
            assert!(said.is_empty(), "{said:?}");
            let mut s = Owned::new(&ws);
            let mut t = Term::new(s.storage(), &ws);
            super::set_pointer_mode(&mut t, cfg.pointer_mode);
            feed(&mut t, b"\x1b[>2p\x1bc");
            assert_eq!(t.hides_pointer_on_key(), hides, "{text:?}: RIS returns it");
        }
        const CALL: &str = concat!("crate::host::", "set_pointer_mode(&mut term, ");
        for (driver, src, arg, key, read) in [
            (
                "macOS",
                include_str!("app.rs"),
                "pointer_mode);",
                concat!("let act = self.hiding.", "key(&self.app.term);"),
                concat!("hide_pointer(self.hiding.", "read(&self.app.term));"),
            ),
            (
                "Linux",
                include_str!("linux.rs"),
                "cfg.pointer_mode);",
                concat!("let act = d.hiding.", "key(&d.term);"),
                concat!("let act = self.hiding.", "read(&self.term);"),
            ),
        ] {
            assert_eq!(src.matches(CALL).count(), 1, "{driver} calls it once");
            let rest = src.split(CALL).nth(1).expect("the call");
            assert!(
                rest.starts_with(arg),
                "{driver} passes the configured value"
            );
            assert_eq!(src.matches(key).count(), 1, "{driver} asks at a key");
            assert_eq!(src.matches(read).count(), 1, "{driver} asks after a read");
        }
    }

    /// **`[font] changes` and the font in use reach the core, from both
    /// drivers, and each drains a program's font changes inside its feed
    /// and performs them through `fonts.rs`.** The config's value handed to
    /// `set_font_changes` decides whether `OSC 50 ; Menlo` is queued; the
    /// family `tell_font` gives is what `OSC 50 ; ?` answers. The drivers
    /// start AppKit or Wayland, so their calls are held in the source.
    #[test]
    fn the_font_setting_and_family_reach_the_core_from_both_drivers() {
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        for (text, queued) in [("", 1), ("[font]\nchanges = \"off\"\n", 0)] {
            let mut cfg = crate::config::Config::defaults();
            assert!(cfg.apply(text).is_empty());
            super::resolved(&mut cfg);
            let data = super::map_faces(&cfg);
            let faces = super::parse_faces(&cfg, &data);
            let fonts = crate::fonts::Fonts::new(&cfg, data, faces);
            let mut s = Owned::new(&ws);
            let mut t = Term::new(s.storage(), &ws);
            super::set_font_changes(&mut t, cfg.font_changes);
            super::tell_font(&mut t, &fonts, cfg.font_size, cfg.font_size);
            feed(&mut t, b"\x1b]50;Menlo\x07\x1b]50;?\x07");
            assert_eq!(t.font_requests().len(), queued, "{text:?}");
            assert_eq!(t.reply(), b"\x1b]50;Proto Hebrew Mono C\x07", "{text:?}");
        }
        for (driver, src, setting, told, drain, perform) in [
            (
                "macOS",
                include_str!("app.rs"),
                "crate::host::set_font_changes(&mut term, font_changes);",
                "crate::host::tell_font(&mut term, &fonts, font_size, font_size);",
                concat!(
                    "fonts_asked.extend_from_slice(t.font_requests());\n",
                    "                t.clear_font_requests();"
                ),
                concat!(
                    "crate::host::plan_font(",
                    "&mut app.term, &app.fonts, c, at, app.font_base)"
                ),
            ),
            (
                "Linux",
                include_str!("linux.rs"),
                "crate::host::set_font_changes(&mut term, cfg.font_changes);",
                "crate::host::tell_font(&mut term, &fonts, cfg.font_size, cfg.font_size);",
                concat!(
                    "fonts_asked.extend_from_slice(t.font_requests());\n",
                    "                t.clear_font_requests();"
                ),
                concat!(
                    "let (at, base) = (self.font_size, self.font_base);\n",
                    "            let plan = crate::host::plan_font(",
                    "&mut self.term, &self.fonts, &c, at, base);"
                ),
            ),
        ] {
            for needle in [setting, told, drain, perform] {
                assert_eq!(src.matches(needle).count(), 1, "{driver}: {needle}");
            }
        }
    }

    /// **A change the app cannot make is counted in the core**: a family
    /// baud has not loaded and a step past the bounds change nothing and
    /// move `Term::fonts_refused`; one it can make plans and moves nothing.
    #[test]
    fn a_change_the_app_cannot_make_is_counted_in_the_core() {
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        let mut cfg = crate::config::Config::defaults();
        super::resolved(&mut cfg);
        let data = super::map_faces(&cfg);
        let faces = super::parse_faces(&cfg, &data);
        let fonts = crate::fonts::Fonts::new(&cfg, data, faces);
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        super::tell_font(&mut t, &fonts, cfg.font_size, cfg.font_size);
        for (pt, plans) in [
            (&b"Nowhere Mono"[..], false),
            (b"#+100", false),
            (b"#-100", false),
            (b"#+1", true),
            (b"Proto Hebrew Mono C", true),
        ] {
            feed(&mut t, &[&b"\x1b]50;"[..], pt, b"\x07"].concat());
            let asked = t.font_requests().to_vec();
            t.clear_font_requests();
            assert_eq!(asked.len(), 1, "{pt:?}");
            let before = t.fonts_refused();
            let plan = super::plan_font(&mut t, &fonts, &asked[0], cfg.font_size, cfg.font_size);
            assert_eq!(plan.is_some(), plans, "{pt:?}");
            assert_eq!(t.fonts_refused() == before, plans, "{pt:?}");
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

    /// **The theme's selection colours are what `OSC 17`/`19 ; ?` answer**:
    /// the colour the renderer draws the selection in, pushed with the rest
    /// of the theme, and reverse video's colours on a theme with none.
    #[test]
    fn osc_17_and_19_answer_the_themes_selection() {
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        let page = baud_render::Argb::rgb(
            Chan::from_u8(0x33),
            Chan::from_u8(0x4e),
            Chan::from_u8(0x5e),
        );
        let theme = Theme {
            selection: Some(baud_domain::themefile::CellPaint::Colour(page)),
            ..Theme::dark()
        };
        super::tell_defaults(&mut t, &theme, baud_core::Overrides::Keep);
        feed(&mut t, b"\x1b]17;?\x07");
        assert_eq!(t.reply(), b"\x1b]17;rgb:3333/4e4e/5e5e\x07");
        t.clear_reply();
        super::tell_defaults(&mut t, &Theme::dark(), baud_core::Overrides::Keep);
        feed(&mut t, b"\x1b]17;?\x07");
        assert_eq!(
            t.reply(),
            b"\x1b]17;rgb:d8d8/d8d8/d8d8\x07",
            "the text shown"
        );
    }

    /// **The theme's cursor colour is what `OSC 12 ; ?` answers and what
    /// `OSC 112` returns to**, pushed with the rest of the theme; a cursor
    /// that takes the cell's own colours answers nothing, as before.
    #[test]
    fn osc_12_answers_the_themes_cursor() {
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        let theme = Theme {
            cursor: Some(baud_domain::themefile::CellPaint::Colour(
                baud_render::Argb::rgb(
                    Chan::from_u8(0xff),
                    Chan::from_u8(0x88),
                    Chan::from_u8(0x00),
                ),
            )),
            ..Theme::dark()
        };
        super::tell_defaults(&mut t, &theme, baud_core::Overrides::Keep);
        feed(&mut t, b"\x1b]12;#000080\x07\x1b]112\x07\x1b]12;?\x07");
        assert_eq!(t.reply(), b"\x1b]12;rgb:ffff/8888/0000\x07");
        t.clear_reply();
        let cell = Theme {
            cursor: Some(baud_domain::themefile::CellPaint::Page),
            ..Theme::dark()
        };
        super::tell_defaults(&mut t, &cell, baud_core::Overrides::Keep);
        feed(&mut t, b"\x1b]12;?\x07");
        assert_eq!(t.reply(), b"", "a cell's own colour is no colour to answer");
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
    /// pump. An image with no pixels is no limit and is not said. The arena
    /// cases are an image over the whole 95 MiB arena and one inside it that
    /// found no gap, which are different fixes.
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

        let over = ImageRefusals {
            arena: 1,
            arena_width: ImagePx::of(8192),
            arena_height: ImagePx::of(4096),
            ..none
        };
        let line = say(&none, &over).expect("an arena refusal is said");
        for want in [
            "8192x4096",
            "33554432 pixels",
            "95 MiB arena",
            "larger than the whole",
            "pixels_mb",
        ] {
            assert!(line.contains(want), "{want:?} in {line:?}");
        }
        assert_eq!(say(&over, &over), None, "said once, not every pump after");
        let more = ImageRefusals { arena: 3, ..over };
        assert!(say(&over, &more).unwrap().contains("2 images"), "a count");
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

        let both = ImageRefusals { slot: 1, ..gap };
        let line = say(&none, &both).unwrap();
        assert!(
            line.contains("every image slot") && line.contains("pixels_mb") && !line.contains('\n'),
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

    /// A zlib stream of one 3840x2160 RGB screen of zeros in one
    /// fixed-Huffman block (`[[source: rfc1951-deflate]]` 3.2.6): a literal
    /// 0, copies of 258 at distance 1, the rest as literals, each code's
    /// bits written as text, most significant first; the block header's
    /// fields go least significant first. The Adler-32 of `n` zeros is
    /// `n mod 65521` over 1 (`[[source: rfc1950-zlib]]`).
    fn zlib_zero_screen() -> Vec<u8> {
        const N: usize = 3840 * 2160 * 3;
        const LITERAL_ZERO: &str = "00110000";
        const COPY_258_AT_1: &str = "1100010100000"; // code 285, distance code 0
        let copies = (N - 1) / 258;
        let mut codes = vec!["110", LITERAL_ZERO]; // BFINAL, BTYPE = 01
        codes.extend(std::iter::repeat_n(COPY_258_AT_1, copies));
        codes.extend(std::iter::repeat_n(LITERAL_ZERO, N - 1 - copies * 258));
        codes.push("0000000"); // end of block, 256
        let bits: Vec<u8> = codes.concat().bytes().map(|b| b - b'0').collect();
        let mut out = vec![0x78, 0x01];
        out.extend(
            bits.chunks(8)
                .map(|c| c.iter().rev().fold(0u8, |byte, &b| (byte << 1) | b)),
        );
        let adler = (u32::try_from(N % 65521).unwrap() << 16) | 1;
        out.extend_from_slice(&adler.to_be_bytes());
        out
    }

    /// RFC 4648 base64, unpadded, as kitten writes it.
    fn b64(data: &[u8]) -> Vec<u8> {
        const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        for q in data.chunks(3) {
            let mut b = [0u8; 3];
            b[..q.len()].copy_from_slice(q);
            let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
            for k in 0..=q.len() {
                out.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]);
            }
        }
        out
    }

    /// **The default holds "at least a few full screen images"**
    /// (`[[source: kitty-graphics]]`, "Image persistence and storage
    /// quotas"): three 3840x2160 kitty images, sent as kitten sends them,
    /// each stored and answered with nothing deleted for space.
    #[test]
    fn the_default_budget_holds_three_3840x2160_screens() {
        let (mut s, ws) = at_default();
        let mut t = Term::new(s.storage(), &ws);
        let screen = b64(&zlib_zero_screen());
        let mut replies = Vec::new();
        for i in 1..=3 {
            let mut cmd = format!("\x1b_Ga=t,f=24,o=z,s=3840,v=2160,i={i};").into_bytes();
            cmd.extend_from_slice(&screen);
            cmd.extend_from_slice(b"\x1b\\");
            pump(&mut t, &cmd);
            while t.decode_step(baud_domain::DecodeBudget::ALL) == baud_domain::DecodeProgress::More
            {
            }
            replies.extend_from_slice(t.reply());
            t.clear_reply();
        }
        assert_eq!(
            String::from_utf8_lossy(&replies),
            "\x1b_Gi=1;OK\x1b\\\x1b_Gi=2;OK\x1b\\\x1b_Gi=3;OK\x1b\\"
        );
        let imgs = t.images();
        assert_eq!((imgs.images_evicted, imgs.images_refused), (0, 0));
    }

    /// A Sixel whose body is `len` bytes the decoder reads, every one of them
    /// `~` or `$`: `$` returns to the band's left edge, so the image is one
    /// column by one band, 1x6 pixels at `P1` 9's 1:1, at any length --
    /// inside the arena and a coordinate, so its bytes are the only thing
    /// about it that is large. (A body of `~` alone is `len` columns wide,
    /// past 65535 at these lengths.)
    fn narrow_sixel(len: ReadLen) -> Vec<u8> {
        let mut s = b"\x1bP9q".to_vec();
        s.extend((0..len.index()).map(|i| if i % 2 == 0 { b'~' } else { b'$' }));
        s.extend_from_slice(b"\x1b\\");
        s
    }

    /// **A body of any length stores at the default budget**, since a Sixel
    /// is decoded as it arrives and no body is held: one column by one band,
    /// refused for nothing and said by no line, through a real terminal fed
    /// the way the pump feeds. The lengths are the 3 MiB scratch of the
    /// interim (2026-10-14) and a byte past it, which that scratch refused
    /// whole and named in a `body_kb` line, and 8 MiB, past any scratch the
    /// app had.
    #[test]
    fn a_body_of_any_length_stores_at_the_default_budget() {
        for len in [3 << 20, (3 << 20) + 1, 8 << 20] {
            let (mut s, ws) = at_default();
            let mut t = Term::new(s.storage(), &ws);
            let before = t.images().refusals;
            pump(&mut t, &narrow_sixel(ReadLen::from_index(len)));
            let imgs = t.images();
            assert_eq!(imgs.refusals, baud_domain::ImageRefusals::NONE, "{len}");
            let placed: Vec<_> = imgs.placements().map(|(_, p)| p.image).collect();
            assert_eq!(placed.len(), 1, "a {len}-byte body stores");
            let v = imgs.image(placed[0]).expect("placed means live");
            assert_eq!((v.width.raw(), v.height.raw()), (1, 6), "one column");
            let line = super::describe_refusals(&before, &imgs.refusals, &super::IMAGE_BUDGET);
            assert_eq!(line, None, "nothing to say");
        }
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
    /// At the 256 KiB body scratch, the default until 2026-10-14, five of the
    /// thirteen were refused for the body -- the screenshot (399,396 bytes),
    /// the photo (485,395), each twice, and the text PDF's page (815,682) --
    /// and the pane went blank; no body is held since. The positive control is in the fixture: those five
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
