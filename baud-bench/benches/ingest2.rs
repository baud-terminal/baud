//! The ingest measurement, driving `baud-core`.
//!
//! It was written as `ingest`'s twin -- same payloads, same rep protocol, same
//! self-timing, same power-source report -- so the two numbers were comparable
//! and the only difference was which core parsed the bytes. `ingest` drove
//! `baud-core2` and went with it (2538310).
//!
//! ```text
//! ingest2 cat-cjk 250
//! ```

use baud_core::*;
use baud_domain::{
    Depth, ImagePx, ImageSlot, ImageStore, PlacementId, PlacementSlot, SixelStream, Stride,
};

fn recording(name: &str) -> Vec<u8> {
    let p = format!("{}/../tests/recordings/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

/// A number recorded on battery should be re-run: the OS is free to move the
/// clock there, and it does.
fn power_source() -> String {
    std::process::Command::new("pmset")
        .args(["-g", "ps"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| match s.contains("AC Power") {
            true => "AC Power".into(),
            false => "BATTERY -- re-run on mains".into(),
        })
        .unwrap_or_else(|| "unknown".into())
}

/// `ESC [ <n> m`, built without a cast.
///
/// `format!` on a `usize` keeps the parameter out of the raw-integer count the
/// audit keeps for this crate; spelling it as a byte literal would spend a site
/// on a benchmark payload generator.
fn alloc_sgr(n: usize) -> String {
    format!("\x1b[{n}m")
}

/// The row pitch the arm under measurement lays the grid out at.
///
/// **One binary per arm**, because a runtime switch in the loop measures the
/// switch (`CONTRIBUTING.md`, "Runtime variant switches in the hot loop").
/// The default is what ships: the pitch `grid_stride` derives from the cache
/// geometry. `stride-live` is the layout this core had before -- the live
/// column count, the +14% over the aliasing one that the fixed pitch has to
/// match. `stride-pad0` is the aliasing layout itself, `MAX_COLS` cells and
/// exactly one L1 way a row on this class of machine, kept as the positive
/// control: an instrument that cannot see the 14% cannot see the pitch
/// either.
fn stride_arm(ws: &WinSize) -> Stride {
    #[cfg(feature = "stride-live")]
    {
        let (cols, _) = ws.geometry();
        Stride::of_cells(cols.index())
    }
    #[cfg(feature = "stride-pad0")]
    {
        let _ = ws;
        Stride::of_cells(MAX_COLS)
    }
    #[cfg(feature = "stride-env")]
    {
        let _ = ws;
        let n = std::env::var("BAUD_STRIDE").expect("BAUD_STRIDE=<cells>");
        Stride::of_cells(n.parse().expect("BAUD_STRIDE is a cell count"))
    }
    #[cfg(not(any(
        feature = "stride-live",
        feature = "stride-pad0",
        feature = "stride-env"
    )))]
    {
        let _ = ws;
        grid_stride(&baud_bench::host_geometry(), Cols::of_index(200))
    }
}

fn main() {
    let mut args = baud_bench::args();
    let which = args.next().unwrap_or_else(|| "cat-cjk".into());
    let reps: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(20);

    // **Synthetic ladder, for finding the ceiling rather than comparing.**
    // Each rung adds exactly one thing to the one below it, so the step that
    // costs can be named instead of inferred. Never quote these against
    // another implementation -- they are not recordings and they resemble no
    // program.
    let mb: usize = 16;
    // A table rather than arithmetic on a byte literal: casting the index
    // down to a byte would spend this crate's raw-integer budget on a
    // benchmark payload generator.
    const LETTERS: &[u8; 26] = b"abcdefghijklmnopqrstuvwxyz";
    let ladder = |kind: &str| -> Vec<u8> {
        let n = mb << 20;
        let mut v = Vec::with_capacity(n);
        let mut i = 0usize;
        while v.len() < n {
            match kind {
                // Printable ASCII only: one route, one bulk writer, one cell a
                // byte, no wrap for 200 columns at a time, no scroll.
                "flat" => v.push(LETTERS[i % 26]),
                // The same, cut into 200-column lines: adds the wrap.
                "wrap" => v.push(match i % 200 {
                    199 => b'\r',
                    _ => LETTERS[i % 26],
                }),
                // Lines ending in CRLF: adds the scroll and its blanking.
                "lines" => match i % 80 {
                    78 => v.push(b'\r'),
                    79 => v.push(b'\n'),
                    _ => v.push(LETTERS[i % 26]),
                },
                // Lines with one SGR each: adds a control sequence.
                _ => match i % 80 {
                    0 => v.extend_from_slice(b"\x1b[32m"),
                    78 => v.push(b'\r'),
                    79 => v.push(b'\n'),
                    _ => v.push(LETTERS[i % 26]),
                },
            }
            i += 1;
        }
        v.truncate(n);
        v
    };

    // **Homogeneous CJK.** The 2,440 MB/s once quoted for this name is a
    // `baud-core2` figure (2026-08-28), and this generated payload did not
    // enter `ingest2` until core2 was deleted -- on `baud-core` it reads
    // 1,137-1,159 over three launches at `bfc6632`, and always has. Not a
    // regression; a different core.
    //
    // Three bytes and two columns a character, so a 200-column row is 100
    // characters and 300 bytes. Deliberately without SGR: mixing in escapes
    // would measure the state machine again rather than the decoder. It is one
    // long run, which is exactly why it reads high -- this file's own rule is
    // that a synthetic payload resembles no program, and the real range on
    // captures is 419 to 819.
    let cjk = || -> Vec<u8> {
        const W: [&str; 8] = [
            "終端", "文字", "解析", "画面", "行列", "描画", "入力", "出力",
        ];
        let n = mb << 20;
        let mut v = Vec::with_capacity(n);
        let (mut col, mut i) = (0usize, 0usize);
        while v.len() < n {
            if col > 200 - 8 {
                v.extend_from_slice(b"\r\n");
                col = 0;
                continue;
            }
            let w = W[i % W.len()];
            i += 1;
            v.extend_from_slice(w.as_bytes());
            col += 2 * w.chars().count();
        }
        v.truncate(n);
        v
    };

    // **Bulk text with an SGR roughly every 37 bytes**, wrapping near column
    // 70. Models `cat`, build output and log tailing, and it is the payload
    // this file names when it says blanking is worth +51%.
    //
    // `CRLF`, not a bare `LF`: `LF` is index only, because LNM is reset by
    // default, so a bare one would march the text rightwards until it wrapped,
    // which is not what a terminal receives.
    let streaming = || -> Vec<u8> {
        const W: [&str; 14] = [
            "static", "inline", "struct", "return", "const", "size_t", "uint32_t", "buffer",
            "render", "terminal", "parse", "handle", "width", "index",
        ];
        let n = mb << 20;
        let mut v = Vec::with_capacity(n);
        let mut col = 0usize;
        while v.len() < n {
            if col > 70 {
                v.extend_from_slice(b"\r\n");
                col = 0;
                continue;
            }
            if v.len() % 37 == 0 {
                v.extend_from_slice(alloc_sgr(30 + (v.len() % 8)).as_bytes());
                continue;
            }
            let w = W[(v.len() / 13) % 14].as_bytes();
            v.extend_from_slice(w);
            col += w.len();
            v.push(b' ');
            col += 1;
        }
        v.truncate(n);
        v
    };

    let data = match which.as_str() {
        "flat" | "wrap" | "lines" | "sgr" => ladder(&which),
        "cjk" => cjk(),
        "streaming" => streaming(),
        // **Every recording, concatenated, unmodified and in order.** Not a
        // new payload: each byte is a byte some program really sent, and
        // nothing is filtered, truncated or generated. It exists to answer
        // whether a single 300-500 KB recording replayed a hundred times is
        // representative -- one stream with eight programs' worth of distinct
        // sequences, styles and scripts puts far more pressure on the tables
        // than any one of them does.
        "mix" => {
            let mut v = Vec::new();
            for name in [
                "ascii/cat-scroll.raw",
                "utf8/btop.raw",
                "ascii/git-log.raw",
                "utf8/cat-cjk.raw",
                "ascii/pipes.raw",
                "utf8/vim-unicode.raw",
                "ascii/less-pager.raw",
                "utf8/less-cjk.raw",
            ] {
                v.extend_from_slice(&recording(name));
            }
            v
        }
        // **A spectrum analyser: dense redraw, block glyphs, and the one
        // recording in the corpus that sets a scrolling region.** One escape
        // per 12.1 bytes, 19.5% non-ASCII, 51,954 control sequences of which
        // 24% are absolute cursor moves -- and 83 `DECSTBM`s, which is the
        // shape a row map would need before it is built again.
        "cava" => recording("utf8/cava.raw"),
        "btop" => recording("utf8/btop.raw"),
        "pipes" => recording("ascii/pipes.raw"),
        "vim" => recording("utf8/vim-unicode.raw"),
        "cat" => recording("ascii/cat-scroll.raw"),
        "less" => recording("ascii/less-pager.raw"),
        "git-log" => recording("ascii/git-log.raw"),
        "cat-cjk" => recording("utf8/cat-cjk.raw"),
        "less-cjk" => recording("utf8/less-cjk.raw"),
        // **An image, then a lot of output**: the committed 64x48 Sixel and
        // then `cat-scroll.raw`, unmodified and in order, with image memory
        // provisioned and a cell size set so the image is placed and the
        // cursor moves below it the way the app's does. What a user sees after
        // `img2sixel` in a shell -- the image stays in the history while the
        // output after it runs -- and the one payload that reaches the
        // per-write and per-scroll image checks at all.
        "sixel-cat" => {
            let mut v = recording("sixel/swirl.raw");
            v.extend_from_slice(&recording("ascii/cat-scroll.raw"));
            v
        }
        other => recording(other),
    };
    let images = which == "sixel-cat";

    // 200 x 50, which is the geometry the recordings were captured at.
    let ws = WinSize {
        ws_row: 50,
        ws_col: 200,
    };
    let (_, rows) = ws.geometry();
    let stride = stride_arm(&ws);
    eprintln!(
        "grid pitch: {} cells, {} B a row",
        stride.index(),
        stride.bytes()
    );
    // The scrollback the app runs with, so a scrolling payload pays what
    // it pays there -- which is nothing a scroll: the grid is the ring and
    // the window moves. `BAUD_HIST` is the exploration arm -- lines behind
    // the screen, zero for none -- read once at start, never in the loop.
    let depth = match std::env::var("BAUD_HIST") {
        Ok(v) => Depth::of(v.parse::<usize>().expect("BAUD_HIST=<lines>")),
        Err(_) => Depth::DEFAULT,
    };
    eprintln!("scrollback: {} lines", depth.lines());
    let ring_rows = Rows::of_index(rows.index() + depth.lines());
    let span = RowBlock::leading(rows, stride);
    let ring = RowBlock::leading(ring_rows, stride);
    let mut arena = vec![0u8; ARENA];
    let mut vp = vec![PackedCell::EMPTY; ring.len()];
    let mut used = vec![RowExtent::ZERO; ring_rows.index()];
    let mut past = vec![PastEdge::Clear; ring_rows.index()];
    // **An empty table is how this benchmark declines it.** The only read is
    // behind `cell-table`; passing the full 2 MiB otherwise costs 262,144
    // property-table lookups and 2 MiB of stores at construction, for a table
    // no rep will consult.
    #[cfg(feature = "cell-table")]
    let mut cells3 = vec![PackedCell::EMPTY; CELLS3];
    #[cfg(not(feature = "cell-table"))]
    let mut cells3: Vec<PackedCell> = Vec::new();
    let mut tmpl = vec![None; NSTYLE];
    let mut order = vec![baud_domain::StyleId::DEFAULT; NSTYLE];
    let mut book = baud_core::StyleBook::new();
    let mut reach = vec![baud_domain::Reach::ZERO; NSTYLE];
    let mut tabs = vec![false; MAX_COLS];
    let mut reply = vec![0u8; REPLY];
    let mut alt = vec![PackedCell::EMPTY; span.len()];
    let mut alt_used = vec![RowExtent::ZERO; rows.index()];
    let mut clusters = vec![ClusterSlot::EMPTY; NCLUSTER];
    let mut pending = vec![0u8; OSC_MAX];
    let mut title = vec![0u8; OSC_MAX];
    let mut icon = vec![0u8; OSC_MAX];
    let mut titles = vec![0u8; baud_core::TITLE_STACK];
    let mut cwd = vec![0u8; baud_core::OSC_MAX];
    let mut palette = vec![ColorWord::DEFAULT; baud_core::PALETTE_LEN];
    let mut links = vec![0u8; NLINK * LINK_MAX];
    let mut link_len = vec![ByteLen::ZERO; NLINK];
    // No `OSC 52` areas: no recording carries one, and an empty slice
    // refuses a write and counts it, as an empty arena does an image.
    let mut clip: Vec<u8> = Vec::new();
    // No images, except for the payload that places one: an empty store
    // refuses every Sixel, and the recordings carry none, so the ingest path
    // never reaches image storage. `sixel-cat` gets room for its one image and
    // the placement tables `Owned` provisions.
    let (px, slots, places) = match images {
        true => (64 * 48 + 64, 32, 64),
        false => (0, 0, 0),
    };
    let mut img_pixels = vec![baud_domain::Pixel::CLEAR; px];
    let mut img_slots = vec![ImageSlot::EMPTY; slots];
    let mut img_places = vec![PlacementSlot::EMPTY; places];
    let mut img_index = vec![PlacementId::NONE; PlacementId::index_len(places)];
    let mut sixel = Box::new(SixelStream::NEW);
    let mut regis = Box::new(baud_domain::Regis::NEW);
    let mut kitty = Box::new(baud_domain::KittyStream::NEW);
    let mut colours = Box::new(baud_core::KittyColours::NEW);
    let mut kitty_scratch: Vec<u8> = Vec::new();
    let mut t = Term::new(
        Storage::new(
            &mut arena,
            stride,
            depth,
            &mut vp,
            &mut used,
            &mut past,
            &mut cells3,
            baud_core::StyleTable {
                slots: &mut tmpl,
                order: &mut order,
                book: &mut book,
                reach: &mut reach,
            },
            &mut tabs,
            &mut reply,
            &mut alt,
            &mut alt_used,
            &mut clusters,
            StrBuf::new(
                &mut pending,
                &mut title,
                &mut icon,
                &mut palette,
                &mut links,
                &mut link_len,
                &mut clip,
                &mut sixel,
                &mut regis,
                &mut kitty,
                &mut colours,
                &mut kitty_scratch,
                &mut titles,
                &mut cwd,
            ),
            ImageStore::new(
                &mut img_pixels,
                &mut img_slots,
                &mut img_places,
                &mut img_index,
            ),
        ),
        &ws,
    );

    if images {
        t.set_cell_size(ImagePx::of(8), ImagePx::of(16));
    }
    // `BAUD_AMBIGUOUS=wide` prices the app's `[text] ambiguous_width =
    // "wide"`; narrow, the default, otherwise. Read once at start.
    if std::env::var("BAUD_AMBIGUOUS").is_ok_and(|v| v == "wide") {
        t.set_ambiguous_width(baud_domain::AmbiguousWidth::Wide);
    }
    eprintln!("ambiguous width: {:?}", t.ambiguous_width());

    let (fed, per_rep) = timed(&mut t, &data, reps);

    let mut rates: Vec<f64> = per_rep
        .iter()
        .map(|(s, n)| *n as f64 / *s / (1 << 20) as f64)
        .collect();
    rates.sort_by(|a, b| a.partial_cmp(b).expect("no NaN: every rep took time"));
    let (lo, med, hi) = (rates[0], rates[rates.len() / 2], rates[rates.len() - 1]);
    let (bytes_in, cells) = t.amplification();
    println!(
        "{which} new: {med:.1} MB/s [{lo:.1}-{hi:.1}] over {reps} reps, {fed} bytes, {}",
        power_source()
    );
    println!(
        "  cells {cells} written ({:.1}x per byte), {} zero-width dropped",
        cells as f64 / bytes_in as f64,
        t.marks_dropped,
    );
    if images {
        let im = t.images();
        println!(
            "  images: {} placed now, {} images refused, {} placements refused",
            im.placements().count(),
            im.images_refused,
            im.placements_refused,
        );
    }
    #[cfg(feature = "diag")]
    {
        // **Bytes an iteration is the unit the loop is designed against.**
        // A number for it belongs beside the throughput, not derived from a
        // run histogram afterwards.
        println!(
            "  loop {} iterations, {:.2} bytes/iter, {:.2} cells/iter",
            t.iters,
            bytes_in as f64 / t.iters as f64,
            cells as f64 / t.iters as f64,
        );
        println!(
            "  scrolls: {} rotated, {} copied ({:.0}% copied)",
            t.scroll_rot,
            t.scroll_copy,
            100.0 * t.scroll_copy as f64 / (t.scroll_rot + t.scroll_copy).max(1) as f64,
        );
        println!(
            "  accelerated CSI: {} tried, {} committed ({:.0}% declined)",
            t.fast_try,
            t.fast_hit,
            100.0 * (t.fast_try - t.fast_hit) as f64 / t.fast_try.max(1) as f64,
        );
        #[cfg(feature = "sgr-table")]
        println!("  SGR from the byte table: {} hits", t.sgr_hit,);
        println!(
            "  table hits {} of {} single-character runs, {} runs total",
            t.table_hits,
            t.runs[1],
            t.runs.iter().sum::<usize>(),
        );
        // **Names come from the core, not from a list kept here.**
        //
        // A `const NODES: [&str; 10]` stood here until `diag` was built again:
        // dead since `named_nodes()` landed, and wrong anyway, because the graph
        // has **twenty** nodes now. Exactly the drift the note below describes,
        // left behind by the change that fixed it. Two
        // parallel arrays in this file drifted from the enums in one session
        // and reported nodes and actions that cannot occur.
        print!("  iterations by node:");
        for (name, n) in t.hist.named_nodes() {
            if n > 0 {
                print!(" {name}:{:.1}%", 100.0 * n as f64 / t.iters as f64);
            }
        }
        println!();
        print!("  iterations by action:");
        for (name, n) in t.hist.named_acts() {
            if 100.0 * n as f64 / t.iters as f64 >= 1.0 {
                print!(" {name}:{:.1}%", 100.0 * n as f64 / t.iters as f64);
            }
        }
        println!();
        let tot: usize = t.runs.iter().sum();
        print!("  runs by character count:");
        for (k, n) in t.runs.iter().enumerate() {
            print!(" {k}:{:.0}%", 100.0 * *n as f64 / tot as f64);
        }
        println!();
        // **Entries into each stride's vector scan**, which is a different
        // number from the runs it produced and the only one that can say
        // whether a bulk path is reached at all.
        print!("  vector scan entered:");
        for (k, n) in t.stride_scan.iter().enumerate().skip(2) {
            print!(" {k}B:{n}");
        }
        println!();
        print!("  runs by encoded length:");
        for (k, n) in t.run_len.iter().enumerate().skip(2) {
            print!(" {k}B:{:.0}%", 100.0 * *n as f64 / tot as f64);
        }
        println!();
    }
}

/// The timed loop: `reps` passes over `data`, `READ_CHUNK` at a time.
///
/// **Out of line, so that the setup cannot move it.** Inlined into `main`,
/// this loop shared one register allocation with everything `main` builds
/// before it, and a setup-only change -- the cluster store's `vec!` filling
/// 1024 slots instead of 64, or that `vec!` moved into a function of its own
/// -- put two stack reloads at the head of the dispatch loop: about two
/// instructions an iteration, +0.69% per rep on `cat` and +0.55% on `btop`,
/// with no change to the core. Out of line, the loop is allocated on its own
/// and the base moved too (`btop` +5.95% a rep, `cat` -0.04%), so per-rep
/// figures from before this function are not comparable with those after.
/// `scripts/loop-head.py` compares this function between two binaries.
///
/// **The chunk is written into the arena and then parsed, which is what a
/// `read(2)` does.** Parsing the recording in place instead -- placed once,
/// never re-copied -- was tried and is *slower*: 509-518 against 517-525,
/// because 327 KB exceeds a 128 KB L1 and every block takes misses, where a
/// real read leaves its 4096 bytes hot. The copy is not a cost baud pays in
/// production, but it reproduces the cache state baud parses in, and the
/// faster number is the one that models nothing.
///
/// The parse itself is zero-copy either way: `feed` borrows the arena and
/// `put_ascii_run` blits straight out of it.
#[inline(never)]
fn timed(t: &mut Term<'_>, data: &[u8], reps: usize) -> (usize, Vec<(f64, usize)>) {
    let mut fed = 0usize;
    let mut per_rep = Vec::with_capacity(reps);
    for _ in 0..reps {
        let t0 = std::time::Instant::now();
        let mut at = 0usize;
        while at < data.len() {
            // `READ_CHUNK` at a time, because where the read boundaries fall
            // is part of what is being measured: a whole-file `feed` would
            // give the block loop a run no `read(2)` ever hands it.
            let chunk: usize = std::env::var("CHUNK")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(READ_CHUNK);
            let n = chunk.min(data.len() - at);
            t.write_at(BytePos::ZERO, &data[at..at + n]);
            #[cfg(not(feature = "harness-only"))]
            t.feed(BytePos::ZERO, ReadLen::from_index(n));
            at += n;
            fed += n;
        }
        per_rep.push((t0.elapsed().as_secs_f64(), at));
    }
    (fed, per_rep)
}
