//! `xtask::forbidden`: what the script printed and how it exited, under
//! Python 3.13.12, with one test per check that fires, and the spans a
//! look-around's stand-in reports. Every list here is invented.

// A row's spans are a list, whether one long or many.
#![allow(clippy::single_range_in_vec_init)]

use std::fs;
use std::io::ErrorKind;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use xtask::features_since::Exit;
use xtask::forbidden::{self, Pattern, Verdict, views};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn dir() -> Dir {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("xtask-forbidden-{}-{n}", std::process::id()));
    fs::create_dir_all(&d).expect("scratch");
    Dir(d)
}

/// What a run printed, the scratch directory's name taken out, and how it
/// exited. An error is what `main` turns into exit 1 and one line.
#[derive(PartialEq, Eq, Debug)]
struct Ran {
    exit: Exit,
    out: String,
    err: String,
}

fn ran(args: &[&str], dir: &Path) -> Result<Ran, ErrorKind> {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let exit = forbidden::run(args, &mut out, &mut err).map_err(|e| e.kind())?;
    let name = format!("{}/", dir.display());
    let text = |b: Vec<u8>| String::from_utf8(b).expect("utf-8").replace(&name, "");
    Ok(Ran {
        exit,
        out: text(out),
        err: text(err),
    })
}

/// A run over `files`, written as named, with the list and its controls.
fn search(list: &[u8], controls: &[u8], files: &[(&str, &[u8])]) -> Ran {
    try_search(list, controls, files).expect("no error")
}

fn try_search(list: &[u8], controls: &[u8], files: &[(&str, &[u8])]) -> Result<Ran, ErrorKind> {
    let d = dir();
    let write = |name: &str, bytes: &[u8]| {
        let p = d.0.join(name);
        fs::write(&p, bytes).expect("write");
        p.display().to_string()
    };
    let (l, c) = (write("LIST", list), write("CONTROLS", controls));
    let mut args = vec![l, c];
    args.extend(files.iter().map(|(n, b)| write(n, b)));
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    ran(&words, &d.0)
}

/// The final line, as the script printed it.
fn summary(patterns: usize, files: usize, hits: usize) -> String {
    format!(
        "readme-forbidden: {patterns} patterns, each found in its control; \
         {files} files, {hits} hits\n"
    )
}

const LIST: &[u8] = b"# made up\nuser::(?i:qzuser)\nhost::qzhost\\.local\n";
const CONTROLS: &[u8] = b"user\tQZUSER\nhost\tat qzhost.local\n";

/// **A clean file passes**: the summary on stdout, nothing on stderr, 0.
#[test]
fn a_clean_file_passes() {
    let r = search(LIST, CONTROLS, &[("f", b"prompt$ ls\r\n")]);
    assert_eq!(
        r,
        Ran {
            exit: Exit(0),
            out: summary(2, 1, 0),
            err: String::new(),
        }
    );
}

/// **A hit names the label and the file and never what matched**, on
/// stderr, and exits 1 with the summary still on stdout.
#[test]
fn a_hit_names_its_label_and_file_and_not_the_match() {
    let r = search(LIST, CONTROLS, &[("f", b"at qzhost.local\n")]);
    assert_eq!(
        r,
        Ran {
            exit: Exit(1),
            out: summary(2, 1, 1),
            err: "readme-forbidden: host in f\n".to_string(),
        }
    );
    assert!(!r.err.contains("qzhost"), "{}", r.err);
}

/// **The patterns' order, the view's, then the file's** set the order of
/// the lines, and a pair is reported once however many views or duplicate
/// arguments find it, while the file count keeps every argument.
#[test]
fn hits_come_in_order_and_each_pair_once() {
    let list = b"a::alpha\nb::beta\nc::gamma\n";
    let controls = b"a\talpha\nb\tbeta\nc\tgamma\n";
    // `gamma` raw, `beta` raw and stripped, `alpha` stripped alone.
    let file = b"gamma beta al\x1b[1mpha\x1b[0m\n";
    let r = search(list, controls, &[("one", file), ("two", b"alpha\n")]);
    assert_eq!(
        r,
        Ran {
            exit: Exit(1),
            out: summary(3, 2, 4),
            err: "readme-forbidden: b in one\nreadme-forbidden: c in one\n\
                  readme-forbidden: a in one\nreadme-forbidden: a in two\n"
                .to_string(),
        }
    );
}

#[test]
fn a_file_named_twice_counts_twice_and_hits_once() {
    let d = dir();
    let (list, controls, f) = (d.0.join("l"), d.0.join("c"), d.0.join("f"));
    fs::write(&list, b"a::alpha\n").expect("write");
    fs::write(&controls, b"a\talpha\n").expect("write");
    fs::write(&f, b"alpha\n").expect("write");
    let (l, c, f) = (
        list.display().to_string(),
        controls.display().to_string(),
        f.display().to_string(),
    );
    assert_eq!(
        ran(&[&l, &c, &f, &f], &d.0),
        Ok(Ran {
            exit: Exit(1),
            out: summary(1, 2, 1),
            err: "readme-forbidden: a in f\n".to_string(),
        })
    );
}

/// **Every form of escape comes out**, and the word split by it is found;
/// the controls `\t`, `\n`, `\x0b`, `\x0c` and `\r` and DEL stay in.
#[test]
fn a_word_split_by_an_escape_is_found_once_it_is_out() {
    let word = |sep: &[u8]| [b"qz".as_slice(), sep, b"user"].concat();
    let out: [&[u8]; 19] = [
        b"\x1b]0;title\x07",
        b"\x1b]8;;x\x1b\\",
        b"\x1b]",
        b"\x1b[m",
        b"\x1b[1;31m",
        b"\x1b[?25l",
        b"\x1b[ q",
        b"\x1b(B",
        b"\x1b)0",
        b"\x1b*A",
        b"\x1b+B",
        b"\x1b#8",
        b"\x1bc",
        b"\x1b%G",
        b"\x00",
        b"\x07",
        b"\x08",
        b"\x0e",
        b"\x1f",
    ];
    for sep in out {
        let f = word(sep);
        let r = search(b"u::qzuser\n", b"u\tqzuser\n", &[("f", &f)]);
        assert_eq!(r.exit, Exit(1), "{sep:?} should come out");
    }
    let kept: [&[u8]; 7] = [b"\t", b"\n", b"\x0b", b"\x0c", b"\r", b"\x7f", b"\xc2\x9b"];
    for sep in kept {
        let f = word(sep);
        let r = search(b"u::qzuser\n", b"u\tqzuser\n", &[("f", &f)]);
        assert_eq!(r.exit, Exit(0), "{sep:?} should stay in");
    }
}

/// `views` itself: the bytes, and the bytes with the escapes out, where an
/// OSC that never ends loses its introducer alone.
#[test]
fn the_two_views_are_the_bytes_and_the_bytes_stripped() {
    let [raw, stripped] = views(b"a\x1b[1mb\x1b]0;open");
    assert_eq!(&*raw, b"a\x1b[1mb\x1b]0;open");
    assert_eq!(&*stripped, b"ab0;open");
}

/// **No control, no search**: the label is named, every one of them, and
/// a file that does not exist is not reached.
#[test]
fn a_label_with_no_control_searches_nothing() {
    let r = search(LIST, b"user\tQZUSER\n", &[("f", b"at qzhost.local\n")]);
    assert_eq!(
        r,
        Ran {
            exit: Exit(2),
            out: String::new(),
            err: "readme-forbidden: host: no control\n".to_string(),
        }
    );
    let d = dir();
    let (l, c) = (d.0.join("l"), d.0.join("c"));
    fs::write(&l, LIST).expect("write");
    fs::write(&c, b"").expect("write");
    let (l, c) = (l.display().to_string(), c.display().to_string());
    assert_eq!(
        ran(&[&l, &c, "no-such-file"], &d.0),
        Ok(Ran {
            exit: Exit(2),
            out: String::new(),
            err: "readme-forbidden: user: no control\nreadme-forbidden: host: no control\n"
                .to_string(),
        })
    );
}

#[test]
fn a_control_its_pattern_does_not_find_searches_nothing() {
    let r = search(LIST, b"user\tsomeone\nhost\tqzhost.local\n", &[("f", b"x")]);
    assert_eq!(
        r,
        Ran {
            exit: Exit(2),
            out: String::new(),
            err: "readme-forbidden: user: its control is not found\n".to_string(),
        }
    );
}

/// **The patterns joined read a pattern as it does alone**: a byte in a
/// joined search compiled as Unicode would be two, so this control would
/// be missed there and nothing else would say so.
#[test]
fn the_joined_patterns_find_what_a_byte_pattern_finds() {
    let r = search(b"b::a\\xe2z\n", b"b\ta\xe2z\n", &[("f", b"a\xe2z")]);
    assert_eq!(r.exit, Exit(1), "{}", r.err);
    assert_eq!(r.err, "readme-forbidden: b in f\n");
}

/// **A control line with no tab is an empty control**, and the last line
/// of a label is the one used. A control keeps a `\r` at its end.
#[test]
fn a_controls_file_reads_as_the_script_read_it() {
    let r = search(b"e::^$\n", b"# c\n\n\t\ne\n", &[("f", b"x")]);
    assert_eq!(
        r.exit,
        Exit(0),
        "the empty control is found, and x is not empty"
    );
    let empty = search(b"a::alpha\n", b"a\n", &[("f", b"x")]);
    assert_eq!(empty.exit, Exit(2));
    assert_eq!(empty.err, "readme-forbidden: a: its control is not found\n");
    let last = search(b"a::alpha\n", b"a\talpha\na\tbeta\n", &[("f", b"x")]);
    assert_eq!(last.err, "readme-forbidden: a: its control is not found\n");
    let first = search(b"a::alpha\n", b"a\tbeta\na\talpha\n", &[("f", b"x")]);
    assert_eq!(first.exit, Exit(0));
    let cr = search(b"a::alpha\\r\n", b"a\talpha\r\n", &[("f", b"x")]);
    assert_eq!(cr.exit, Exit(0), "the \\r of a control is its own");
    let tabs = search(b"a::x\\ty\n", b"a\tx\ty\n", &[("f", b"x")]);
    assert_eq!(tabs.exit, Exit(0), "split at the first tab");
}

/// **A list reads as the script read it**: split at the first `::`, `#` a
/// comment in the first column alone, a line of any of `bytes.strip()`'s
/// whitespace blank, and a `\r` part of the line.
#[test]
fn a_list_reads_as_the_script_read_it() {
    let r = search(
        b"# c\n\n \t\r\x0b\x0c\na::x::y\n #b::z\nno-end::q",
        b"a\tx::y\n #b\tz\nno-end\tq\n",
        &[("f", b"none")],
    );
    assert_eq!(r.exit, Exit(0), "{}", r.err);
    assert_eq!(r.out, summary(3, 1, 0));
    let crlf = search(b"a::alpha\r\n", b"a\talpha\n", &[("f", b"x")]);
    assert_eq!(crlf.exit, Exit(2), "the \\r is in the pattern");
    let crlf = search(b"a::alpha\r\n", b"a\talpha\r\n", &[("f", b"alpha\r")]);
    assert_eq!(crlf.exit, Exit(1));
}

#[test]
fn a_line_with_no_label_exits_1_and_says_so() {
    let r = search(b"a::x\nno label\n", b"a\tx\n", &[("f", b"x")]);
    assert_eq!(r.exit, Exit(1));
    assert_eq!(r.out, "");
    assert_eq!(r.err, "readme-forbidden: a line with no label in LIST\n");
}

#[test]
fn a_list_with_no_pattern_exits_1_and_says_so() {
    let r = search(b"# nothing\n\n", b"", &[("f", b"x")]);
    assert_eq!(r.exit, Exit(1));
    assert_eq!(r.out, "");
    assert_eq!(r.err, "readme-forbidden: LIST holds no pattern\n");
}

/// A label the script could not decode stopped it with exit 1; so does this.
#[test]
fn a_label_that_is_not_utf8_is_an_error() {
    let e = try_search(b"a\xff::x\n", b"", &[("f", b"x")]);
    assert_eq!(e, Err(ErrorKind::InvalidData));
    let e = try_search(b"a::x\n", b"a\xff\tx\n", &[("f", b"x")]);
    assert_eq!(e, Err(ErrorKind::InvalidData));
}

/// **The usage**: fewer than three arguments, exit 1, nothing read.
#[test]
fn fewer_than_three_arguments_is_the_usage() {
    let usage = "usage: cargo xtask forbidden PATTERNS CONTROLS FILE...\n";
    for args in [&[][..], &["l"], &["l", "c"]] {
        assert_eq!(
            ran(args, Path::new("/")),
            Ok(Ran {
                exit: Exit(1),
                out: String::new(),
                err: usage.to_string(),
            })
        );
    }
}

/// **A file that cannot be read stops the run**, with no hit and no
/// summary printed even for the files before it, and `main` exits 1.
#[test]
fn a_file_that_cannot_be_read_stops_the_run() {
    let d = dir();
    let (l, c, f) = (d.0.join("l"), d.0.join("c"), d.0.join("f"));
    fs::write(&l, b"a::alpha\n").expect("write");
    fs::write(&c, b"a\talpha\n").expect("write");
    fs::write(&f, b"alpha\n").expect("write");
    let (l, c, f) = (
        l.display().to_string(),
        c.display().to_string(),
        f.display().to_string(),
    );
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let e = forbidden::run(&[&l, &c, &f, "no-such-file"], &mut out, &mut err);
    assert_eq!(e.map_err(|e| e.kind()).err(), Some(ErrorKind::NotFound));
    assert!(out.is_empty() && err.is_empty());
    for args in [["no-list", &c, &f], [&l, "no-controls", &f]] {
        let e = forbidden::run(&args, &mut out, &mut err);
        assert_eq!(e.map_err(|e| e.kind()).err(), Some(ErrorKind::NotFound));
    }
    // A directory is no file either.
    let dir_name = d.0.display().to_string();
    let e = forbidden::run(&[&l, &c, &dir_name], &mut out, &mut err);
    assert!(e.is_err());
}

/// **A pattern that does not compile exits 2**, where the script's
/// uncaught `re.error` exited 1, naming its label and the reason and never
/// its text, and the controls are not read. Look-around is the case that
/// matters: the list must have been rewritten, and one that was not
/// searches nothing rather than search with what compiled.
#[test]
fn a_pattern_that_does_not_compile_exits_2_and_quotes_nothing() {
    for (rx, reason) in [
        (
            "(?<!qz)secret",
            "look-around, including look-ahead and look-behind, is not supported",
        ),
        (
            "secret(?!qz)",
            "look-around, including look-ahead and look-behind, is not supported",
        ),
        ("(secret", "unclosed group"),
        ("secret\\1", "backreferences are not supported"),
        ("secret\\Z", "unrecognized escape sequence"),
    ] {
        let list = format!("a::{rx}\n");
        let d = dir();
        let (l, f) = (d.0.join("l"), d.0.join("f"));
        fs::write(&l, list).expect("write");
        fs::write(&f, b"x").expect("write");
        let l = l.display().to_string();
        let f = f.display().to_string();
        let r = ran(&[&l, "no-controls-read", &f], &d.0).expect("no error");
        assert_eq!(r.exit, Exit(2), "{rx}");
        assert_eq!(r.out, "", "{rx}");
        assert_eq!(
            r.err,
            format!("readme-forbidden: a: does not compile: {reason}\n"),
            "{rx}"
        );
        assert!(!r.err.contains("secret"), "{}", r.err);
    }
}

/// **A byte above 0x7f is refused rather than read as a character**, which
/// would repeat and fold differently from the byte the script took. `\xHH`
/// is the same search.
#[test]
fn a_raw_high_byte_is_refused_and_an_escaped_one_is_the_byte() {
    for raw in [&b"a::caf\xc3\xa9\n"[..], b"a::caf\xe9\n"] {
        let r = search(raw, b"a\tcaf\xc3\xa9\n", &[("f", b"x")]);
        assert_eq!(r.exit, Exit(2));
        assert_eq!(
            r.err,
            "readme-forbidden: a: does not compile: a byte above 0x7f, to be written \\xHH\n"
        );
    }
    let r = search(b"a::caf\\xc3\\xa9\n", b"a\tcaf\xc3\xa9\n", &[("f", b"x")]);
    assert_eq!(r.exit, Exit(0), "{}", r.err);
    let r = search(
        b"a::caf\\xc3+\\xa9\n",
        b"a\tcaf\xc3\xc3\xa9\n",
        &[("f", b"caf\xc3\xc3\xc3\xa9")],
    );
    assert_eq!(r.exit, Exit(1), "the byte repeats, not the character");
}

/// **Every class is ASCII, as on a Python bytes pattern.** Each row holds
/// what `re.findall` printed for the same pattern and bytes.
#[test]
fn the_classes_are_ascii_as_on_a_python_bytes_pattern() {
    let spans = |rx: &str, hay: &[u8]| -> Vec<Range<usize>> {
        Pattern::compile("x", rx)
            .expect("compiles")
            .spans(hay)
            .collect()
    };
    // A letter of Latin-1 and the Kelvin sign are not words.
    assert_eq!(spans(r"\w+", b"a\xc3\xa9b"), [0..1, 3..4]);
    assert_eq!(spans(r"(?i:k)", b"k\xe2\x84\xaaK"), [0..1, 4..5]);
    // An Arabic-Indic digit is no digit, a no-break space no space.
    assert_eq!(spans(r"\d", b"1\xd9\xa12"), [0..1, 3..4]);
    assert_eq!(
        spans(r"\s", b" \xc2\xa0\t\n\r\x0b\x0c"),
        [0..1, 3..4, 4..5, 5..6, 6..7, 7..8]
    );
    // A boundary falls between an ASCII word byte and any other.
    assert_eq!(spans(r"\ba\b", b"a\xc3\xa9 a_ a"), [0..1, 7..8]);
    // Any byte but the newline; any byte but the one named, newline too.
    assert_eq!(spans(r".", b"\xff\n"), [0..1]);
    assert_eq!(spans(r"[^a]", b"\xffa\n"), [0..1, 2..3]);
    // `(?i)` folds ASCII alone, and leaves `\xc3` from `\xe3`.
    assert_eq!(spans(r"(?i:\xc3)", b"\xc3\xe3"), [0..1]);
    assert_eq!(spans(r"(?i:qz)", b"QZ qZ"), [0..2, 3..5]);
}

/// A look-around and what stands in for it, and what `re.finditer` reports
/// for the first on each text, as printed by Python 3.13.12.
type Row<'a> = (&'a [u8], &'a [Range<usize>]);

fn stands_in(rx: &str, rows: &[Row]) {
    let p = Pattern::compile("x", rx).expect("compiles");
    for (hay, want) in rows {
        let got: Vec<Range<usize>> = p.spans(hay).collect();
        assert_eq!(got, *want, "on {:?}", String::from_utf8_lossy(hay));
        assert_eq!(p.finds(hay), !want.is_empty());
    }
}

/// `(?<![-\w])name@`. The second hit of `name@name@` takes the last byte of
/// the first as what it looks behind at.
#[test]
fn a_look_behind_is_a_leading_class() {
    stands_in(
        r"(?:^|[^-\w])(?P<hit>name@)",
        &[
            (b"", &[]),
            (b"name@", &[0..5]),
            (b" name@", &[1..6]),
            (b"\nname@", &[1..6]),
            (b".name@", &[1..6]),
            (b"\xffname@", &[1..6]),
            (b"-name@", &[]),
            (b"_name@", &[]),
            (b"xname@", &[]),
            (b"0name@", &[]),
            (b"name@name@", &[0..5, 5..10]),
            (b"name@ name@", &[0..5, 6..11]),
            (b"name@xname@", &[0..5]),
            (b"name", &[]),
        ],
    );
}

/// `@name(?![\w-])`. `@name@name` is two hits: the first looked ahead at
/// the `@` the second begins with.
#[test]
fn a_look_ahead_is_a_following_class_or_the_end() {
    stands_in(
        r"(?P<hit>@name)(?:\z|[^\w-])",
        &[
            (b"@name", &[0..5]),
            (b"@name.", &[0..5]),
            (b"@name\n", &[0..5]),
            (b"@name\xff", &[0..5]),
            (b" @name", &[1..6]),
            (b"@name-", &[]),
            (b"@name_", &[]),
            (b"@namex", &[]),
            (b"@name@name", &[0..5, 5..10]),
            (b"@name @name", &[0..5, 6..11]),
            (b"@namex@name", &[6..11]),
        ],
    );
}

/// `/path/name(?![\w.-])`: a home directory, by itself or with a child.
#[test]
fn a_path_ends_where_the_name_does() {
    stands_in(
        r"(?P<hit>/path/name)(?:\z|[^\w.-])",
        &[
            (b"/path/name", &[0..10]),
            (b"/path/name/", &[0..10]),
            (b"x/path/name:", &[1..11]),
            (b"/path/name.", &[]),
            (b"/path/name-", &[]),
            (b"/path/names", &[]),
            (b"/path/name/path/name", &[0..10, 10..20]),
        ],
    );
}

/// `(?<![\w-])name(?![\w.-])`: the byte between two hits is looked at from
/// both sides.
#[test]
fn a_look_behind_and_a_look_ahead_share_a_byte() {
    stands_in(
        r"(?:^|[^\w-])(?P<hit>name)(?:\z|[^\w.-])",
        &[
            (b"name", &[0..4]),
            (b"(name)", &[1..5]),
            (b"name name", &[0..4, 5..9]),
            (b"name,name,name", &[0..4, 5..9, 10..14]),
            (b"name.name", &[5..9]),
            (b"name-name", &[]),
            (b"xname", &[]),
            (b"namex", &[]),
        ],
    );
}

/// `(?<![-\w])name@(?!allowed\.example)`: what the look-ahead refused
/// comes first, under the same look-behind, and a match of it is no hit.
#[test]
fn a_refused_sequence_is_matched_first_and_is_no_hit() {
    stands_in(
        r"(?:^|[^-\w])name@allowed\.example|(?:^|[^-\w])(?P<hit>name@)",
        &[
            (b"name@allowed.example", &[]),
            (b"name@allowed.example.org", &[]),
            (b" name@allowed.example", &[]),
            (b"name@allowed", &[0..5]),
            (b"name@other", &[0..5]),
            (b"name@allowedXexample", &[0..5]),
            (b"xname@other", &[]),
            (b"name@allowed.example name@x", &[21..26]),
            (b"name@allowed.examplename@", &[]),
            (b"name@allowed.example@name@", &[21..26]),
            (b"name@name@allowed.example", &[0..5]),
        ],
    );
}

/// `/path/(?!qa/|qbb/|qcc)[A-Za-z][a-z.-]{1,3}/`: a refusal in mid-pattern,
/// with a name of a bounded length after it.
#[test]
fn a_refusal_inside_a_pattern_is_the_same_form() {
    stands_in(
        r"/path/(?:qa/|qbb/|qcc)|(?P<hit>/path/[A-Za-z][a-z.-]{1,3}/)",
        &[
            (b"/path/qa/", &[]),
            (b"/path/qbb/", &[]),
            (b"/path/qccx/", &[]),
            (b"/path/qcc/", &[]),
            (b"/path/qax/", &[0..10]),
            (b"/path/qb/", &[0..9]),
            (b"/path/ab/", &[0..9]),
            (b"/path/a/", &[]),
            (b"/path/abcde/", &[]),
            (b"/path/ab//path/cd/", &[0..9, 9..18]),
            (b"/path/qa//path/ab/", &[9..18]),
            (b"/path/ab/path/cd/", &[0..9]),
        ],
    );
}

/// `(?i:(?<![\w.-])name[- ](?:qzlongs|qzl)(?![\w]))`: an end that varies,
/// where the look-ahead's refusal of the longer sends it back to the shorter
/// and then past both, and the look-around sits inside `(?i:...)`.
#[test]
fn a_look_ahead_after_alternatives_backs_off_as_it_did() {
    stands_in(
        r"(?i:(?:^|[^\w.-])(?P<hit>name[- ](?:qzlongs|qzl))(?:\z|[^\w]))",
        &[
            (b"name-qzlongs", &[0..12]),
            (b"name-qzl", &[0..8]),
            (b"name qzl", &[0..8]),
            (b"NAME-QZL", &[0..8]),
            (b"Name-QzLongs.", &[0..12]),
            (b"name-qzlo", &[]),
            (b"name-qzlongss", &[]),
            (b"xname-qzl", &[]),
            (b".name-qzl", &[]),
            (b"a name-qzl b", &[2..10]),
            (b"name-qzl name-qzlongs", &[0..8, 9..21]),
            (b"name-qzl-name-qzl", &[0..8]),
        ],
    );
}

/// `aa(?!b)`: a text that overlaps itself. The next search starts a byte
/// back, and a match there before the last hit's end is no hit.
#[test]
fn a_hit_does_not_overlap_the_last() {
    stands_in(
        r"(?P<hit>aa)(?:\z|[^b])",
        &[
            (b"aa", &[0..2]),
            (b"aaa", &[0..2]),
            (b"aaaa", &[0..2, 2..4]),
            (b"aaab", &[0..2]),
            (b"aab", &[]),
        ],
    );
}

/// `(?<!\w)a@(?!a@b)`: a candidate inside what was refused is still one,
/// since a refusal moves on a byte and not past its match.
#[test]
fn a_refusal_moves_on_a_byte() {
    stands_in(
        r"(?:^|[^\w])a@a@b|(?:^|[^\w])(?P<hit>a@)",
        &[
            (b"a@a@b", &[2..4]),
            (b"a@a@c", &[0..2, 2..4]),
            (b"a@a@a@b", &[0..2, 4..6]),
        ],
    );
}

/// `(?<!b)a`, the one position the module's doc comment names: `^` holds at
/// offset 0 alone, so a one-byte hit there leaves offset 1 unread, where
/// `re.finditer` reports `[0..1, 1..2]`. It takes a hit of one byte, or a
/// refusal with a look-behind on a text that matches at both offsets.
#[test]
fn offset_one_is_not_looked_at_twice() {
    stands_in(r"(?:^|[^b])(?P<hit>a)", &[(b"aa", &[0..1])]);
}

/// **A file of the stand-ins passes through the whole search**: its control
/// fires, and a hit and an allowed address are told apart by the verdict.
#[test]
fn the_stand_ins_run_through_the_search() {
    let list = br"addr::(?:^|[^-\w])name@allowed\.example|(?:^|[^-\w])(?P<hit>name@)
home::(?P<hit>/path/name)(?:\z|[^\w.-])
";
    let controls = b"addr\tname@x\nhome\t/path/name\n";
    let allowed = search(
        list,
        controls,
        &[("f", b"name@allowed.example /path/names\n")],
    );
    assert_eq!(allowed.exit, Exit(0), "{}", allowed.err);
    let found = search(list, controls, &[("f", b"xname@ /path/name/x\n")]);
    assert_eq!(found.exit, Exit(1));
    assert_eq!(found.err, "readme-forbidden: home in f\n");
    // A control only the refused branch matches is not found.
    let refused = search(
        list,
        b"addr\tname@allowed.example\nhome\t/path/name\n",
        &[("f", b"")],
    );
    assert_eq!(refused.exit, Exit(2));
    assert_eq!(
        refused.err,
        "readme-forbidden: addr: its control is not found\n"
    );
}

/// **Where this differs from the script, and why**, on patterns the
/// script stopped on or read another way. Group names and `(?i)` hold to
/// their own pattern here; the script's text join refused both, exit 1.
#[test]
fn a_group_name_or_a_flag_holds_to_its_own_pattern() {
    let list = b"a::(?P<hit>one)x\nb::(?P<hit>two)x\nc::(?i)three\n";
    let controls = b"a\tonex\nb\ttwox\nc\tTHREE\n";
    let r = search(list, controls, &[("f", b"none\n")]);
    assert_eq!(r.exit, Exit(0), "{}", r.err);
    assert_eq!(r.out, summary(3, 1, 0));
    let r = search(list, controls, &[("f", b"THREE\n")]);
    assert_eq!(r.err, "readme-forbidden: c in f\n");
    let r = search(
        b"d::(?i)three\ne::four\n",
        b"d\tthree\ne\tfour\n",
        &[("f", b"FOUR\n")],
    );
    assert_eq!(r.exit, Exit(0), "the flag stays with its pattern");
}

/// `$` is the end of the text alone, where `re` also takes it before a
/// final newline: a list that means the line's end writes `\r?\n`.
#[test]
fn a_dollar_is_the_end_of_the_text_alone() {
    let p = Pattern::compile("x", "name$").expect("compiles");
    assert!(p.finds(b"name"));
    assert!(!p.finds(b"name\n"));
}

/// **An empty pattern finds everything**, as the script's did, and an
/// iterator over empty hits comes to an end.
#[test]
fn an_empty_pattern_finds_everything() {
    let r = search(b"a::\n", b"a\t\n", &[("f", b"")]);
    assert_eq!(r.exit, Exit(1));
    assert_eq!(r.err, "readme-forbidden: a in f\n");
    let p = Pattern::compile("x", "").expect("compiles");
    let got: Vec<Range<usize>> = p.spans(b"ab").collect();
    assert_eq!(got, [0..0, 1..1, 2..2]);
}

/// A hit and a pattern with no `hit` group report the whole match.
#[test]
fn a_pattern_with_no_hit_group_reports_the_whole_match() {
    let p = Pattern::compile("x", "(a)b|c").expect("compiles");
    let got: Vec<Range<usize>> = p.spans(b"ab c ab").collect();
    assert_eq!(got, [0..2, 3..4, 5..7]);
    assert_eq!(p.label(), "x");
}

/// A group of another name is not the reported span.
#[test]
fn only_the_group_named_hit_is_the_span() {
    let p = Pattern::compile("x", "(?P<other>a)b").expect("compiles");
    let got: Vec<Range<usize>> = p.spans(b"ab").collect();
    assert_eq!(got, [0..2]);
}

/// The library's verdict, which `demo` reads: a list that could not be
/// used is told from a hit, though both exit 1 from the command.
#[test]
fn the_verdict_tells_a_bad_list_from_a_hit() {
    let d = dir();
    let (l, c, f) = (d.0.join("l"), d.0.join("c"), d.0.join("f"));
    fs::write(&c, b"a\talpha\n").expect("write");
    fs::write(&f, b"alpha\n").expect("write");
    let verdict = |list: &[u8]| {
        fs::write(&l, list).expect("write");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        forbidden::search(&l, &c, std::slice::from_ref(&f), &mut out, &mut err).expect("read")
    };
    assert_eq!(verdict(b"a::beta|alpha\n"), Verdict::Hits);
    assert_eq!(verdict(b"a::alpha\n").exit(), Exit(1));
    assert_eq!(verdict(b"nolabel\n"), Verdict::Refused(Exit(1)));
    assert_eq!(verdict(b"# empty\n"), Verdict::Refused(Exit(1)));
    assert_eq!(verdict(b"a::(\n"), Verdict::Refused(Exit(2)));
    assert_eq!(verdict(b"b::x\n"), Verdict::Refused(Exit(2)));
    fs::write(&f, b"none\n").expect("write");
    assert_eq!(verdict(b"a::alpha\n"), Verdict::Clean);
    assert_eq!(Verdict::Clean.exit(), Exit(0));
}

/// **On files of the tree**, with an invented list: the licence's name is
/// found and the crate's manifest does not hold it. The run on every
/// tracked file with the real list is not a test, as the list is not here.
#[test]
fn it_fires_on_the_tree() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let d = dir();
    let (l, c) = (d.0.join("l"), d.0.join("c"));
    fs::write(&l, b"licence::Creative\\s+Commons\n").expect("write");
    fs::write(&c, b"licence\tCreative Commons\n").expect("write");
    let hit = root.join("LICENSE").display().to_string();
    let clean = root.join("crates/xtask/Cargo.toml").display().to_string();
    let (l, c) = (l.display().to_string(), c.display().to_string());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let exit = forbidden::run(&[&l, &c, &hit, &clean], &mut out, &mut err).expect("read");
    assert_eq!(exit, Exit(1));
    assert_eq!(
        String::from_utf8(err).expect("utf-8"),
        format!("readme-forbidden: licence in {hit}\n")
    );
    assert_eq!(String::from_utf8(out).expect("utf-8"), summary(1, 2, 1));
}
