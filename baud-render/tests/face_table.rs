//! **The face table is `Stack::select_presented`, at every codepoint.**
//!
//! `Renderer::face_for` reads a table built once from the stack, so a frame
//! asks two array reads a cell rather than walking the faces. The table is
//! built from each face's `cmap` candidates and a restatement of the
//! selection order (`Stack::choose`); both could drift from the function
//! they replace, so this sweeps all of Unicode, every style and both
//! presentations, against `select_presented` itself.

mod support;

use baud_domain::Codepoint;
use baud_font::{Face, FaceIndex};
use baud_render::{FaceRole, FaceStyle, Presentation, Stack};
use std::time::Instant;

fn embedded(dir: &str, name: &str) -> Vec<u8> {
    let p = format!(
        "{}/../baud-app/fonts/{dir}/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

/// Every codepoint, style and presentation agrees, and the sweep reaches
/// more than one face -- a table that answered the primary everywhere would
/// agree wherever the stack does too.
fn sweep(name: &str, stack: &Stack<'_, '_>) {
    let t0 = Instant::now();
    let mut table = stack.table();
    let built = t0.elapsed();
    let mut seen = std::collections::BTreeSet::new();
    for v in 0..=0x10_FFFFu32 {
        let Some(c) = Codepoint::new(v) else { continue };
        for style in FaceStyle::ALL {
            for p in [Presentation::Text, Presentation::Colour] {
                let want = stack.select_presented(c, style, p);
                let got = table.face(stack, c, style, p);
                assert_eq!(got, want, "{name}: U+{v:04X} {style:?} {p:?}");
                seen.insert(got.index());
            }
        }
    }
    eprintln!(
        "{name}: made in {:.2} ms; {table}; faces reached {seen:?}",
        built.as_secs_f64() * 1e3
    );
    assert!(
        seen.len() > 2,
        "{name}: control: the table reaches several faces"
    );
}

#[test]
fn the_table_is_select_presented_on_the_bundled_stack() {
    let r = embedded("jetbrains-mono", "JetBrainsMono-Regular.ttf");
    let b = embedded("jetbrains-mono", "JetBrainsMono-Bold.ttf");
    let i = embedded("jetbrains-mono", "JetBrainsMono-Italic.ttf");
    let bi = embedded("jetbrains-mono", "JetBrainsMono-BoldItalic.ttf");
    let nerd = embedded("symbols-nerd-font", "SymbolsNerdFontMono-Regular.ttf");
    let emoji = support::font("NotoColorEmoji-subset.ttf");
    let faces = [
        Face::parse(&r).unwrap(),
        Face::parse(&nerd).unwrap(),
        Face::parse(&emoji).unwrap(),
        Face::parse(&b).unwrap(),
        Face::parse(&i).unwrap(),
        Face::parse(&bi).unwrap(),
    ];
    let roles = [
        FaceRole::Fallback,
        FaceRole::Fallback,
        FaceRole::Fallback,
        FaceRole::Style(FaceStyle::Bold),
        FaceRole::Style(FaceStyle::Italic),
        FaceRole::Style(FaceStyle::BoldItalic),
    ];
    sweep("bundled", &Stack::with_roles(&faces, &roles).unwrap());
}

#[test]
fn the_table_is_select_presented_on_the_macos_stack() {
    if !cfg!(target_os = "macos") {
        eprintln!("skipped: the faces are macOS's");
        return;
    }
    let r = embedded("jetbrains-mono", "JetBrainsMono-Regular.ttf");
    let b = embedded("jetbrains-mono", "JetBrainsMono-Bold.ttf");
    let nerd = embedded("symbols-nerd-font", "SymbolsNerdFontMono-Regular.ttf");
    let sys = |p: &str| std::fs::read(format!("/System/Library/Fonts/{p}")).unwrap();
    let (menlo, emoji, symbols, hira, sdg) = (
        sys("Menlo.ttc"),
        sys("Apple Color Emoji.ttc"),
        sys("Apple Symbols.ttf"),
        sys("Hiragino Sans GB.ttc"),
        sys("AppleSDGothicNeo.ttc"),
    );
    let at = |d: &'static [u8]| Face::parse_index(d, FaceIndex::of(0)).unwrap();
    let leak = |v: Vec<u8>| -> &'static [u8] { Box::leak(v.into_boxed_slice()) };
    let faces = [
        at(leak(r)),
        at(leak(nerd)),
        at(leak(menlo)),
        at(leak(emoji)),
        at(leak(symbols)),
        at(leak(hira)),
        at(leak(sdg)),
        at(leak(b)),
    ];
    let mut roles = vec![FaceRole::Fallback; 7];
    roles.push(FaceRole::Style(FaceStyle::Bold));
    sweep("macOS", &Stack::with_roles(&faces, &roles).unwrap());
}
