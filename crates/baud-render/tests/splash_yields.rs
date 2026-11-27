//! **The launch splash yields to the shell** (note AJ, section 26): no
//! cell the shell has drawn is ever under the logo or its plasma, on the
//! CPU path the Linux driver draws with and in the records every GPU path
//! draws from.

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::splash::{GONE, Hole};
use baud_render::{
    Blend, Damage, FaceRole, FaceStyle, Instance, Kind, PxSize, Renderer, SplashLook, SplashPhase,
    Stack, Target, Theme,
};

const WS: WinSize = WinSize {
    ws_col: 80,
    ws_row: 24,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn font(p: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../../{p}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn renderer<'f>(faces: &'f [Face<'f>], theme: Theme) -> Renderer<'f, 'f> {
    Renderer::new(
        Stack::with_roles(
            faces,
            &[FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)],
        )
        .unwrap(),
        PxSize::of(14.0).unwrap(),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap()
}

/// A frame painted into `px`, which holds the last.
fn paint(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, px: &mut Vec<u32>, damage: Damage) {
    let (w, h) = r.pixel_size();
    px.resize(usize::try_from(w.value() * h.value()).unwrap(), 0);
    let mut target = Target::new(px, w, w, h).unwrap();
    let seal = t.seal_writes();
    r.render(t, seal, &mut target, damage);
}

/// The cells holding a character other than a space.
fn inked(t: &Term<'_>) -> Vec<(usize, usize)> {
    let v = t.view(Rows::ZERO);
    let mut out = Vec::new();
    for y in 0..v.rows().index() {
        for (x, c) in v.row(Row::from_index(y)).iter().enumerate() {
            if !c.content.is_empty() && c.content.as_codepoint().is_none_or(|p| p.value() != 0x20) {
                out.push((y, x));
            }
        }
    }
    out
}

/// The pixels of cell `(y, x)` of a frame `w` pixels wide.
fn cell(px: &[u32], r: &Renderer<'_, '_>, (y, x): (usize, usize)) -> Vec<u32> {
    let m = r.metrics();
    let n = |p: baud_font::Px| usize::try_from(p.value()).unwrap();
    let (cw, ch, w) = (n(m.cell_w), n(m.cell_h), n(r.pixel_size().0));
    (0..ch)
        .flat_map(|dy| {
            let at = (y * ch + dy) * w + x * cw;
            px[at..at + cw].to_vec()
        })
        .collect()
}

/// The shell's output: fourteen lines, a coloured run of spaces and a
/// prompt, which reach into the left of the logo's box in the middle of
/// the window and leave the rest of it to the logo.
fn output() -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..14 {
        out.extend_from_slice(format!("line {i:02}: drawn the frame it lands\r\n").as_bytes());
    }
    out.extend_from_slice(b"\x1b[44m      \x1b[0m ~/src $ ");
    out
}

/// The phases a launch shows: the plasma at 60 Hz from its first frame to
/// its last, the reduced-motion hold and its fade.
fn phases() -> Vec<SplashPhase> {
    let mut out: Vec<SplashPhase> = (0u16..)
        .map(|f| f32::from(f) / 60.0)
        .take_while(|&a| a < GONE)
        .map(SplashPhase::Plasma)
        .collect();
    out.extend([
        SplashPhase::Hold,
        SplashPhase::Fade(0.3),
        SplashPhase::Fade(0.7),
    ]);
    out
}

/// **No cell the shell has drawn is under the logo**: at every phase the
/// splash shows, with the shell's output arriving while it plays, each
/// cell the output inked is drawn pixel for pixel as it is with no splash,
/// on the frame it arrives on, frame after frame of incremental damage, on
/// the dark, light and glass pages -- so the prompt is visible and its
/// text has the page's own contrast from that frame. The controls: the
/// output runs through the logo, so with the splash set and not told what
/// the shell drew, some inked cell is drawn otherwise; and the logo is
/// drawn, so some pixel outside the shell's cells differs from the frame
/// with no splash.
#[test]
fn no_cell_the_shell_has_drawn_is_under_the_logo() {
    let (regular, bold) = (
        font("crates/baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("crates/baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    let glass = Theme {
        bg_alpha: baud_domain::Chan::from_u8(0xC0),
        ..Theme::dark()
    };
    for (name, theme) in [
        ("dark", Theme::dark()),
        ("light", Theme::light()),
        ("glass", glass),
    ] {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let (mut on, mut bare, mut off) = (
            renderer(&faces, theme.clone()),
            renderer(&faces, theme.clone()),
            renderer(&faces, theme),
        );
        let (mut on_px, mut bare_px, mut off_px) = (Vec::new(), Vec::new(), Vec::new());
        paint(&mut on, &mut t, &mut on_px, Damage::Full);
        paint(&mut bare, &mut t, &mut bare_px, Damage::Full);
        let (mut covered, mut logo) = (0usize, 0usize);
        let all = phases();
        let arrives = all.len() / 3;
        for (i, &phase) in all.iter().enumerate() {
            if i == arrives {
                feed(&mut t, &output());
            }
            let look = Some(SplashLook { phase });
            on.set_splash(look);
            on.splash_yields_to(&t, Px::of(0));
            bare.set_splash(look);
            paint(&mut on, &mut t, &mut on_px, Damage::Incremental);
            paint(&mut bare, &mut t, &mut bare_px, Damage::Incremental);
            paint(&mut off, &mut t, &mut off_px, Damage::Full);
            let cells = inked(&t);
            assert_eq!(cells.is_empty(), i < arrives, "{name} {phase:?}");
            for &c in &cells {
                assert!(
                    cell(&on_px, &on, c) == cell(&off_px, &off, c),
                    "{name} {phase:?}: cell {c:?} is under the logo"
                );
                if cell(&bare_px, &bare, c) != cell(&off_px, &off, c) {
                    covered += 1;
                }
            }
            let differs = on_px.iter().zip(&off_px).filter(|(a, b)| a != b).count();
            if differs > 0 {
                logo += 1;
            }
        }
        assert!(covered > 0, "{name}: the output never ran under the logo");
        assert!(
            logo * 2 > all.len(),
            "{name}: the logo drew on {logo} of {} frames",
            all.len()
        );
    }
}

/// **Every record the splash draws is outside what the shell has drawn**:
/// its glyphs, its block and its plasma's bands and ions, which every GPU
/// path draws as the CPU path does -- the records any path uploads meet no
/// hole. The control: before the output arrives they meet the cells the
/// output then inks.
#[test]
fn no_record_of_the_splash_meets_a_cell_the_shell_has_drawn() {
    let (regular, bold) = (
        font("crates/baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("crates/baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    let mut r = renderer(&faces, Theme::dark());
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let m = r.metrics();
    let n = |p: baud_font::Px| i32::try_from(p.value()).unwrap();
    let (cw, ch) = (n(m.cell_w), n(m.cell_h));
    let mut shell = Owned::new(&WS);
    let mut drawn = Term::new(shell.storage(), &WS);
    feed(&mut drawn, &output());
    let holes: Vec<Hole> = inked(&drawn)
        .iter()
        .map(|&(y, x)| {
            let (x, y) = (i32::try_from(x).unwrap(), i32::try_from(y).unwrap());
            Hole {
                left: x * cw,
                top: y * ch,
                right: (x + 1) * cw,
                bottom: (y + 1) * ch,
            }
        })
        .collect();
    let meets = |recs: &[Instance]| recs.iter().any(|i| holes.iter().any(|h| h.meets(i)));
    let mut met = 0;
    for phase in phases() {
        r.set_splash(Some(SplashLook { phase }));
        r.splash_yields_to(&t, Px::of(0));
        if meets(r.splash()) || meets(r.trail()) {
            met += 1;
        }
    }
    assert!(met > 0, "the control: the logo is where the output goes");
    feed(&mut t, &output());
    for phase in phases() {
        r.set_splash(Some(SplashLook { phase }));
        r.splash_yields_to(&t, Px::of(0));
        assert!(!meets(r.splash()), "{phase:?}: a glyph over the shell's");
        assert!(!meets(r.trail()), "{phase:?}: the plasma over the shell's");
    }
}

/// **Around the shell's cells the logo is drawn as it was**: every pixel
/// outside the rows' spans the shell has drawn -- each row's from its first
/// cell to its last, the cursor's cell with them -- is the pixel the splash
/// draws when it is not told what the shell drew, at every phase, but
/// under an ion that meets one of them, which is left out whole: the cut
/// glyphs keep their texels and the cut bands their light. The control:
/// outside those spans the logo is drawn, the frame differing from the one
/// with no splash.
#[test]
fn around_the_shells_cells_the_logo_is_drawn_as_it_was() {
    let (regular, bold) = (
        font("crates/baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("crates/baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &output());
    let (mut on, mut bare, mut off) = (
        renderer(&faces, Theme::dark()),
        renderer(&faces, Theme::dark()),
        renderer(&faces, Theme::dark()),
    );
    let (mut on_px, mut bare_px, mut off_px) = (Vec::new(), Vec::new(), Vec::new());
    paint(&mut off, &mut t, &mut off_px, Damage::Full);
    let m = on.metrics();
    let n = |p: baud_font::Px| usize::try_from(p.value()).unwrap();
    let (cw, ch, w) = (n(m.cell_w), n(m.cell_h), n(on.pixel_size().0));
    // Each row's last drawn column: every row of the output starts at the
    // left edge.
    let mut last = vec![None::<usize>; usize::from(WS.ws_row)];
    for (y, x) in inked(&t) {
        last[y] = Some(last[y].map_or(x, |l: usize| l.max(x)));
    }
    let (cy, cx) = t.cursor();
    last[cy.index()] = Some(last[cy.index()].map_or(cx.index(), |l| l.max(cx.index())));
    let outside = |i: usize| last[(i / w) / ch].is_none_or(|l| (i % w) / cw > l);
    let mut drawn = 0;
    for phase in phases() {
        let look = Some(SplashLook { phase });
        on.set_splash(look);
        on.splash_yields_to(&t, Px::of(0));
        bare.set_splash(look);
        paint(&mut on, &mut t, &mut on_px, Damage::Full);
        paint(&mut bare, &mut t, &mut bare_px, Damage::Full);
        // The ions left out: each that meets a span the shell drew.
        let left_out: Vec<Instance> = bare
            .trail()
            .iter()
            .filter(|r| r.kind == Kind::Particle)
            .filter(|r| {
                let (x0, y0) = (
                    usize::try_from(r.x.0.max(0)).unwrap(),
                    usize::try_from(r.y.0.max(0)).unwrap(),
                );
                let (x1, y1) = (x0 + usize::from(r.w.0), y0 + usize::from(r.h.0));
                (y0..y1).any(|y| (x0..x1).any(|x| y < on_px.len() / w && !outside(y * w + x)))
            })
            .copied()
            .collect();
        let in_ion = |i: usize| {
            let (x, y) = (i % w, i / w);
            left_out.iter().any(|r| {
                let (x0, y0) = (i64::from(r.x.0), i64::from(r.y.0));
                let (x, y) = (i64::try_from(x).unwrap(), i64::try_from(y).unwrap());
                x >= x0 && x < x0 + i64::from(r.w.0) && y >= y0 && y < y0 + i64::from(r.h.0)
            })
        };
        for i in (0..on_px.len()).filter(|&i| outside(i) && !in_ion(i)) {
            assert_eq!(
                on_px[i],
                bare_px[i],
                "{phase:?}: pixel ({}, {}) outside the shell's",
                i % w,
                i / w
            );
        }
        if (0..on_px.len()).any(|i| outside(i) && on_px[i] != off_px[i]) {
            drawn += 1;
        }
    }
    assert!(drawn > 0, "the control: the logo is drawn around them");
}

/// **A slide's leaving row is out from under the logo too**: while a
/// smooth scroll lowers the grid by `lift`, the row it is leaving is drawn
/// on the window's top `lift` pixels, so no record of the splash meets
/// that row's span there. Two rows high, so the logo reaches the top.
/// The control: with no lift, which draws no leaving row, a record does.
#[test]
fn no_record_of_the_splash_meets_a_slides_leaving_row() {
    let (regular, bold) = (
        font("crates/baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("crates/baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    let ws = WinSize {
        ws_col: 40,
        ws_row: 2,
    };
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    // Long lines above the window, and short ones on it: the strip is
    // outside the window's own rows' spans.
    feed(&mut t, b"\x1b[?25l");
    for _ in 0..3 {
        feed(&mut t, "Wgjpqy".repeat(6).as_bytes());
        feed(&mut t, b"\r\n");
    }
    feed(&mut t, b"a\r\nb");
    let mut r = Renderer::new(
        Stack::with_roles(
            &faces,
            &[FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)],
        )
        .unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .unwrap();
    let m = r.metrics();
    let cw = i32::try_from(m.cell_w.value()).unwrap();
    let lift = Px::of(m.cell_h.value() * 3 / 4);
    let strip = Hole {
        left: 2 * cw,
        top: 0,
        right: 36 * cw,
        bottom: i32::try_from(lift.value()).unwrap(),
    };
    let meets = |r: &Renderer<'_, '_>| {
        r.splash()
            .iter()
            .chain(r.trail())
            .filter(|i| strip.meets(i))
            .count()
    };
    let mut reached = 0;
    for phase in phases() {
        r.set_splash(Some(SplashLook { phase }));
        r.splash_yields_to(&t, Px::of(0));
        reached += meets(&r);
        r.splash_yields_to(&t, lift);
        assert_eq!(meets(&r), 0, "{phase:?}: a record over the leaving row");
    }
    assert!(reached > 0, "the control: the logo reaches the top rows");
}
