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
/// container's bash writes with its `printf` for the `\e` and the `\a`.
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

/// **`hello.txt`, baud defined**: one line in each of Hebrew, Arabic,
/// Chinese, Japanese, Korean and Hindi, each the word for baud in that
/// language, a colon and its definition, quoted from the source named and
/// cut only where a clause ends; then a line of emoji and the six
/// languages' flags:
///
/// - Hebrew, "bod: yehidat mida asher metsayenet et mispar ha-ma'avarim
///   shel ot ha-modem bi-shniya ahat": baud: a unit of measure giving the
///   number of transitions of the modem's signal in one second. The word,
///   pointed, is the Academy of the Hebrew Language's
///   (`[[source: hebrew-academy-baud]]`), the definition the Standards
///   Institution of Israel's term 95.06.34 (`[[source: sii-95-06-34-baud]]`),
///   its first sentence.
/// - Arabic, "al-bawd: wahdat mu'addal at-tashkil fi al-ibraq wa-irsal
///   al-mu'tayat": baud: the unit of modulation rate in telegraphy and
///   data transmission. ITU-R V.607-3, Annex 1, in ITU's Arabic
///   (`[[source: itu-r-v607-3-ar]]`), up to its "or".
/// - Chinese, "bote: zai dianbao he shuju tongxin zhong de tiaozhilü de
///   danwei": baud: the unit of modulation rate in telegraphy and data
///   communication. V.607-3 in ITU's Chinese
///   (`[[source: itu-r-v607-3-zh]]`), up to its "or".
/// - Japanese, "bo: deta tsushin ni okeru henchō sokudo no tan'i. ichibyōkan
///   ni densō dekiru denki shingō no kazu o arawasu": baud: the unit of
///   modulation rate in data communication; it gives the number of
///   electrical signals that can be sent in a second. The Seisenban Nihon
///   Kokugo Daijiten (`[[source: kotobank-nikkoku-bo]]`), its definition
///   without the 1928 citation after it.
/// - Korean, "bo: deiteo jeonsong-eseo byeonjo sokdoreul natanaeneun
///   danwi": baud: the unit that gives the modulation rate in data
///   transmission. The Standard Korean Language Dictionary, bo 21
///   (`[[source: stdict-bo-21]]`), its first sentence.
/// - Hindi, "baud: tar/durmudrak prachalan mein gati ko mapne ka ek map":
///   baud: a measure of speed in telegraph and teleprinter working. The
///   word is the Commission for Scientific and Technical Terminology's
///   (`[[source: cstt-glossary-computer-science]]`), the definition the
///   glossary of Mohan Sundar Rajan's Doorsanchar Katha, National Book
///   Trust (`[[source: nbt-doorsanchar-katha]]`), p. 203.
/// - A pager, an arrow, signal bars and a stopwatch, a modem's signal
///   counted by the second; then the flags of Israel, Egypt, China, Japan,
///   South Korea and India.
pub const HELLO: &str = concat!(
    "בּוֹד: יחידת מידה אשר מציינת את מספר המעברים של אות המודם בשנייה אחת.\n",
    "البود: وحدة معدل التشكيل في الإبراق وإرسال المعطيات\n",
    "波特：在电报和数据通信中的调制率的单位\n",
    "ボー：データ通信における変調速度の単位。一秒間に伝送できる電気信号の数を表わす。\n",
    "보: 데이터 전송에서 변조 속도를 나타내는 단위.\n",
    "बॉड: तार/दूरमुद्रक प्रचालन में गति को मापने का एक माप।\n",
    // 📟️ ➡️ 📶 ⏱️, two spaces, 🇮🇱 🇪🇬 🇨🇳 🇯🇵 🇰🇷 🇮🇳; the pager, the
    // arrow and the stopwatch carry U+FE0F, each a sequence
    // emoji-variation-sequences lists, so they are drawn as emoji rather
    // than as text: baud's default presentation is the first face's to
    // have the character, and Noto Sans Symbols 2, pinned for btop's
    // braille, draws a pager in outline. The bars have no text form.
    "\u{1F4DF}\u{FE0F} \u{27A1}\u{FE0F} \u{1F4F6} \u{23F1}\u{FE0F}  ",
    "\u{1F1EE}\u{1F1F1} \u{1F1EA}\u{1F1EC} \u{1F1E8}\u{1F1F3} ",
    "\u{1F1EF}\u{1F1F5} \u{1F1F0}\u{1F1F7} \u{1F1EE}\u{1F1F3}\n",
);

/// **The container's prompt as it is drawn**, for the font check:
/// starship's, from baud's preset (`assets/starship/baud.toml`), in the
/// user's home, `~` and the ❯ in baud's amber. The image turns off the
/// line starship adds above it over ssh, the user and the host, and the
/// container's name, so the server is named by the session's badge alone
/// (`nix/demo-container/default.nix`). Before it bash writes
/// [`PROMPT_MARK`], as baud's own shell does, so a [`Prompt`] counts the
/// container's prompts too (`nix/demo-container/profile`).
pub const REMOTE_PROMPT: &str = "~ \u{276F} ";

/// **baud's shell's prompt as it is drawn**, for the font check:
/// starship's from the same preset, in the working directory under the
/// scratch home (`scratch::Layout::work`), nothing of the machine in it.
pub const PROMPT_SHOWN: &str = "~/baud \u{276F} ";

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
        assert_eq!(
            marks(b"\x1b]133;A;\x07~ \xe2\x9d\xaf "),
            1,
            "the container's"
        );
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

    /// **`hello.txt` is seven lines, baud defined in the six languages
    /// and the emoji**: each language's line is its word, a colon and the
    /// definition as its source words it -- the Hebrew word pointed as the
    /// Academy points it, a dagesh and a holam, so the font check meets
    /// them -- in the order Hebrew, Arabic, Chinese, Japanese, Korean,
    /// Hindi; the emoji line's pager and stopwatch are emoji, each with
    /// U+FE0F, which Noto Sans Symbols 2 would otherwise draw as text.
    #[test]
    fn hello_holds_the_seven_lines() {
        let lines: Vec<&str> = HELLO.lines().collect();
        assert_eq!(lines.len(), 7);
        for (line, word) in lines
            .iter()
            .zip(["בּוֹד", "البود", "波特", "ボー", "보", "बॉड"])
        {
            assert!(line.starts_with(word), "{line}");
        }
        for point in ['\u{05BC}', '\u{05B9}'] {
            assert!(lines[0].contains(point), "U+{:04X}", u32::from(point));
        }
        assert!(lines[1].ends_with("وحدة معدل التشكيل في الإبراق وإرسال المعطيات"));
        assert_eq!(lines[2], "波特：在电报和数据通信中的调制率的单位");
        assert!(lines[3].ends_with("一秒間に伝送できる電気信号の数を表わす。"));
        assert_eq!(lines[4], "보: 데이터 전송에서 변조 속도를 나타내는 단위.");
        assert!(HELLO.contains("\u{1F4DF}\u{FE0F} "), "the pager as emoji");
        assert!(HELLO.contains("\u{23F1}\u{FE0F}"), "the stopwatch as emoji");
    }

    /// **Each line of `hello.txt` is one row of the demo's 80 columns**:
    /// written into baud's core at 80x24, each line, the emoji's too, ends
    /// on the row it began on, so no definition wraps. Japanese's 40 wide
    /// characters fill the row exactly. The control: the Japanese line
    /// with one more wide character takes a second row.
    #[test]
    fn each_line_of_hello_is_one_row() {
        use baud_domain::{BytePos, ReadLen, WinSize};
        let ws = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let rows = |line: &str| {
            let mut owned = baud_core::Owned::new(&ws);
            let mut t = baud_core::Term::new(owned.storage(), &ws);
            let b = line.as_bytes();
            t.write_at(BytePos::ZERO, b);
            t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
            t.cursor().0.index() + 1
        };
        for line in HELLO.lines() {
            assert_eq!(rows(line), 1, "{line}");
        }
        let japanese = HELLO.lines().nth(3).unwrap();
        assert_eq!(rows(&format!("{japanese}語")), 2, "the control");
    }
}
