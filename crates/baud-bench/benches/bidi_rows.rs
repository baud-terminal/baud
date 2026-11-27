//! **Nanoseconds a character for UAX #9 on crafted worst-case rows**,
//! against rows of real recorded text, at four lengths up to the grid's
//! 2048 columns: whether N0's bracket pairs and X5c's first-strong scan
//! grow with the row's length or with its square
//! (`[[source: uax9-bidi]]`). A linear rule reads the same
//! nanoseconds a character at every length; a quadratic one doubles them
//! with each doubling.
//!
//! Each shape is resolved `reps` times a round with `Bidi::resolve`, the
//! form that reads paired brackets (`resolve_classes` reads none), the
//! shapes and lengths rotated so none always runs first. The first round
//! is dropped and the median and range of the rest printed. Before any
//! timing, each bracket shape asserts the level N0 gives its last closer,
//! so a shape that never reached N0 cannot time as one that did.
//!
//!     cargo bench -p baud-bench --bench bidi_rows -- <rounds> <reps>

use baud_domain::{Bidi, BidiParagraph};
use std::hint::black_box;
use std::time::Instant;

const FSI: char = '\u{2068}';
const PDI: char = '\u{2069}';
const LENGTHS: [usize; 4] = [256, 512, 1024, 2048];

/// The rows of a recording holding a right-to-left letter, its escapes
/// cut, joined with a space and repeated to `n` characters.
fn recorded(name: &str, n: usize) -> Vec<char> {
    let p = format!(
        "{}/../../tools/tests/recordings/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e}"));
    let text = String::from_utf8_lossy(&raw);
    let mut lines = Vec::new();
    for line in text.split(['\n', '\r']) {
        let plain = without_csi(line);
        if plain.chars().any(|c| ('\u{590}'..='\u{6ff}').contains(&c)) {
            lines.push(plain);
        }
    }
    assert!(!lines.is_empty(), "{name}: no right-to-left row");
    lines.join(" ").chars().cycle().take(n).collect()
}

/// `line` with each `ESC [` sequence, through its final byte, cut.
fn without_csi(line: &str) -> String {
    let mut out = String::new();
    let mut it = line.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        if it.next_if_eq(&'[').is_some() {
            while it
                .next()
                .is_some_and(|b| !('\u{40}'..='\u{7e}').contains(&b))
            {}
        }
    }
    out
}

/// `a`, then `fill` repeated, then `c`: `n` characters.
fn framed(a: &str, fill: &str, c: &str, n: usize) -> Vec<char> {
    let (a, c): (Vec<char>, Vec<char>) = (a.chars().collect(), c.chars().collect());
    let room = n - a.len() - c.len();
    let mut v = a;
    v.extend(fill.chars().cycle().take(room));
    v.extend(c);
    v
}

fn repeat(unit: &str, n: usize) -> Vec<char> {
    unit.chars().cycle().take(n).collect()
}

struct Shape {
    name: &'static str,
    dir: BidiParagraph,
    row: fn(usize) -> Vec<char>,
    /// The level N0 gives the row's last closer, for the shapes it reaches.
    closer: Option<u8>,
}

fn shapes() -> Vec<Shape> {
    vec![
        Shape {
            name: "recorded vim-unicode",
            dir: BidiParagraph::FirstStrong,
            row: |n| recorded("utf8/vim-unicode.raw", n),
            closer: None,
        },
        Shape {
            name: "recorded less-cjk",
            dir: BidiParagraph::FirstStrong,
            row: |n| recorded("utf8/less-cjk.raw", n),
            closer: None,
        },
        // BD16's deepest nesting, every character inside 63 pairs: N0
        // scans each pair's whole contents and finds only the opposite
        // direction, then reads the context before it.
        Shape {
            name: "63 nested pairs",
            dir: BidiParagraph::Ltr,
            row: |n| {
                let (open, close) = ("(".repeat(63), ")".repeat(63));
                framed(
                    &format!("\u{5d0}{open}"),
                    "\u{5d1}",
                    &format!("{close}c"),
                    n,
                )
            },
            closer: Some(1),
        },
        // A 64th opener overflows BD16's stack, which stops it: no pairs.
        Shape {
            name: "64 nested, overflowed",
            dir: BidiParagraph::Ltr,
            row: |n| {
                let (open, close) = ("(".repeat(64), ")".repeat(64));
                framed(
                    &format!("\u{5d0}{open}"),
                    "\u{5d1}",
                    &format!("{close}c"),
                    n,
                )
            },
            closer: Some(0),
        },
        // As many pairs as fit, each holding the opposite direction, so
        // each reads its context.
        Shape {
            name: "pairs side by side",
            dir: BidiParagraph::Ltr,
            row: |n| framed("\u{5d0}", "(\u{5d1}) ", ")", n),
            closer: Some(1),
        },
        // Pairs holding no strong character stay neutral, so the one
        // pair after them reads its context back over all of them.
        Shape {
            name: "neutral pairs, then one",
            dir: BidiParagraph::Ltr,
            row: |n| framed("\u{5d0}", "() ", "(\u{5d1})c", n),
            closer: Some(1),
        },
        // X5c: each FSI finds its first strong character up to its PDI,
        // the nested ones past 125 levels overflowing.
        Shape {
            name: "nested FSIs",
            dir: BidiParagraph::FirstStrong,
            row: |n| {
                let half = n / 2 - 1;
                let mut v = vec![FSI; half];
                v.extend(['\u{5d0}', 'a']);
                v.extend(std::iter::repeat_n(PDI, half));
                v
            },
            closer: None,
        },
        // Unmatched FSIs, each scanning to the next.
        Shape {
            name: "unmatched FSIs",
            dir: BidiParagraph::FirstStrong,
            row: |n| repeat("\u{2068}. ", n),
            closer: None,
        },
    ]
}

fn main() {
    baud_bench::warm::refuse_while_warm();
    let mut args = baud_bench::args();
    let rounds: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(5);
    let reps: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(200);
    let shapes = shapes();
    let rows: Vec<Vec<Vec<char>>> = shapes
        .iter()
        .map(|s| LENGTHS.iter().map(|&n| (s.row)(n)).collect())
        .collect();
    let mut bidi = Box::new(Bidi::<2048>::new());
    for (s, rows) in shapes.iter().zip(&rows) {
        for row in rows {
            bidi.resolve(row, s.dir).expect("fits");
            if let Some(want) = s.closer {
                let last = row.iter().rposition(|&c| c == ')').expect("a closer");
                assert_eq!(bidi.levels()[last].number(), want, "{}: N0", s.name);
            }
        }
    }
    let jobs: Vec<(usize, usize)> = (0..shapes.len())
        .flat_map(|s| (0..LENGTHS.len()).map(move |l| (s, l)))
        .collect();
    let mut ns = vec![vec![Vec::new(); LENGTHS.len()]; shapes.len()];
    for round in 0..rounds {
        for k in 0..jobs.len() {
            let (s, l) = jobs[(k + round) % jobs.len()];
            let row = &rows[s][l];
            let t = Instant::now();
            for _ in 0..reps {
                black_box(bidi.resolve(black_box(row), shapes[s].dir).ok());
            }
            let per = t.elapsed().as_secs_f64() * 1e9 / (reps * row.len()) as f64;
            if round > 0 {
                ns[s][l].push(per);
            }
        }
    }
    println!(
        "ns a character, median [range] of {} rounds",
        rounds.saturating_sub(1)
    );
    print!("{:26}", "shape");
    for n in LENGTHS {
        print!("{n:>22}");
    }
    println!();
    for (s, shape) in shapes.iter().enumerate() {
        print!("{:26}", shape.name);
        for v in &mut ns[s] {
            v.sort_by(f64::total_cmp);
            match v.len() {
                0 => print!("{:>22}", "-"),
                k => print!(
                    "{:>22}",
                    format!("{:.2} [{:.2}-{:.2}]", v[k / 2], v[0], v[k - 1])
                ),
            }
        }
        println!();
    }
}
