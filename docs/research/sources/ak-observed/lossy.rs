fn main() {
    for (name, b) in [("63 61 66 E9", &b"caf\xe9"[..]), ("E9 A9 41", &b"\xe9\xa9A"[..]), ("63 61 66 C3 A9", &b"caf\xc3\xa9"[..]), ("63 61 66 C3 A9 20 E9", &b"caf\xc3\xa9 \xe9"[..])] {
        let s = String::from_utf8_lossy(b);
        let n = s.chars().filter(|&c| c == '\u{fffd}').count();
        println!("{name}: {s:?} ({n} U+FFFD)");
    }
}
