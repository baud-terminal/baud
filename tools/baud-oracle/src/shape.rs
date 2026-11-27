//! **`baud-oracle shape-helpers`: what `tools/scripts/gen-shape-fixtures.sh`
//! asked Python for**, between its `hb-shape` calls. hb-shape stays the
//! oracle; these only prepare its input and label its output, and each is
//! the port of the inline block it replaces, output for output:
//!
//! | helper | prints |
//! |---|---|
//! | `fnv64 <file>` | the file's 64-bit FNV-1a, 16 hex digits, the fixtures' digest column |
//! | `cps <text>` | the text's code points, `%04X`, space-separated |
//! | `rename-table <in> <out> <from> <to>` | nothing; writes `<in>` with every table record tagged `<from>` retagged `<to>` |
//! | `morx-features <font> <index>` | the `--features` that leave HarfBuzz at a face's `morx` chain defaults |
//! | `meteg` | the meteg fixture's sequences, `<faces>\t<code points>` |
//! | `devanagari <page>...` | the Devanagari fixture's lines, from the specification's lines on stdin and the pinned pages' words |
//!
//! None reads a font through baud-font, so a fixture's labels do not come
//! from the code the fixture checks. Each refuses, with exit code 1, where
//! its Python stopped.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

/// FNV-1a, 64-bit, over the bytes.
pub fn fnv64(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// A command-line argument as Python's `sys.argv` held it: UTF-8, with
/// each byte that is not part of a well-formed sequence made the lone
/// surrogate U+DC80-U+DCFF (`surrogateescape`), as code point numbers,
/// since a surrogate is no Rust `char`.
pub fn argv_code_points(arg: &OsString) -> Vec<u32> {
    let bytes = arg.as_encoded_bytes();
    let mut v = Vec::new();
    for chunk in bytes.utf8_chunks() {
        v.extend(chunk.valid().chars().map(u32::from));
        v.extend(chunk.invalid().iter().map(|&b| 0xdc00 + u32::from(b)));
    }
    v
}

/// `" ".join("%04X" % ord(c) for c in text)`.
pub fn cps(points: impl IntoIterator<Item = u32>) -> String {
    points
        .into_iter()
        .map(|c| format!("{c:04X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The block that renamed a table: `numTables` read from the directory,
/// and every record whose tag is `from` given `to`, the rest of the file
/// untouched.
pub fn rename_table(mut data: Vec<u8>, from: &[u8], to: &[u8]) -> Result<Vec<u8>, String> {
    let n = data
        .get(4..6)
        .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))
        .ok_or("struct.error: unpack requires a buffer of 2 bytes")?;
    for i in 0..n {
        let o = 12 + 16 * i;
        if data.get(o..o + 4) == Some(from) {
            if to.len() != 4 {
                return Err("a tag is four bytes".to_string());
            }
            data[o..o + 4].copy_from_slice(to);
        }
    }
    Ok(data)
}

fn u16_at(d: &[u8], o: usize) -> Result<u16, String> {
    d.get(o..o + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .ok_or_else(|| format!("a read past the end at {o}"))
}

fn u32_at(d: &[u8], o: usize) -> Result<u32, String> {
    d.get(o..o + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| format!("a read past the end at {o}"))
}

/// A table's bytes from the face's table directory at `dir`; `None` if
/// the face has no such table.
fn table<'a>(d: &'a [u8], dir: usize, tag: &[u8; 4]) -> Result<Option<&'a [u8]>, String> {
    let n = usize::from(u16_at(d, dir + 4)?);
    for i in 0..n {
        let r = dir + 12 + 16 * i;
        if d.get(r..r + 4) == Some(tag) {
            let off = u32_at(d, r + 8)? as usize;
            let len = u32_at(d, r + 12)? as usize;
            return d
                .get(off..off + len)
                .map(Some)
                .ok_or_else(|| format!("{} past the end", String::from_utf8_lossy(tag)));
        }
    }
    Ok(None)
}

/// One `morx` chain: its default flags and its feature entries, as
/// `(type, setting, enable, disable)`.
pub struct Chain {
    /// `defaultFlags`.
    pub default: u32,
    /// The feature subtable entries.
    pub features: Vec<(u16, u16, u32, u32)>,
}

/// The `feat` table's feature types and the `morx` chains of face `index`
/// of the file: a collection by the path's `.ttc` suffix, as the Python
/// chose `TTCollection`, else one face at offset 0.
pub fn aat(path: &Path, index: usize) -> Result<(HashSet<u16>, Vec<Chain>), String> {
    let d = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let dir = if path.to_string_lossy().ends_with(".ttc") {
        if d.get(0..4) != Some(b"ttcf") {
            return Err(format!("{}: not a collection", path.display()));
        }
        let n = u32_at(&d, 8)? as usize;
        if index >= n {
            return Err("IndexError: list index out of range".to_string());
        }
        u32_at(&d, 12 + 4 * index)? as usize
    } else {
        0
    };
    let mut exposed = HashSet::new();
    if let Some(feat) = table(&d, dir, b"feat")? {
        // version (4), featureNameCount, reserved (2 + 4), then 12-byte
        // FeatureName records led by their feature type.
        let n = usize::from(u16_at(feat, 4)?);
        for i in 0..n {
            exposed.insert(u16_at(feat, 12 + 12 * i)?);
        }
    }
    let morx = table(&d, dir, b"morx")?.ok_or("KeyError: 'morx'")?;
    // version, unused, nChains; each chain: defaultFlags, chainLength,
    // nFeatureEntries, nSubtables, then 12-byte feature entries.
    let n = u32_at(morx, 4)? as usize;
    let mut chains = Vec::with_capacity(n);
    let mut at = 8;
    for _ in 0..n {
        let default = u32_at(morx, at)?;
        let len = u32_at(morx, at + 4)? as usize;
        let m = u32_at(morx, at + 8)? as usize;
        let mut features = Vec::with_capacity(m);
        for j in 0..m {
            let f = at + 16 + 12 * j;
            features.push((
                u16_at(morx, f)?,
                u16_at(morx, f + 2)?,
                u32_at(morx, f + 4)?,
                u32_at(morx, f + 8)?,
            ));
        }
        chains.push(Chain { default, features });
        at += len;
    }
    Ok((exposed, chains))
}

/// A `morx` feature selector: (feature type, feature setting).
type Selector = (u16, u16);

/// HarfBuzz's default requests that map onto `morx`, as (tag, on
/// selector, off selector).
const DEFAULTS: [(&str, Selector, Selector); 4] = [
    ("liga", (1, 2), (1, 3)),
    ("clig", (1, 18), (1, 19)),
    ("calt", (36, 0), (36, 1)),
    ("rlig", (1, 0), (1, 1)),
];

/// A chain's flags with these selectors requested, from its defaults.
fn compile_flags(chain: &Chain, requested: &HashSet<Selector>) -> u32 {
    chain
        .features
        .iter()
        .fold(chain.default, |flags, &(t, s, enable, disable)| {
            if requested.contains(&(t, s)) {
                (flags & disable) | enable
            } else {
                flags
            }
        })
}

/// The `--features` that leave HarfBuzz at the chains' default flags: its
/// default requests that would move a flag cancelled, `kern` off, and an
/// error if what is left would still move one.
pub fn morx_features(path: &Path, index: usize) -> Result<String, String> {
    let (exposed, chains) = aat(path, index)?;
    let mut off: Vec<(&str, Selector)> = Vec::new();
    for &(tag, on, off_sel) in &DEFAULTS {
        if !exposed.contains(&on.0) {
            continue;
        }
        let one = HashSet::from([on]);
        if chains.iter().any(|c| compile_flags(c, &one) != c.default) {
            off.push((tag, off_sel));
        }
    }
    let cancelled: HashSet<&str> = off.iter().map(|&(t, _)| t).collect();
    let mut requested: HashSet<Selector> = DEFAULTS
        .iter()
        .filter(|(tag, on, _)| exposed.contains(&on.0) && !cancelled.contains(tag))
        .map(|&(_, on, _)| on)
        .collect();
    requested.extend(off.iter().map(|&(_, sel)| sel));
    for c in &chains {
        let got = compile_flags(c, &requested);
        if got != c.default {
            return Err(format!(
                "{}: HarfBuzz would reach flags {got:#x} against the chain's {:#x}",
                path.display(),
                c.default
            ));
        }
    }
    let mut s: String = off.iter().map(|(t, _)| format!("-{t},")).collect();
    s += "-kern";
    Ok(s)
}

/// The meteg fixture's sequences: every letter with every vowel meteg may
/// follow, on each side, for all four faces; then bet, qof, final nun and
/// shin with its dot, with a dagesh after the vowel and before it, for the
/// upright faces.
pub fn meteg() -> String {
    const VOWELS: [u32; 11] = [
        0x05B0, 0x05B1, 0x05B2, 0x05B3, 0x05B4, 0x05B5, 0x05B6, 0x05B7, 0x05B8, 0x05BB, 0x05C7,
    ];
    let mut out = String::new();
    for u in 0x05D0..0x05EB {
        for v in VOWELS {
            out += &format!("all\t{}\n", cps([u, v, 0x05BD]));
            out += &format!("all\t{}\n", cps([u, 0x05BD, 0x034F, v]));
        }
    }
    let bases: [&[u32]; 4] = [&[0x05D1], &[0x05E7], &[0x05DF], &[0x05E9, 0x05C1]];
    for u in bases {
        for v in VOWELS {
            let a: Vec<u32> = u.iter().copied().chain([v, 0x05BC, 0x05BD]).collect();
            let b: Vec<u32> = u.iter().copied().chain([0x05BC, v, 0x05BD]).collect();
            out += &format!("upright\t{}\n", cps(a));
            out += &format!("upright\t{}\n", cps(b));
        }
    }
    out
}

/// Python's `\s` on a `str`: `str.isspace`, which is Rust's White_Space
/// and the four information separators U+001C-U+001F besides.
pub fn py_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// The word separators of the Devanagari block's `re.split` class, `\s`
/// aside: danda, double danda, ASCII punctuation, dashes and quotes.
const SEPARATORS: &str = "।॥,;:!?-—–‘’“”\"'()[]{}|.";

/// `re.sub(open + ".*?" + close, " ", text, flags=re.S)`, and with an
/// empty `open` the `<[^>]*>` form: each leftmost shortest match made one
/// space.
fn strip_spans(text: &str, open: &str, close: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(open) {
        let after = &rest[i + open.len()..];
        let Some(j) = after.find(close) else {
            break;
        };
        out += &rest[..i];
        out.push(' ');
        rest = &after[j + close.len()..];
    }
    out + rest
}

/// The words of a page, as the block took them: tags and `<noinclude>`
/// spans made spaces, the text split at whitespace and punctuation, ZWNJ
/// and ZWJ stripped from each word's ends, and only the words holding a
/// Devanagari letter kept. The page is refused if it holds a character
/// reference, which `html.unescape` would have decoded and this port does
/// not; neither pinned page holds one.
pub fn page_words(data: &[u8]) -> Result<Vec<String>, String> {
    let text = std::str::from_utf8(data).map_err(|e| format!("UnicodeDecodeError: {e}"))?;
    if text.contains('&') {
        return Err(
            "the page holds a character reference, which this port does not decode".to_string(),
        );
    }
    let text = strip_spans(text, "<noinclude>", "</noinclude>");
    let text = strip_spans(&text, "<", ">");
    Ok(text
        .split(|c: char| py_space(c) || SEPARATORS.contains(c))
        .map(|w| w.trim_matches(['\u{200c}', '\u{200d}']).to_string())
        .filter(|w| w.chars().any(|c| ('\u{0900}'..='\u{097f}').contains(&c)))
        .collect())
}

/// The Devanagari fixture's lines: the specification's lines, then every
/// page's new words, each as code points, each distinct spelling once.
pub fn devanagari(spec: &str, pages: &[Vec<u8>]) -> Result<String, String> {
    let lines = spec.split('\n').filter(|l| !l.is_empty());
    let mut words: Vec<String> = Vec::new();
    for p in pages {
        for w in page_words(p)? {
            if !words.contains(&w) {
                words.push(w);
            }
        }
    }
    let mut seen = HashSet::new();
    let mut out = String::new();
    for l in lines.chain(words.iter().map(String::as_str)) {
        let c = cps(l.chars().map(u32::from));
        if seen.insert(c.clone()) {
            out += &c;
            out.push('\n');
        }
    }
    Ok(out)
}

/// `baud-oracle shape-helpers`'s arguments.
pub fn main(args: &[OsString]) -> ExitCode {
    let s = |i: usize| args.get(i).map(|a| a.to_string_lossy().into_owned());
    let result: Result<Option<String>, String> = match s(0).as_deref() {
        Some("fnv64") if args.len() == 2 => std::fs::read(&args[1])
            .map(|d| Some(format!("{:016x}\n", fnv64(&d))))
            .map_err(|e| format!("{}: {e}", args[1].to_string_lossy())),
        Some("cps") if args.len() == 2 => Ok(Some(cps(argv_code_points(&args[1])) + "\n")),
        Some("rename-table") if args.len() == 5 => std::fs::read(&args[1])
            .map_err(|e| format!("{}: {e}", args[1].to_string_lossy()))
            .and_then(|d| rename_table(d, args[3].as_encoded_bytes(), args[4].as_encoded_bytes()))
            .and_then(|d| std::fs::write(&args[2], d).map_err(|e| e.to_string()))
            .map(|()| None),
        Some("morx-features") if args.len() == 3 => s(2)
            .and_then(|i| i.trim().parse::<usize>().ok())
            .ok_or_else(|| "a face index".to_string())
            .and_then(|i| morx_features(Path::new(&args[1]), i))
            .map(|f| Some(f + "\n")),
        Some("meteg") if args.len() == 1 => Ok(Some(meteg())),
        Some("devanagari") if args.len() >= 2 => {
            let mut spec = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut spec)
                .map_err(|e| format!("stdin: {e}"))
                .and_then(|_| {
                    args[1..]
                        .iter()
                        .map(|p| {
                            std::fs::read(p).map_err(|e| format!("{}: {e}", p.to_string_lossy()))
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .and_then(|pages| devanagari(&spec, &pages))
                .map(Some)
        }
        _ => {
            eprintln!(
                "usage: baud-oracle shape-helpers fnv64 <file> | cps <text> | \
                 rename-table <in> <out> <from> <to> | morx-features <font> <index> | \
                 meteg | devanagari <page>... (the specification on stdin)"
            );
            return ExitCode::FAILURE;
        }
    };
    match result {
        Ok(text) => {
            if let Some(t) = text {
                print!("{t}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The digest of the empty input and of `a`, FNV's own test vectors.
    #[test]
    fn fnv64_is_fnv_1a() {
        assert_eq!(fnv64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    /// Every code point CPython's `re` matches with `\s` in a `str`
    /// pattern, enumerated over all of Unicode by CPython 3.14: the set
    /// `py_space` must be, and not Rust's `is_whitespace`, which leaves
    /// U+001C-U+001F out.
    #[test]
    fn py_space_is_python_s_class() {
        const PYTHON: [u32; 29] = [
            0x9, 0xa, 0xb, 0xc, 0xd, 0x1c, 0x1d, 0x1e, 0x1f, 0x20, 0x85, 0xa0, 0x1680, 0x2000,
            0x2001, 0x2002, 0x2003, 0x2004, 0x2005, 0x2006, 0x2007, 0x2008, 0x2009, 0x200a, 0x2028,
            0x2029, 0x202f, 0x205f, 0x3000,
        ];
        let have: Vec<u32> = (0..=0x10ffffu32)
            .filter_map(char::from_u32)
            .filter(|&c| py_space(c))
            .map(u32::from)
            .collect();
        assert_eq!(have, PYTHON);
        assert!(!'\u{1c}'.is_whitespace(), "the control: Rust's set differs");
    }

    #[test]
    fn spans_go_leftmost_shortest() {
        assert_eq!(strip_spans("a<b>c<d>e", "<", ">"), "a c e");
        assert_eq!(strip_spans("a<b c", "<", ">"), "a<b c");
        assert_eq!(
            strip_spans(
                "x<noinclude>1</noinclude>y<noinclude>2",
                "<noinclude>",
                "</noinclude>"
            ),
            "x y<noinclude>2"
        );
    }

    /// **Positive controls**: a character reference, which the port does
    /// not decode, and a page that is not UTF-8 are refused.
    #[test]
    fn a_page_the_port_cannot_read_is_refused() {
        assert!(page_words("क &amp; ख".as_bytes()).is_err());
        assert!(page_words(b"\xff").is_err());
        assert_eq!(page_words("‍क‌, x (ख)।".as_bytes()).unwrap(), ["क", "ख"]);
    }

    #[test]
    fn an_argument_keeps_its_bad_bytes_as_surrogates() {
        use std::os::unix::ffi::OsStringExt;
        let a = OsString::from_vec(b"a\xffb".to_vec());
        assert_eq!(argv_code_points(&a), [0x61, 0xdcff, 0x62]);
    }

    /// A face with one `feat` type and one `morx` chain whose default
    /// flags are 1, the chain carrying these feature entries: the same
    /// bytes as the synthetic fonts fontTools read on the Python arm.
    fn synthetic(features: &[(u16, u16, u32, u32)]) -> Vec<u8> {
        let mut feat = Vec::new();
        feat.extend(0x0001_0000u32.to_be_bytes());
        feat.extend(1u16.to_be_bytes());
        feat.extend([0u8; 6]);
        feat.extend(1u16.to_be_bytes());
        feat.extend(0u16.to_be_bytes());
        feat.extend(24u32.to_be_bytes());
        feat.extend(0u16.to_be_bytes());
        feat.extend(256i16.to_be_bytes());
        let mut morx = Vec::new();
        morx.extend(2u16.to_be_bytes());
        morx.extend(0u16.to_be_bytes());
        morx.extend(1u32.to_be_bytes());
        let len = u32::try_from(16 + 12 * features.len()).unwrap();
        let n = u32::try_from(features.len()).unwrap();
        for v in [1, len, n, 0] {
            morx.extend(v.to_be_bytes());
        }
        for &(t, s, e, d) in features {
            morx.extend(t.to_be_bytes());
            morx.extend(s.to_be_bytes());
            morx.extend(e.to_be_bytes());
            morx.extend(d.to_be_bytes());
        }
        let mut f = Vec::new();
        f.extend(0x0001_0000u32.to_be_bytes());
        for v in [2u16, 32, 1, 0] {
            f.extend(v.to_be_bytes());
        }
        let mut body = Vec::new();
        let base = 12 + 16 * 2;
        for (tag, data) in [(b"feat", &feat), (b"morx", &morx)] {
            f.extend(tag);
            f.extend(0u32.to_be_bytes());
            f.extend(u32::try_from(base + body.len()).unwrap().to_be_bytes());
            f.extend(u32::try_from(data.len()).unwrap().to_be_bytes());
            body.extend(data.iter());
            body.resize(body.len().next_multiple_of(4), 0);
        }
        f.extend(body);
        f
    }

    fn features_of(name: &str, font: &[u8]) -> Result<String, String> {
        let p = std::env::temp_dir().join(format!("baud-oracle-{name}-{}.ttf", std::process::id()));
        std::fs::write(&p, font).unwrap();
        let r = morx_features(&p, 0).map_err(|e| e.replace(&p.display().to_string(), "<font>"));
        let _ = std::fs::remove_file(&p);
        r
    }

    /// **The refusal, reached**: no system face takes it, so a chain whose
    /// off selector does not restore its defaults is built; fontTools' arm
    /// printed the same message and exit code on the same bytes. Beside it
    /// a chain whose off selector does restore them, which cancels `liga`.
    #[test]
    fn a_request_that_moves_a_flag_either_way_is_refused() {
        let stuck = synthetic(&[(1, 2, 2, u32::MAX), (1, 3, 4, u32::MAX)]);
        assert_eq!(
            features_of("stuck", &stuck),
            Err("<font>: HarfBuzz would reach flags 0x5 against the chain's 0x1".to_string())
        );
        let liga = synthetic(&[(1, 2, 2, u32::MAX), (1, 3, 0, 0xffff_fffd)]);
        assert_eq!(features_of("liga", &liga).as_deref(), Ok("-liga,-kern"));
        let plain = synthetic(&[]);
        assert_eq!(features_of("plain", &plain).as_deref(), Ok("-kern"));
    }

    /// The meteg block printed 682 lines: 27 letters by 11 vowels by two
    /// sides, then four bases by 11 vowels by two dagesh places.
    #[test]
    fn meteg_is_the_block_s_count() {
        let m = meteg();
        assert_eq!(m.lines().count(), 27 * 11 * 2 + 4 * 11 * 2);
        assert!(m.starts_with("all\t05D0 05B0 05BD\nall\t05D0 05BD 034F 05B0\n"));
        assert!(m.ends_with("upright\t05E9 05C1 05BC 05C7 05BD\n"));
    }

    #[test]
    fn a_rename_touches_only_the_tag() {
        let mut d = vec![0u8; 12 + 32];
        d[5] = 2;
        d[12..16].copy_from_slice(b"GPOS");
        d[28..32].copy_from_slice(b"GSUB");
        let r = rename_table(d.clone(), b"GPOS", b"GPOZ").unwrap();
        assert_eq!(&r[12..16], b"GPOZ");
        assert_eq!(&r[28..32], b"GSUB");
        assert_eq!(r[16..], d[16..]);
    }
}
