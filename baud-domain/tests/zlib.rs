//! The resumable inflate against `[[source: rfc1950-zlib]]` and
//! `[[source: rfc1951-deflate]]`, with `miniz_oxide` as the oracle: it
//! writes the streams, at every level and strategy and with flushes that cut
//! blocks mid-stream, and it inflates them too.
//!
//! **Chunk invariance is the contract.** A stream fed in any number of reads,
//! cut anywhere, ends in exactly the state one read would leave: the same
//! output, the same verdict, the same count of bytes after the trailer. Each
//! check here compares a split feed against the one-read feed, and the
//! one-read feed against the oracle, so a bug both feeds share is caught by
//! the second comparison and a carry lost at a boundary by the first.

use baud_domain::png::Png;
use baud_domain::zlib::{Inflate, Inflated, ZlibError};
use miniz_oxide::deflate::core::{
    CompressionStrategy, CompressorOxide, TDEFLFlush, TDEFLStatus, compress_to_output,
    create_comp_flags_from_zip_params,
};
use proptest::prelude::*;
use std::path::{Path, PathBuf};

/// How a stream is written: miniz's level, its strategy, and where its
/// input is cut by a sync flush (each cut ends a block and adds an empty
/// stored one).
#[derive(Clone, Debug)]
struct Shape {
    level: Level,
    strategy: CompressionStrategy,
    flushes: Vec<usize>,
}

/// miniz's level, 0 (stored) to 10.
#[derive(Clone, Copy, Debug)]
struct Level(u8);

/// Bytes fed after the trailer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Excess(usize);

fn compress(data: &[u8], shape: &Shape) -> Vec<u8> {
    let flags =
        create_comp_flags_from_zip_params(i32::from(shape.level.0), 15, shape.strategy as i32);
    let mut c = CompressorOxide::new(flags);
    let mut out = Vec::new();
    let mut from = 0;
    let mut cuts: Vec<usize> = shape.flushes.iter().map(|&f| f.min(data.len())).collect();
    cuts.sort_unstable();
    for to in cuts.into_iter().chain([data.len()]) {
        let to = to.max(from);
        let flush = if to == data.len() {
            TDEFLFlush::Finish
        } else {
            TDEFLFlush::Sync
        };
        let (status, read) = compress_to_output(&mut c, &data[from..to], flush, |b| {
            out.extend_from_slice(b);
            true
        });
        assert_eq!(read, to - from, "miniz took the whole piece");
        assert!(matches!(status, TDEFLStatus::Okay | TDEFLStatus::Done));
        from = to;
        if flush == TDEFLFlush::Finish {
            break;
        }
    }
    out
}

/// Where a feed ended: `finish`'s verdict, the buffer, and the excess.
#[derive(PartialEq, Eq, Debug)]
struct End {
    verdict: Result<(), ZlibError>,
    out: Vec<u8>,
    excess: Excess,
}

/// Feeds `z` in the reads `cuts` makes of it (sorted offsets), into a buffer
/// of `n` bytes, and finishes.
fn feed_cut(z: &[u8], n: usize, cuts: &[usize]) -> End {
    let mut inflate = Inflate::NEW;
    let mut out = vec![0u8; n];
    let mut from = 0;
    for to in cuts.iter().copied().chain([z.len()]) {
        let to = to.clamp(from, z.len());
        if inflate.feed(&z[from..to], &mut out).is_err() {
            break;
        }
        from = to;
    }
    End {
        verdict: inflate.finish(),
        out,
        excess: Excess(inflate.excess()),
    }
}

fn one_read(z: &[u8], n: usize) -> End {
    feed_cut(z, n, &[])
}

fn reads_of(z: &[u8], n: usize, size: usize) -> End {
    let cuts: Vec<usize> = (size..z.len()).step_by(size).collect();
    feed_cut(z, n, &cuts)
}

/// Every split into two reads and every read size, against one read.
fn every_split_and_size(z: &[u8], n: usize, what: &str) -> End {
    let whole = one_read(z, n);
    for k in 0..=z.len() {
        assert_eq!(feed_cut(z, n, &[k]), whole, "{what}: split at {k}");
    }
    for size in 1..=z.len().max(1) {
        assert_eq!(reads_of(z, n, size), whole, "{what}: reads of {size}");
    }
    whole
}

/// Data with both kinds of redundancy deflate finds: short runs, and one
/// stretch repeated from up to `far` bytes back.
fn sample(len: usize, far: usize, seed: u64) -> Vec<u8> {
    let mut s = seed | 1;
    let mut next = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    let mut v: Vec<u8> = Vec::with_capacity(len);
    while v.len() < len {
        let r = next();
        match r % 4 {
            0 => v.push(r.to_le_bytes()[1]),
            1 => {
                let run = usize::from(r.to_le_bytes()[2] % 40);
                v.extend(std::iter::repeat_n(r.to_le_bytes()[3], run));
            }
            2 if v.len() > 3 => {
                let back =
                    1 + usize::from(u16::from_le_bytes([r.to_le_bytes()[4], r.to_le_bytes()[5]]))
                        % far.min(v.len());
                let start = v.len() - back;
                for i in 0..usize::from(r.to_le_bytes()[6] % 64) {
                    v.push(v[start + i]);
                }
            }
            _ => v.extend_from_slice(b"the quick brown fox "),
        }
    }
    v.truncate(len);
    v
}

/// A corpus small enough to split everywhere: stored, fixed and dynamic
/// streams, one cut into blocks by sync flushes, empty data, and one byte.
fn corpus() -> Vec<(String, Vec<u8>, Vec<u8>)> {
    let shapes = [
        (0, CompressionStrategy::Default, vec![]),
        (1, CompressionStrategy::Fixed, vec![]),
        (6, CompressionStrategy::Default, vec![]),
        (9, CompressionStrategy::Default, vec![100, 300, 301]),
        (6, CompressionStrategy::HuffmanOnly, vec![]),
        (3, CompressionStrategy::RLE, vec![50]),
    ];
    let mut out = Vec::new();
    for (i, (level, strategy, flushes)) in shapes.into_iter().enumerate() {
        let shape = Shape {
            level: Level(level),
            strategy,
            flushes,
        };
        for (len, seed) in [(0, 1), (1, 2), (700, 3 + u64::try_from(i).expect("small"))] {
            let data = sample(len, 600, seed);
            let z = compress(&data, &shape);
            out.push((format!("{shape:?} len {len}"), data, z));
        }
    }
    out
}

/// The first block's BTYPE: bits 1-2 of the first byte after CMF and FLG.
fn first_block_kind(z: &[u8]) -> u8 {
    (z[2] >> 1) & 3
}

#[test]
fn the_corpus_carries_every_block_kind_a_mid_stream_flush_and_a_far_reference() {
    // A positive control: the splitting tests below are only as wide as
    // what miniz was made to write.
    let kinds: Vec<u8> = corpus()
        .iter()
        .map(|(_, _, z)| first_block_kind(z))
        .collect();
    for k in 0..3 {
        assert!(
            kinds.contains(&k),
            "no stream starts with BTYPE {k}: {kinds:?}"
        );
    }
    let flushed = corpus()
        .into_iter()
        .filter(|(_, _, z)| z.windows(4).any(|w| w == [0, 0, 0xff, 0xff]))
        .count();
    assert!(flushed > 0, "no sync flush's empty stored block");
    // 20,000 random bytes then the same again: only a 20,000-byte
    // back-reference makes it smaller than its first half plus a little.
    let half = sample(20_000, 1, 7)
        .iter()
        .map(|b| b.wrapping_mul(31))
        .collect::<Vec<u8>>();
    let data = [half.clone(), half].concat();
    let shape = Shape {
        level: Level(9),
        strategy: CompressionStrategy::Default,
        flushes: vec![],
    };
    let z = compress(&data, &shape);
    let alone = compress(&data[..20_000], &shape);
    assert!(
        z.len() < alone.len() + 1000,
        "{} against {}",
        z.len(),
        alone.len()
    );
    assert_eq!(one_read(&z, data.len()).out, data);
}

#[test]
fn a_stream_cut_anywhere_inflates_as_one_read_and_as_the_oracle() {
    for (what, data, z) in corpus() {
        let end = every_split_and_size(&z, data.len(), &what);
        assert_eq!(end.verdict, Ok(()), "{what}");
        assert_eq!(end.out, data, "{what}");
        assert_eq!(end.excess, Excess(0), "{what}");
        let oracle = miniz_oxide::inflate::decompress_to_vec_zlib(&z).expect("the oracle reads it");
        assert_eq!(end.out, oracle, "{what}");
    }
}

#[test]
fn a_buffer_one_short_or_one_long_is_refused_wherever_the_reads_fall() {
    // The bomb bound: the stream fills exactly the length the caller
    // declared or is refused, long at the step that would overrun and short
    // at the final block.
    for (what, data, z) in corpus() {
        let long = every_split_and_size(&z, data.len() + 1, &what);
        assert_eq!(long.verdict, Err(ZlibError::Short), "{what}");
        if let Some(n) = data.len().checked_sub(1) {
            let short = every_split_and_size(&z, n, &what);
            assert_eq!(short.verdict, Err(ZlibError::Overrun), "{what}");
        }
    }
}

#[test]
fn bytes_after_the_trailer_are_counted_not_inflated() {
    for (what, data, z) in corpus().into_iter().take(6) {
        let mut tail = z.clone();
        tail.extend_from_slice(b"after");
        let end = every_split_and_size(&tail, data.len(), &what);
        assert_eq!(end.verdict, Ok(()), "{what}");
        assert_eq!(end.out, data, "{what}");
        assert_eq!(end.excess, Excess(5), "{what}");
        // And a feed after the end counts all it is given.
        let mut inflate = Inflate::NEW;
        let mut out = vec![0u8; data.len()];
        assert_eq!(inflate.feed(&z, &mut out), Ok(Inflated::Done));
        assert_eq!(inflate.feed(b"xyz", &mut out), Ok(Inflated::Done));
        assert_eq!(inflate.excess(), 3);
    }
}

#[test]
fn every_prefix_is_truncated_never_accepted() {
    for (what, data, z) in corpus() {
        for k in 0..z.len() {
            let end = one_read(&z[..k], data.len());
            assert_eq!(end.verdict, Err(ZlibError::Truncated), "{what}: {k} bytes");
        }
    }
}

/// `z` with CMF and FLG replaced, FCHECK recomputed so only the named field
/// is wrong.
fn with_header(z: &[u8], cmf: u8, flg_high: u8) -> Vec<u8> {
    let mut v = z.to_vec();
    let check = 31 - (u16::from(cmf) * 256 + u16::from(flg_high)) % 31;
    v[0] = cmf;
    v[1] = flg_high | u8::try_from(check % 31).expect("below 31");
    v
}

#[test]
fn the_header_rfc_1950_requires_a_check_of_is_checked() {
    // 2.3: "A compliant decompressor must check CMF, FLG, and ADLER32".
    let data = sample(300, 100, 9);
    let z = compress(
        &data,
        &Shape {
            level: Level(6),
            strategy: CompressionStrategy::Default,
            flushes: vec![],
        },
    );
    let n = data.len();
    // The control: miniz's own header, rebuilt, still reads.
    assert_eq!(
        one_read(&with_header(&z, z[0], z[1] & 0xe0), n).verdict,
        Ok(())
    );
    // Every window size 2.2 allows, CINFO 0 to 7, reads.
    for cinfo in 0..=7u8 {
        assert_eq!(
            one_read(&with_header(&z, (cinfo << 4) | 8, 0x80), n).verdict,
            Ok(()),
            "CINFO {cinfo}"
        );
    }
    // "Values of CINFO above 7 are not allowed in this version" (2.2).
    for cinfo in 8..=15u8 {
        let bad = with_header(&z, (cinfo << 4) | 8, 0x80);
        assert_eq!(
            every_split_and_size(&bad[..8], n, "CINFO").verdict,
            Err(ZlibError::Header),
            "CINFO {cinfo}"
        );
    }
    // CM other than 8, FDICT set, and FCHECK off by one.
    assert_eq!(
        one_read(&with_header(&z, 0x77, 0x80), n).verdict,
        Err(ZlibError::Header)
    );
    assert_eq!(
        one_read(&with_header(&z, 0x78, 0xa0), n).verdict,
        Err(ZlibError::Header)
    );
    let mut fcheck = with_header(&z, 0x78, 0x80);
    fcheck[1] ^= 1;
    assert_eq!(one_read(&fcheck, n).verdict, Err(ZlibError::Header));
    // The trailer: one flipped bit of the Adler-32, at every split.
    let mut adler = z.clone();
    *adler.last_mut().expect("a trailer") ^= 1;
    assert_eq!(
        every_split_and_size(&adler, n, "adler").verdict,
        Err(ZlibError::Adler)
    );
}

#[test]
fn a_reference_before_the_start_and_a_bad_stored_length_are_refused() {
    // Hand-written fixed blocks (3.2.6): BFINAL 1, BTYPE 01, then length
    // code 257 (3 bytes, seven bits 0000001) and distance code 0 (one byte
    // back, five bits 00000) with nothing written yet.
    let fixed = [0x78, 0x01, 0x03, 0x02, 0x00];
    assert_eq!(
        every_split_and_size(&fixed, 3, "distance").verdict,
        Err(ZlibError::Distance)
    );
    // A stored block whose NLEN is not LEN's complement.
    let stored = [0x78, 0x01, 0x01, 0x01, 0x00, 0xfe, 0xfe, 0x41];
    assert_eq!(
        every_split_and_size(&stored, 1, "nlen").verdict,
        Err(ZlibError::Block)
    );
    // BTYPE 11 is reserved.
    let reserved = [0x78, 0x01, 0x07];
    assert_eq!(
        every_split_and_size(&reserved, 1, "btype").verdict,
        Err(ZlibError::Block)
    );
}

/// Every PNG under the repository's own directories, sorted: the font
/// fixtures, the cursor captures and the sources' images (185 tracked).
fn repo_pngs() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut v = Vec::new();
    walk(&root, &mut v, &|p| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        !matches!(name, "target" | ".git" | ".claude" | ".devenv" | ".direnv")
    });
    v.sort();
    v
}

/// Every `.png` file under `dir` whose directories `enter` allows.
fn walk(dir: &Path, out: &mut Vec<PathBuf>, enter: &dyn Fn(&Path) -> bool) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let Ok(t) = e.file_type() else { continue };
        let p = e.path();
        if t.is_dir() && enter(&p) {
            walk(&p, out, enter);
        } else if t.is_file() && p.extension().is_some_and(|x| x == "png") {
            out.push(p);
        }
    }
}

/// A stream short enough, inflating to little enough, is split everywhere
/// and read at every size; any other at five points and two read sizes,
/// which keeps the debug-built suite to seconds. The ignored system test
/// and the generated properties carry the rest.
fn cut_every_way_it_can(z: &[u8], n: usize, what: &str) -> End {
    if z.len() <= 2048 && n <= 65_536 {
        return every_split_and_size(z, n, what);
    }
    let whole = one_read(z, n);
    let len = z.len();
    for k in [1, 7, len / 3, len / 2, len - 1] {
        assert_eq!(feed_cut(z, n, &[k]), whole, "{what}: split {k}");
    }
    let smallest = if len <= 65_536 { 1 } else { 7 };
    for size in [smallest, 4093] {
        assert_eq!(reads_of(z, n, size), whole, "{what}: reads {size}");
    }
    whole
}

/// A PNG's `IDAT` data, concatenated: its zlib stream.
fn idat_stream(png: &[u8]) -> Vec<u8> {
    let mut at = 8;
    let mut z = Vec::new();
    while let Some(head) = png.get(at..at + 8) {
        let len = usize::try_from(u32::from_be_bytes(head[..4].try_into().expect("four")))
            .expect("a length");
        if &head[4..] == b"IDAT" {
            z.extend_from_slice(&png[at + 8..at + 8 + len]);
        }
        at += 12 + len;
    }
    z
}

#[test]
fn the_repo_pngs_cut_anywhere_decode_as_one_read_and_the_oracle() {
    let files = repo_pngs();
    assert!(files.len() >= 185, "found {}", files.len());
    for p in files {
        let bytes = std::fs::read(&p).expect("readable");
        agrees(&bytes, &p.display().to_string()).expect("the oracle reads every one");
    }
}

/// `Png` against the `png` crate on one file, and its zlib stream cut every
/// way it can be against one read: `None` where both refuse it, a panic
/// where only one does or they differ.
fn agrees(bytes: &[u8], what: &str) -> Option<()> {
    let theirs = oracle(bytes);
    let mine = Png::parse(bytes).and_then(|png| {
        let (r, a) = (png.raw_len().unwrap_or(0), png.rgba_len().unwrap_or(0));
        let (mut raw, mut rgba) = (vec![0u8; r], vec![0u8; a]);
        png.decode(&mut raw, &mut rgba)?;
        Ok((r, rgba))
    });
    match (mine, theirs) {
        (Ok((r, rgba)), Some(t)) => {
            assert_eq!(rgba, t, "{what}");
            let end = cut_every_way_it_can(&idat_stream(bytes), r, what);
            assert_eq!(end.verdict, Ok(()), "{what}");
            Some(())
        }
        (Err(_), None) => None,
        (Err(e), Some(_)) => panic!("refused, the oracle reads it: {what} {e:?}"),
        (Ok(_), None) => panic!("read, the oracle refuses it: {what}"),
    }
}

/// Every PNG under the system's own directories, sorted, every `step`-th.
fn system_pngs(step: usize) -> Vec<PathBuf> {
    let mut v = Vec::new();
    for root in ["/System/Library", "/Applications"] {
        walk(Path::new(root), &mut v, &|_| true);
    }
    v.sort();
    v.into_iter().step_by(step.max(1)).collect()
}

/// The PNGs embedded in the fonts under the system's font directories, cut
/// out by signature and walked to `IEND`: `sbix` and `CBDT` strikes are
/// PNG files stored whole. Sorted by font, every `step`-th.
fn font_pngs(step: usize) -> Vec<(String, Vec<u8>)> {
    let mut fonts = Vec::new();
    for root in ["/System/Library/Fonts", "/Library/Fonts"] {
        walk_fonts(Path::new(root), &mut fonts);
    }
    fonts.sort();
    let mut out = Vec::new();
    for f in fonts {
        let Ok(data) = std::fs::read(&f) else {
            continue;
        };
        let mut from = 0;
        while let Some(i) = data[from..].windows(8).position(|w| w == SIGNATURE) {
            let start = from + i;
            from = start + 8;
            if let Some(end) = png_end(&data, start) {
                out.push((
                    format!("{}@{start}", f.display()),
                    data[start..end].to_vec(),
                ));
                from = end;
            }
        }
    }
    out.into_iter().step_by(step.max(1)).collect()
}

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

fn walk_fonts(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_fonts(&p, out);
        } else if p
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| matches!(x.to_ascii_lowercase().as_str(), "ttf" | "ttc" | "otf"))
        {
            out.push(p);
        }
    }
}

/// Where the PNG starting at `start` ends: past its `IEND` chunk.
fn png_end(data: &[u8], start: usize) -> Option<usize> {
    let mut at = start + 8;
    loop {
        let head = data.get(at..at + 8)?;
        let len = usize::try_from(u32::from_be_bytes(head[..4].try_into().ok()?)).ok()?;
        let next = at.checked_add(12)?.checked_add(len)?;
        if next > data.len() {
            return None;
        }
        if &head[4..] == b"IEND" {
            return Some(next);
        }
        at = next;
    }
}

/// The system's PNG files and the PNGs inside its fonts, every `PNG_STEP`-th
/// of each (default 10), through [`agrees`]. Machine-specific, so ignored.
#[test]
#[ignore = "reads /System/Library, /Library/Fonts and /Applications"]
fn the_system_pngs_cut_anywhere_decode_as_one_read_and_the_oracle() {
    let step = std::env::var("PNG_STEP")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    let (mut files, mut agreed) = (0, 0);
    for p in system_pngs(step) {
        let Ok(bytes) = std::fs::read(&p) else {
            continue;
        };
        files += 1;
        agreed += usize::from(agrees(&bytes, &p.display().to_string()).is_some());
    }
    eprintln!(
        "{files} files: {agreed} agree, {} refused by both",
        files - agreed
    );
    let (mut strikes, mut read) = (0, 0);
    for (what, bytes) in font_pngs(step) {
        strikes += 1;
        read += usize::from(agrees(&bytes, &what).is_some());
    }
    eprintln!(
        "{strikes} font PNGs: {read} agree, {} refused by both",
        strikes - read
    );
    assert!(agreed > 0 && read > 0);
}

/// RGBA from the `png` crate, expanded and stripped to 8 bits as `Png`
/// decodes, or `None` where it refuses the file.
fn oracle(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut dec = png::Decoder::new(bytes);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let px = &buf[..info.buffer_size()];
    Some(match info.color_type {
        png::ColorType::Grayscale => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => px
            .chunks_exact(2)
            .flat_map(|c| [c[0], c[0], c[0], c[1]])
            .collect(),
        png::ColorType::Rgb => px
            .chunks_exact(3)
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect(),
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Indexed => return None,
    })
}

/// A generated stream's data, shape and cuts.
fn streams() -> impl Strategy<Value = (Vec<u8>, Shape, Vec<usize>)> {
    let strategy = prop_oneof![
        Just(CompressionStrategy::Default),
        Just(CompressionStrategy::Fixed),
        Just(CompressionStrategy::HuffmanOnly),
        Just(CompressionStrategy::RLE),
        Just(CompressionStrategy::Filtered),
    ];
    (
        0usize..3000,
        1usize..4000,
        any::<u64>(),
        0u8..=10,
        strategy,
        prop::collection::vec(0usize..3000, 0..4),
        prop::collection::vec(0usize..2000, 0..12),
    )
        .prop_map(|(len, far, seed, level, strategy, flushes, cuts)| {
            let data = sample(len, far, seed);
            let mut cuts = cuts;
            cuts.sort_unstable();
            (
                data,
                Shape {
                    level: Level(level),
                    strategy,
                    flushes,
                },
                cuts,
            )
        })
}

proptest! {
    /// Generated streams, cut at generated points, against one read and
    /// against the data they were made from.
    #[test]
    fn a_generated_stream_cut_anywhere_inflates_as_one_read((data, shape, cuts) in streams()) {
        let z = compress(&data, &shape);
        let whole = one_read(&z, data.len());
        prop_assert_eq!(&whole.verdict, &Ok(()));
        prop_assert_eq!(&whole.out, &data);
        prop_assert_eq!(feed_cut(&z, data.len(), &cuts), whole);
    }

    /// The hostile half: a valid stream with bytes overwritten, cut short or
    /// run on, which mostly reaches inflate's refusals rather than its happy
    /// path. Whatever the verdict, it is the same wherever the reads fall,
    /// and nothing panics; a stream it accepts, the oracle reads the same.
    #[test]
    fn a_damaged_stream_gets_one_verdict_wherever_the_reads_fall(
        (data, shape, cuts) in streams(),
        hits in prop::collection::vec((0usize..4000, any::<u8>()), 0..4),
        cut in prop::option::of(0usize..4000),
        slack in 0usize..3,
    ) {
        let mut z = compress(&data, &shape);
        for (at, b) in hits {
            let len = z.len().max(1);
            if let Some(slot) = z.get_mut(at % len) {
                *slot ^= b;
            }
        }
        if let Some(c) = cut {
            z.truncate(c);
        }
        let n = (data.len() + slack).saturating_sub(1);
        let whole = one_read(&z, n);
        prop_assert_eq!(feed_cut(&z, n, &cuts), one_read(&z, n));
        if whole.verdict.is_ok() {
            let theirs = miniz_oxide::inflate::decompress_to_vec_zlib(&z);
            prop_assert_eq!(theirs.ok(), Some(whole.out));
        }
    }
}
