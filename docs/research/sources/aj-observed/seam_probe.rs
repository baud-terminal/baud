//! The band seam's probe (note AJ section 21): the splash's plasma at
//! every 60 Hz frame, the pixels two band records cover, the luminance
//! step there, the flash census, and whole frames for `seam_strip.py`.
//! Copied into `baud-bench/tests/` and run with `--ignored` on the base
//! with `SEAM_ARM=before`, then on the change with `SEAM_ARM=after`, both
//! with `SEAM_DIR` naming one directory: the change's run reads the base's
//! masks and hashes from it.

use baud_bench::flash::{Census, Rate, Scale};
use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{
    Argb, Blend, Damage, FaceRole, FaceStyle, Kind, PxSize, Renderer, SplashLook, SplashPhase,
    Stack, Target, Theme,
};
use std::io::Write;

fn font(p: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn arm() -> String {
    std::env::var("SEAM_ARM").unwrap()
}
fn dir() -> String {
    std::env::var("SEAM_DIR").unwrap()
}

struct Rig {
    r: Renderer<'static, 'static>,
    t: Term<'static>,
    px: Vec<u32>,
    w: Px,
    h: Px,
}

fn rig(cols: u16, rows: u16, size: f32, theme: Theme) -> Rig {
    let regular: &'static [u8] =
        Box::leak(font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf").into_boxed_slice());
    let bold: &'static [u8] =
        Box::leak(font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf").into_boxed_slice());
    let faces: &'static [Face<'static>] = Box::leak(Box::new([Face::parse(regular).unwrap(), Face::parse(bold).unwrap()]));
    let r = Renderer::new(
        Stack::with_roles(
            faces,
            &[FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)],
        )
        .unwrap(),
        PxSize::of(size).unwrap(),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(cols)),
        Rows::of_index(usize::from(rows)),
    )
    .unwrap();
    let ws = WinSize {
        ws_col: cols,
        ws_row: rows,
    };
    let owned: &'static mut Owned = Box::leak(Box::new(Owned::new(&ws)));
    let mut t = Term::new(owned.storage(), &ws);
    let hide = b"\x1b[?25l";
    t.write_at(BytePos::ZERO, hide);
    t.feed(BytePos::ZERO, ReadLen::from_index(hide.len()));
    let (w, h) = r.pixel_size();
    let px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    Rig { r, t, px, w, h }
}

impl Rig {
    fn draw(&mut self, phase: Option<SplashPhase>) {
        self.r.set_splash(phase.map(|phase| SplashLook { phase }));
        let mut target = Target::new(&mut self.px, self.w, self.w, self.h).unwrap();
        self.r.render(&mut self.t, &mut target, Damage::Full);
    }
    /// How many band records cover each pixel.
    fn cover(&self) -> Vec<u8> {
        let (w, h) = (self.w.value() as i32, self.h.value() as i32);
        let mut c = vec![0u8; (w * h) as usize];
        for r in self.r.instances().filter(|r| r.kind == Kind::Band) {
            let (x0, y0) = (i32::from(r.x.0).max(0), i32::from(r.y.0).max(0));
            let x1 = (i32::from(r.x.0) + i32::from(r.w.0)).min(w);
            let y1 = (i32::from(r.y.0) + i32::from(r.h.0)).min(h);
            for y in y0..y1 {
                for x in x0..x1 {
                    c[(y * w + x) as usize] += 1;
                }
            }
        }
        c
    }
}

fn lum(p: u32) -> f64 {
    let c = |v: u32| {
        let s = f64::from(v & 255) / 255.0;
        if s <= 0.040_45 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * c(p >> 16) + 0.7152 * c(p >> 8) + 0.0722 * c(p)
}

fn ages(rate: u32) -> Vec<f32> {
    (0u32..)
        .map(|f| f as f32 / rate as f32)
        .take_while(|&a| a < baud_render::splash::GONE)
        .collect()
}

fn fnv(px: &[u32], skip: &std::collections::HashSet<usize>) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for (i, &w) in px.iter().enumerate() {
        let w = if skip.contains(&i) { 0 } else { w };
        for b in w.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    }
    h
}

/// The seam: per config, every frame, doubled pixels and the luminance
/// step at them; the base's mask and its masked hash, compared by the fix.
#[test]
#[ignore]
fn seam() {
    let out = format!("{}/seam-{}.txt", dir(), arm());
    let mut log = std::fs::File::create(&out).unwrap();
    for (name, cols, rows, size, theme) in [
        ("1x-dark", 64u16, 20u16, 14.0f32, Theme::dark()),
        ("1x-light", 64, 20, 14.0, Theme::light()),
        ("2x-dark", 200, 50, 28.0, Theme::dark()),
        ("2x-light", 200, 50, 28.0, Theme::light()),
    ] {
        let mut rg = rig(cols, rows, size, theme);
        let w = rg.w.value() as usize;
        let (mut doubled_px, mut frames_doubled) = (0usize, 0usize);
        let (mut step_max, mut step_sum, mut step_n) = (0.0f64, 0.0f64, 0usize);
        let (mut ctl_sum, mut ctl_n) = (0.0f64, 0usize);
        let mut maskfail = 0usize;
        let (mut dl_max, mut dl_sum, mut dl_n, mut dl_up) = (0.0f64, 0.0f64, 0usize, 0usize);
        for (k, &age) in ages(60).iter().enumerate() {
            rg.draw(Some(SplashPhase::Plasma(age)));
            let c = rg.cover();
            let maskfile = format!("{}/mask-{name}-{k}.txt", dir());
            let mask: Vec<usize> = if arm() == "before" {
                let m: Vec<usize> = (0..c.len()).filter(|&i| c[i] >= 2).collect();
                std::fs::write(
                    &maskfile,
                    m.iter().map(|i| i.to_string()).collect::<Vec<_>>().join("\n"),
                )
                .unwrap();
                m
            } else {
                let s = std::fs::read_to_string(&maskfile).unwrap();
                s.lines().filter(|l| !l.is_empty()).map(|l| l.parse().unwrap()).collect()
            };
            let now_doubled = c.iter().filter(|&&v| v >= 2).count();
            doubled_px += now_doubled;
            frames_doubled += usize::from(now_doubled > 0);
            let set: std::collections::HashSet<usize> = mask.iter().copied().collect();
            let hfile = format!("{}/hash-{name}-{k}.txt", dir());
            let hv = fnv(&rg.px, &set);
            if arm() == "before" {
                std::fs::write(&hfile, hv.to_string()).unwrap();
            } else {
                let want: u64 = std::fs::read_to_string(&hfile).unwrap().trim().parse().unwrap();
                maskfail += usize::from(want != hv);
            }
            let vfile = format!("{}/vals-{name}-{k}.txt", dir());
            if arm() == "before" {
                std::fs::write(&vfile, mask.iter().map(|&i| rg.px[i].to_string()).collect::<Vec<_>>().join("\n")).unwrap();
            } else {
                let s = std::fs::read_to_string(&vfile).unwrap();
                for (l, &i) in s.lines().filter(|l| !l.is_empty()).zip(&mask) {
                    let b: u32 = l.parse().unwrap();
                    let d = lum(b) - lum(rg.px[i]);
                    dl_max = dl_max.max(d.abs());
                    dl_sum += d.abs();
                    dl_n += 1;
                    dl_up += usize::from(d > 0.0);
                }
            }
            // The step at the base's doubled pixels: the pixel against the
            // mean of the one above and the one below.
            for &i in &mask {
                if i < w || i + w >= rg.px.len() {
                    continue;
                }
                let d = lum(rg.px[i]) - 0.5 * (lum(rg.px[i - w]) + lum(rg.px[i + w]));
                step_max = step_max.max(d.abs());
                step_sum += d.abs();
                step_n += 1;
            }
            // The control: the same second difference at every band pixel
            // covered once, one row above each doubled pixel's neighbour.
            for &i in &mask {
                let j = i.wrapping_sub(3 * w);
                if j >= w && j + w < rg.px.len() && c[j] == 1 {
                    let d = lum(rg.px[j]) - 0.5 * (lum(rg.px[j - w]) + lum(rg.px[j + w]));
                    ctl_sum += d.abs();
                    ctl_n += 1;
                }
            }
        }
        writeln!(
            log,
            "{name} ({}x{} px) {}: doubled pixel-frames {doubled_px} in {frames_doubled} of {} frames; |step| at the base's doubled pixels mean {:.5} max {:.5} over {step_n}; control 3 rows up mean {:.5} over {ctl_n}; masked-hash mismatches {maskfail}; base minus fix at the base's doubled pixels mean {:.5} max {:.5} over {dl_n}, brighter in the base at {dl_up}",
            rg.w.value(),
            rg.h.value(),
            arm(),
            ages(60).len(),
            step_sum / step_n.max(1) as f64,
            step_max,
            ctl_sum / ctl_n.max(1) as f64,
            dl_sum / dl_n.max(1) as f64,
            dl_max,
        )
        .unwrap();
        // Strip: the frame at 0.05 s, the doubled row with the most pixels,
        // cropped and magnified.
        let age = 0.05f32;
        rg.draw(Some(SplashPhase::Plasma(age)));
        let k = ages(60).iter().position(|&a| a >= age).unwrap();
        let s = std::fs::read_to_string(format!("{}/mask-{name}-{k}.txt", dir())).unwrap();
        let mask: Vec<usize> = s.lines().filter(|l| !l.is_empty()).map(|l| l.parse().unwrap()).collect();
        let _ = k;
        let mut rows: std::collections::BTreeMap<usize, (usize, usize, usize)> = Default::default();
        for &i in &mask {
            let e = rows.entry(i / w).or_insert((0, usize::MAX, 0));
            e.0 += 1;
            e.1 = e.1.min(i % w);
            e.2 = e.2.max(i % w);
        }
        let (&y, &(_, x0, x1)) = rows.iter().max_by_key(|(_, v)| v.0).unwrap();
        let (cw, ch) = (((x1 - x0 + 1).min(240)).max(60), 24usize);
        let cx = x0 + (x1 - x0) / 2 - cw / 2;
        let cy = y - ch / 2;
        let mut crop = Vec::with_capacity(cw * ch);
        for yy in cy..cy + ch {
            for xx in cx..cx + cw {
                crop.push(rg.px[yy * w + xx]);
            }
        }
        let f = format!("{}/crop-{name}-{}.txt", dir(), arm());
        std::fs::write(
            &f,
            format!(
                "{cw} {ch} {cx} {cy} {y}\n{}",
                crop.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(" ")
            ),
        )
        .unwrap();
        // The luminance profile down the crop's middle columns.
        let mut prof = String::new();
        for yy in cy..cy + ch {
            let m: f64 = (cx..cx + cw).map(|xx| lum(rg.px[yy * w + xx])).sum::<f64>() / cw as f64;
            prof.push_str(&format!("{yy}{} {m:.5}\n", if yy == y { "*" } else { "" }));
        }
        std::fs::write(format!("{}/profile-{name}-{}.txt", dir(), arm()), prof).unwrap();
    }
}

/// The census, as note AJ section 17 took it.
#[test]
#[ignore]
fn census() {
    let out = format!("{}/census-{}.txt", dir(), arm());
    let mut log = std::fs::File::create(&out).unwrap();
    let red = |bg: u32| Theme {
        bg: Argb(bg),
        ..Theme::dark()
    };
    let only = std::env::var("SEAM_ONLY").ok();
    for (name, cols, rows, size, scale, rate) in [
        ("64x20@1x 60Hz", 64u16, 20u16, 14.0f32, 1u32, 60u32),
        ("200x50@2x 60Hz", 200, 50, 28.0, 2, 60),
        ("200x50@2x 120Hz", 200, 50, 28.0, 2, 120),
        ("360x90@2x 60Hz", 360, 90, 28.0, 2, 60),
    ] {
        if only.as_deref().is_some_and(|o| !name.starts_with(o)) {
            continue;
        }
        for (g, theme) in [
            ("dark", Theme::dark()),
            ("light", Theme::light()),
            ("cd", red(0xFFCD_0000)),
            ("1a", red(0xFF1A_0000)),
        ] {
            let mut rg = rig(cols, rows, size, theme);
            let mut census = Census::new(rg.w, rg.h, Scale(scale), Rate(rate));
            let blank = (rate / 5) as usize;
            let mut phases: Vec<Option<SplashPhase>> = vec![None; blank];
            phases.extend(ages(rate).into_iter().map(|a| Some(SplashPhase::Plasma(a))));
            phases.extend(std::iter::repeat_n(None, blank));
            for p in phases {
                rg.draw(p);
                census.frame(&rg.px);
            }
            let r = census.finish();
            let pct = |a: f64| 100.0 * a / r.threshold.0;
            writeln!(
                log,
                "{name} {g}: general {} red {} red-enc {} area {:.1} strict {:.1} ({:.2}%/{:.1}% of {}) translucent {}",
                r.general.flashes.0,
                r.red.flashes.0,
                r.red_encoded.flashes.0,
                r.area.0,
                r.strict.0,
                pct(r.area.0),
                pct(r.strict.0),
                r.threshold.0,
                r.translucent.value()
            )
            .unwrap();
            log.flush().unwrap();
        }
    }
}

/// Whole frames for the strips: raw little-endian words.
#[test]
#[ignore]
fn frames() {
    for (name, cols, rows, size, theme) in [
        ("1x-dark", 64u16, 20u16, 14.0f32, Theme::dark()),
        ("1x-light", 64, 20, 14.0, Theme::light()),
        ("2x-dark", 200, 50, 28.0, Theme::dark()),
        ("2x-light", 200, 50, 28.0, Theme::light()),
    ] {
        let mut rg = rig(cols, rows, size, theme);
        for (tag, age) in [("in", 0.05f32), ("out", baud_render::splash::OUT + 0.05)] {
            rg.draw(Some(SplashPhase::Plasma(age)));
            let mut b = Vec::with_capacity(rg.px.len() * 4 + 8);
            b.extend_from_slice(&rg.w.value().to_le_bytes());
            b.extend_from_slice(&rg.h.value().to_le_bytes());
            for p in &rg.px {
                b.extend_from_slice(&p.to_le_bytes());
            }
            std::fs::write(format!("{}/frame-{name}-{tag}-{}.raw", dir(), arm()), b).unwrap();
        }
    }
}
