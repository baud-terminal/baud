//! **The Linux arm's key poster**: the keys `key-latency` types into baud
//! on a Wayland desktop, through `linux-pointer`'s virtual keyboard, in
//! place of the macOS helper's CGEvents. Each key is a `post K C` line of
//! that tool's script, which presses and releases key K at once and prints
//! `post <n> <ns> <C>` with the CLOCK_MONOTONIC moment the press was sent:
//! the line the macOS helper prints, so the pairing and the report read
//! both arms alike. The release follows the press at once, well inside
//! any repeat delay, so baud never repeats a posted key and pairing by
//! order holds.
//!
//! The gaps follow the macOS helper's rule: the mean gap, each drawn
//! uniformly from [mean - mean/4, mean + mean/4] by a SplitMix64 seeded with
//! baud's pid, so a run replays from its pid and a fixed gap cannot
//! phase-lock to the refresh; a mean of 0 stays 0. The sleep is whole
//! milliseconds, the script's unit. The keymap is the pinned US one, whose
//! codes this module's table names.

use super::parse::Count;
use super::stats::Ms;
use super::sweep::Pid;

/// An evdev key code, as the virtual keyboard sends it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Evdev(pub u32);

/// The pinned US keymap the poster sends with.
pub const KEYMAP: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../baud-xkb/tests/fixtures/keymap-us.xkb"
);

/// The key that types `c` on the US keymap, unshifted: the letters and
/// digits, which is what `--chars` cycles through.
pub fn code_of(c: char) -> Option<Evdev> {
    const ROWS: [(&str, u32); 4] = [
        ("1234567890", 2),
        ("qwertyuiop", 16),
        ("asdfghjkl", 30),
        ("zxcvbnm", 44),
    ];
    ROWS.iter().find_map(|(row, first)| {
        row.chars()
            .position(|r| r == c)
            .and_then(|i| u32::try_from(i).ok())
            .map(|i| Evdev(first + i))
    })
}

/// A SplitMix64 generator, the macOS helper's.
struct SplitMix(u64);

impl SplitMix {
    /// The next draw, uniform in [0, 1): the top 32 bits of the next word.
    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        let top = u32::try_from((z ^ (z >> 31)) >> 32).unwrap_or(u32::MAX);
        f64::from(top) / 4_294_967_296.0
    }
}

/// The gaps after each key but the last, in whole milliseconds: uniform
/// in [mean - mean/4, mean + mean/4], seeded by `pid`.
pub fn gaps(mean: Ms, keys: Count, pid: Pid) -> Vec<Ms> {
    let mut rng = SplitMix(u64::from(pid.0));
    let m = mean.0 as f64;
    let q = m / 4.0;
    (1..keys.0.max(1))
        .map(|_| match mean.0 {
            0 => Ms(0),
            _ => Ms((m - q + rng.unit() * 2.0 * q).round() as i128),
        })
        .collect()
}

/// **The script that posts `keys` keys**, cycling through `chars`, with
/// the jittered gaps between them; an error names a character the US
/// keymap's table does not type.
pub fn script(chars: &str, keys: Count, mean: Ms, pid: Pid) -> Result<String, String> {
    let cs: Vec<char> = chars.chars().collect();
    if cs.is_empty() {
        return Err("no characters to post".into());
    }
    let gaps = gaps(mean, keys, pid);
    let mut out = String::new();
    for k in 0..keys.0 {
        let c = cs[k % cs.len()];
        let code = code_of(c).ok_or_else(|| format!("{c:?} is not a key the US keymap types"))?;
        out.push_str(&format!("post {} {c}\n", code.0));
        if let Some(g) = gaps.get(k) {
            out.push_str(&format!("sleep {}\n", g.0));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The table types what it says**: every letter and digit's code,
    /// pressed alone on the pinned US keymap, has that character as its
    /// text, so a posted key echoes the character its post line names.
    #[test]
    fn each_code_types_its_character_on_the_pinned_keymap() {
        use baud_xkb::{Keycode, Keymap, State};
        let text = std::fs::read_to_string(KEYMAP).expect("the pinned keymap");
        let km = Keymap::from_text(&text).expect("it compiles");
        let st = State::new(&km);
        let mut seen = 0;
        for c in "abcdefghijklmnopqrstuvwxyz0123456789".chars() {
            let code = code_of(c).expect("a key");
            let ev = baud_present_wayland::keys::press(&st, Keycode(code.0 + 8)).expect("an event");
            assert_eq!(ev.text, Some(baud_domain::Codepoint::from_char(c)), "{c}");
            seen += 1;
        }
        assert_eq!(seen, 36);
        assert_eq!(
            code_of('!'),
            None,
            "the control: a shifted character has no row"
        );
    }

    /// **The gaps are the macOS helper's rule**: within a quarter of the
    /// mean either way, the same for the same pid and different for
    /// another, all zero for a mean of zero, and one fewer than the keys.
    #[test]
    fn the_gaps_are_jittered_by_a_quarter_and_replay_from_the_pid() {
        let a = gaps(Ms(40), Count(200), Pid(4242));
        assert_eq!(a.len(), 199);
        assert!(a.iter().all(|g| (30..=50).contains(&g.0)), "{a:?}");
        assert_eq!(a, gaps(Ms(40), Count(200), Pid(4242)));
        assert_ne!(a, gaps(Ms(40), Count(200), Pid(4243)));
        let spread = a.iter().map(|g| g.0).max().unwrap() - a.iter().map(|g| g.0).min().unwrap();
        assert!(spread >= 15, "the gaps vary ({spread} ms)");
        assert!(gaps(Ms(0), Count(5), Pid(1)).iter().all(|g| g.0 == 0));
    }

    /// **A script posts each key, cycling the characters, with a sleep
    /// between keys and none after the last**; a character the table
    /// cannot type is an error, not a skipped key.
    #[test]
    fn a_script_posts_the_keys_with_the_gaps_between() {
        let s = script("ab", Count(3), Ms(0), Pid(1)).expect("a script");
        assert_eq!(s, "post 30 a\nsleep 0\npost 48 b\nsleep 0\npost 30 a\n");
        assert!(script("a!", Count(2), Ms(0), Pid(1)).is_err());
        assert!(script("", Count(2), Ms(0), Pid(1)).is_err());
    }
}
