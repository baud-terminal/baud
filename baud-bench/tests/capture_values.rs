//! What the capture tools compute, against Python 3.13.12 and zlib 1.3.1:
//! each constant below is what those printed for the same input, before the
//! scripts were removed. Nothing here starts a process.

use baud_bench::capture::deflate::compress;
use baud_bench::capture::draws::Draws;
use baud_bench::capture::kitten::{self, Direct, Png};
use baud_bench::capture::resize::{Line, parse_sizes};
use baud_bench::capture::shell_start;
use baud_bench::capture::tui::keys_from;
use baud_bench::capture::yazi_files::{self, Colour};
use baud_bench::capture::yazi_walk::finished_previews;
use baud_bench::capture::{marks, py, vim_unicode};

use baud_bench::key_latency::sha256::digest as sha256_hex;

/// `repr` of `bytes`, its quote chosen as Python chooses it.
#[test]
fn bytes_are_shown_as_python_shows_them() {
    for (b, want) in [
        (b"".as_slice(), "b''"),
        (b"a'b", r#"b"a'b""#),
        (br#"a"b"#, r#"b'a"b'"#),
        (br#"'""#, r#"b'\'"'"#),
        (b"\x00\x1b\x7f\x80\xff", r"b'\x00\x1b\x7f\x80\xff'"),
        (b"\t\n\r\\", r"b'\t\n\r\\'"),
        (b" ~", "b' ~'"),
    ] {
        assert_eq!(py::bytes_repr(b), want);
    }
}

/// The `.steps` line a key is written as, against a committed fixture.
#[test]
fn a_key_is_written_as_the_committed_steps_write_it() {
    let steps = std::fs::read_to_string("../tests/completion/bash-full.steps")
        .expect("a committed fixture");
    let key = py::bytes_repr(b"g");
    assert!(steps.contains(&format!("# sent {key}\n")));
    assert!(steps.starts_with("# COLSxROWS <bytes of the .raw that one read returned>\n"));
}

/// `unicode_escape` and then `latin-1`: what each escape comes to.
#[test]
fn an_escape_string_means_what_python_read() {
    for (s, want) in [
        (r"\x1b[c", b"\x1b[c".as_slice()),
        (r"a\qb", br"a\qb"),
        (r"\101\7", b"A\x07"),
        (r"\u00e9", b"\xe9"),
        (r"\U000000e9", b"\xe9"),
        ("\\\n", b""),
        (r#"\'\"\\"#, b"'\"\\"),
        (r"\a\b\f\n\r\t\v", b"\x07\x08\x0c\n\r\t\x0b"),
        ("é", "é".as_bytes()),
        (r"\x7f\377", b"\x7f\xff"),
    ] {
        assert_eq!(py::unescape(s).as_deref(), Ok(want), "{s}");
    }
}

/// Each of these is an exception in Python, and exit 1.
#[test]
fn an_escape_python_died_on_is_refused() {
    for s in [
        r"\xZ",
        r"\x1",
        r"\u0100",
        r"\400",
        r"\",
        r"\U00110000",
        r"\N{DASH}",
    ] {
        assert!(py::unescape(s).is_err(), "{s}");
    }
}

/// The one exit code that differs: Python took a character's name at or
/// below U+00FF. `MEASUREMENTS.md` argues it.
#[test]
fn a_named_escape_is_refused_whatever_the_name() {
    assert!(py::unescape(r"\N{LATIN SMALL LETTER A}").is_err());
}

#[test]
fn numbers_are_read_as_python_reads_them() {
    for (s, f, i) in [
        ("3", Some(3.0), Some(3)),
        (" 3 ", Some(3.0), Some(3)),
        ("1_0", Some(10.0), Some(10)),
        ("_1", None, None),
        ("1__0", None, None),
        ("1_", None, None),
        ("1e3", Some(1000.0), None),
        (".5", Some(0.5), None),
        ("5.", Some(5.0), None),
        ("inf", Some(f64::INFINITY), None),
        ("-inf", Some(f64::NEG_INFINITY), None),
        ("infinity", Some(f64::INFINITY), None),
        ("+1", Some(1.0), Some(1)),
        ("abc", None, None),
        ("", None, None),
        ("1_0.5_0", Some(10.5), None),
        ("0x10", None, None),
    ] {
        assert_eq!(py::float(s), f, "{s}");
        assert_eq!(py::int(s), i, "{s}");
    }
    assert!(py::float("nan").is_some_and(f64::is_nan));
    assert_eq!(py::int("99999999999999999999"), Some(i64::MAX));
    assert_eq!(py::int("-99999999999999999999"), Some(i64::MIN));
}

#[test]
fn count_is_python_s() {
    assert_eq!(py::count(b"aXaXa", b"aXa"), 1);
    assert_eq!(py::count(b"abc", b""), 4);
    assert_eq!(py::count(b"", b""), 1);
    assert_eq!(py::count(b"aaa", b"aa"), 1);
}

#[test]
fn sleep_refuses_what_python_refused() {
    assert!(py::sleep(-1.0).is_err());
    assert!(py::sleep(f64::NAN).is_err());
    assert!(py::sleep(f64::INFINITY).is_err());
    assert!(py::sleep(0.0).is_ok());
}

#[test]
fn keys_split_at_the_separator_or_by_the_byte() {
    assert_eq!(keys_from("ab", "").unwrap(), [b"a", b"b"]);
    assert_eq!(keys_from(r"a\x1b", "").unwrap(), [b"a", b"\x1b"]);
    assert_eq!(keys_from("ab,,cd,", ",").unwrap(), [b"ab", b"cd"]);
    // The separator is not an escape string.
    assert_eq!(keys_from(r"a\tb", r"\t").unwrap(), [b"a\tb"]);
    assert!(keys_from(r"\xZ", "").is_err());
}

/// The draws for one seed, a run of each kind, as `random.Random(20261009)`
/// gave them.
#[test]
fn the_draws_are_random_random_s() {
    let mut r = Draws::new(20_261_009);
    let got: Vec<f64> = (0..3).map(|_| r.random()).collect();
    assert_eq!(
        got,
        [0.6671329806093301, 0.781700433788676, 0.8376189579158705]
    );
    let mut r = Draws::new(20_261_009);
    let got: Vec<u32> = (0..6).map(|_| r.randint(20, 120)).collect();
    assert_eq!(got, [105, 120, 45, 79, 37, 62]);
    let mut r = Draws::new(20_261_009);
    let got: Vec<f64> = (0..3).map(|_| r.gauss(0.0, 1.0)).collect();
    assert_eq!(
        got,
        [
            -0.8678901650420573,
            -1.5134532257467286,
            0.28629092420671826
        ]
    );
}

/// `gauss` keeps its second value for the next call, so a run of draws holds
/// half as many generator words as a fresh draw per value would.
#[test]
fn gauss_keeps_its_second_value() {
    let mut paired = Draws::new(1);
    paired.gauss(0.0, 1.0);
    paired.gauss(0.0, 1.0);
    let mut one = Draws::new(1);
    one.gauss(0.0, 1.0);
    assert_eq!(paired.random(), one.random());
}

/// The inputs the deflate goldens were taken over, as the Python built them.
fn deflate_cases() -> Vec<(&'static str, Vec<u8>, &'static str, u8)> {
    let mut r = Draws::new(1);
    let noise: Vec<u8> = (0..100_000)
        .map(|_| u8::try_from(r.randint(0, 255)).expect("a byte"))
        .collect();
    // Literal frequencies along Fibonacci numbers, shuffled: a code longer
    // than fifteen bits before the limit, so `gen_bitlen` takes its overflow
    // arm.
    let mut fib = Vec::new();
    let (mut a, mut b) = (1usize, 1usize);
    for sym in 0..24u32 {
        let byte = u8::try_from(sym * 7 % 256).expect("a byte");
        fib.extend(std::iter::repeat_n(byte, a));
        (a, b) = (b, a + b);
    }
    for i in (1..fib.len()).rev() {
        let j = r.randint(0, u32::try_from(i).expect("a short input"));
        fib.swap(i, usize::try_from(j).expect("an index"));
    }
    let words: Vec<String> = (0..60_000)
        .map(|_| format!("w{}", r.randint(0, 4000)))
        .collect();
    let chunk: Vec<u8> = (0..4100)
        .map(|_| u8::try_from(r.randint(0, 255)).expect("a byte"))
        .collect();
    let mut far = [chunk.as_slice(), chunk.as_slice(), chunk.as_slice(), b"abc"].concat();
    far.extend(std::iter::repeat_n(0, 5000));
    far.extend_from_slice(b"abc");
    // Copies of the lengths level 6's limits turn on, within and past the
    // window, among literals.
    let mut structured: Vec<u8> = Vec::new();
    const LENGTHS: [u32; 14] = [3, 4, 5, 6, 15, 16, 17, 18, 127, 128, 129, 130, 258, 300];
    let mut r2 = Draws::new(2);
    while structured.len() < 600_000 {
        if !structured.is_empty() && r2.random() < 0.7 {
            let top = structured.len().min(33_000);
            let d = usize::try_from(r2.randint(1, u32::try_from(top).expect("a distance")))
                .expect("a distance");
            for _ in 0..*r2.choice(&LENGTHS) {
                structured.push(structured[structured.len() - d]);
            }
        } else {
            structured.push(u8::try_from(r2.randint(0, 255)).expect("a byte"));
        }
    }
    // Twenty-three letters: three-byte matches at every distance, past 4096 too.
    let letters: Vec<u8> = (0..60_000)
        .map(|_| 100 + u8::try_from(r2.randint(0, 22)).expect("a byte"))
        .collect();
    // `abc` again at a distance of exactly 4096, the last a three-byte match
    // keeps, over a filler no three bytes of which repeat; not at position 0,
    // which a hash chain cannot name.
    let mut at_4096 = b"Zabc".to_vec();
    for i in 0..2046u32 {
        at_4096.push(128 + u8::try_from(i >> 8).expect("a byte"));
        at_4096.push(u8::try_from(i & 255).expect("a byte"));
    }
    at_4096.push(1);
    at_4096.extend_from_slice(b"abcQRS");
    // Fifteen nine-bit literals: a stored block exactly as long as a static one.
    let edge: Vec<u8> = (0xa0..0xafu8).collect();
    let ramp: Vec<u8> = (0..200_000u32)
        .map(|i| u8::try_from(i % 251).expect("a byte"))
        .collect();
    vec![
        (
            "empty",
            Vec::new(),
            "aca503def43dc086a5aaa5c3d6f86bcbbc2aa6a80007ddea4a81bce2b376fceb",
            1,
        ),
        (
            "one",
            b"a".to_vec(),
            "8ecacf17e3a7f86d1b03a3012ce1270e80b2a8468ad3942580a4c85c6231414e",
            1,
        ),
        (
            "three",
            b"abc".to_vec(),
            "32c466a222e2a30f1a0f2f1c84b64795d4ec1ee121f19c5793917fa60c21bee2",
            1,
        ),
        (
            "words",
            b"the quick brown fox jumps over the lazy dog ".repeat(40),
            "0fba01a0fcf96839e436cf88d12d84bf700aa5fae84b5047948bc7dd624bafd5",
            1,
        ),
        (
            "zeros",
            vec![0; 1 << 20],
            "1e6d5d105f8ee78fd7a403adeac8aef4913dc356ac3f66d3ab300309a1ca8a66",
            2,
        ),
        (
            "ramp",
            ramp,
            "336e68f5a9d8aeb8b14a6f51b03496827b4ccb6226f57c50a9cae2947406abe7",
            2,
        ),
        (
            "noise",
            noise,
            "387664327cfec01e0d5c1887cf4829fa8182aecdf8765f78bacceeb7aacebcb5",
            0,
        ),
        (
            "fib",
            fib,
            "aa17e4a43361db5b1072d37bb40d3c616cb946eb028b05217148b28e1de7e425",
            2,
        ),
        (
            "text",
            words.join(" ").into_bytes(),
            "ece6bc8f7045dc52c1374dd60b8c961e618efca05266bb4a5e1e9a1cb8f865ad",
            2,
        ),
        (
            "structured",
            structured,
            "a545d909b74a4c1cb0a1a64be03000a6afe0981361d959379717083bb098cd36",
            2,
        ),
        (
            "letters",
            letters,
            "ff77fb54b0bee0b052140b5d72e48d0da999b2b3cbaf502a7d2029fb00fc7c1d",
            2,
        ),
        (
            "at 4096",
            at_4096,
            "9fc3bdae273260cfad746b797c089fd5f073d51920747c8ecaf2d517161a6c62",
            2,
        ),
        (
            "edge",
            edge,
            "72d3efe5ceab3ac0bc8daafc61ff9bcc0feed24a79ce36af7adba6c44d171ee7",
            0,
        ),
        (
            "far",
            far,
            "85e4211c6e4fe57e9e33cacb15ff5468e2ddd679bcb64acc963f2f1ed9777559",
            2,
        ),
    ]
}

/// zlib's bytes, a stored, a static and a dynamic first block among them.
#[test]
fn deflate_writes_zlib_s_bytes() {
    let mut kinds = [false; 3];
    for (name, input, want, kind) in deflate_cases() {
        let z = compress(&input);
        assert_eq!(sha256_hex(&z), want, "{name}");
        // The first block's type: stored 0, static 1, dynamic 2.
        assert_eq!((z[2] >> 1) & 3, kind, "{name}");
        kinds[usize::from(kind)] = true;
    }
    assert_eq!(kinds, [true; 3]);
}

/// A golden the deflate could not have met by chance: the stream inflates to
/// the input, read by the decoder the app ships.
#[test]
fn deflate_inflates_back() {
    use baud_domain::zlib::Inflate;
    for (name, input, _, _) in deflate_cases() {
        let z = compress(&input);
        let mut out = vec![0u8; input.len()];
        let mut inflate = Inflate::NEW;
        inflate.feed(&z, &mut out).expect(name);
        inflate.finish().expect(name);
        assert_eq!(out, input, "{name}");
    }
}

/// The two generators with nothing random in them or one seed: the bytes the
/// scripts wrote.
#[test]
fn the_generators_write_the_scripts_bytes() {
    assert_eq!(
        sha256_hex(&vim_unicode::contents()),
        "8ed81bcbf06b32583e3ddefdade5121abc91753d0592bfa6916de26bd2dd19db"
    );
    let (out, lines) = marks::generate("marks", 1 << 20).expect("a mix");
    assert_eq!(
        (out.len(), lines, sha256_hex(&out).as_str()),
        (
            1_048_665,
            3368,
            "8c2e3988396883944f9339d708583d088e1817a98f326dc3cbc95677c7b0ff65"
        )
    );
    let (out, lines) = marks::generate("covered", 1 << 20).expect("a mix");
    assert_eq!(
        (out.len(), lines, sha256_hex(&out).as_str()),
        (
            1_048_653,
            3297,
            "90537df14c26813a8930669e915aeff681669eacc2cafa6bae644ddd8895210b"
        )
    );
    assert!(marks::generate("other", 1).is_none());
    assert_eq!(marks::generate("marks", 0).expect("a mix"), (Vec::new(), 0));
    assert_eq!(
        marks::generate("marks", -5).expect("a mix"),
        (Vec::new(), 0)
    );
}

/// The six files that take nothing from the host's libm, on any host.
#[test]
fn the_yazi_files_are_the_script_s() {
    let files = yazi_files::files();
    let got: Vec<(&str, String)> = files
        .others
        .iter()
        .map(|(n, b)| (*n, sha256_hex(b)))
        .collect();
    let want = [
        (
            "flat.png",
            "6bd0575cef1074bc264d2a5795dfab5e96dcd30e2f11653e5939fe8a0a3d7583",
        ),
        (
            "small.png",
            "0785a2c47d273ca87674eebd924a53b52664d5e919729126fd59b3211e38690b",
        ),
        (
            "screenshot.png",
            "6201ed5ae4fc321fdffb35087456f22d0d7389cbea902535135bf1b6342160a9",
        ),
        (
            "alpha.png",
            "332ceb3a9ce2575bd9360ccfd7e7c65163c9736d3c23e79a333988d9aa51e514",
        ),
        (
            "text.pdf",
            "d85c6076890a246cd643a791c94744f64d8a53bae92a498a657d5c7d387a2334",
        ),
        (
            "sparse.pdf",
            "d5290172a3be457e808325ac37be0838f3b685192c5392de04a07221df4f0e36",
        ),
    ];
    for ((name, sha), (want_name, want_sha)) in got.iter().zip(want) {
        assert_eq!(*name, want_name);
        assert_eq!(sha, want_sha, "{name}");
    }
    assert_eq!(files.photo.len(), 1200 * 1600 * 3);
}

/// The photo's noise goes through the host's libm: this pin is this kind of
/// host's. Python read `log`, `sin` and `cos` from the same libSystem.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
fn the_photo_s_pixels_are_the_script_s() {
    let mut rnd = Draws::new(yazi_files::SEED);
    let px = yazi_files::photo(&mut rnd);
    assert_eq!(sha256_hex(&px), PHOTO_ROWS);
}

/// `hashlib.sha256(px)` of the script's photo, before its PNG.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const PHOTO_ROWS: &str = "61575bc0ac2b6df6333758f91cc429f88c48915cb6dc2d3b6a4be86085a3dfe4";

#[test]
fn a_png_holds_its_rows() {
    let z = yazi_files::png(2, 1, &[1, 2, 3, 4, 5, 6], Colour::Rgb);
    assert_eq!(&z[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(
        sha256_hex(&z),
        "e6a5a4a74c60dc373f33e06acc7957f9e40d74451ddde6aeb32cb661a72d5e38"
    );
    let z = yazi_files::png(1, 1, &[1, 2, 3, 4], Colour::Rgba);
    assert_eq!(
        sha256_hex(&z),
        "69bf296785d54e4ec117aea10eac3a7269fc6d9fba672dc6819bdbc7817e883c"
    );
}

#[test]
fn a_pdf_is_the_script_s() {
    assert_eq!(
        sha256_hex(&yazi_files::pdf(b"x")),
        "b24d2891fce135802c0e468883a4aa96d27ab20345bc5ea20d31ce0df4ec2b19"
    );
}

#[test]
fn a_sizes_file_is_read_line_by_line() {
    let text = "# a comment\n\n80x20\r\n  60x10 50x6  \nenter\r45x4";
    assert_eq!(
        parse_sizes("f", text).unwrap(),
        [
            Line::Sizes(vec![(80, 20)]),
            Line::Sizes(vec![(60, 10), (50, 6)]),
            Line::Enter,
            Line::Sizes(vec![(45, 4)]),
        ]
    );
    // A number Python's `int` took; a field too large is refused at the ioctl.
    assert_eq!(
        parse_sizes("f", "+8_0x-1\n").unwrap(),
        [Line::Sizes(vec![(80, -1)])]
    );
}

#[test]
fn a_sizes_file_is_refused_with_the_line_it_stopped_at() {
    assert_eq!(
        parse_sizes("f", "80x20\n80\n").unwrap_err(),
        "f:2: expected COLSxROWS or `enter`, got '80'"
    );
    assert_eq!(
        parse_sizes("f", "ax1\n").unwrap_err(),
        "f:1: expected COLSxROWS, got 'ax1'"
    );
    assert_eq!(
        parse_sizes("f", "1x2x3\n").unwrap_err(),
        "f:1: expected COLSxROWS, got '1x2x3'"
    );
    assert_eq!(
        parse_sizes("f", "# nothing\nenter\n").unwrap_err(),
        "f: no sizes, so there is nothing to capture"
    );
}

#[test]
fn a_preview_counts_once_its_st_has_arrived() {
    assert_eq!(finished_previews(b""), 0);
    assert_eq!(finished_previews(b"\x1bP9;1qabc"), 0);
    assert_eq!(finished_previews(b"\x1bP9;1qabc\x1b\\"), 1);
    assert_eq!(finished_previews(b"\x1bP9;1qa\x1b\\ \x1bP9;1qb"), 1);
    assert_eq!(finished_previews(b"\x1bP9;1qa\x1b\\\x1bP9;1qb\x1b\\"), 2);
    // Another terminal's Sixel, and an ST with no Sixel, count nothing.
    assert_eq!(finished_previews(b"\x1bP0;1qa\x1b\\"), 0);
}

#[test]
fn a_shell_start_file_is_time_length_bytes() {
    let got = shell_start::file_bytes(&[(0.5, b"ab".to_vec()), (1.0, Vec::new())]);
    let mut want = 0.5f64.to_le_bytes().to_vec();
    want.extend_from_slice(&2u32.to_le_bytes());
    want.extend_from_slice(b"ab");
    want.extend_from_slice(&1.0f64.to_le_bytes());
    want.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(got, want);
}

#[test]
fn a_login_shell_is_named_with_its_dash() {
    assert_eq!(shell_start::login_name("/bin/zsh".as_ref()), "-zsh");
    assert_eq!(shell_start::login_name("zsh".as_ref()), "-zsh");
    assert_eq!(shell_start::login_name("/x/".as_ref()), "-");
}

#[test]
fn kitten_s_queries_are_found_as_the_pattern_found_them() {
    let out = b"x\x1b_Ga=q,t=d,i=1;AAAA\x1b\\\x1b_Ga=T,i=2;BB\x1b\\\x1b_Ga=q,t=s,i=3\x1b\\\x1b_Gt=f,a=q,i=4;CC\x1b\\\x1b[c";
    assert_eq!(
        kitten::queries(out),
        [b"a=q,t=d,i=1".as_slice(), b"t=f,a=q,i=4"]
    );
}

#[test]
fn only_the_direct_query_is_answered_and_only_when_asked() {
    let out = b"\x1b_Ga=q,t=d,i=1;AA\x1b\\\x1b_Ga=q,t=f,i=2;BB\x1b\\\x1b_Ga=q,i=3;CC\x1b\\\x1b[c";
    assert_eq!(
        kitten::reply(out, b"\x1b[?6c", Direct::Silent).unwrap(),
        b"\x1b[?6c"
    );
    assert_eq!(
        kitten::reply(out, b"\x1b[?6;4c", Direct::Answered).unwrap(),
        b"\x1b_Gi=1;OK\x1b\\\x1b_Gi=3;OK\x1b\\\x1b[?6;4c"
    );
    // A key with no value, and a direct query with no id, are the exceptions
    // the script died of.
    assert!(kitten::reply(b"\x1b_Ga=q,bad;A\x1b\\", b"", Direct::Silent).is_err());
    assert!(kitten::reply(b"\x1b_Ga=q,t=d;A\x1b\\", b"", Direct::Answered).is_err());
    assert!(kitten::reply(b"\x1b_Ga=q,t=d;A\x1b\\", b"", Direct::Silent).is_ok());
}

#[test]
fn a_recording_is_summarised_by_its_commands() {
    assert_eq!(kitten::summary(b"no graphics").unwrap(), "no APC G command");
    let raw = b"\x1b_Ga=T,f=100,m=1;ABCD\x1b\\\x1b_Gm=0;EF\x1b\\\x1b_Gq=2\x1b\\ \x1b_Gbroken\x1b[c";
    assert_eq!(
        kitten::summary(raw).unwrap(),
        "3 commands, first keys a=T,f=100,m=1, base64 6 bytes, largest chunk 4"
    );
    assert!(kitten::summary(b"\x1b_G\xff\x1b\\").is_err());
}

#[test]
fn every_input_runs_every_window_and_mosaic_blue_every_mode() {
    assert_eq!(kitten::cases(&[]).len(), 0);
    assert_eq!(kitten::cases(&["skyglow-adam7"]).len(), 4);
    let all = kitten::cases(&["mosaic-blue", "waterfall", "skyglow-adam7"]);
    assert_eq!(all.len(), 16);
    assert_eq!(all[0], ("mosaic-blue", "113x32", 0));
    assert_eq!(all[15], ("mosaic-blue", "113x32", 4));
}

#[test]
fn a_png_head_is_read_or_refused() {
    let head = |w: u32, il: u8| {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&[0, 0, 0, 1, 8, 2, 0, 0, il]);
        b
    };
    assert_eq!(kitten::ihdr(&head(300, 1)).unwrap(), Some((300, 1)));
    assert_eq!(kitten::ihdr(b"not a png").unwrap(), None);
    assert_eq!(kitten::ihdr(&head(300, 1)[..12]).unwrap(), None);
    assert!(kitten::ihdr(&head(300, 1)[..20]).is_err());
}

#[test]
fn a_scan_counts_by_window_and_dies_where_the_script_divided_by_zero() {
    let pngs = [
        Png {
            head: (100, 0),
            bytes: 10,
        },
        Png {
            head: (1000, 1),
            bytes: 30,
        },
        Png {
            head: (9000, 0),
            bytes: 500,
        },
    ];
    let (lines, died) = kitten::scan_lines(&pngs);
    assert_eq!(died, None);
    assert_eq!(
        lines,
        [
            "3 PNGs, 1 interlaced",
            "113x32 (1921 wide): 2 sent as-is, 40 bytes; Adam7 1 (50.00%), 30 bytes (75.00%); 1 sent raw",
            "80x24 (1360 wide): 2 sent as-is, 40 bytes; Adam7 1 (50.00%), 30 bytes (75.00%); 1 sent raw",
            "113x24 (1921 wide): 2 sent as-is, 40 bytes; Adam7 1 (50.00%), 30 bytes (75.00%); 1 sent raw",
            "226x64 (3842 wide): 2 sent as-is, 40 bytes; Adam7 1 (50.00%), 30 bytes (75.00%); 1 sent raw",
        ]
    );
    let (lines, died) = kitten::scan_lines(&pngs[2..]);
    assert_eq!(lines, ["1 PNGs, 0 interlaced"]);
    assert!(died.is_some());
    let (lines, died) = kitten::scan_lines(&[]);
    assert_eq!(lines, ["0 PNGs, 0 interlaced"]);
    assert!(died.is_some());
}
