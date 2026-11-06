//! `cargo xtask pdf-search`: the pinned documents through their controls,
//! and one perturbation per branch of the extractors and of the command
//! line. Every expected output on the tree is what the Python scripts
//! printed; the extractor branches are held on PDFs built here, where the
//! control can be made to fail.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use xtask::features_since::Exit;
use xtask::html::Count;
use xtask::pdf::{self, Stop};
use xtask::pdf_search::{self, DOCUMENTS, Document, Form, Method, parse, parse_flate};
use xtask::{md5, sha256};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

struct Ran {
    exit: Exit,
    out: String,
    err: String,
}

fn run(args: &[&str]) -> Ran {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let exit = pdf_search::run(&root(), args, &mut out, &mut err).expect("the writers");
    Ran {
        exit,
        out: String::from_utf8(out).expect("utf-8"),
        err: String::from_utf8(err).expect("utf-8"),
    }
}

fn document(key: &str) -> Document {
    *DOCUMENTS
        .iter()
        .find(|d| d.key == key)
        .expect("a document's row")
}

fn searched(key: &str, data: &[u8], form: &Form) -> Ran {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let exit = pdf_search::search(&document(key), "x.pdf", data, form, &mut out, &mut err)
        .expect("the writers");
    Ran {
        exit,
        out: String::from_utf8(out).expect("utf-8"),
        err: String::from_utf8(err).expect("utf-8"),
    }
}

fn digest(s: &str) -> String {
    sha256::hex(s.as_bytes())
}

// ------------------------------------------------------------------ the tree

/// The positive control per PDF: every document in the tree, through every
/// control its row names.
#[test]
fn every_tracked_document_fires_its_controls() {
    let mut read = 0;
    for doc in &DOCUMENTS {
        if matches!(doc.key, "parabix" | "pdf-reference-1-4") {
            continue;
        }
        let r = run(&[doc.key, "zzqqxx"]);
        assert_eq!(r.exit, Exit(0), "{}: {}{}", doc.key, r.out, r.err);
        for c in doc.controls {
            let line = format!("  control {c:<22} HIT\n");
            assert!(r.out.contains(&line), "{} {c}: {}", doc.key, r.out);
        }
        assert!(r.out.contains("  probe   zzqqxx                 MISS\n"));
        read += 1;
    }
    assert_eq!(read, 9);
}

/// A PDF committed without a row would be a document nothing can search.
#[test]
fn every_committed_pdf_has_a_row() {
    let manifest = fs::read_to_string(root().join(xtask::sources::MANIFEST)).expect("the manifest");
    let entries = xtask::sources::line_reader(&manifest);
    let mut files = 0;
    for entry in fs::read_dir(root().join(xtask::sources::SRC)).expect("the sources") {
        let name = entry.expect("an entry").file_name();
        let name = name.to_string_lossy();
        if !name.ends_with(".pdf") {
            continue;
        }
        files += 1;
        let keyed = entries.iter().find(|e| e.file == name).expect("pinned");
        assert!(
            DOCUMENTS.iter().any(|d| d.key == keyed.key),
            "{name} ({}) has no row in DOCUMENTS",
            keyed.key
        );
    }
    assert!(files >= 9, "the walk found {files}");
}

#[test]
fn ecma48s_survey_is_the_scripts() {
    let r = run(&["ecma48"]);
    assert_eq!(r.exit, Exit(0));
    assert_eq!(
        r.out,
        "positive control -- these are known present:
  HIT  selectgraphicrendition   x6
  HIT  blink                    x3
  HIT  negativeimage            x1
  HIT  underline                x6
  HIT  italicized               x4
  HIT  concealed                x1

probes -- these were once reported absent:
  HIT  fraktur                  x2
  HIT  gothic                   x1
  HIT  proportional             x2
  HIT  framed                   x2
  HIT  encircled                x2
  HIT  overlined                x2
  HIT  notframed                x1
  HIT  notoverlined             x1
  HIT  ideogram                 x5
  HIT  stressmarking            x1

controls passed, so a MISS above would be real absence.
"
    );
}

#[test]
fn ecma48s_other_forms_are_the_scripts() {
    let r = run(&["ecma48", "--find", "stress marking"]);
    assert_eq!(r.exit, Exit(0));
    assert!(r.out.starts_with(
        "HIT  stress marking x1\n\ne 63 id eogram doubl e overl ine or doubl eline on thele ft si"
    ));
    assert!(
        r.out.ends_with("SET CHARACTER SPAC ING (SCS) or \n"),
        "{}",
        r.out
    );

    let r = run(&["ecma48", "--find", "xyzzyplugh"]);
    assert_eq!((r.exit, r.out.as_str()), (Exit(1), "MISS xyzzyplugh x0\n"));

    let r = run(&["ecma48", "--section", "sgr"]);
    assert_eq!(r.exit, Exit(0));
    assert_eq!(
        digest(&r.out),
        "2198885f3d9ca8bbe25e490c2b1af1536a90bee3a94cd0d9276859495ea75e54"
    );

    let r = run(&["ecma48", "--dump"]);
    assert_eq!(r.exit, Exit(0));
    assert_eq!(
        digest(&r.out),
        "6b7d2d2271ca9e1c17463899a2bf024aebdb72dd3e2bbc7bdccd31d6f80affa1"
    );
}

#[test]
fn the_lzw_forms_are_the_scripts() {
    let r = run(&["ecma35", "--find", "DCSM"]);
    assert_eq!(r.exit, Exit(0));
    assert_eq!(
        r.out,
        "control 'graphic character': HIT x141\n'DCSM': MISS x0\n"
    );

    let r = run(&["ecma-tr53", "--find", "free of charge"]);
    assert_eq!(r.exit, Exit(0));
    assert_eq!(
        r.out,
        "control 'graphic character': HIT x23\n'free of charge': HIT x1\n"
    );

    let r = run(&["ecma35"]);
    assert_eq!(
        digest(&r.out),
        "ffcb76b2ec851761a993aac7d3de9d2cad77730c0cc6cadf7c85e245de689da3"
    );
    let r = run(&["ecma-tr53"]);
    assert_eq!(
        digest(&r.out),
        "9166dcdd2a7ec3900eab2425b3d0ce4565c1b5f317a60c89fe70f79d30bb0de5"
    );
}

#[test]
fn the_encrypted_forms_are_the_scripts() {
    let r = run(&["adobe-5176-cff", "germandbls", "zzqqxx"]);
    assert_eq!(r.exit, Exit(0));
    assert_eq!(
        r.out,
        "adobe-5176-cff.pdf: 67901 chars of text from 75 decrypted streams
  control compactfontformat      HIT
  control charstrings            HIT
  control fdselect               HIT
  control privatedict            HIT
  control index                  HIT
  probe   germandbls             HIT
  probe   zzqqxx                 MISS
"
    );
    // Nine `/Length N 0 R` in this one, read short by the backtrack: the
    // stream count is what holds that.
    let r = run(&["adobe-5177-type2", "hstem", "zzqqxx"]);
    assert_eq!(r.exit, Exit(0));
    assert_eq!(
        r.out,
        "adobe-5177-type2.pdf: 47834 chars of text from 46 decrypted streams
  control type2charstring        HIT
  control hintmask               HIT
  control callsubr               HIT
  control endchar                HIT
  control transientarray         HIT
  probe   hstem                  HIT
  probe   zzqqxx                 MISS
"
    );
}

// --------------------------------------------------- what cannot be searched

#[test]
fn a_key_with_no_row_is_refused_and_says_why() {
    let r = run(&["ctlseqs", "x"]);
    assert_eq!(r.exit, Exit(2));
    assert!(r.err.contains("positive control"), "{}", r.err);
    assert_eq!(r.out, "");
}

#[test]
fn no_key_is_a_usage_error() {
    let r = run(&[]);
    assert_eq!(r.exit, Exit(2));
    assert!(r.err.starts_with("usage:"));
}

/// An `external` document is not in the repository, so it needs `--file`,
/// and any `--file` must be the pinned bytes.
#[test]
fn an_external_document_needs_a_copy_and_it_must_be_the_pinned_one() {
    let r = run(&["parabix", "x"]);
    assert_eq!(r.exit, Exit(2));
    assert!(r.err.contains("--file"), "{}", r.err);

    let r = run(&["ecma48", "--file"]);
    assert_eq!(r.exit, Exit(2));

    let r = run(&["ecma48", "--file", "/nonexistent/x.pdf", "framed"]);
    assert_eq!(r.exit, Exit(2));
    assert!(r.err.starts_with("missing "), "{}", r.err);

    let wrong = root().join("xtask/Cargo.toml");
    let r = run(&["ecma48", "--file", wrong.to_str().expect("utf-8"), "framed"]);
    assert_eq!(r.exit, Exit(2));
    assert!(
        r.err.contains("is not the document ecma48 pins"),
        "{}",
        r.err
    );
}

/// A `--file` that is the pinned bytes is read, and its own name printed.
#[test]
fn a_copy_of_the_pinned_bytes_is_read() {
    let tree = Tree::new();
    let copy = tree.0.join("renamed.pdf");
    let pinned = root().join("docs/research/sources/adobe-5177-type2.pdf");
    fs::copy(&pinned, &copy).expect("the copy");
    let r = run(&[
        "adobe-5177-type2",
        "--file",
        copy.to_str().expect("utf-8"),
        "hstem",
    ]);
    assert_eq!(r.exit, Exit(0));
    assert!(r.out.starts_with("renamed.pdf: 47834 chars"), "{}", r.out);
}

/// A directory under the system's temporary directory, removed on drop.
struct Tree(PathBuf);

impl Tree {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("pdf-search-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).expect("the directory");
        Self(dir)
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// --------------------------------------------------------- the command line

fn flate_form(args: &[&str]) -> Result<Form, Exit> {
    parse_flate(args, &document("ecma48")).map_err(|r| r.exit)
}

fn found(n: &str) -> Result<Form, Exit> {
    Ok(Form::Find(n.to_string()))
}

fn sgr() -> Result<Form, Exit> {
    Ok(Form::Section(document("ecma48").section.expect("sgr")))
}

fn terms(words: &[&str]) -> Result<Form, Exit> {
    Ok(Form::Terms(words.iter().map(ToString::to_string).collect()))
}

/// Each row was run against the script under Python 3.13.12: same exit,
/// same choice, but for the terms below.
#[test]
fn the_options_read_as_argparse_read_them() {
    let table: [(&[&str], Result<Form, Exit>); 41] = [
        (&[], Ok(Form::Survey)),
        (&["--dump"], Ok(Form::Dump)),
        (&["--d"], Ok(Form::Dump)),
        (&["--du"], Ok(Form::Dump)),
        (&["--dump", "--dump"], Ok(Form::Dump)),
        (&["--dump=x"], Err(Exit(2))),
        (&["--d=x"], Err(Exit(2))),
        (&["--find", "framed"], found("framed")),
        (&["--f", "framed"], found("framed")),
        (&["--fin=framed"], found("framed")),
        (&["--find=-x"], found("-x")),
        (&["--find=a=b"], found("a=b")),
        (&["--find", "-1"], found("-1")),
        (&["--find", "-1.5"], found("-1.5")),
        (&["--find", "-.5"], found("-.5")),
        (&["--find", "-a b"], found("-a b")),
        // Not a number to it, and nothing else this reads as a value.
        (&["--find", "-1_000"], Err(Exit(2))),
        (&["--find", "-x"], Err(Exit(2))),
        (&["--find", "--dump"], Err(Exit(2))),
        (&["--find"], Err(Exit(2))),
        (&["--find", "--"], Err(Exit(2))),
        (&["--find", "--", "x"], Err(Exit(2))),
        (&["--find", "x", "--"], Err(Exit(2))),
        // `if args.find:`, so no search at all.
        (&["--find="], Ok(Form::Survey)),
        (&["--find", ""], Ok(Form::Survey)),
        (&["--find", "a", "--find", ""], Ok(Form::Survey)),
        (&["--find", "a", "--find", "b"], found("b")),
        (&["--section", "sgr"], sgr()),
        (&["--s=sgr"], sgr()),
        (&["--section=bad"], Err(Exit(2))),
        (&["--section"], Err(Exit(2))),
        (&["--section="], Err(Exit(2))),
        // The order the script chose in: dump, then section, then find.
        (&["--dump", "--find", "x"], Ok(Form::Dump)),
        (&["--find", "x", "--section", "sgr"], sgr()),
        (&["-x"], Err(Exit(2))),
        (&["--bogus"], Err(Exit(2))),
        (&["---x"], Err(Exit(2))),
        (&["--"], Err(Exit(2))),
        (&["--dump", "--"], Err(Exit(2))),
        (&["--=x"], Err(Exit(2))),
        (&["-=x"], Err(Exit(2))),
    ];
    for (args, want) in table {
        assert_eq!(flate_form(args), want, "{args:?}");
    }
}

/// `--help` where it stands: before an error it wins, after one it is never
/// reached, and an unknown word waits for the end.
#[test]
fn help_is_where_it_stands() {
    for args in [
        &["-h"][..],
        &["--help"],
        &["--h"],
        &["--he"],
        &["-hh"],
        &["-hx"],
        &["-hhx"],
        &["-hx=y"],
        &["--dump", "-h"],
        &["--bogus", "-h"],
        &["-x", "-h"],
        &["-h", "--section", "bad"],
        &["--find", "x", "--help"],
    ] {
        assert_eq!(flate_form(args), Ok(Form::Help), "{args:?}");
    }
    for args in [
        &["-h="][..],
        &["-h=x"],
        &["-h-"],
        &["-hh=x"],
        &["-hh-"],
        &["--h=x"],
        &["--help=x"],
        &["--section=bad", "-h"],
    ] {
        assert_eq!(flate_form(args), Err(Exit(2)), "{args:?}");
    }
    let r = run(&["ecma48", "-h"]);
    assert_eq!(r.exit, Exit(0));
    assert!(r.out.starts_with("usage:"));
}

/// The script's `error: unrecognized arguments` is the `<key> <terms>` form
/// now, and only with no option beside it.
#[test]
fn a_word_is_a_term_only_alone() {
    let table: [(&[&str], Result<Form, Exit>); 9] = [
        (&["framed"], terms(&["framed"])),
        (&["a", "b"], terms(&["a", "b"])),
        (&["-"], terms(&["-"])),
        (&["-1"], terms(&["-1"])),
        (&["-a b"], terms(&["-a b"])),
        (&[""], terms(&[""])),
        (&["--", "-x"], terms(&["-x"])),
        (&["a", "--dump"], Err(Exit(2))),
        (&["--find", "x", "y"], Err(Exit(2))),
    ];
    for (args, want) in table {
        assert_eq!(flate_form(args), want, "{args:?}");
    }
}

#[test]
fn the_lzw_command_line_reads_only_find() {
    let doc = document("ecma35");
    let form = |a: &[&str]| parse(&doc, a).map_err(|r| r.exit);
    assert_eq!(form(&[]), Ok(Form::Dump));
    assert_eq!(form(&["--find", "x"]), found("x"));
    assert_eq!(form(&["a", "--find", "x", "b"]), found("x"));
    assert_eq!(form(&["--find", "x", "--find", "y"]), found("x"));
    assert_eq!(form(&["--find", ""]), found(""));
    // The script's `IndexError`.
    assert_eq!(form(&["--find"]), Err(Exit(1)));
    // Ignored and the text dumped, there; a search here.
    assert_eq!(form(&["a"]), terms(&["a"]));
}

#[test]
fn every_word_after_an_encrypted_key_is_a_probe() {
    let doc = document("adobe-5176-cff");
    assert_eq!(
        parse(&doc, &["--find", "-h"]).map_err(|r| r.exit),
        terms(&["--find", "-h"])
    );
    assert_eq!(parse(&doc, &[]).map_err(|r| r.exit), terms(&[]));
}

// ------------------------------------------------------------------ inflate

/// A zlib stream of a stored block: the inflater's framing, with no code
/// table to get wrong.
fn stored(data: &[u8]) -> Vec<u8> {
    let mut z = vec![0x78, 0x01, 0x01];
    let len = u16::try_from(data.len()).expect("one stored block");
    z.extend_from_slice(&len.to_le_bytes());
    z.extend_from_slice(&(!len).to_le_bytes());
    z.extend_from_slice(data);
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    z
}

#[test]
fn inflate_takes_a_whole_stream_or_nothing() {
    let z = stored(b"hello");
    assert_eq!(pdf::inflate(&z), Some(b"hello".to_vec()));
    // Trailing bytes are zlib's to ignore.
    assert_eq!(
        pdf::inflate(&[&z[..], b"tail\r\n"].concat()),
        Some(b"hello".to_vec())
    );
    // The trailer is read, so a wrong one is no stream.
    let mut bad = z.clone();
    *bad.last_mut().expect("a trailer") ^= 1;
    assert_eq!(pdf::inflate(&bad), None);
    // Truncated in the trailer, and in the block.
    assert_eq!(pdf::inflate(&z[..z.len() - 1]), None);
    assert_eq!(pdf::inflate(&z[..8]), None);
    assert_eq!(pdf::inflate(b""), None);
    // Not zlib: what `stream\n` of an `endstream\n` is followed by.
    assert_eq!(pdf::inflate(b"endobj\n"), None);
    // Nothing inflated is a stream of nothing.
    assert_eq!(pdf::inflate(&stored(b"")), Some(Vec::new()));
}

/// The first guess at the length is sixty-four kilobytes: this is past it,
/// so the buffer grows on the refusal and the digest still holds.
#[test]
fn inflate_grows_past_its_first_guess() {
    let mut z = vec![0x78, 0x01];
    let block = vec![b'x'; 60_000];
    let mut all = Vec::new();
    for last in [0u8, 0, 1] {
        z.push(last);
        let len = u16::try_from(block.len()).expect("a block");
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(&block);
        all.extend_from_slice(&block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &all {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    assert_eq!(pdf::inflate(&z).map(|o| o.len()), Some(180_000));
}

/// A dynamic block whose literal code `(a)` is incomplete: the one place the
/// port reads what the script could not. zlib allows an incomplete set only
/// for a single distance code; `baud_domain::zlib` allows any it can decode.
/// No pinned document has one, and each one's text is byte-equal. This holds
/// the record's line true: if the inflater tightens, so does `pdf-search`.
#[test]
fn an_incomplete_code_set_inflates_where_zlib_refused_it() {
    let hex = concat!(
        "780105e001902449922449020000000000000000000000000000000000000010",
        "0300000000000000000000000000000000000000000000000000000001000000",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "000000000000000000000010e202016600b3",
    );
    let stream: Vec<u8> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).expect("ascii"), 16).expect("hex"))
        .collect();
    assert_eq!(pdf::inflate(&stream), Some(b"(a)".to_vec()));
}

// ----------------------------------------------------- the Flate extractor

const HEADER: &[u8] = b"%PDF-1.3\n";

fn object(n: usize, data: &[u8], filter: &str) -> Vec<u8> {
    let head = format!("{n} 0 obj\n<</Length {} {filter}>>\nstream\n", data.len());
    [head.as_bytes(), data, b"\nendstream\nendobj\n"].concat()
}

fn flate_pdf(contents: &[&[u8]]) -> Vec<u8> {
    let mut pdf = HEADER.to_vec();
    for (i, c) in contents.iter().enumerate() {
        pdf.extend(object(i + 1, &stored(c), "/Filter /FlateDecode"));
    }
    pdf
}

/// Everything `ecma48`'s row needs found, spelled as the document spells it.
const ECMA48_TEXT: &[u8] =
    b"(SELECT GRAPHIC RENDITION) Tj (blink negative image underline italicized concealed) Tj";

#[test]
fn flate_reads_every_string_and_skips_what_is_not_flate() {
    let pdf = flate_pdf(&[br"(a\(b\)c\\d) Tj (e\nf) Tj", b"(a(b)c) Tj (open Tj"]);
    let e = pdf::flate(&pdf);
    assert_eq!(e.streams, Count(2));
    // `\n` is not one of its three escapes, and `(a(b)c)` holds only `(b)`.
    assert_eq!(e.text, r"a(b)c\d e\nf b");

    let e = pdf::flate(&[HEADER, b"1 0 obj\n<<>>\nstream\nnot zlib\nendstream\n"].concat());
    assert_eq!(
        e,
        pdf::Extract {
            text: String::new(),
            streams: Count(0)
        }
    );

    // A `stream` with no `endstream` after it.
    let open = [HEADER, b"1 0 obj\n<<>>\nstream\n", &stored(b"(gone)")[..]].concat();
    assert_eq!(pdf::flate(&open).streams, Count(0));

    // `\r\n` after `stream` is read, and a lone `\r` is not.
    let crlf = [&b"stream\r\n"[..], &stored(b"(crlf)"), b"\nendstream\n"].concat();
    assert_eq!(pdf::flate(&crlf).text, "crlf");
    let cr = [&b"stream\r"[..], &stored(b"(cr)"), b"\nendstream\n"].concat();
    assert_eq!(pdf::flate(&cr).text, "");
}

#[test]
fn a_search_whose_control_does_not_fire_says_the_extraction_is_broken() {
    // The needle is there, and the script would have said HIT and exit 0.
    let pdf = flate_pdf(&[b"(framed) Tj"]);
    let r = searched("ecma48", &pdf, &found("framed").expect("a form"));
    assert_eq!(r.exit, Exit(1));
    assert_eq!(r.out, "");
    assert!(r.err.contains("the extraction is broken"), "{}", r.err);

    let r = searched("ecma48", &pdf, &sgr().expect("a form"));
    assert_eq!((r.exit, r.out.as_str()), (Exit(1), ""));
    assert!(r.err.contains("the extraction is broken"), "{}", r.err);

    // The survey says it on stdout, as it did, and stops before the probes.
    let r = searched("ecma48", &pdf, &Form::Survey);
    assert_eq!(r.exit, Exit(1));
    assert!(
        r.out.contains("  MISS selectgraphicrendition   x0\n"),
        "{}",
        r.out
    );
    assert!(
        r.out
            .ends_with("\n6 control term(s) missed: the extraction is broken, not the PDF.\n"),
        "{}",
        r.out
    );
    assert!(!r.out.contains("probes"));

    // So does the term form, and a term that hits does not rescue it.
    let r = searched("ecma48", &pdf, &terms(&["framed"]).expect("a form"));
    assert_eq!(r.exit, Exit(1));
    assert!(r.out.contains("  probe   framed                 HIT\n"));
    assert!(
        r.out.ends_with("the document is not exonerated\n"),
        "{}",
        r.out
    );
}

#[test]
fn the_flate_forms_with_the_controls_firing() {
    let pdf = flate_pdf(&[ECMA48_TEXT]);

    // Passed, a probe missed: the script's last line and exit 1.
    let r = searched("ecma48", &pdf, &Form::Survey);
    assert_eq!(r.exit, Exit(1));
    assert!(r.out.contains("  MISS fraktur"), "{}", r.out);
    assert!(r.out.ends_with("\n10 probe(s) missed.\n"), "{}", r.out);

    // A miss on `--find`, exit 1, and no window.
    let r = searched("ecma48", &pdf, &found("framed").expect("a form"));
    assert_eq!((r.exit, r.out.as_str()), (Exit(1), "MISS framed x0\n"));

    // Folded both sides; a needle of nothing but punctuation is every gap.
    let r = searched("ecma48", &pdf, &found("B-L-I-N-K").expect("a form"));
    assert_eq!(r.exit, Exit(0));
    assert!(r.out.starts_with("HIT  B-L-I-N-K x1\n\n"), "{}", r.out);
    let r = searched("ecma48", &pdf, &found("!!").expect("a form"));
    assert_eq!(r.exit, Exit(0));
    assert!(r.out.starts_with("HIT  !! x"), "{}", r.out);

    // A section's anchor missing, the controls firing.
    let r = searched("ecma48", &pdf, &sgr().expect("a form"));
    assert_eq!((r.exit, r.out.as_str()), (Exit(1), ""));
    assert!(r.err.contains("the section is not found"), "{}", r.err);

    // A document with no probes stops after its controls.
    let r = searched(
        "utf8-lookup",
        &flate_pdf(&[b"(validating utf8 John Keiser Daniel Lemire)"]),
        &Form::Survey,
    );
    assert_eq!(r.exit, Exit(0));
    assert!(!r.out.contains("probes"), "{}", r.out);

    // The dump is the folded text, with no control, because it is no search.
    let r = searched("ecma48", &flate_pdf(&[b"(A b-1)"]), &Form::Dump);
    assert_eq!((r.exit, r.out.as_str()), (Exit(0), "ab1"));
}

// ------------------------------------------------------- the LZW extractor

/// Codes written at the widths `EarlyChange 1` reads them at. `first` is
/// what the decoder's table holds before the first code.
fn lzw_packed(codes: &[usize]) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut acc, mut held) = (0usize, 0);
    let (mut entries, mut width, mut after_clear) = (258, 9, true);
    for &c in codes {
        acc = (acc << width) | c;
        held += width;
        while held >= 8 {
            held -= 8;
            out.push(u8::try_from((acc >> held) & 255).expect("a byte"));
        }
        acc &= (1 << held) - 1;
        if c == 256 {
            entries = 258;
            width = 9;
            after_clear = true;
        } else if after_clear {
            after_clear = false;
        } else {
            entries += 1;
            let n = entries + 1;
            width = match n {
                2048.. => 12,
                1024.. => 11,
                512.. => 10,
                _ => 9,
            };
        }
    }
    if held != 0 {
        out.push(u8::try_from((acc << (8 - held)) & 255).expect("a byte"));
    }
    out
}

fn lzw_literal(data: &[u8]) -> Vec<usize> {
    let mut codes = vec![256];
    codes.extend(data.iter().map(|b| usize::from(*b)));
    codes.push(257);
    codes
}

#[test]
fn lzw_decodes_literals_entries_and_the_code_being_defined() {
    assert_eq!(
        pdf::lzw_decode(&lzw_packed(&lzw_literal(b"abc"))),
        Some(b"abc".to_vec())
    );
    // 258 = "ab", defined by the pair before it.
    let codes = [256, 97, 98, 258, 257];
    assert_eq!(pdf::lzw_decode(&lzw_packed(&codes)), Some(b"abab".to_vec()));
    // `KwKwK`: 258 is named as it is being defined.
    let codes = [256, 97, 258, 257];
    assert_eq!(pdf::lzw_decode(&lzw_packed(&codes)), Some(b"aaa".to_vec()));
    // A second clear forgets the entries.
    let codes = [256, 97, 98, 256, 99, 257];
    assert_eq!(pdf::lzw_decode(&lzw_packed(&codes)), Some(b"abc".to_vec()));
    // No EOD: the bits run out, and what came is kept. No clear is fine too.
    let codes = [97, 98];
    assert_eq!(pdf::lzw_decode(&lzw_packed(&codes)), Some(b"ab".to_vec()));
    // Fewer bits than a code.
    assert_eq!(pdf::lzw_decode(&[0xff]), Some(Vec::new()));
    // The first code names nothing: no stream at all, where a later code
    // past the table is taken as being defined.
    assert_eq!(pdf::lzw_decode(&lzw_packed(&[300])), None);
    assert_eq!(pdf::lzw_decode(&lzw_packed(&[256, 300])), None);
    let codes = [256, 97, 300, 257];
    assert_eq!(pdf::lzw_decode(&lzw_packed(&codes)), Some(b"aaa".to_vec()));
}

#[test]
fn lzw_widens_where_early_change_says() {
    // 253 entries past the first code reach 511 and then widen, one early.
    let mut text = Vec::new();
    let mut codes = vec![256];
    for i in 0..800usize {
        let b = u8::try_from(97 + i % 26).expect("a letter");
        text.push(b);
        codes.push(usize::from(b));
    }
    codes.push(257);
    assert_eq!(pdf::lzw_decode(&lzw_packed(&codes)), Some(text));
}

#[test]
fn lzw_strings_take_two_passes_of_escapes() {
    let s = pdf::strings_of(br"BT (a\nb\rc\td\be\ff\(g\)h\\i) Tj ET");
    assert_eq!(s, "a\nbc def(g)h\\i\n");
    // Octal, wrapped to a byte; and `\\101` comes out of the first pass as
    // `\101`, which the second reads.
    assert_eq!(pdf::strings_of(br"(\101\7\777\1012)"), "A\u{7}\u{ff}A2");
    assert_eq!(pdf::strings_of(br"(\\101)"), "A");
    // `BT` joins nothing, `ET` ends a line, wherever they fall outside a
    // string, and a `(` inside a string is only a byte.
    assert_eq!(pdf::strings_of(b"xBTy (a) ETz (b(c) BT"), "a\nb(c");
    assert_eq!(pdf::strings_of(b"(\xe9)"), "\u{e9}");
}

fn lzw_pdf(streams: &[Vec<u8>]) -> Vec<u8> {
    let mut pdf = HEADER.to_vec();
    for (i, s) in streams.iter().enumerate() {
        pdf.extend(object(i + 1, s, "/Filter /LZWDecode"));
    }
    pdf
}

fn lzw_text(content: &[u8]) -> Vec<u8> {
    lzw_packed(&lzw_literal(content))
}

#[test]
fn lzw_numbers_only_the_streams_with_text() {
    let pdf = lzw_pdf(&[
        lzw_text(b"(one)"),
        lzw_text(b"(\xa0 ) BT ET"),
        lzw_packed(&[300]),
        lzw_text(b"(two)"),
    ]);
    let e = pdf::lzw(&pdf);
    assert_eq!(e.streams, Count(2));
    assert_eq!(e.text, "\n=== stream 1\none\n=== stream 2\ntwo");
}

#[test]
fn lzw_reads_only_what_its_dictionary_says() {
    let content = lzw_text(b"(text)");
    // Not LZW.
    let plain = [HEADER, &object(1, &content, "/Filter /FlateDecode")[..]].concat();
    assert_eq!(pdf::lzw(&plain).streams, Count(0));
    // No `/Length`.
    let none = [
        HEADER,
        b"1 0 obj\n<</Filter /LZWDecode>>\nstream\n",
        &content,
        b"\nendstream\n",
    ]
    .concat();
    assert_eq!(pdf::lzw(&none).streams, Count(0));
    // A length past the file is cut at the file; past `usize` too.
    for length in ["999999", "99999999999999999999999"] {
        let long = [
            HEADER,
            format!("1 0 obj\n<</Length {length} /Filter /LZWDecode>>\nstream\n").as_bytes(),
            &content,
        ]
        .concat();
        assert_eq!(pdf::lzw(&long).text, "\n=== stream 1\ntext", "{length}");
    }
    // A length of nothing reads nothing.
    let zero = [
        HEADER,
        b"1 0 obj\n<</Length 0 /Filter /LZWDecode>>\nstream\n",
        &content,
    ]
    .concat();
    assert_eq!(pdf::lzw(&zero).streams, Count(0));
    // The dictionary reaches back to the first `<<`, as the lazy pattern did,
    // so an entry in an earlier one counts.
    let spans = [
        HEADER,
        b"<</Length 5>> <<",
        b"/Filter /LZWDecode>>\nstream\n",
        &content,
    ]
    .concat();
    assert_eq!(pdf::lzw(&spans).streams, Count(0));
}

#[test]
fn the_lzw_search_exits_2_only_on_its_control() {
    let good = lzw_pdf(&[lzw_text(b"(graphic character)")]);
    let r = searched("ecma35", &good, &found("zzqq").expect("a form"));
    assert_eq!(r.exit, Exit(0));
    assert_eq!(
        r.out,
        "control 'graphic character': HIT x1\n'zzqq': MISS x0\n"
    );
    let r = searched("ecma35", &good, &terms(&["zzqq"]).expect("a form"));
    assert_eq!(r.exit, Exit(0));
    assert!(
        r.out
            .starts_with("x.pdf: 31 chars of text from 1 streams\n"),
        "{}",
        r.out
    );

    let gone = lzw_pdf(&[lzw_text(b"(nothing)")]);
    let r = searched("ecma35", &gone, &found("nothing").expect("a form"));
    assert_eq!(r.exit, Exit(2));
    assert_eq!(
        r.out,
        "control 'graphic character': MISS x0\n'nothing': HIT x1\n"
    );
    assert!(
        r.err.contains("the extraction is broken"),
        "stderr was {:?}",
        r.err
    );
    let r = searched("ecma35", &gone, &terms(&["nothing"]).expect("a form"));
    assert_eq!(r.exit, Exit(1));

    // The dump is the text, no control, for the same reason.
    let r = searched("ecma35", &gone, &Form::Dump);
    assert_eq!(
        (r.exit, r.out.as_str()),
        (Exit(0), "\n=== stream 1\nnothing")
    );
}

// ------------------------------------------------- the encrypted extractor

const PAD: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08,
    0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

fn hexed(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A way a built document departs from a clean one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flaw {
    /// The stream is not deflated.
    Clear,
    WrongU,
    NoEncrypt,
    NoP,
    NoR,
    OddO,
    LiteralO,
    NoId,
}

/// What a PDF with an empty user password needs, every field a test changes.
#[derive(Clone)]
struct Encrypted {
    o: Vec<u8>,
    p: i64,
    revision: i64,
    bits: i64,
    id: Vec<u8>,
    number: i64,
    generation: i64,
    length: Option<String>,
    flaws: Vec<Flaw>,
    extra: Vec<u8>,
}

impl Encrypted {
    fn new() -> Self {
        Self {
            o: vec![b'o'; 32],
            p: -44,
            revision: 2,
            bits: 128,
            id: vec![1, 2, 3, 4],
            number: 10,
            generation: 0,
            length: None,
            flaws: Vec::new(),
            extra: Vec::new(),
        }
    }

    fn flawed(flaw: Flaw) -> Self {
        let mut e = Self::new();
        e.flaws.push(flaw);
        e
    }

    fn has(&self, flaw: Flaw) -> bool {
        self.flaws.contains(&flaw)
    }

    /// The file key, by Algorithm 3.2, spelled apart from `pdf::file_key`
    /// so each can be wrong alone.
    fn key(&self) -> Vec<u8> {
        let n = if self.revision >= 3 {
            usize::try_from(self.bits / 8).expect("a key's length")
        } else {
            5
        };
        let mut input = PAD.to_vec();
        input.extend_from_slice(&self.o[..self.o.len().min(32)]);
        input.extend_from_slice(&self.p.to_le_bytes()[..4]);
        if !self.has(Flaw::NoId) {
            input.extend_from_slice(&self.id);
        }
        let mut key = md5::digest(&input)[..n].to_vec();
        if self.revision >= 3 {
            for _ in 0..50 {
                key = md5::digest(&key)[..n].to_vec();
            }
        }
        key
    }

    fn file(&self, content: &[u8]) -> Vec<u8> {
        let key = self.key();
        let mut u = if self.revision == 2 {
            pdf::rc4(&key, &PAD)
        } else {
            vec![b'u'; 32]
        };
        if self.has(Flaw::WrongU) {
            u[0] ^= 1;
        }
        let mut out = HEADER.to_vec();
        out.extend_from_slice(b"99 0 obj\n<</Filter /Standard /V 1 /O ");
        if self.has(Flaw::LiteralO) {
            out.push(b'(');
            for &b in &self.o {
                match b {
                    b'\\' | b'(' | b')' => out.extend_from_slice(&[b'\\', b]),
                    b'\n' => out.extend_from_slice(b"\\n"),
                    b'\r' => out.extend_from_slice(b"\\r"),
                    _ => out.push(b),
                }
            }
            out.push(b')');
        } else {
            let odd = if self.has(Flaw::OddO) { "a" } else { "" };
            out.extend_from_slice(format!("<{}{odd}>", hexed(&self.o)).as_bytes());
        }
        out.extend_from_slice(format!(" /U <{}>", hexed(&u)).as_bytes());
        if !self.has(Flaw::NoR) {
            out.extend_from_slice(format!(" /R {}", self.revision).as_bytes());
        }
        if self.revision >= 3 {
            out.extend_from_slice(format!(" /Length {}", self.bits).as_bytes());
        }
        if !self.has(Flaw::NoP) {
            out.extend_from_slice(format!(" /P {}", self.p).as_bytes());
        }
        out.extend_from_slice(b">>\nendobj\n");

        let flate = !self.has(Flaw::Clear);
        let plain = if flate {
            stored(content)
        } else {
            content.to_vec()
        };
        let mut input = key.clone();
        input.extend_from_slice(&self.number.to_le_bytes()[..3]);
        input.extend_from_slice(&self.generation.to_le_bytes()[..2]);
        let object_key = md5::digest(&input)[..(key.len() + 5).min(16)].to_vec();
        let cipher = pdf::rc4(&object_key, &plain);
        let length = self
            .length
            .clone()
            .unwrap_or_else(|| cipher.len().to_string());
        let filter = if flate { "/Filter /FlateDecode " } else { "" };
        out.extend_from_slice(
            format!(
                "{} {} obj\n<</Length {length} {filter}>>\nstream\n",
                self.number, self.generation
            )
            .as_bytes(),
        );
        out.extend_from_slice(&cipher);
        out.extend_from_slice(b"\nendstream\nendobj\n");
        out.extend_from_slice(&self.extra);
        out.extend_from_slice(b"trailer <<");
        if !self.has(Flaw::NoEncrypt) {
            out.extend_from_slice(b"/Encrypt 99 0 R ");
        }
        if !self.has(Flaw::NoId) {
            out.extend_from_slice(format!("/ID [<{}><00>]", hexed(&self.id)).as_bytes());
        }
        out.extend_from_slice(b">>\n");
        out
    }
}

const TEXT: &[u8] =
    b"[(Compact Font Format)] TJ (CharStrings) Tj (FDSelect) Tj (Private DICT) Tj (INDEX) Tj";

fn stopped(pdf: &[u8]) -> String {
    match pdf::encrypted(pdf) {
        Err(Stop(m)) => m,
        Ok(e) => panic!("expected a stop, read {} streams", e.streams.0),
    }
}

#[test]
fn the_handler_reads_a_document_it_built() {
    let e = pdf::encrypted(&Encrypted::new().file(TEXT)).expect("the document");
    assert_eq!(e.streams, Count(1));
    assert_eq!(
        e.text,
        "Compact Font Format CharStrings FDSelect Private DICT INDEX "
    );
    let r = searched(
        "adobe-5176-cff",
        &Encrypted::new().file(TEXT),
        &terms(&["fd select"]).expect("a form"),
    );
    assert_eq!(r.exit, Exit(0));
    assert!(
        r.out
            .starts_with("x.pdf: 60 chars of text from 1 decrypted streams\n"),
        "{}",
        r.out
    );
    assert!(
        r.out.contains("  probe   fd select              HIT\n"),
        "{}",
        r.out
    );
}

#[test]
fn a_control_the_text_does_not_hold_fails_the_handler_too() {
    let pdf = Encrypted::new().file(b"(nothing) Tj");
    let r = searched(
        "adobe-5176-cff",
        &pdf,
        &terms(&["nothing"]).expect("a form"),
    );
    assert_eq!(r.exit, Exit(1));
    assert!(
        r.out.contains("  control compactfontformat      MISS\n"),
        "{}",
        r.out
    );
    assert!(
        r.out.contains("  probe   nothing                HIT\n"),
        "{}",
        r.out
    );
    assert!(
        r.out.ends_with(
            "POSITIVE CONTROL FAILED -- the extraction is broken, the document is not exonerated\n"
        ),
        "{}",
        r.out
    );
}

#[test]
fn a_wrong_key_derivation_stops_at_u() {
    let e = Encrypted::flawed(Flaw::WrongU);
    assert_eq!(
        stopped(&e.file(TEXT)),
        "file key does not reproduce /U -- the key derivation is wrong, not the document"
    );
    // Through the command too, on stderr, exit 1.
    let r = searched(
        "adobe-5176-cff",
        &e.file(TEXT),
        &terms(&[]).expect("a form"),
    );
    assert_eq!((r.exit, r.out.as_str()), (Exit(1), ""));
    assert!(r.err.contains("the key derivation is wrong"), "{}", r.err);
}

#[test]
fn each_missing_entry_stops_the_handler() {
    let file = |flaw| Encrypted::flawed(flaw).file(TEXT);
    assert!(stopped(&file(Flaw::NoEncrypt)).starts_with("no /Encrypt"));
    assert_eq!(stopped(&file(Flaw::NoP)), "no /P in the Encrypt dict");
    assert_eq!(stopped(&file(Flaw::NoR)), "no /R in the Encrypt dict");
    assert_eq!(stopped(&file(Flaw::OddO)), "an odd number of hex digits");
    // The object it names is not there.
    let pdf = Encrypted::new().file(TEXT);
    let moved = String::from_utf8_lossy(&pdf).replace("99 0 obj", "98 0 obj");
    assert_eq!(
        stopped(&moved.into_bytes()),
        "the /Encrypt object is not in the file"
    );
    // A digit before the number means it is not that object.
    let prefixed = String::from_utf8_lossy(&pdf).replace("99 0 obj", "199 0 obj");
    assert_eq!(
        stopped(&prefixed.into_bytes()),
        "the /Encrypt object is not in the file"
    );
}

#[test]
fn o_is_read_hex_or_literal_and_p_as_four_bytes() {
    let mut e = Encrypted::flawed(Flaw::LiteralO);
    e.o = b"a\\b(c)d\re\nf"
        .iter()
        .copied()
        .chain(std::iter::repeat_n(b'g', 21))
        .collect();
    pdf::encrypted(&e.file(TEXT)).expect("escapes in a literal /O");
    let mut e = Encrypted::new();
    e.p = 4;
    pdf::encrypted(&e.file(TEXT)).expect("a positive /P");
    let mut e = Encrypted::new();
    e.p = -3904;
    pdf::encrypted(&e.file(TEXT)).expect("a negative /P");
    let e = Encrypted::flawed(Flaw::NoId);
    pdf::encrypted(&e.file(TEXT)).expect("no /ID");
    // Longer than Algorithm 3.2 reads: the key is from the first 32.
    let mut e = Encrypted::new();
    e.o = vec![b'o'; 40];
    pdf::encrypted(&e.file(TEXT)).expect("a long /O");
}

#[test]
fn revision_three_takes_its_50_rounds_and_is_not_held_to_u() {
    let mut e = Encrypted::new();
    e.revision = 3;
    assert_eq!(
        pdf::encrypted(&e.file(TEXT)).expect("128 bits").streams,
        Count(1)
    );
    e.bits = 40;
    assert_eq!(
        pdf::encrypted(&e.file(TEXT)).expect("40 bits").streams,
        Count(1)
    );
}

#[test]
fn a_clear_stream_is_read_as_it_is() {
    let e = Encrypted::flawed(Flaw::Clear);
    let got = pdf::encrypted(&e.file(TEXT)).expect("a clear stream");
    assert_eq!(
        got.text,
        "Compact Font Format CharStrings FDSelect Private DICT INDEX "
    );
}

#[test]
fn a_flate_stream_that_does_not_inflate_is_skipped_and_not_counted() {
    let mut e = Encrypted::new();
    e.number = 11;
    let mut moved = e.file(TEXT);
    // Decrypted under the wrong object number, so it is noise to the inflater.
    let at = moved
        .windows(7)
        .position(|w| w == b"11 0 ob")
        .expect("the object");
    moved[at + 1] = b'2';
    assert_eq!(
        pdf::encrypted(&moved).expect("the document").streams,
        Count(0)
    );
}

/// `/Length 123 0 R`: `\d+` gives back a digit, so `12` reads as the length.
/// Through `pdf_search`, whose stream count is what the script's was.
#[test]
fn an_indirect_length_is_read_short_by_the_backtrack() {
    // One digit has nothing to give back: there is no length, no stream.
    let mut e = Encrypted::new();
    e.length = Some("7 0 R".to_string());
    assert_eq!(
        pdf::encrypted(&e.file(TEXT)).expect("one digit").streams,
        Count(0)
    );
    // The next `/Length` is read instead.
    e.length = Some("7 0 R /Length 200".to_string());
    assert_eq!(
        pdf::encrypted(&e.file(TEXT)).expect("a later one").streams,
        Count(1)
    );
    // Two digits: read as one, too short to inflate.
    e.length = Some("12 0 R".to_string());
    assert_eq!(
        pdf::encrypted(&e.file(TEXT)).expect("two digits").streams,
        Count(0)
    );
    // Clear, so the short read is what is shown: the twelve bytes `[(Compact
    // Fo` of `123`'s `12`, which close no string, so no text.
    e.flaws.push(Flaw::Clear);
    e.length = Some("123 0 R".to_string());
    let clear = pdf::encrypted(&e.file(TEXT)).expect("clear");
    assert_eq!(clear.streams, Count(1));
    assert_eq!(clear.text, "");
    // And the one before the backtrack: nothing follows the digits.
    e.length = Some("86 /X 1".to_string());
    let whole = pdf::encrypted(&e.file(TEXT)).expect("direct");
    assert!(
        whole.text.starts_with("Compact Font Format"),
        "{}",
        whole.text
    );
}

#[test]
fn numbers_past_algorithm_31s_bytes_stop_the_run() {
    let mut e = Encrypted::new();
    e.number = 1 << 24;
    assert!(stopped(&e.file(TEXT)).starts_with("an object number past"));
    let mut e = Encrypted::new();
    e.generation = 65536;
    assert!(stopped(&e.file(TEXT)).starts_with("an object number past"));
}

#[test]
fn an_object_is_only_what_its_pattern_says() {
    let plain = stored(TEXT);
    // A `/Length`-less dictionary is passed over without a stop.
    let mut e = Encrypted::new();
    e.extra = b"50 0 obj\n<</Filter /FlateDecode>>\nstream\nabc\nendstream\nendobj\n".to_vec();
    assert_eq!(
        pdf::encrypted(&e.file(TEXT)).expect("no length").streams,
        Count(1)
    );
    // `endobj` stops a dictionary, so this one has no `stream`.
    let mut e = Encrypted::new();
    e.extra = b"51 0 obj\n<</Length 3 endobj /X 1>>\nstream\nabc\nendstream\nendobj\n".to_vec();
    assert_eq!(
        pdf::encrypted(&e.file(TEXT)).expect("endobj").streams,
        Count(1)
    );
    // A nested `>>` is not the end when no `stream` follows it.
    let mut e = Encrypted::flawed(Flaw::Clear);
    e.extra = format!(
        "52 0 obj\n<</D <</A 1>> /Length {}>>\nstream\n",
        plain.len()
    )
    .into_bytes();
    e.extra.extend_from_slice(&plain);
    e.extra.extend_from_slice(b"\nendstream\nendobj\n");
    assert_eq!(
        pdf::encrypted(&e.file(TEXT)).expect("nested").streams,
        Count(2)
    );
    // A digit before the number means it is not an object at all.
    let mut e = Encrypted::new();
    e.extra = b"x123 0 obj\n<</Length 3>>\nstream\nabc\nendstream\n".to_vec();
    assert_eq!(
        pdf::encrypted(&e.file(TEXT)).expect("prefixed").streams,
        Count(2)
    );
    // `stream\r\n` is a stream too, and a lone `\r` is not.
    let good = Encrypted::new().file(TEXT);
    let crlf = String::from_utf8_lossy(&good).replace(">>\nstream\n", ">>\r\nstream\r\n");
    assert!(pdf::encrypted(&crlf.into_bytes()).is_ok());
}

#[test]
fn a_literal_string_keeps_its_escapes_and_folds_its_line_ends() {
    let s = |b: &[u8]| pdf::pdf_string(b).expect("a string");
    assert_eq!(s(br"(a\nb\rc\td\be\ff)"), b"a\nb\rc\td\x08e\x0cf");
    assert_eq!(s(br"(\(\)\\\x)"), br"()\x");
    assert_eq!(s(br"(\101\7\1012\777)"), b"A\x07A2\xff");
    assert_eq!(s(b"(a\r\nb\rc)"), b"a\nb\nc");
    assert_eq!(s(b"(end\\)"), b"end\\");
    assert_eq!(s(b"<41 4 2>"), b"AB");
    assert_eq!(
        pdf::pdf_string(b"<414>"),
        Err(Stop("an odd number of hex digits".to_string()))
    );
}

#[test]
fn tj_and_tj_each_end_with_a_space() {
    let e = Encrypted::new();
    let shown = |text: &[u8]| pdf::encrypted(&e.file(text)).expect("the document").text;
    assert_eq!(shown(br"[(a)-120(b\)c)12(\101)] TJ"), "ab)cA ");
    assert_eq!(shown(br"(d\050e) Tj"), "d(e ");
    assert_eq!(shown(b"[] TJ"), " ");
    assert_eq!(shown(b"(f)Tj[(g)]TJ"), "f g ");
    // A bare string is shown by nothing.
    assert_eq!(shown(b"(h) x"), "");
}

// -------------------------------------------------------------------- text

#[test]
fn fold_keeps_lowercase_letters_and_digits() {
    assert_eq!(pdf::fold("f r-a_M.e D 1"), "framed1");
    assert_eq!(pdf::fold("\u{e9}\u{b2}"), "");
    // Whole-string `lower()`: the Kelvin sign is a `k`, so it reads as one.
    assert_eq!(pdf::fold("\u{212a}"), "k");
    let flat = pdf::flatten("A \u{e9}B");
    assert_eq!(flat.flat, "ab");
    assert_eq!(flat.index, vec![0, 4]);
}

#[test]
fn unspace_joins_one_letter_words_on_the_collapsed_text() {
    assert_eq!(pdf::unspace("Operat i ng sy st em"), "Operat i ng sy st em");
    assert_eq!(pdf::unspace("a b c"), "abc");
    assert_eq!(pdf::unspace("a  b\tc\n"), "abc ");
    assert_eq!(pdf::unspace("ab c"), "ab c");
    assert_eq!(pdf::unspace("a bc"), "a bc");
    assert_eq!(pdf::unspace(" a b"), " ab");
    assert_eq!(pdf::unspace("- a - b"), "- a - b");
    // `\s` takes the information separators and NBSP, and `\w` takes `\u{b2}`.
    assert_eq!(pdf::unspace("a\u{1c}\u{a0}b"), "ab");
    assert_eq!(pdf::unspace("\u{b2} \u{e9}"), "\u{b2}\u{e9}");
    assert_eq!(pdf::unspace("_ 1"), "_1");
}

#[test]
fn the_table_holds_each_method_to_the_documents() {
    let method = |key: &str| document(key).method;
    assert_eq!(method("ecma48"), Method::Flate);
    assert_eq!(method("ecma35"), Method::Lzw);
    assert_eq!(method("ecma-tr53"), Method::Lzw);
    assert_eq!(method("adobe-5176-cff"), Method::Encrypted);
    assert_eq!(method("adobe-5177-type2"), Method::Encrypted);
    for doc in &DOCUMENTS {
        assert!(!doc.controls.is_empty(), "{} has no control", doc.key);
    }
}
