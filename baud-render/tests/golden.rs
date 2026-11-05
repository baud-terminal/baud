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
/// cat-cjk and vim-unicode moved when a precomposed character the face
/// lacks and cannot draw in parts stayed whole (2026-10-18): each Hangul
/// syllable, and in cat-cjk each kana with a voiced mark, is one `.notdef`
/// in its own cells where its parts' boxes had spilled into the cells after
/// it -- 334 and 77 cells changed, every one a syllable's or such a kana's
/// or just right of one; with the letter taken apart as before both frames
/// are their old pins again. cat-cjk and vim-unicode moved when BDSM came
/// to be set at power-on (2026-10-24), their Hebrew and Arabic rows drawn
/// in the order UAX #9 gives them; with `CSI 8 l` before each recording,
/// all nine frames are their old pins again. They moved again when a
/// paragraph came to take its direction from its first strong letter
/// (UAX #9 P2-P3, 2026-10-24): one row of cat-cjk and eleven of
/// vim-unicode start with a right-to-left letter and are drawn right to
/// left against the right edge; every other row's pixels are the old
/// frame's. cat-cjk and vim-unicode moved when `?2027` became a switch
/// reset at power-on (2026-11-02): a cell holds a legacy grapheme cluster,
/// so the ा and ी of each देवनागरी and the त of each नमस्ते take a cell of
/// their own, and each syllable they part is drawn across its cells. Both
/// moved again when each Devanagari word came to be drawn whole, at its
/// face's natural advances and centred in its columns (2026-11-02). Seven
/// moved when coverage came to be rounded to the nearest level rather
/// than truncated, every changed mask byte by one; with the text-contrast
/// curve held flat, every changed channel moves by one too. cava and
/// less-cjk draw no glyph whose coverage rounds differently. Six moved,
/// and the light git-log, when the cursor became baud's amber, the
/// brand's ground under it on the dark page and the page under it on the
/// light: the cursor's cell alone, since pipes, btop
/// and cava end with it hidden; with the cursor of no colour of its own
/// (`support::classic`) every frame is its old pin again.
const GOLDEN: [(&str, u64); 9] = [
    ("ascii/pipes.raw", 0x80e26dd2d956c001),
    ("utf8/btop.raw", 0x5da146deb01b1ee1),
    ("ascii/cat-scroll.raw", 0x39a9a862fd9581dc),
    ("ascii/git-log.raw", 0x46f79e0ef3644cd2),
    ("ascii/less-pager.raw", 0x423f6f3a2afe3e6e),
    ("utf8/cat-cjk.raw", 0xbdf5a31234c404e5),
    ("utf8/cava.raw", 0x5428b240a6ca62e5),
    ("utf8/less-cjk.raw", 0xdfcbe0bbd153eea5),
    ("utf8/vim-unicode.raw", 0xe61c6a3872ffb5f6),
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
        // alternate screen last, whose restored primary is blank.
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
    const LIGHT_GIT_LOG: u64 = 0xafa79c97e05778b1;
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
