//! `present`: frames shown on a real Wayland compositor through the Linux
//! driver's window, each told the rows its renderer painted or told that
//! everything changed -- what the window copies and damages, what that
//! costs, and what the compositor then shows (`scripts/linux-damage.sh`).
//!
//! Usage: `present <rows|whole> <frames> [shots]`, with `WAYLAND_DISPLAY`
//! naming the compositor. `rows` hands `Window::present` the bands
//! `Renderer::painted` names, as the Linux driver does; `whole` hands it
//! `Changed::Whole` on every frame, the driver before it sent its rows,
//! which copied and damaged the whole buffer each time.
//!
//! Four workloads, each the same bytes on both arms, at 100x35 cells of
//! the Linux driver's default stack, with no system face installed, and
//! its theme (`linux_faces`, 13 px, the dark glass dim): `line`, one row
//! rewritten a frame; `walk`, a row rewritten a frame, the next row down
//! each time, so a buffer used again misses rows its own frame does not
//! change; `scroll`, a line printed at the bottom of a full screen a
//! frame, so every row moves; and `full`, every row rewritten a frame.
//! Each starts with a whole frame and ten frames of its own that are not
//! counted, so both buffers exist and a new buffer's whole copy is not in
//! the figures. Then `<frames>` frames, each presented once the last
//! one's callback came: the time `present` takes (the copy, the
//! premultiply and the requests), the rows it copied and damaged, and how
//! many times it waited for a buffer the compositor still held. The rows
//! copied are the window's count; the rows damaged are the bench's own
//! `RowSet` of what it told the window, not a count of the
//! `wl_surface.damage_buffer` requests sent, which the compositor's
//! `WAYLAND_DEBUG=server` trace in `scripts/linux-damage.sh`
//! corroborates: on sway 1.7 and 1.10.1 the opaque `rows` run's requests
//! are 17, 34, 595 and 600 rows high, the 600 the whole frame each
//! workload starts with.
//!
//! `PRESENT_SHOT=<command>`: after each of the first `[shots]` frames of
//! each workload, the command is run and its standard output read as a
//! binary PNM (`P6`, or `P7` with three or four channels), the screen;
//! the window's area at its origin is compared with the frame
//! premultiplied, which is what the compositor shows over black or where
//! the frame is opaque, and hashed (FNV-1a over its RGB), so two arms'
//! screens can be compared frame for frame. `PRESENT_ALPHA=<0-255>` sets
//! the page's alpha in place of the dim. `PRESENT_COMPOSITOR=<pid>`
//! reads that process's CPU time from `/proc` around each workload's
//! counted frames.
//!
//! Prints one line a workload and one a shot; asserts only that the window
//! holds the grid.

use baud_core::{Owned, Term};
use baud_domain::{
    AmbiguousWidth, BytePos, Chan, Cols, ImageBudget, ReadLen, Rows, Vs16Width, WinSize,
};
use baud_font::Face;
use baud_present_wayland::damage::RowSet;
use baud_present_wayland::{Changed, Event, Presented, Px, Strip, Window};
use baud_render::{Backdrop, Blend, Damage, PxSize, Renderer, Stack, Target, Theme};
use baud_wayland::Connection;
use std::time::{Duration, Instant};

/// `host.rs`'s `IMAGE_BUDGET`, the default `[images]` budget.
const IMAGE_BUDGET: ImageBudget = ImageBudget { pixels: 95 << 18 };

/// The frames of a workload shown before its counted ones.
const WARM: usize = 10;

/// What an arm tells the window a frame changed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Arm {
    Rows,
    Whole,
}

/// A workload: what it writes first, and what each frame writes.
struct Work {
    name: &'static str,
    first: String,
    step: fn(usize) -> String,
}

/// Text to fill a row with: 77 columns, so a row with its prefix fits.
const WORDS: &str = "the quick brown fox jumps over the lazy dog, pack my box with five dozen jugs";

fn fill() -> String {
    let mut s = String::from("\x1b[H\x1b[2J");
    for r in 0..35 {
        s.push_str(&format!("{r:02} {WORDS}"));
        if r < 34 {
            s.push_str("\r\n");
        }
    }
    s
}

fn works() -> [Work; 4] {
    [
        Work {
            name: "line",
            first: fill() + "\x1b[18;1H",
            step: |i| format!("\r\x1b[2Kframe {i:06} of one line changed"),
        },
        Work {
            name: "walk",
            first: fill(),
            step: |i| format!("\x1b[{};1H\x1b[2K{i:06} {WORDS}", i % 35 + 1),
        },
        Work {
            name: "scroll",
            first: fill() + "\x1b[35;1H",
            step: |i| format!("\r\n{i:06} {WORDS}"),
        },
        Work {
            name: "full",
            first: fill(),
            step: |i| {
                let mut s = String::from("\x1b[H");
                for r in 0..35 {
                    s.push_str(&format!("{i:06} {r:02} {WORDS}\x1b[K"));
                    if r < 34 {
                        s.push_str("\r\n");
                    }
                }
                s
            },
        },
    ]
}

/// The window's frame: the driver's buffer, a stride of the window's
/// width, the grid at its origin and the margin below it.
struct Shown {
    frame: Vec<u32>,
    stride: Px,
    strips: Vec<Strip>,
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for chunk in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, chunk);
        t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
    }
}

/// Dispatch until the frame in flight has its callback, and say whether
/// the window was sized again meanwhile.
fn settle(conn: &mut Connection, w: &mut Window) -> Resized {
    while w.in_flight() {
        conn.roundtrip_once(w).expect("the compositor");
    }
    let mut resized = Resized::No;
    while let Some(e) = w.next_event() {
        if let Event::Configured { .. } = e {
            resized = Resized::Yes;
        }
    }
    resized
}

#[derive(PartialEq, Eq)]
enum Resized {
    No,
    Yes,
}

/// Present until the window takes the frame, waiting for a buffer the
/// compositor still holds; a refused frame's rows are the window's to
/// keep, so a retry says nothing changed. The time of the call that
/// took it, and how many waited.
fn present(
    conn: &mut Connection,
    w: &mut Window,
    s: &Shown,
    changed: Changed<'_>,
) -> (Duration, usize) {
    let pitch = usize::try_from(s.stride.0).expect("the stride");
    let mut waited = 0;
    let mut changed = changed;
    loop {
        let t0 = Instant::now();
        let took = w.present(&mut conn.sender(), &s.frame, pitch, changed);
        let dt = t0.elapsed();
        if took == Presented::Yes {
            conn.flush().expect("the compositor");
            return (dt, waited);
        }
        waited += 1;
        changed = Changed::Strips(&[]);
        conn.roundtrip_once(w).expect("the compositor");
    }
}

/// The CPU time a process's threads have had, in nanoseconds: the first
/// field of each `/proc/<pid>/task/<tid>/schedstat`, where `stat`'s
/// clock ticks are ten milliseconds and a compositor's frame is a
/// fraction of one.
fn cpu_of(pid: &str) -> Option<Duration> {
    let mut ns: u64 = 0;
    for task in std::fs::read_dir(format!("/proc/{pid}/task")).ok()? {
        let stat = std::fs::read_to_string(task.ok()?.path().join("schedstat")).ok()?;
        ns += stat.split_whitespace().next()?.parse::<u64>().ok()?;
    }
    Some(Duration::from_nanos(ns))
}

/// A binary PNM's width, height and RGB bytes.
fn pnm(data: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let text_end = |at: usize, lines: usize| {
        let mut n = 0;
        data[at..].iter().position(|&b| {
            n += usize::from(b == b'\n');
            n == lines
        })
    };
    match data.get(..2)? {
        b"P6" => {
            let end = text_end(0, 3)? + 1;
            let head = std::str::from_utf8(&data[..end]).ok()?;
            let v: Vec<usize> = head[2..]
                .split_whitespace()
                .filter_map(|t| t.parse().ok())
                .collect();
            let (w, h) = (*v.first()?, *v.get(1)?);
            Some((w, h, data.get(end..end + w * h * 3)?.to_vec()))
        }
        b"P7" => {
            let end = data.windows(7).position(|x| x == b"ENDHDR\n")? + 7;
            let head = std::str::from_utf8(&data[..end]).ok()?;
            let key = |k: &str| -> Option<usize> {
                head.lines()
                    .find_map(|l| l.strip_prefix(k))
                    .and_then(|v| v.trim().parse().ok())
            };
            let (w, h, depth) = (key("WIDTH")?, key("HEIGHT")?, key("DEPTH")?);
            let body = data.get(end..end + w * h * depth)?;
            Some((
                w,
                h,
                body.chunks(depth.max(1))
                    .flat_map(|p| p.iter().take(3).copied())
                    .collect(),
            ))
        }
        _ => None,
    }
}

/// The screen now, by `command`, against the frame: the window's pixels
/// the screen does not show as the frame premultiplied, and the FNV-1a of
/// the window's area.
fn shot(command: &str, s: &Shown, wu: usize, hu: usize) -> (usize, String) {
    let out = std::process::Command::new("sh")
        .args(["-c", command])
        .output()
        .expect("the shot command");
    let Some((sw, sh, rgb)) = pnm(&out.stdout) else {
        return (wu * hu, "unreadable".to_string());
    };
    if sw < wu || sh < hu {
        return (wu * hu, format!("{sw}x{sh}"));
    }
    let pitch = usize::try_from(s.stride.0).expect("the stride");
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut differ = 0;
    for y in 0..hu {
        for x in 0..wu {
            let got = &rgb[(y * sw + x) * 3..(y * sw + x) * 3 + 3];
            for &b in got {
                hash = (hash ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
            }
            let [_, r, g, b] = baud_domain::premultiply(s.frame[y * pitch + x]).to_be_bytes();
            differ += usize::from(got != [r, g, b]);
        }
    }
    (differ, format!("{hash:016x}"))
}

/// Median, least and most of some durations, in microseconds.
fn spread(mut v: Vec<Duration>) -> String {
    v.sort();
    let us = |d: Duration| d.as_secs_f64() * 1e6;
    match (v.first(), v.get(v.len() / 2), v.last()) {
        (Some(&lo), Some(&mid), Some(&hi)) => {
            format!("{:.1} [{:.1}-{:.1}]", us(mid), us(lo), us(hi))
        }
        _ => "-".to_string(),
    }
}

fn main() {
    baud_bench::warm::refuse_while_warm();
    let mut args = baud_bench::args();
    let arm = match args.next().as_deref() {
        Some("rows") => Arm::Rows,
        Some("whole") => Arm::Whole,
        _ => {
            eprintln!("usage: present <rows|whole> <frames> [shots]");
            std::process::exit(2);
        }
    };
    let frames: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(300);
    let shots: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let shoot = std::env::var("PRESENT_SHOT").ok();
    let compositor = std::env::var("PRESENT_COMPOSITOR").ok();

    let files = baud_bench::linux_faces(Vec::new());
    let faces: Vec<Face<'_>> = files
        .iter()
        .map(|(bytes, index, _)| Face::parse_index(bytes, *index).expect("a face"))
        .collect();
    let roles: Vec<_> = files.iter().map(|(_, _, r)| *r).collect();
    let ws = WinSize {
        ws_row: 35,
        ws_col: 100,
    };
    let theme = Theme {
        bg_alpha: std::env::var("PRESENT_ALPHA")
            .ok()
            .and_then(|s| s.parse().ok())
            .map_or(Chan::from_unit(0.70), Chan::from_u8),
        backdrop: Backdrop::any(),
        ..Theme::dark()
    };
    let mut r = Renderer::new(
        Stack::with_roles(&faces, &roles).expect("the stack"),
        PxSize::of(13.0).expect("size"),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .expect("cell metrics");
    let mut owned = Owned::new(&ws).with_images(IMAGE_BUDGET);
    let mut t = Term::new(owned.storage(), &ws);
    let m = r.metrics();
    t.set_cell_size(
        baud_domain::ImagePx::of(u16::try_from(m.cell_w.value()).unwrap_or(u16::MAX)),
        baud_domain::ImagePx::of(u16::try_from(m.cell_h.value()).unwrap_or(u16::MAX)),
    );
    t.set_backing_scale(baud_domain::BackingScale::of(1.0));
    t.set_vs16_width(Vs16Width::Two);
    t.set_ambiguous_width(AmbiguousWidth::Narrow);
    r.fit(&t);

    let mut conn = Connection::connect().expect("a Wayland compositor");
    let mut w = Window::open(&mut conn, "present", "org.baud.present").expect("the window");
    while w.size().is_none() {
        conn.roundtrip_once(&mut w).expect("the compositor");
    }
    let (gw, gh) = r.whole_size();
    let size = w.size().expect("configured");
    let (wu, hu) = (
        usize::try_from(size.0.0).expect("width"),
        usize::try_from(size.1.0).expect("height"),
    );
    assert!(
        size.0.0 >= gw.value() && size.1.0 >= gh.value(),
        "the window is {}x{}, the grid {}x{}",
        size.0.0,
        size.1.0,
        gw.value(),
        gh.value()
    );
    let mut s = Shown {
        frame: vec![r.clear_color_of(&t).0; wu * hu],
        stride: size.0,
        strips: Vec::new(),
    };
    let stride = baud_font::Px::of(size.0.0);
    println!(
        "present {}: window {}x{}, grid {}x{} px, cell {}x{}",
        if arm == Arm::Rows { "rows" } else { "whole" },
        wu,
        hu,
        gw.value(),
        gh.value(),
        m.cell_w.value(),
        m.cell_h.value()
    );

    for work in works() {
        let mut copied = Vec::with_capacity(frames);
        let mut damaged = Vec::with_capacity(frames);
        let mut times = Vec::with_capacity(frames);
        let mut renders = Vec::with_capacity(frames);
        let mut waits = 0;
        let mut cpu = None;
        feed(&mut t, work.first.as_bytes());
        for i in 0..WARM + 1 + frames {
            if i > 0 {
                feed(&mut t, (work.step)(i).as_bytes());
            }
            if i == WARM + 1 {
                cpu = compositor.as_deref().and_then(cpu_of);
            }
            let damage = if i == 0 {
                Damage::Full
            } else {
                Damage::Incremental
            };
            let mut target = Target::new(&mut s.frame, stride, gw, gh).expect("the target");
            r.place_marks(
                &t,
                t.window_back(),
                baud_render::Leaving::None,
                baud_font::Px::of(0),
            );
            let back = t.window_back();
            let t0 = Instant::now();
            r.render_at(&mut t, back, &mut target, damage);
            let rendered = t0.elapsed();
            s.strips.clear();
            s.strips.extend(r.painted().map(|b| Strip {
                top: Px(b.top.value()),
                bottom: Px(b.bottom.value()),
            }));
            let changed = match (arm, i) {
                (Arm::Whole, _) | (_, 0) => Changed::Whole,
                (Arm::Rows, _) => Changed::Strips(&s.strips),
            };
            let mut rows = RowSet::none(size.1);
            rows.add_changed(changed);
            let (dt, waited) = present(&mut conn, &mut w, &s, changed);
            if settle(&mut conn, &mut w) == Resized::Yes && w.size() != Some(size) {
                eprintln!("the window was sized again, to {:?}", w.size());
                std::process::exit(1);
            }
            if i <= WARM {
                continue;
            }
            let n = i - WARM - 1;
            times.push(dt);
            renders.push(rendered);
            copied.push(w.rows_copied().0);
            damaged.push(rows.count().0);
            waits += waited;
            if let (Some(cmd), true) = (shoot.as_deref(), n < shots) {
                let (differ, hash) = shot(cmd, &s, wu, hu);
                println!(
                    "shot {} {n} {hash} differ {differ} copied {} damaged {}",
                    work.name,
                    w.rows_copied().0,
                    rows.count().0
                );
            }
        }
        let used = compositor
            .as_deref()
            .and_then(cpu_of)
            .zip(cpu)
            .map(|(b, a)| b.saturating_sub(a));
        let row_bytes = u64::try_from(wu * 4).expect("a row");
        let total = |v: &[u32]| v.iter().map(|&n| u64::from(n)).sum::<u64>();
        let per = |v: &[u32]| total(v) * row_bytes / u64::try_from(v.len().max(1)).unwrap_or(1);
        println!(
            "{:6} frames {frames} present_us {} render_us {} copied_bytes/frame {} \
             damaged_rows/frame {} waits {waits} compositor_ms/frame {}",
            work.name,
            spread(times),
            spread(renders),
            per(&copied),
            total(&damaged) / u64::try_from(damaged.len().max(1)).unwrap_or(1),
            used.map_or("-".to_string(), |d| format!(
                "{:.3}",
                d.as_secs_f64() * 1e3 / f64::from(u32::try_from(frames.max(1)).unwrap_or(1))
            ))
        );
    }
}
