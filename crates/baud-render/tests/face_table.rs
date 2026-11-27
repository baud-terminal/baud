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
use baud_render::{FaceIx, FaceRole, FaceStyle, Presentation, Stack};
use std::backtrace::Backtrace;
use std::cell::Cell;
use std::ffi::OsStr;
use std::sync::Once;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

fn embedded(dir: &str, name: &str) -> Vec<u8> {
    let p = format!(
        "{}/../baud-app/fonts/{dir}/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

/// Blocks of 256 codepoints in all of Unicode, `0..=U+10FFFF`: the table's
/// first stage (`c >> 8`), and the sweep's unit of work.
const BLOCKS: usize = 0x11_0000 >> 8;

/// Every codepoint of block `blk` with every style and presentation, in the
/// order the sweep has always visited them. A surrogate is no codepoint and
/// is skipped, as it always was.
fn each_in(blk: usize, mut f: impl FnMut(Codepoint, FaceStyle, Presentation)) {
    let lo = u32::try_from(blk).expect("a block below 0x1100") << 8;
    for v in lo..lo + 256 {
        let Some(c) = Codepoint::new(v) else { continue };
        for style in FaceStyle::ALL {
            for p in [Presentation::Text, Presentation::Colour] {
                f(c, style, p);
            }
        }
    }
}

/// What the stack answered in one block, in [`each_in`]'s order: every
/// question, or those before the one it panicked on, and the panic.
struct Answers {
    want: Vec<FaceIx>,
    panicked: Option<Panic>,
}

/// A panic the stack raised on a worker: its payload, and what the panic
/// hook would have printed for it (`panicked at <where>:\n<message>`, and,
/// when `RUST_BACKTRACE` asks, the worker's whole backtrace, untrimmed),
/// which [`hush_the_asking`] kept instead.
struct Panic {
    payload: Box<dyn std::any::Any + Send>,
    report: Option<String>,
}

thread_local! {
    /// Set on a worker for exactly as long as it asks the stack one block.
    static ASKING: Cell<bool> = const { Cell::new(false) };
    /// The hook's report of a panic raised while [`ASKING`].
    static REPORT: Cell<Option<String>> = const { Cell::new(None) };
}

/// Installs, once for the process, a panic hook that prints nothing for a
/// panic raised while a worker is asking the stack and keeps its report for
/// the worker instead; any other panic, on any thread, goes to the hook that
/// was there before. Installed once and never restored because `cargo test`
/// runs this file's two sweeps at once in one process, and a hook swapped
/// in and back per sweep would race with the other's.
fn hush_the_asking() {
    static HUSH: Once = Once::new();
    HUSH.call_once(|| {
        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if ASKING.get() {
                // The worker's call chain into the stack, which the hook
                // prints under `RUST_BACKTRACE` and the test thread's panic
                // cannot show: its own backtrace starts at the re-raise.
                // The variable is read with the hook's rules, those of
                // `std::panic::get_backtrace_style`: unset or `0` is none,
                // `full` is full, and any other value, UTF-8 or not, is
                // short.
                let style = std::env::var_os("RUST_BACKTRACE");
                let trace = match style.as_deref().map(OsStr::as_encoded_bytes) {
                    None | Some(b"0") => String::new(),
                    Some(b"full") => format!(
                        "\nthe worker's backtrace:\n{:#}",
                        Backtrace::force_capture()
                    ),
                    Some(_) => format!("\nthe worker's backtrace:\n{}", Backtrace::force_capture()),
                };
                REPORT.set(Some(format!("{info}{trace}")));
            } else {
                before(info);
            }
        }));
    });
}

/// Every codepoint, style and presentation agrees, and the sweep reaches
/// more than one face -- a table that answered the primary everywhere would
/// agree wherever the stack does too.
fn sweep(name: &str, stack: &Stack<'_, '_>) {
    let t0 = Instant::now();
    let mut table = stack.table();
    let built = t0.elapsed();
    // **The stack answers on every core, a block a thread at a time; the
    // table answers on this thread, in the order it always has.** The stack
    // is the slow side: on the macOS stack, asking it alone took about 3.2 s
    // in release and 370 s in debug (one probe run each, 2026-10-07, load
    // not recorded), against about 0.035 s and 1.4 s for the table and the
    // set of faces reached over the same 8.9 million questions. So the table is not split: one table, filled block
    // by block in codepoint order as a single thread fills it, prints the
    // same rows and blocks filled as before, and the first disagreement
    // below is the one a single thread hits first. Each worker takes the
    // next block from a counter, in codepoint order and not largest first:
    // the units are 4352 blocks of 256 codepoints, so the last to finish
    // trails the rest by one block. The answers are kept by block until the
    // table reads them: one `FaceIx` for each of 1,112,064 codepoints times
    // eight, 8.9 MB at one byte each, beside the fonts a single thread holds
    // too.
    //
    // **A panic in the stack is reported by this thread, at the question a
    // single thread stops on, and by nothing else.** It is caught per block
    // with the answers before it, and raised again below once every earlier
    // question has been checked. The panic hook is hushed while a worker
    // asks ([`hush_the_asking`]): left to print, every block that panicked
    // put its line on the test's output when it happened, so which lines
    // came and in what order followed the scheduler -- a later block's panic
    // ahead of the first, or ahead of an earlier disagreement that a single
    // thread stops at and never gets past; with two panics a block apart,
    // ten runs under `--test-threads=1` printed three or four different
    // reports (2026-10-07 and 2026-10-08). A worker that dies outside a
    // block still prints as any thread does, and is raised again at `join`.
    // Before each block a worker reads the lowest block caught panicking so
    // far, and stops if its own is later: the table would never reach it. A
    // later block taken before that catch is seen still runs, hushed, and
    // its answers are never read.
    hush_the_asking();
    let next = AtomicUsize::new(0);
    let failed = AtomicUsize::new(BLOCKS);
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(BLOCKS);
    let mut answers: Vec<Option<Answers>> = (0..BLOCKS).map(|_| None).collect();
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                let worker = std::thread::Builder::new().name(format!("{name} stack"));
                let work = || {
                    let mut done = Vec::new();
                    loop {
                        let blk = next.fetch_add(1, Ordering::Relaxed);
                        if blk >= BLOCKS || blk > failed.load(Ordering::Relaxed) {
                            break;
                        }
                        let mut want = Vec::with_capacity(256 * 8);
                        ASKING.set(true);
                        let asked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            each_in(blk, |c, style, p| {
                                want.push(stack.select_presented(c, style, p));
                            });
                        }));
                        ASKING.set(false);
                        let report = REPORT.take();
                        let panicked = asked.err().map(|payload| Panic { payload, report });
                        if panicked.is_some() {
                            failed.fetch_min(blk, Ordering::Relaxed);
                        }
                        done.push((blk, Answers { want, panicked }));
                    }
                    done
                };
                worker.spawn_scoped(s, work).expect("a worker thread")
            })
            .collect();
        for h in handles {
            let done = h.join().unwrap_or_else(|e| std::panic::resume_unwind(e));
            for (blk, a) in done {
                answers[blk] = Some(a);
            }
        }
    });
    let mut seen = std::collections::BTreeSet::new();
    for (blk, a) in answers.into_iter().enumerate() {
        let Answers { want, mut panicked } = a.expect("every block was asked");
        let mut want = want.into_iter();
        each_in(blk, |c, style, p| {
            let v = c.value();
            let Some(want) = want.next() else {
                match panicked.take() {
                    Some(Panic {
                        report: Some(r), ..
                    }) => panic!("{name}: U+{v:04X} {style:?} {p:?}: the stack {r}"),
                    // No report: the panic skipped the hush -- another hook
                    // replaced it and has printed the panic already, or it
                    // was raised with `resume_unwind`, which no hook sees.
                    Some(Panic {
                        payload,
                        report: None,
                    }) => std::panic::resume_unwind(payload),
                    None => panic!("{name}: U+{v:04X} {style:?} {p:?}: no answer"),
                }
            };
            let got = table.face(stack, c, style, p);
            assert_eq!(got, want, "{name}: U+{v:04X} {style:?} {p:?}");
            seen.insert(got.index());
        });
        assert!(
            want.next().is_none() && panicked.is_none(),
            "{name}: block {blk:#x}: answers left over"
        );
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
