//! The final frame of every recording, pinned as a digest.
//!
//! Each recording is replayed through the core in `read(2)`-sized chunks
//! and the last frame painted at 14 px with JetBrains Mono; the FNV-1a of
//! the pixels is pinned here. A change to shaping, rasterising, colour
//! resolution or compositing moves a digest and has to say so. The digests
//! were taken on the first run and are not an oracle; the oracle for each
//! stage is its own crate's differential.

mod support;

use baud_font::Face;
use baud_render::Damage;
use support::*;

/// Recordings whose last act is leaving the alternate screen.
const BLANK_AT_EXIT: [&str; 1] = ["utf8/less-cjk.raw"];

/// Writes the frame as a binary PPM, for looking at rather than testing.
fn dump_ppm(c: &Canvas, path: &str) {
    let mut out = format!("P6\n{} {}\n255\n", c.width, c.height).into_bytes();
    for &p in &c.pixels {
        out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
    }
    std::fs::write(path, out).unwrap();
}

/// `(recording, digest)`, regenerate with `GOLDEN_PRINT=1`. btop and
/// cava carry box drawing and braille, drawn by `baud_render::boxes`
/// rather than by JetBrains Mono, so their digests are baud's own masks:
/// btop's graph braille was `.notdef` boxes in the font and is now dots.
/// Every outline's coverage is bent by its colour's text-contrast curve
/// (`baud_domain::contrast`), which moved every pin with outline text in
/// it; cava's frame is all procedural blocks at full coverage, where every
/// curve is the identity, and did not move. cat-cjk and vim-unicode moved
/// when a cluster's marks that no face draws stopped drawing as `.notdef`
/// boxes (`Renderer::marks_unmapped`): with JetBrains Mono alone, in
/// cat-cjk the joined emoji of a ZWJ sequence and the vowel signs of 18
/// Devanagari letters, and in vim-unicode the Hebrew points it lacks, a
/// flag's second regional indicator and a Devanagari conjunct's tail. 61 and
/// 77 cells changed, as many as codepoints were left out, every one in or just
/// right of a cell whose marks were; with the drop disabled both frames are
/// their old pins again. vim-unicode moved again when a cursor move began
/// ending a grapheme cluster: vim writes each regional indicator of the JP
/// flag at a position of its own, so the eleven on its final screen draw as
/// the two cells vim addressed rather than as one flag beside a gap
/// (`baud-core/tests/golden/recordings.txt`). cat-cjk moved again when the
/// cluster store grew to 1024 slots: two wide kana on its final screen,
/// U+30C5 and U+30C7, keep the U+3099 the 63-slot store refused them, and
/// all 96 pixels that changed are in their tails, where JetBrains Mono
/// draws the mark; no other cell's base or marks moved. asciiquarium and
/// vim-unicode moved when four of the dark sixteen were made readable on
/// the page (2026-10-18), both drawing cells in those four; with xterm's
/// sixteen back under the dark theme both frames are their old pins again.
const GOLDEN: [(&str, u64); 9] = [
    ("ascii/asciiquarium.raw", 0x02759005377d677c),
    ("utf8/btop.raw", 0x378fe94215f0d9b5),
    ("ascii/cat-scroll.raw", 0x1a613c1d646560b1),
    ("ascii/git-log.raw", 0x2303bd10c2107db7),
    ("ascii/less-pager.raw", 0x41e082191ab41018),
    ("utf8/cat-cjk.raw", 0x253f57b6a135417d),
    ("utf8/cava.raw", 0x5428b240a6ca62e5),
    ("utf8/less-cjk.raw", 0x08602238a4813a25),
    ("utf8/vim-unicode.raw", 0x0be499a38a8a8396),
];

#[test]
fn every_recording_paints_to_its_pinned_frame() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let print = std::env::var("GOLDEN_PRINT").is_ok();
    let mut seen = 0;
    for &(name, want) in &GOLDEN {
        let bytes = recording(name);
        let mut r = renderer(&faces);
        let mut canvas = Canvas::for_renderer(&r);
        // One full paint of the final screen; the per-read frames are the
        // incremental test's business.
        let mut frame = None;
        replay_final(&bytes, |t| {
            frame = Some(paint(&mut r, t, &mut canvas, Damage::Full))
        });
        let frame = frame.unwrap();
        let got = canvas.digest();
        // Positive controls: something was drawn, and not everywhere the
        // same colour -- except the one recording that leaves the
        // alternate screen last, whose restored primary is blank, as
        // CLAUDE.md records of it.
        if BLANK_AT_EXIT.contains(&name) {
            assert_eq!(frame.instances.value(), 0, "{name}");
        } else {
            assert!(
                frame.instances.value() > 100,
                "{name}: {:?} instances",
                frame.instances
            );
            let first = canvas.pixels[0];
            assert!(
                canvas.pixels.iter().any(|&p| p != first),
                "{name}: one colour"
            );
        }
        if let Ok(dir) = std::env::var("GOLDEN_DUMP") {
            dump_ppm(&canvas, &format!("{dir}/{}.ppm", name.replace('/', "-")));
        }
        if print {
            println!("    (\"{name}\", 0x{got:016x}),");
        } else {
            assert_eq!(got, want, "{name}: frame digest 0x{got:016x}");
        }
        seen += 1;
    }
    assert_eq!(seen, RECORDINGS.len());
}

/// **One recording under the light theme**, whose sixteen are Modus Operandi
/// (note Y, decision 3). git-log colours its hashes and refs from the
/// sixteen, so a change to the light table moves this digest; the same frame
/// under xterm's sixteen on the light page is a different digest, the control
/// that the pin is the table's.
#[test]
fn git_log_paints_to_its_pinned_light_frame() {
    const LIGHT_GIT_LOG: u64 = 0x8ccf93fcb310b1da;
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let bytes = recording("ascii/git-log.raw");
    let digest = |palette: Option<[baud_render::Argb; 256]>| {
        let mut r = renderer(&faces);
        let mut theme = baud_render::Theme::light();
        if let Some(p) = palette {
            theme.palette = p;
        }
        r.set_theme(theme);
        let mut canvas = Canvas::for_renderer(&r);
        replay_final(&bytes, |t| {
            paint(&mut r, t, &mut canvas, Damage::Full);
        });
        canvas.digest()
    };
    let light = digest(None);
    let xterm = digest(Some(baud_render::xterm_palette()));
    if std::env::var("GOLDEN_PRINT").is_ok() {
        eprintln!("light git-log: {light:#018x}");
    }
    assert_ne!(light, xterm, "the control: the sixteen reach the pixels");
    assert_eq!(light, LIGHT_GIT_LOG, "light git-log frame moved");
}
