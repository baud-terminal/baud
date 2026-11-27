//! **Every check the brand run makes fires**: each is held to the
//! committed files, then to a perturbed copy that must fail it, with the
//! message the run stops with. None needs the release archive, the
//! network or the plasma example: each check is a function of what it
//! reads.

use baud_pictures::brand::{self, Fonts, Type};
use baud_pictures::json;
use baud_pictures::png::{self, Image, Mode};
use baud_pictures::raster;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(root().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn text(name: &str) -> String {
    String::from_utf8(read(name)).expect("utf-8")
}

/// The committed 400 and 700, which every mark and the tagline are drawn
/// in; the 500 lives only in the release archive.
fn committed_fonts() -> Fonts {
    Fonts {
        faces: brand::COMMITTED
            .iter()
            .map(|(w, n, _, _)| (*w, read(&format!("{}/{n}", brand::JBM))))
            .collect(),
    }
}

fn must_fail<T: std::fmt::Debug>(r: Result<T, String>, want: &str) {
    match r {
        Ok(v) => panic!("the check passed {v:?}; it should have said {want:?}"),
        Err(msg) => assert!(msg.contains(want), "{msg:?} does not say {want:?}"),
    }
}

#[test]
fn the_readme_tagline_check_fires() {
    let readme = text("README.md");
    brand::tagline_holds(&readme).expect("the committed README");
    let moved = readme.replacen("its own font,", "its own fonts,", 1);
    must_fail(
        brand::tagline_holds(&moved),
        "README.md line 10 is not the card's tagline: '**The terminal",
    );
}

#[test]
fn the_font_pins_fire() {
    let a = (
        "JetBrainsMono-2.304.zip".to_string(),
        b"not the archive".to_vec(),
    );
    let mut copy = std::env::temp_dir().join(format!("baud-pictures-pins-{}", std::process::id()));
    copy.push(brand::JBM);
    std::fs::create_dir_all(&copy).expect("a temp tree");
    for (_, n, _, _) in brand::COMMITTED {
        std::fs::write(copy.join(n), read(&format!("{}/{n}", brand::JBM))).expect("copy a face");
    }
    let tree = copy
        .ancestors()
        .nth(3)
        .expect("the copy's root")
        .to_path_buf();
    // The committed faces pass, and the archive check is the next refusal.
    must_fail(brand::fonts(&tree, &a), "is not the release archive pinned");
    let mut bold = read(&format!("{}/JetBrainsMono-Bold.ttf", brand::JBM));
    bold[1000] ^= 1;
    std::fs::write(copy.join("JetBrainsMono-Bold.ttf"), bold).expect("perturb");
    must_fail(
        brand::fonts(&tree, &a),
        "crates/baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf is not the file SOURCE pins",
    );
    let _ = std::fs::remove_dir_all(tree);
    must_fail(
        brand::medium_holds(&read(&format!("{}/JetBrainsMono-Regular.ttf", brand::JBM))),
        "fonts/ttf/JetBrainsMono-Medium.ttf in the archive is not the face pinned",
    );
}

#[test]
fn a_character_the_face_lacks_is_refused() {
    let f = committed_fonts();
    let t = Type::new(&f).expect("the faces read");
    t.text("❯", 0.0, 0.0, 200.0, 700, 0.0)
        .expect("the prompt glyph");
    must_fail(
        t.text("❯\u{10FFFD}", 0.0, 0.0, 200.0, 700, 0.0),
        "U+10FFFD is not in JetBrains Mono at 700",
    );
}

/// **The marks are drawn as the committed files draw them**, from the
/// committed 700 alone: the 512 and 32 marks and icons and the Dock.
#[test]
fn the_marks_are_the_committed_files() {
    let f = committed_fonts();
    let t = Type::new(&f).expect("the faces read");
    for (name, fill) in [
        ("dark", brand::AMBER),
        ("light", brand::AMBER),
        ("amber", brand::AMBER),
    ] {
        let m = brand::centred(&t, 256.0, 172.0, 168.0, fill).expect("drawn");
        let want = text(&format!("docs/assets/logo/baud-mark-{name}.svg"));
        assert_eq!(
            brand::svg("512", "512", &m.body, "baud", None),
            want,
            "{name}"
        );
    }
    let small = brand::centred(&t, 16.0, 10.75, 10.5, brand::AMBER).expect("drawn");
    let want = text("docs/assets/logo/baud-icon-32-dark.svg");
    let body = brand::square(32, 7, brand::GROUND) + &small.body;
    assert_eq!(brand::svg("32", "32", &body, "baud", None), want);
}

/// **The run refuses a lockup whose block's top is not on the word's top**:
/// the two decimals each is written to pass, the 8.26 units of the title
/// bar's lockup before they were lined up do not.
#[test]
fn the_lined_up_check_fires() {
    brand::lined_up(156.0939, 156.09).expect("two decimals' rounding");
    must_fail(
        brand::lined_up(156.09, 164.35),
        "the block's top is at 164.35, not on the word's top at 156.09",
    );
    must_fail(
        brand::lined_up(42.74, 42.76),
        "the block's top is at 42.76, not on the word's top at 42.74",
    );
}

/// **The tagline's paths are the card's**, at 400, where `i` is a
/// composite of `dotlessi` and an offset `uni0307`: its points go through
/// the composed transform, not the offset and then the pen's.
#[test]
fn the_tagline_is_the_cards_composite_glyphs_and_all() {
    let f = committed_fonts();
    let t = Type::new(&f).expect("the faces read");
    let card = text("docs/assets/readme/social.svg");
    let mid = (616.0 - (5.0 * 21.0 + 40.0)) / 2.0;
    for (i, line) in brand::LINES.iter().enumerate() {
        let y = mid + (i as f64 - 1.5) * 56.0 + 0.73 * 38.0 / 2.0;
        let (d, _) = t.text(line, 560.0, y, 38.0, 400, 0.0).expect("drawn");
        assert!(card.contains(&brand::path(&d, brand::TYPE_DARK)), "{line}");
    }
}

#[test]
fn the_organisation_clear_space_check_fires() {
    brand::clear_space([100.0, 100.0, 400.0, 400.0], 500.0, 40.0).expect("clear");
    must_fail(
        brand::clear_space([30.0, 100.0, 400.0, 400.0], 500.0, 40.0),
        "the organisation picture's logo has 30.0 px to an edge, under a cursor width (40.0)",
    );
}

#[test]
fn the_card_space_checks_fire() {
    let (l, t) = ([60.0, 180.0, 500.0, 290.0], [560.0, 160.0, 1200.0, 310.0]);
    brand::card_space(l, t, 30.0, 471.0, 1280.0).expect("clear");
    must_fail(
        brand::card_space(l, [520.0, 160.0, 1200.0, 310.0], 30.0, 471.0, 1280.0),
        "inside the lockup's clear space of another element",
    );
    must_fail(
        brand::card_space(l, [560.0, 160.0, 1260.0, 310.0], 30.0, 471.0, 1280.0),
        "the tagline reaches the 29 px a 1.91:1 crop of the card would trim",
    );
}

/// The committed card's tape reads `baud baud ...` with a feed hole a
/// column; a code hole taped over does not.
#[test]
fn the_tape_check_reads_the_png() {
    let card = png::decode(&read("docs/assets/readme/social.png")).expect("it reads");
    brand::tape_reads(&card).expect("the committed tape");
    let (spelled, feed) = brand::read_tape(&card);
    assert_eq!((spelled.len(), feed), (44, 44));
    let mut taped = card.clone();
    // The first column's first element: b is ZAAZZ, a hole at (38, 491).
    for y in 484..=498 {
        for x in 31..=45 {
            let i = (y * taped.w + x) * 3;
            taped.px[i..i + 3].copy_from_slice(&[0xF4, 0xA3, 0x2C]);
        }
    }
    must_fail(brand::tape_reads(&taped), "the tape reads '?aud baud");
}

#[test]
fn a_pixel_short_of_opaque_is_refused() {
    let mut im = raster::draw(&text("docs/assets/logo/baud-github-org.svg"), None).expect("drawn");
    brand::opaque(&im, "org").expect("the committed picture is opaque");
    im.px[4 * 1000 + 3] = 254;
    must_fail(
        brand::opaque(&im, "org"),
        "org: resvg drew a pixel that is not opaque",
    );
}

#[test]
fn the_dock_icon_draws_at_each_iconset_size() {
    let dock = text("docs/assets/logo/baud-icon-dock.svg");
    for n in brand::DOCK_SIZES {
        let im = raster::draw(&dock, Some(n)).expect("drawn");
        assert_eq!((im.w, im.h, im.mode), (n as usize, n as usize, Mode::Rgba));
        let committed = read(&format!("docs/assets/logo/baud-icon-dock-{n}.png"));
        assert_eq!(png::encode(&im), committed, "{n}");
    }
}

#[test]
fn the_github_limits_fire() {
    let org = read("docs/assets/logo/baud-github-org.png");
    let card = read("docs/assets/readme/social.png");
    assert!(brand::github_limits("org.png", &org, true).is_empty());
    assert!(brand::github_limits("social.png", &card, false).is_empty());
    assert_eq!(
        brand::github_limits("x.png", b"GIF89a", true),
        ["x.png: not a PNG"]
    );
    // A text chunk before IEND.
    let mut extra = org[..org.len() - 12].to_vec();
    extra.extend([0, 0, 0, 0]);
    extra.extend(b"tEXt");
    extra.extend([0, 0, 0, 0]);
    extra.extend(&org[org.len() - 12..]);
    assert_eq!(
        brand::github_limits("org.png", &extra, true),
        ["org.png: chunks ['IDAT', 'IEND', 'IHDR', 'tEXt'], not IHDR, IDAT and IEND alone"]
    );
    // The card as the organisation picture, and the other way round.
    assert_eq!(
        brand::github_limits("org.png", &card, true),
        ["org.png: 1280x640, not the 500 square GitHub recommends"]
    );
    assert_eq!(
        brand::github_limits("social.png", &org, false),
        ["social.png: 500x500, not the 1280 by 640 GitHub recommends"]
    );
    // An RGBA picture: the Dock's at 512, as the organisation picture.
    let dock = read("docs/assets/logo/baud-icon-dock-512.png");
    assert_eq!(
        brand::github_limits("org.png", &dock, true),
        [
            "org.png: 512x512, not the 500 square GitHub recommends",
            "org.png: bit depth 8, colour type 6: not 8-bit RGB, which has no alpha"
        ]
    );
    // A megabyte of noise.
    let mut seed = 7u64;
    let px = (0..800 * 500 * 3)
        .map(|_| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            (seed >> 56) as u8
        })
        .collect();
    let big = png::encode(&Image {
        w: 800,
        h: 500,
        mode: Mode::Rgb,
        px,
    });
    let bad = brand::github_limits("org.png", &big, true);
    assert_eq!(
        bad[0],
        format!("org.png: {} bytes, not under 1 MB", big.len())
    );
    // 3000 by 3000 or more.
    let huge = png::encode(&Image {
        w: 3000,
        h: 1,
        mode: Mode::Rgb,
        px: vec![0; 9000],
    });
    assert!(
        brand::github_limits("org.png", &huge, true)
            .contains(&"org.png: 3000x1, not smaller than 3000 by 3000".to_string())
    );
}

#[test]
fn every_committed_svg_is_outlines_and_names_no_font() {
    let mut n = 0;
    for e in std::fs::read_dir(root().join("docs/assets/logo")).expect("the logo folder") {
        let name = e
            .expect("an entry")
            .file_name()
            .to_string_lossy()
            .into_owned();
        if name.ends_with(".svg") {
            brand::outlines_only(&name, &text(&format!("docs/assets/logo/{name}")))
                .expect("outlines");
            n += 1;
        }
    }
    assert_eq!(n, 23, "every SVG the run draws but the card");
    brand::outlines_only("social.svg", &text("docs/assets/readme/social.svg")).expect("outlines");
}

#[test]
fn the_outline_checks_fire() {
    let good = text("docs/assets/logo/baud-mark-dark.svg");
    let text_el = good.replace("</svg>", "  <text>baud</text>\n</svg>");
    must_fail(
        brand::outlines_only("m.svg", &text_el),
        "m.svg: elements ['path', 'rect', 'svg', 'text']: an outline source holds paths and shapes alone",
    );
    let no_path = good.replace("<path d=", "<rect d=");
    must_fail(
        brand::outlines_only("m.svg", &no_path),
        "m.svg: elements ['rect', 'svg']",
    );
    for named in [
        good.replace(
            "fill=\"#F4A32C\"/>",
            "font-family=\"JetBrains Mono\" fill=\"#F4A32C\"/>",
        ),
        good.replace(
            "fill=\"#F4A32C\"/>",
            "style=\"FONT : 12px x\" fill=\"#F4A32C\"/>",
        ),
        good.replace("fill=\"#F4A32C\"/>", "font\u{1c}=\"x\" fill=\"#F4A32C\"/>"),
    ] {
        must_fail(brand::outlines_only("m.svg", &named), "m.svg: names a font");
    }
    // Inside a comment, neither counts.
    let commented = good.replace("</svg>", "  <!-- <text font-family=\"x\"> -->\n</svg>");
    brand::outlines_only("m.svg", &commented).expect("a comment is not the drawing");
}

/// The plasma report as the example writes it on the base, and each
/// check it is held to, perturbed.
#[test]
fn the_plasma_report_checks_fire() {
    let rep = |pen: &str, hole: &str, cell: &str, dark: &str, light: &str| {
        json::parse(&format!(
            "{{\"amber\": {{\"dark\": \"{dark}\", \"light\": \"{light}\"}},\n \"word\": {{\"width\": 2040, \"height\": 1480, \"px\": 28, \"seed\": \"BA0D276F\", \"t\": 0.0999, \"pen\": {pen}, \"box\": [50.362, 45, 187.962, 91], \"hole\": {hole}, \"scale\": 2.580645, \"origin\": [-61.127743, -0.14193548], \"gate_columns\": 364, \"gate_differing\": 0}},\n \"mark\": {{\"width\": 600, \"height\": 792, \"px\": 200, \"cell_w\": {cell}, \"cell_h\": 264, \"baseline\": 204, \"row\": 1, \"col\": 1, \"seed\": 3, \"age\": 0.12}}}}\n"
        ))
        .expect("the report parses")
    };
    let (pen, hole) = ("[50.362, 88]", "[223.27109, 45, 244.18018, 91]");
    let (x0, cur) = (50.362_15, [223.27, 45.0, 244.18, 91.0]);
    brand::plasma_holds(&rep(pen, hole, "120", "#F4A32C", "#F4A32C"), x0, cur)
        .expect("the base's report");
    must_fail(
        brand::plasma_holds(
            &rep("[50.37, 88]", hole, "120", "#F4A32C", "#F4A32C"),
            x0,
            cur,
        ),
        "the splash's word pen is [50.37, 88], not the lockup's (50.362, 88.0)",
    );
    must_fail(
        brand::plasma_holds(
            &rep(
                pen,
                "[223.3, 45, 244.18018, 91]",
                "120",
                "#F4A32C",
                "#F4A32C",
            ),
            x0,
            cur,
        ),
        "the splash's block is [223.3, 45, 244.18018, 91], not the lockup's cursor (223.27, 45.0, 244.18, 91.0)",
    );
    must_fail(
        brand::plasma_holds(&rep(pen, hole, "121", "#F4A32C", "#F4A32C"), x0, cur),
        "baud's renderer draws a 121 by 264 cell, baseline 204, at 200 px",
    );
    must_fail(
        brand::plasma_holds(&rep(pen, hole, "120", "#F4A32D", "#F4A32C"), x0, cur),
        "baud's cursor on the dark page is #F4A32D, not the brand's #F4A32C",
    );
    must_fail(
        brand::plasma_holds(&rep(pen, hole, "120", "#F4A32C", "#986100"), x0, cur),
        "baud's cursor on the light page is #986100, not the brand's #F4A32C",
    );
}

#[test]
fn the_light_control_checks_fire() {
    let base = Image {
        w: 2,
        h: 1,
        mode: Mode::Rgb,
        px: vec![1, 2, 3, 4, 5, 6],
    };
    brand::lights_hold(&base, &base, 0, 5).expect("the control holds");
    let mut moved = base.clone();
    moved.px[0] = 9;
    must_fail(
        brand::lights_hold(&base, &moved, 1, 5),
        "with no light the organisation picture is not its SVG's drawing: 1 pixels differ",
    );
    must_fail(
        brand::lights_hold(&base, &base, 0, 0),
        "the plasma lit no pixel of the organisation picture",
    );
}

/// With every frame the bare ground, the blend gives the SVG's drawing
/// back, every pixel: the control the run makes, made here on the
/// committed picture's SVG.
#[test]
fn no_light_is_the_svgs_drawing() {
    let svg = text("docs/assets/logo/baud-github-org.svg");
    let base = brand::opaque(&raster::draw(&svg, None).expect("drawn"), "org").expect("opaque");
    let f = brand::Frames {
        mark: (600, 792),
        word: (2040, 1480),
        col: 1.0,
        row: 1.0,
    };
    let dir = std::env::temp_dir().join(format!("baud-pictures-dark-{}", std::process::id()));
    brand::dark_frames(&dir, &f).expect("written");
    let cells = [174.0, 268.0, 199.0, 224.0, 330.0];
    let (zero, _, moved) = brand::org_picture(&base, &f, &dir, cells).expect("blended");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(moved, 0);
    assert_eq!(zero, base);
}

#[test]
fn a_logo_file_the_run_does_not_draw_is_named() {
    let out: brand::Files = vec![("docs/assets/logo/a.svg".into(), Vec::new())];
    let there = vec![
        "docs/assets/logo/a.svg".to_string(),
        "docs/assets/logo/b.png".to_string(),
    ];
    assert_eq!(
        brand::undrawn(&there, &out),
        ["docs/assets/logo/b.png is a logo file the generator does not draw"]
    );
    let all = brand::there(&root());
    assert!(all.contains(&"docs/assets/readme/social.png".to_string()));
    assert_eq!(
        all.iter()
            .filter(|n| n.starts_with("docs/assets/logo/"))
            .count(),
        31
    );
}

/// **`convert("RGB")` drops the alpha and composites nothing**: Pillow
/// 12.3.0 gives `[10, 20, 30, 200, 100, 50]` for these two pixels, alpha
/// 128 and 0, where compositing over black would darken both.
#[test]
fn rgb_drops_the_alpha_and_composites_nothing() {
    let im = Image {
        w: 2,
        h: 1,
        mode: Mode::Rgba,
        px: vec![10, 20, 30, 128, 200, 100, 50, 0],
    };
    assert_eq!(brand::rgb_of(&im).px, [10, 20, 30, 200, 100, 50]);
}
