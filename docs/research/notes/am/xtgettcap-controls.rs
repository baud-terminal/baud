//! Controls for note AM (XTGETTCAP), committed in docs/research/notes/am/ and not built
//! there. Run in a git-archive copy of baud at 5477bf7 as baud-core/tests/xtgettcap_controls.rs:
//!   cargo test -p baud-core --test xtgettcap_controls -- --nocapture
//!
//! Q1 kitty's `CSI ? u` is answered at this commit, so a conforming
//!    application can turn the flags on (key.rs 86, 369, 418 and term.rs
//!    471-472, 1841-1843 say it is refused; write.rs 1668-1677 answers it).
//! Q2 F21 through `Term::key`: nothing under the base layer, `CSI 57384 u`
//!    once an application pushes a kitty flag.
//! Q3 today's path stores the `q` of `DCS + q` in the body: with the `q`,
//!    511 bytes of hex fit `OSC_MAX` = 512 and 512 do not.
//! Q4 a BEL ends a `DCS + q` body (state `Node::Str`, so `string()` takes it).
#[path = "support/mod.rs"]
mod support;
use baud_core::*;

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn show(b: &[u8]) -> String {
    b.iter()
        .map(|&c| match c {
            0x1b => "ESC ".to_string(),
            0x20..=0x7e => (c as char).to_string(),
            c => format!("<{c:02X}>"),
        })
        .collect()
}

#[test]
fn controls() {
    let ws = support::CORPUS;

    // Q1, with a positive control: `CSI ? u` before and after `CSI > 1 u`.
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    feed(&mut t, b"\x1b[?u");
    let before = t.reply().to_vec();
    t.clear_reply();
    feed(&mut t, b"\x1b[>1u\x1b[?u");
    let after = t.reply().to_vec();
    t.clear_reply();
    println!(
        "Q1 CSI ? u: reply {:?} with no flags; after CSI > 1 u: {:?}; unsupported {}",
        show(&before),
        show(&after),
        t.unsupported
    );
    assert_eq!(after, b"\x1b[?1u");

    // Q2: F21 and F35 through Term::key, before and after the push.
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    for (label, setup) in [
        ("base layer", &b""[..]),
        ("after CSI > 1 u", &b"\x1b[>1u"[..]),
    ] {
        feed(&mut t, setup);
        t.clear_reply();
        for p in [PuaKey::F21, PuaKey::F35] {
            let sent = t.key(KeyEvent::press(KeyCode::Pua(p), Modifiers::NONE));
            println!(
                "Q2 {label}: {p:?} key() -> {sent}, bytes {:?}",
                show(t.reply())
            );
            t.clear_reply();
        }
        // Positive control: F13 through the same path.
        let f13 = KeyCode::Function(baud_domain_fn(13));
        let sent = t.key(KeyEvent::press(f13, Modifiers::NONE));
        println!(
            "Q2 {label}: F13 key() -> {sent}, bytes {:?} (control)",
            show(t.reply())
        );
        t.clear_reply();
    }

    // Q3: `DCS + q` + N hex bytes + ST; count truncations.
    for n in [509usize, 510, 511, 512, 513] {
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        let tr0 = t.string_truncations;
        let mut q = b"\x1bP+q".to_vec();
        q.extend(std::iter::repeat_n(b'6', n));
        q.extend_from_slice(b"\x1b\\");
        feed(&mut t, &q);
        println!(
            "Q3 DCS + q + {n} B: string_truncations {} -> {}, strings_ignored {}",
            tr0, t.string_truncations, t.strings_ignored
        );
    }
    // Positive control for Q3: DECRQSS's prefix consumes `$ q`, so 512 bytes
    // of body after it fit and 513 do not.
    for n in [512usize, 513] {
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        let mut q = b"\x1bP$q".to_vec();
        q.extend(std::iter::repeat_n(b'm', n));
        q.extend_from_slice(b"\x1b\\");
        feed(&mut t, &q);
        println!(
            "Q3 control DCS $ q + {n} B: string_truncations {}",
            t.string_truncations
        );
    }

    // Q4: BEL ends a `DCS + q` body; the text after it prints.
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    feed(&mut t, b"\x1bP+q6b62");
    let ig1 = t.strings_ignored;
    feed(&mut t, b"\x07QQ");
    println!(
        "Q4 BEL after a DCS + q body: strings_ignored {} -> {}, cursor after QQ {:?}",
        ig1,
        t.strings_ignored,
        t.cursor()
    );
    // Control: without the BEL, `QQ` is absorbed into the body.
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    feed(&mut t, b"\x1bP+q6b62QQ");
    println!(
        "Q4 control, no BEL: strings_ignored {}, cursor after QQ {:?}",
        t.strings_ignored,
        t.cursor()
    );
}

fn baud_domain_fn(n: u8) -> FnKey {
    FnKey::new(n).unwrap()
}
