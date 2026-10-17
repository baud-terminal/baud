//! Scratch: counts of the work past the live edge on the recordings.
use baud_core::{Owned, Term, Zone};
use baud_domain::{BytePos, ReadLen, WinSize, probe};

fn feed(t: &mut Term<'_>, data: &[u8]) {
    let mut at = 0;
    while at < data.len() {
        let n = 4096.min(data.len() - at);
        t.write_at(BytePos::ZERO, &data[at..at + n]);
        t.feed(BytePos::ZERO, ReadLen::from_index(n));
        at += n;
    }
}
fn show(tag: &str, c: &[u64; probe::N]) {
    let parts: Vec<String> = (0..probe::N)
        .filter(|&i| c[i] != 0)
        .map(|i| format!("{}={}", probe::NAMES[i], c[i]))
        .collect();
    println!(
        "  {tag}: {}",
        if parts.is_empty() {
            "-".into()
        } else {
            parts.join(" ")
        }
    );
}
fn ws(c: u16, r: u16) -> WinSize {
    WinSize {
        ws_col: c,
        ws_row: r,
    }
}

fn ingest() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/recordings/");
    let recs = [
        ("git-log", "ascii/git-log.raw"),
        ("cat-cjk", "utf8/cat-cjk.raw"),
        ("cat", "ascii/cat-scroll.raw"),
        ("less-cjk", "utf8/less-cjk.raw"),
        ("less", "ascii/less-pager.raw"),
        ("vim", "utf8/vim-unicode.raw"),
        ("asciiquarium", "ascii/asciiquarium.raw"),
        ("yazi", "sixel/yazi-walk.raw"),
        ("cava", "utf8/cava.raw"),
        ("btop", "utf8/btop.raw"),
    ];
    let geom = baud_bench::host_geometry();
    for (name, path) in recs {
        let data = std::fs::read(format!("{root}{path}")).unwrap();
        for mode in ["prompt", "output"] {
            let mut owned = Owned::with_geometry(&ws(200, 50), &geom);
            let mut t = Term::new(owned.storage(), &ws(200, 50));
            feed(&mut t, &data);
            match mode {
                "prompt" => feed(&mut t, b"\x1b]133;A\x07"),
                _ => {
                    if t.zone() != Zone::Output {
                        feed(&mut t, b"\x1b]133;C\x07")
                    }
                }
            }
            let want = if mode == "prompt" {
                Zone::Prompt
            } else {
                Zone::Output
            };
            assert_eq!(t.zone(), want, "{name} {mode}");
            let alt = format!("{:?}", t.screen());
            let _ = probe::take();
            let _ = t.resize(&ws(100, 50));
            let held = t.past_edge_check().held.0.index();
            println!(
                "{name} {mode}: held after narrowing {held} (reach {:?}) screen {}",
                t.past_edge_check().reach.0.index(),
                alt
            );
            show("resize", &probe::take());
            for k in 0..4 {
                feed(&mut t, &data);
                show(&format!("feed{}", k + 1), &probe::take());
            }
        }
    }
}

fn load(path: &str) -> Vec<(WinSize, Vec<u8>)> {
    let raw = std::fs::read(format!("{path}.raw")).unwrap();
    let text = std::fs::read_to_string(format!("{path}.steps")).unwrap();
    let mut out = Vec::new();
    let mut at = 0;
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let (size, len) = line.split_once(' ').unwrap();
        let (c, r) = size.split_once('x').unwrap();
        let len: usize = len.parse().unwrap();
        out.push((
            ws(c.parse().unwrap(), r.parse().unwrap()),
            raw[at..at + len].to_vec(),
        ));
        at += len;
    }
    out
}

fn winch() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/resize/");
    let geom = baud_bench::host_geometry();
    for name in [
        "drag",
        "fresh-burst",
        "fresh-drag",
        "fresh-zoom",
        "live-drag",
        "short",
        "zoom",
    ] {
        let steps = load(&format!("{root}{name}"));
        let max = ws(
            steps.iter().map(|s| s.0.ws_col).max().unwrap().max(400),
            steps.iter().map(|s| s.0.ws_row).max().unwrap().max(150),
        );
        let mut owned = Owned::with_geometry(&max, &geom);
        let mut t = Term::new(owned.storage(), &steps[0].0);
        let (mut narrow, mut narrow_held) = (0, 0);
        let mut rz = [0u64; probe::N];
        let mut fd = [0u64; probe::N];
        let _ = probe::take();
        for (w, bytes) in &steps {
            let before = t.geometry().0.index();
            let _ = t.resize(w);
            let c = probe::take();
            for i in 0..probe::N {
                rz[i] += c[i];
            }
            if usize::from(w.ws_col) < before {
                narrow += 1;
                if t.past_edge_check().held.0.index() > 0 {
                    narrow_held += 1;
                }
            }
            feed(&mut t, bytes);
            let c = probe::take();
            for i in 0..probe::N {
                fd[i] += c[i];
            }
        }
        println!(
            "{name}: {} steps, narrowings {narrow}, leaving a row held {narrow_held}, ring {}",
            steps.len(),
            t.scrollback().len()
        );
        show("resizes", &rz);
        show("feeds", &fd);
    }
}

fn control() {
    let geom = baud_bench::host_geometry();
    let mut all = [0u64; probe::N];
    let mut tally = |tag: &str| {
        let c = probe::take();
        show(tag, &c);
        for i in 0..probe::N {
            all[i] += c[i];
        }
    };
    let long = "L".repeat(39);
    let bar = "abc\x1b[44m\x1b[K\x1b[m";
    // A: a short ring, so the scrolls bring back rows that held lines.
    {
        let mut owned = Owned::with_history(&ws(40, 10), &geom, baud_domain::Depth::of(5));
        let mut t = Term::new(owned.storage(), &ws(40, 10));
        for k in 0..14 {
            let l = if k % 2 == 0 {
                long.clone()
            } else {
                bar.to_string()
            };
            feed(&mut t, format!("{l}\r\n").as_bytes());
        }
        feed(&mut t, b"\x1b]133;A\x07");
        tally("A: fourteen lines, half long, half bars");
        let _ = t.resize(&ws(20, 10));
        tally("A: narrow to 20 at a prompt");
        feed(&mut t, b"\x1b]133;C\x07a\r\nb\r\nc\r\n");
        tally("A: line feeds");
        feed(&mut t, b"\x1b[1;5r\x1b[5;1H\n\n\n\x1b[r");
        tally("A: top region scrolls");
        feed(&mut t, b"\x1b[3;1H\x1b[2L\x1b[2M");
        tally("A: IL DL");
        feed(&mut t, b"\x1b[1;1H\x1bM\x1bM");
        tally("A: RI at the top");
        feed(&mut t, b"\x1b[10;1Hx\r\ny\r\nz\r\n");
        tally("A: line feeds 2");
        let _ = t.resize(&ws(20, 9));
        tally("A: resize height");
        for y in 1..=9 {
            feed(
                &mut t,
                format!("\x1b[{y};1H\x1b[44m\x1b[K\x1b[m").as_bytes(),
            );
        }
        tally("A: blue EL on each row");
        feed(&mut t, b"\x1b[2;3H\x1b[K\x1b[3;1H\x1b[2K");
        tally("A: EL 0, EL 2");
        feed(&mut t, b"\x1b[2J");
        tally("A: ED 2");
    }
    // B: one bar, erased: the reach comes down.
    {
        let mut owned = Owned::with_history(&ws(40, 10), &geom, baud_domain::Depth::of(5));
        let mut t = Term::new(owned.storage(), &ws(40, 10));
        feed(&mut t, format!("{bar}\r\n\x1b]133;A\x07").as_bytes());
        let _ = t.resize(&ws(20, 10));
        tally("B: one bar, narrowed");
        feed(&mut t, b"\x1b]133;C\x07\x1b[1;1H\x1b[2K");
        tally("B: EL 2 on it");
        feed(&mut t, b"\x1b[1;1H");
        let _ = t.resize(&ws(19, 10));
        tally("B: narrow again");
        let _ = t.resize(&ws(30, 10));
        let _ = t.resize(&ws(10, 10));
        let _ = t.resize(&ws(15, 10));
        tally("B: grow, narrow, grow less");
    }
    // C: a prompt block that scrolled off, redrawn.
    {
        let mut owned = Owned::with_geometry(&ws(40, 10), &geom);
        let mut t = Term::new(owned.storage(), &ws(40, 10));
        for _ in 0..4 {
            feed(&mut t, format!("{bar}\r\n").as_bytes());
        }
        feed(&mut t, b"\x1b]133;A\x07");
        for k in 0..12 {
            feed(&mut t, format!("{}{k:02}\r\n", "L".repeat(30)).as_bytes());
        }
        let _ = t.resize(&ws(20, 10));
        tally("C: block of 12 long rows, narrowed at a prompt");
        feed(&mut t, b"\r\x1b[3A\x1b[J");
        println!(
            "  C: cursor {:?} prompt {:?} hist {}",
            t.cursor(),
            t.prompt_row(),
            t.scrollback().len()
        );
        feed(&mut t, b"\x1b]133;P\x07prompt");
        tally("C: CUU, ED, redraw mark");
        println!(
            "  C: cursor {:?} prompt {:?} hist {}",
            t.cursor(),
            t.prompt_row(),
            t.scrollback().len()
        );
    }
    // B2: three bars, ED 2.
    {
        let mut owned = Owned::with_history(&ws(40, 10), &geom, baud_domain::Depth::of(5));
        let mut t = Term::new(owned.storage(), &ws(40, 10));
        feed(
            &mut t,
            format!("{bar}\r\n{bar}\r\n{bar}\r\n\x1b]133;A\x07").as_bytes(),
        );
        let _ = t.resize(&ws(20, 10));
        tally("B2: three bars, narrowed");
        feed(&mut t, b"\x1b]133;C\x07\x1b[2J");
        tally("B2: ED 2");
    }
    println!("control totals:");
    show("all", &all);
    let missing: Vec<_> = (0..probe::N - 1)
        .filter(|&i| all[i] == 0)
        .map(|i| probe::NAMES[i])
        .collect();
    println!("control missing: {missing:?}");
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    match mode.as_str() {
        "ingest" => ingest(),
        "winch" => winch(),
        _ => control(),
    }
}
