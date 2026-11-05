//! **The README session, as data.** [`SESSION`] is the film, step by step:
//! edit it to change what is typed, what is waited for and how long each
//! screen holds. [`HELLO`] is the languages file the session prints.

use std::time::Duration;

/// One thing the session does, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Wait for the shell's next prompt: one more `OSC 133 ; A` mark in
    /// baud's pty log than the last `Prompt` waited for. The launch's own
    /// prompt is the first.
    Prompt,
    /// Type a command line at the prompt, wait until the shell has echoed
    /// it, then press Return.
    Run(&'static str),
    /// Type keys into the program on screen, without Return.
    Keys(&'static str),
    /// Type keys into the program on screen, then Return.
    Line(&'static str),
    /// Press Escape: the key, which baud encodes as the program on screen
    /// asked (`Term::key`), not an ESC typed as text.
    Escape,
    /// Hold the screen, in milliseconds of real time.
    Hold(u32),
    /// Turn the wheel at the centre of baud's window: `turns` events of
    /// `lines` lines each, [`WHEEL_GAP_MS`] apart. Positive shows earlier
    /// lines, so it scrolls up into the history. No shot of this take
    /// turns it.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "no shot of the README take turns the wheel")
    )]
    Wheel { turns: i32, lines: i32 },
}

#[cfg(test)]
use Step::Wheel;
use Step::{Escape, Hold, Keys, Line, Prompt, Run};

/// **The session the README's clip films**, about 19 seconds of it: one
/// visible edit of baud's own config changes the cursor's trail, then the
/// scripts, a photo drawn in the grid, a server over ssh, its system card
/// and its load, fireworks sent over the wire, home, and the empty prompt
/// the clip loops to.
pub const SESSION: &[Step] = &[
    // The launch: the splash carried off, then the prompt.
    Prompt,
    Hold(600),
    // baud's config in helix: the plasma trail down to its last line, the
    // word changed to liquid and saved, and the liquid trails up and down
    // again, by baud's live reload of the file it reads.
    Run("hx ~/.config/baud/config.toml"),
    Hold(300),
    Keys("ge"),
    Hold(600),
    Line("/plasma"),
    Keys("cliquid"),
    Escape,
    Line(":w"),
    // Past the 334 ms after the cursor's return from the command line lit
    // the last plasma, inside which a liquid only restarts the gel and
    // lights nothing new (`REIGNITE` in `trail.rs`).
    Hold(400),
    Keys("gg"),
    Hold(600),
    Keys("ge"),
    Hold(500),
    Line(":q"),
    Prompt,
    Hold(150),
    // Six languages and a line of emoji in the grid.
    Run("cat hello.txt"),
    Prompt,
    Hold(1100),
    // A photo drawn in place by kitten, in kitty's graphics protocol, FAX
    // lit while it is placed.
    Run(ICAT),
    Prompt,
    Hold(1200),
    // ssh into a container on this machine, its badge and CD lit while
    // ssh holds its connection (`container.rs`); its shell marks each
    // prompt as baud's does.
    Run("ssh demo"),
    Prompt,
    Hold(300),
    // Its system card, baud's mark beside it as a picture.
    Run("fastfetch"),
    Prompt,
    Hold(1400),
    // btop, its cpu box alone, drawn every 100 ms, sliding in and out.
    Run("btop"),
    Hold(1200),
    Keys("q"),
    Prompt,
    Hold(150),
    // iTerm2's fireworks, asked for over the wire, the request typed out.
    Run(FIREWORKS),
    Prompt,
    Hold(1100),
    // Home: the badge cleared, CD dark.
    Run("exit"),
    Prompt,
    Hold(200),
    // The cursor back at the top, and the empty prompt the clip loops to.
    Run("clear"),
    Prompt,
    Hold(1000),
];

/// **The picture shot's command**: kitten draws `canazei.jpg`, the
/// pinned photo (`scratch::PHOTO`) at `scratch::PHOTO_WIDTH` pixels,
/// centred below the command, and moves the cursor to the line after it,
/// where the next prompt comes. `--transfer-mode=stream` is typed rather
/// than hidden in an alias: kitten's default, detect, timed out after its
/// ten seconds in baud at e73843fa (`scripts/readme-capture.sh`). No
/// `--place`: kitten's help says its trailing new line has no effect with
/// it, and the earlier take moved the cursor below the photo itself, which
/// a typed session cannot do unseen.
pub const ICAT: &str = "kitten icat --transfer-mode=stream canazei.jpg";

/// **The fireworks' request**, typed at the container's shell in full:
/// `OSC 1337 ; RequestAttention=fireworks BEL`, iTerm2's, which the
/// container's busybox `printf` writes for its `\e` and `\a`.
pub const FIREWORKS: &str = "printf '\\e]1337;RequestAttention=fireworks\\a'";

/// The gap between two keys, in milliseconds, around which each is drawn
/// (`keys::Gaps`): about a fast typist's.
pub const KEY_GAP_MS: f64 = 40.0;

/// How long a key is held down: 40% of the gap, at most 25 ms.
pub fn key_hold_ms() -> f64 {
    (KEY_GAP_MS * 0.4).min(25.0)
}

/// The gap between two wheel events, in milliseconds.
pub const WHEEL_GAP_MS: u64 = 50;

/// The pause between moving the pointer to the window's centre and the
/// first wheel event, in milliseconds.
pub const WHEEL_SETTLE_MS: u64 = 30;

/// **`hello.txt`**: one line in each of Hebrew (pointed), Arabic, Chinese,
/// Japanese, Korean and Hindi, each saying "the terminal puts every
/// character in the grid", then the same in emoji and the six languages'
/// flags:
///
/// - Hebrew, "ha-masof sam kol tav ba-reshet": the terminal puts every
///   character in the grid.
/// - Arabic, "at-tarafiyya tada'u kull harf fi ash-shabaka": the terminal
///   puts every letter in the grid.
/// - Chinese, "zhongduan ba mei yi ge zi dou fang jin wangge li": the
///   terminal puts every single character into the grid.
/// - Japanese, "tanmatsu wa subete no moji o koshi ni naraberu": the
///   terminal lines every character up in the grid.
/// - Korean, "teomineoreun modeun geuljareul gyeokjae neonneunda": the
///   terminal puts every character into the grid.
/// - Hindi, "terminal har akshar ko grid mein rakhta hai": the terminal
///   keeps every character in the grid.
/// - A laptop, an arrow, the Latin letters, an arrow, a square; then the
///   flags of Israel, Egypt, China, Japan, South Korea and India.
pub const HELLO: &str = concat!(
    "הַמָּסוֹף שָׂם כָּל תָּו בָּרֶשֶׁת.\n",
    "الطرفية تضع كل حرف في الشبكة.\n",
    "终端把每一个字都放进网格里。\n",
    "端末はすべての文字を格子に並べる。\n",
    "터미널은 모든 글자를 격자에 넣는다.\n",
    "टर्मिनल हर अक्षर को ग्रिड में रखता है।\n",
    // 💻 ➡️ 🔤 ➡️ 🔲, two spaces, 🇮🇱 🇪🇬 🇨🇳 🇯🇵 🇰🇷 🇮🇳; the laptop and
    // the arrows carry U+FE0F, each a sequence emoji-variation-sequences
    // lists, so they are drawn as emoji rather than as text: baud's default
    // presentation is the first face's to have the character, and Noto
    // Sans Symbols 2, pinned for btop's braille, draws a laptop in outline.
    "\u{1F4BB}\u{FE0F} \u{27A1}\u{FE0F} \u{1F524} \u{27A1}\u{FE0F} \u{1F532}  ",
    "\u{1F1EE}\u{1F1F1} \u{1F1EA}\u{1F1EC} \u{1F1E8}\u{1F1F3} ",
    "\u{1F1EF}\u{1F1F5} \u{1F1F0}\u{1F1F7} \u{1F1EE}\u{1F1F3}\n",
);

/// **The container's prompt as it is drawn**, busybox ash's `PS1` of
/// `\u@\h:\w\$ ` for the container's user and host (`container::USER`,
/// `container::HOST`) in the user's home, for the font check. Before it
/// the prompt writes [`PROMPT_MARK`], as baud's own shell does, so a
/// [`Prompt`] counts the container's prompts too.
pub const REMOTE_PROMPT: &str = "baud@demo:~$ ";

/// The shell's prompt, for zsh's `PS1`: nothing of the machine in it.
pub const PS1: &str = "%F{cyan}baud%f %F{magenta}\u{276F}%f ";

/// The prompt as it is drawn, for the font check.
pub const PROMPT_SHOWN: &str = "baud \u{276F} ";

/// How many prompts the session waits for, the launch's included.
pub fn prompts(steps: &[Step]) -> usize {
    steps.iter().filter(|s| **s == Prompt).count()
}

/// How long the session takes at least: its holds, its typing at
/// [`KEY_GAP_MS`] a key and its wheel turns. What it waits for -- each
/// program's start, each prompt -- comes on top.
pub fn duration(steps: &[Step]) -> Duration {
    let ms: f64 = steps
        .iter()
        .map(|s| match *s {
            Prompt => 0.0,
            Run(t) => (t.chars().count() + 1) as f64 * KEY_GAP_MS,
            Keys(t) => t.chars().count() as f64 * KEY_GAP_MS,
            Line(t) => (t.chars().count() + 1) as f64 * KEY_GAP_MS,
            Escape => KEY_GAP_MS,
            Hold(ms) => f64::from(ms),
            Step::Wheel { turns, .. } => {
                (WHEEL_SETTLE_MS + u64::from(turns.unsigned_abs()) * WHEEL_GAP_MS) as f64
            }
        })
        .sum();
    Duration::from_secs_f64(ms / 1000.0)
}

/// One line of the plan for a step; `prompt` is the number a `Prompt`
/// step waits for.
pub fn describe(step: Step, prompt: usize) -> String {
    match step {
        Prompt => format!("wait for prompt {prompt}"),
        Run(t) => format!("type `{t}`, wait for its echo, Return"),
        Keys(t) => format!("type `{t}`"),
        Line(t) => format!("type `{t}`, Return"),
        Escape => "press Escape".to_string(),
        Hold(ms) => format!("hold {:.2} s", f64::from(ms) / 1000.0),
        Step::Wheel { turns, lines } => format!(
            "wheel {} turns of {lines} lines {} at the window's centre",
            turns.unsigned_abs(),
            if turns > 0 { "up" } else { "down" }
        ),
    }
}

/// **The events log's line for a step**, one for every step: `prompt N`
/// once the shell's prompt has come, `keys ...` as typing begins, `key
/// Escape` as it is pressed, `hold N ms` as a hold begins, `wheel T x L`
/// once the wheel has turned.
pub fn event(step: Step, prompt: usize) -> String {
    match step {
        Prompt => format!("prompt {prompt}"),
        Run(t) | Keys(t) | Line(t) => format!("keys {t}"),
        Escape => "key Escape".to_string(),
        Hold(ms) => format!("hold {ms} ms"),
        Step::Wheel { turns, lines } => format!("wheel {turns} x {lines}"),
    }
}

/// The mark baud's zsh integration prints before each prompt
/// (`baud-app/src/shell/zshenv.zsh`): `ESC ] 133 ; A ;`, then the shell's
/// pid. The container's prompt writes it too, with nothing after the
/// semicolon (`nix/demo-container/profile`).
pub const PROMPT_MARK: &[u8] = b"\x1b]133;A;";

/// How many prompt marks `bytes` holds.
pub fn marks(bytes: &[u8]) -> usize {
    bytes
        .windows(PROMPT_MARK.len())
        .filter(|w| *w == PROMPT_MARK)
        .count()
}

/// Whether `text` came back in `log` at or after `from`: what the keys
/// spelled was echoed. Only the bytes since the typing began count, so a
/// word an earlier screen printed -- helix's screen holds the config's
/// words -- cannot stand in for the keys arriving.
pub fn echoed(log: &[u8], from: usize, text: &str) -> bool {
    let tail = log.get(from..).unwrap_or(&[]);
    let t = text.as_bytes();
    !t.is_empty() && tail.windows(t.len()).any(|w| w == t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The session waits for ten prompts**: the launch and one after
    /// each of the nine commands, the four typed in the container among
    /// them, whose shell marks its prompts as baud's does; helix's `:q` is
    /// the first. Perturbation: drop the `Prompt` after `cat hello.txt` and
    /// the count is nine.
    #[test]
    fn the_session_waits_for_a_prompt_after_every_command() {
        assert_eq!(prompts(SESSION), 10);
        let runs = SESSION.iter().filter(|s| matches!(s, Run(_))).count();
        assert_eq!(runs, 9, "five at baud's shell, four in the container");
        let at = SESSION.iter().position(|s| *s == Run("ssh demo")).unwrap();
        assert_eq!(
            &SESSION[at..at + 3],
            [Run("ssh demo"), Prompt, Hold(300)],
            "into the container, to its own marked prompt"
        );
        let ran: Vec<&str> = SESSION
            .iter()
            .filter_map(|s| match s {
                Run(t) => Some(*t),
                _ => None,
            })
            .collect();
        assert_eq!(
            ran,
            [
                "hx ~/.config/baud/config.toml",
                "cat hello.txt",
                ICAT,
                "ssh demo",
                "fastfetch",
                "btop",
                FIREWORKS,
                "exit",
                "clear"
            ]
        );
        assert_eq!(SESSION.first(), Some(&Prompt), "the launch's prompt first");
        assert_eq!(SESSION.last(), Some(&Hold(1000)), "the clear screen held");
    }

    /// **The trail is changed by one visible edit, saved before the jump
    /// that shows it**: `ge` to the config's last line, `/plasma` selects
    /// the word, `c` replaces it with `liquid`, Escape leaves insert mode
    /// and `:w` saves, and only then `gg`. The hold after `:w` is past
    /// baud's reload and past the 334 ms after the cursor's return from
    /// helix's command line lit its last plasma, inside which a liquid only
    /// restarts the gel and lights nothing new (`REIGNITE` in `trail.rs`),
    /// so `gg` lights a liquid of its own; at 150 ms, in the take that typed
    /// fire, the headless session drew none there. The
    /// control: the steps in that order, with the save before the jump
    /// back. Perturbation: put `gg` before `:w` and the save's place fails.
    #[test]
    fn the_trail_is_changed_by_one_saved_edit() {
        let at = |step: Step| SESSION.iter().position(|s| *s == step).unwrap();
        let edit = at(Line("/plasma"));
        assert_eq!(
            &SESSION[edit - 2..edit + 6],
            [
                Keys("ge"),
                Hold(600),
                Line("/plasma"),
                Keys("cliquid"),
                Escape,
                Line(":w"),
                Hold(400),
                Keys("gg"),
            ]
        );
        assert!(at(Line(":w")) < at(Keys("gg")));
        assert!(matches!(SESSION[at(Line(":w")) + 1], Hold(ms) if ms > 334));
        assert_eq!(FIREWORKS, "printf '\\e]1337;RequestAttention=fireworks\\a'");
    }

    /// **The photo is drawn at baud's shell between the scripts and the
    /// server**, its command typed plain -- kitten, its transfer mode and
    /// the file's bare name, no path -- and held past its prompt. The
    /// control: the step before it is the scripts' hold and the one after
    /// its hold is `ssh`. Perturbation: put `--place 50x16@2x2` back in
    /// and the command's check fails.
    #[test]
    fn the_photo_is_drawn_between_the_scripts_and_the_server() {
        let at = SESSION.iter().position(|s| *s == Run(ICAT)).unwrap();
        assert_eq!(
            &SESSION[at - 1..at + 4],
            [Hold(1100), Run(ICAT), Prompt, Hold(1200), Run("ssh demo")]
        );
        assert_eq!(ICAT, "kitten icat --transfer-mode=stream canazei.jpg");
        assert!(!ICAT.contains('/'), "no path on screen");
    }

    /// **The plan's duration is its holds, its keys and its wheel**: 1.5 s
    /// of holds, `ab` with its Return at 40 ms a key (0.12 s), `q` (0.04 s),
    /// `:q` and Return (0.12 s), and two wheels of 4 turns, each 30 ms
    /// then 4 x 50 ms (0.46 s): 2.24 s. Perturbation: count a wheel's turns
    /// signed and the second wheel takes nothing off, which the total
    /// catches.
    #[test]
    fn the_plan_adds_its_holds_typing_and_wheel() {
        let steps = [
            Prompt,
            Run("ab"),
            Hold(1000),
            Keys("q"),
            Line(":q"),
            Wheel { turns: 4, lines: 6 },
            Hold(500),
            Wheel {
                turns: -4,
                lines: 6,
            },
        ];
        let d = duration(&steps).as_secs_f64();
        assert!((d - 2.24).abs() < 1e-9, "{d}");
        let whole = duration(SESSION).as_secs_f64();
        assert!(
            (15.0..20.0).contains(&whole),
            "the README session is about 18 s before its waits: {whole}"
        );
        let esc = duration(&[Escape]).as_secs_f64();
        assert!((esc - 0.04).abs() < 1e-9, "Escape is one key: {esc}");
    }

    /// **A prompt is its mark, counted exactly**, the container's bare
    /// `133;A;` as baud's own, and the echo is looked for only past the
    /// offset the typing began at. The control: the word before the offset
    /// is not found after it.
    #[test]
    fn prompts_are_counted_and_echo_is_looked_for_after_the_typing() {
        let log = b"\x1b]133;A;aid=7\x07baud > neo\r\n\x1b]133;D;0\x07\x1b]133;A;aid=7\x07";
        assert_eq!(marks(log), 2);
        assert_eq!(marks(b"\x1b]133;A;\x07baud@demo:~$ "), 1, "the container's");
        assert_eq!(marks(b"\x1b]133;B\x07\x1b]133;C\x07"), 0);
        let at = log.len();
        let more = [&log[..], b"neo"].concat();
        assert!(echoed(&more, at, "neo"));
        assert!(!echoed(log, at, "neo"), "the earlier neo does not count");
        assert!(echoed(log, 0, "neo"), "the control: it is in the log");
        assert!(!echoed(log, 0, ""), "nothing typed is never echoed");
    }

    /// **Every step writes its line to the events log**, the holds among
    /// them, so a recorder or a reader of the provenance file sees each:
    /// as many lines as steps, and one `hold` line a hold. Perturbation:
    /// give `Hold` no line and the two counts part.
    #[test]
    fn every_step_writes_a_line() {
        let mut prompt = 0;
        let lines: Vec<String> = SESSION
            .iter()
            .map(|s| {
                if *s == Prompt {
                    prompt += 1;
                }
                event(*s, prompt)
            })
            .filter(|l| !l.is_empty())
            .collect();
        assert_eq!(lines.len(), SESSION.len());
        let holds = SESSION.iter().filter(|s| matches!(s, Hold(_))).count();
        assert_eq!(
            lines.iter().filter(|l| l.starts_with("hold ")).count(),
            holds
        );
        assert_eq!(event(Hold(700), 0), "hold 700 ms");
        assert_eq!(event(Escape, 0), "key Escape");
        assert_eq!(lines[0], "prompt 1");
        assert_eq!(lines.last().map(String::as_str), Some("hold 1000 ms"));
    }

    /// **`hello.txt` is seven lines, the six languages and the emoji**,
    /// pointed Hebrew among them: the points and the sin and shin dots are
    /// there, so the font check meets them.
    #[test]
    fn hello_holds_the_seven_lines() {
        assert_eq!(HELLO.lines().count(), 7);
        let hebrew = HELLO.lines().next().unwrap();
        for point in [
            '\u{05B7}', '\u{05B8}', '\u{05BC}', '\u{05B9}', '\u{05C2}', '\u{05C1}',
        ] {
            assert!(hebrew.contains(point), "U+{:04X}", u32::from(point));
        }
        assert!(HELLO.contains("终端把每一个字都放进网格里。"));
        assert!(HELLO.contains("\u{1F4BB}\u{FE0F} "), "the laptop as emoji");
        assert!(HELLO.contains("端末はすべての文字を格子に並べる。"));
    }
}
