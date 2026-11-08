//! **The tour and the trailer, as data.** [`SESSION`] is the tour, the
//! film for YouTube, scene by scene: edit it to change what is typed, what
//! is waited for and how long each screen holds. Its scenes are the
//! storyline's 6 to 16, the README film's 1 to 5 being a session of their
//! own; each opens with a [`Step::Scene`] mark and, where the storyline
//! gives the scene a line, the line typed at the prompt as a shell comment:
//! the narration, which the shell does nothing with, so it reads with no
//! sound. [`trailer`] is take 2, built around the tour's address on
//! YouTube, which it refuses to be built without. [`HELLO`] is the
//! languages file the tour prints, in the container.

use std::time::Duration;

/// One thing the session does, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The start of a scene of the tour, by its number: it presses nothing,
    /// and the events log names it.
    Scene(u8),
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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "no scene of the tour presses Escape")
    )]
    Escape,
    /// Press Control and this letter's key, encoded the same way: a program
    /// that pushed kitty's keyboard flags, as helix does, hears the key
    /// rather than a control code typed as text.
    Ctrl(char),
    /// Press an arrow key, encoded the same way.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "no scene of the tour presses an arrow key")
    )]
    Arrow(Arrow),
    /// Hold the screen, in milliseconds of real time.
    Hold(u32),
    /// Turn the wheel at the centre of baud's window: `turns` events of
    /// `lines` lines each, [`WHEEL_GAP_MS`] apart. Positive shows earlier
    /// lines, so it scrolls up into the history; negative scrolls a page
    /// down.
    Wheel { turns: i32, lines: i32 },
    /// Rest the pointer on the middle of a cell of the grid, and leave it
    /// there.
    Hover(Cell),
    /// Press the left button in `from`, move to `to` across
    /// [`DRAG_MOVES`] moves [`DRAG_GAP_MS`] apart, and let go: a selection
    /// of the cells from `from` to `to`, pressed [`DRAG_IN`] of a cell in
    /// from the first's left edge and let go as far in from the last's
    /// right edge, so both ends are inside the selection.
    Drag { from: Cell, to: Cell },
    /// Command-C: the selection to the pasteboard, which must read as the
    /// text given (the session without a window checks it).
    Copy(&'static str),
    /// Command-V: the pasteboard pasted at the program on screen.
    Paste,
    /// Hold Command down this many milliseconds, the pointer where it
    /// rests: over a link, the window's bar shows where it goes.
    #[cfg_attr(not(test), expect(dead_code, reason = "no scene of the tour peeks"))]
    Peek(u32),
    /// A left click with Command held, in the middle of a cell: a link's
    /// open, which a program that tracks the mouse, as helix does, cannot
    /// take for its own as it would a plain click.
    CmdClick(Cell),
    /// Wait for the program on screen to show its first picture: a kitty
    /// graphics command, `ESC _ G`, in what it printed since the last
    /// typing began.
    Picture,
}

/// An arrow key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "no scene of the tour presses an arrow key")
)]
pub enum Arrow {
    Up,
    Down,
}

/// **A cell of the grid**, its row and its column from the top left, from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub row: u16,
    pub col: u16,
}

use Step::{
    Arrow as Move, CmdClick, Ctrl, Drag, Escape, Hold, Hover, Keys, Line, Paste, Peek, Picture,
    Prompt, Run, Scene, Wheel,
};

/// **The README film's opening line**, its scene 2's, and the trailer's.
pub const OPENING: &str = "# are you ready for this?";

/// **Where scene 11 copies from**: the word `copy` of its own line,
/// `# copy. paste.`, typed at the container's prompt, `~ ❯ `
/// ([`REMOTE_PROMPT`], four columns), so the word is columns 6 to 9. The
/// row is the one the line lands on after fastfetch's card and helix, 22:
/// read off the headless session; the take re-checks it.
pub const COPY_FROM: Cell = Cell { row: 22, col: 6 };
pub const COPY_TO: Cell = Cell { row: 22, col: 9 };
pub const COPIED: &str = "copy";

/// **Where scene 9's click lands**: inside `https://nixos.org` on the
/// README's line 29 (`[Nix](https://nixos.org)`), as helix shows it after
/// `/Nix`, read off a take: **row 12, column 40 is a placeholder** until
/// one is filmed.
pub const NIX_AT: Cell = Cell { row: 12, col: 40 };

/// **The tour**, the film for YouTube: the storyline's scenes 6 to 16 in
/// their order, from ssh into the container to the closing line. It opens
/// on the launch's prompt, which the first [`Prompt`] counts, so the first
/// line typed is not typed before the shell is up.
pub const SESSION: &[Step] = &[
    // 6. ssh into a container on this machine: OH as ssh starts, CD once
    // its connection is up, RD and SD with the data (`container.rs`); its
    // shell marks each prompt as baud's does.
    Scene(6),
    Prompt,
    Hold(600),
    Run("# now dial out. watch the lights."),
    Prompt,
    Run("ssh demo"),
    Prompt,
    Hold(1500),
    // 7. Its system card, baud's mark beside it as a picture.
    Scene(7),
    Run("fastfetch"),
    Prompt,
    Hold(1400),
    // 8. baud's README in the container's helix, the take's commit's
    // (`container::home_files`): the plasma trail four rows down and four
    // up, each one counted motion helix redraws at once, the word the
    // tagline turns on, three half pages down, its end and its top.
    Scene(8),
    Run("# watch the cursor."),
    Prompt,
    Run("hx README.md"),
    Hold(900),
    Keys(DOWN),
    Hold(300),
    Keys(UP),
    Hold(450),
    Line("/character"),
    Hold(700),
    Ctrl('d'),
    Hold(400),
    Ctrl('d'),
    Hold(400),
    Ctrl('d'),
    Hold(400),
    Keys("ge"),
    Hold(450),
    Keys("gg"),
    Hold(400),
    // 9. The Nix link Command-clicked in helix: nixos.org in Chawan on this
    // Mac, a short scroll, Chawan's `q`, then helix closes.
    Scene(9),
    Line("/Nix"),
    Hold(600),
    Hover(NIX_AT),
    Hold(400),
    CmdClick(NIX_AT),
    Picture,
    Hold(1500),
    Wheel {
        turns: -1,
        lines: 3,
    },
    Hold(1200),
    Keys("q"),
    Hold(400),
    Line(":q"),
    Prompt,
    Hold(300),
    // 10. Six languages and a line of emoji in the grid.
    Scene(10),
    Run("# every language. right to left too."),
    Prompt,
    Run("cat hello.txt"),
    Prompt,
    Hold(1100),
    // 11. A word of the scene's own line selected, copied -- the copy's
    // words in their box -- and pasted at the prompt after a `#`, so the
    // shell takes the pasted line as one more comment.
    Scene(11),
    Run("# copy. paste."),
    Prompt,
    Hold(300),
    Drag {
        from: COPY_FROM,
        to: COPY_TO,
    },
    Hold(400),
    Step::Copy(COPIED),
    Hold(1200),
    Keys("# "),
    Paste,
    Hold(900),
    Line(""),
    Prompt,
    Hold(300),
    // 12. btop, its cpu box alone, drawn every 100 ms, sliding in and out.
    Scene(12),
    Run("btop"),
    Hold(1200),
    Keys("q"),
    Prompt,
    Hold(150),
    // 13. iTerm2's fireworks, asked for over the wire, the request typed
    // out.
    Scene(13),
    Run("# one more thing"),
    Prompt,
    Run(FIREWORKS),
    Prompt,
    Hold(1100),
    // 14. Home: the badge cleared, CD dark.
    Scene(14),
    Run("exit"),
    Prompt,
    Hold(600),
    // 15. The trail, plasma until here, changed to the saucer by one `sed`
    // and baud's live reload, then the cursor sent to the top, and the
    // saucer flies it there.
    Scene(15),
    Run("# the finale"),
    Prompt,
    Run(SED_UFO),
    Prompt,
    Hold(600),
    Run("clear"),
    Prompt,
    Hold(1500),
    // 16. The end, held.
    Scene(16),
    Run(CLOSING),
    Prompt,
    Hold(2000),
];

/// **The tour's last line**, scene 16's.
pub const CLOSING: &str = "# that's baud. github.com/baud-terminal/baud";

/// **The finale's edit**: macOS's own `sed`, in place with no backup,
/// turning the trail's one word, `plasma`, to `ufo`, which baud reads again
/// as it changes.
pub const SED_UFO: &str = "sed -i '' s/plasma/ufo/ ~/.config/baud/config.toml";

/// **The fireworks' request**, typed at the container's shell in full:
/// `OSC 1337 ; RequestAttention=fireworks BEL`, iTerm2's, which the
/// container's bash writes with its `printf` for the `\e` and the `\a`.
pub const FIREWORKS: &str = "printf '\\e]1337;RequestAttention=fireworks\\a'";

/// **What the storyline requires on screen**, each found in the steps
/// themselves by [`shows`], not declared beside them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Show {
    /// A link followed: the pointer rested on a cell, then a Command-click
    /// on the same cell, and the picture of what it opened waited for.
    LinkClick,
    /// The lights: ssh out, the connection held, and home again.
    Lights,
    /// The README opened in helix and jumped through: Ctrl-d, `ge`, `gg`.
    ReadmeJumps,
    /// fastfetch run in the container.
    Fastfetch,
    /// A drag, Command-C and Command-V, in that order.
    CopyPaste,
    /// helix's cursor moved down and then up while the trail is plasma,
    /// each by one counted motion of more than [`THRESHOLD_ROWS`] rows,
    /// which helix redraws at once: a move past baud's snap, so each lights
    /// a plasma of its own whatever the round trip over ssh.
    ArrowTrail,
}

impl Show {
    pub const ALL: [Show; 6] = [
        Show::LinkClick,
        Show::Lights,
        Show::ReadmeJumps,
        Show::Fastfetch,
        Show::CopyPaste,
        Show::ArrowTrail,
    ];
}

/// The 334 ms inside which a move lights no new trail (`REIGNITE` in
/// `baud-app/src/trail.rs`); the plasma is exempt, and lights every long
/// move.
#[cfg(test)]
pub const REIGNITE_MS: u32 = 334;

/// **The rows a move may span and still snap**, drawing no trail
/// (`THRESHOLD` in `baud-app/src/trail.rs`, 2.0 cells): helix redraws
/// each arrow key as its own frame, and over ssh a frame of one row snaps,
/// so scene 8 moves by counted motions instead.
pub const THRESHOLD_ROWS: u32 = 2;

/// **Scene 8's two motions**, helix's `j` and `k` with a count: four
/// rows down and four up, each one redraw (amendment c.1).
pub const DOWN: &str = "4j";
pub const UP: &str = "4k";

/// **The rows a counted motion `keys` moves by `motion`**: its count
/// before the motion's key, `1` with none; none if it is another key.
pub fn counted(keys: &str, motion: char) -> Option<u32> {
    let count = keys.strip_suffix(motion)?;
    match count.is_empty() {
        true => Some(1),
        false => count.parse().ok(),
    }
}

/// **What of [`Show::ALL`] `steps` puts on screen**, each by the steps
/// that show it.
pub fn shows(steps: &[Step]) -> Vec<Show> {
    let at = |want: Step| steps.iter().position(|s| *s == want);
    let after = |from: usize, want: Step| {
        steps[from..]
            .iter()
            .position(|s| *s == want)
            .map(|i| from + i)
    };
    let mut out = Vec::new();
    let click = steps.iter().position(|s| matches!(s, CmdClick(_)));
    if let Some(c) = click {
        let rested = steps[..c].iter().rev().find_map(|s| match s {
            Hover(cell) => Some(*cell),
            _ => None,
        });
        let same = matches!((rested, steps[c]), (Some(h), CmdClick(k)) if h == k);
        if same && steps.get(c + 1) == Some(&Picture) {
            out.push(Show::LinkClick);
        }
    }
    let ssh = at(Run("ssh demo"));
    let exit = ssh.and_then(|s| after(s, Run("exit")));
    if let (Some(s), Some(_)) = (ssh, exit) {
        if matches!(steps.get(s + 2), Some(Hold(ms)) if *ms >= 1000) {
            out.push(Show::Lights);
        }
    }
    let hx = at(Run("hx README.md"));
    if let Some(open) = hx {
        let quit = after(open, Line(":q")).unwrap_or(steps.len());
        let inside = &steps[open..quit];
        if [Ctrl('d'), Keys("ge"), Keys("gg")]
            .iter()
            .all(|s| inside.contains(s))
        {
            out.push(Show::ReadmeJumps);
        }
    }
    if let (Some(s), Some(e), Some(f)) = (ssh, exit, at(Run("fastfetch"))) {
        if s < f && f < e {
            out.push(Show::Fastfetch);
        }
    }
    let drag = steps.iter().position(|s| matches!(s, Drag { .. }));
    let copy = steps.iter().position(|s| matches!(s, Step::Copy(_)));
    let paste = at(Paste);
    if let (Some(d), Some(c), Some(p)) = (drag, copy, paste) {
        let between = |a: usize, b: usize| steps[a..b].iter().all(|s| !matches!(s, Run(_)));
        if d < c && c < p && between(d, p) {
            out.push(Show::CopyPaste);
        }
    }
    if let (Some(hx), Some(search)) = (hx, at(Line("/character"))) {
        let long = |motion: char| {
            (hx..search).find(|&i| {
                matches!(steps[i], Keys(k) if counted(k, motion).is_some_and(|n| n > THRESHOLD_ROWS))
            })
        };
        if let (Some(d), Some(u)) = (long('j'), long('k')) {
            if d < u {
                out.push(Show::ArrowTrail);
            }
        }
    }
    out
}

/// **The tour's scenes as the steps hold them**: each [`Step::Scene`]'s
/// number, its narration -- the first command it types, when that is a
/// shell comment, `# ` and words -- and the steps up to the next scene.
pub fn scenes(steps: &[Step]) -> Vec<(u8, Option<&'static str>, std::ops::Range<usize>)> {
    let marks: Vec<(usize, u8)> = steps
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            Scene(n) => Some((i, *n)),
            _ => None,
        })
        .collect();
    marks
        .iter()
        .enumerate()
        .map(|(k, &(i, n))| {
            let end = marks.get(k + 1).map_or(steps.len(), |m| m.0);
            let says = steps[i..end].iter().find_map(|s| match s {
                Run(t) | Keys(t) | Line(t) => Some(*t),
                _ => None,
            });
            (n, says.filter(|t| t.starts_with("# ")), i..end)
        })
        .collect()
}

/// **The tour's address, checked**: `https://youtu.be/ID` or
/// `https://www.youtube.com/watch?v=ID`, either with more of a query after
/// the ID (`?si=...` from YouTube's share button, `&t=...`), the ID
/// YouTube's eleven characters of letters, digits, `-` and `_`; returns the
/// ID. The trailer types the address as `https://youtu.be/ID` whatever form
/// it was given in: typed bare at zsh, a `?` is a pattern, which fails to
/// match and runs nothing, and a `&` puts mpv in the background.
pub fn tour_id(url: &str) -> Result<&str, String> {
    let (rest, after) = match url.strip_prefix("https://youtu.be/") {
        Some(rest) => (rest, '?'),
        None => match url.strip_prefix("https://www.youtube.com/watch?v=") {
            Some(rest) => (rest, '&'),
            None => {
                return Err(format!(
                    "the trailer plays the tour from YouTube, and {url:?} is not a YouTube \
                     address: give https://youtu.be/ID or https://www.youtube.com/watch?v=ID"
                ));
            }
        },
    };
    let id = rest.split(after).next().unwrap_or(rest);
    let ok = id.len() == 11
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    match ok {
        true => Ok(id),
        false => Err(format!(
            "{id:?} in {url:?} is not a YouTube video's eleven-character id"
        )),
    }
}

/// **The longest stretch of the tour the trailer plays**, in milliseconds:
/// with its typing, its waits and its holds, the trailer runs about 20 s.
pub const STRETCH_MAX_MS: u32 = 12_000;

/// **A time in the tour as mpv takes it**, `SS`, `MM:SS` or `HH:MM:SS`,
/// each with an optional decimal part on the seconds; returns it in
/// milliseconds. Only digits, `:` and `.`, so the typed command holds
/// nothing the shell reads.
pub fn tour_time(t: &str) -> Result<u32, String> {
    let bad = || {
        format!(
            "{t:?} is not a time in the tour: give seconds, MM:SS or HH:MM:SS, as 83.5 or 1:23.5"
        )
    };
    let parts: Vec<&str> = t.split(':').collect();
    if t.is_empty() || parts.len() > 3 {
        return Err(bad());
    }
    let (whole, secs) = parts.split_at(parts.len() - 1);
    let (int, frac) = secs[0].split_once('.').unwrap_or((secs[0], ""));
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    if !digits(int) || !(frac.is_empty() || digits(frac)) || !whole.iter().all(|w| digits(w)) {
        return Err(bad());
    }
    if secs[0].ends_with('.') {
        return Err(bad());
    }
    let mut ms: u64 = 0;
    for (i, w) in whole.iter().enumerate() {
        let n: u64 = w.parse().map_err(|_| bad())?;
        if i > 0 && n >= 60 {
            return Err(bad());
        }
        ms = (ms + n) * 60;
    }
    let s: u64 = int.parse().map_err(|_| bad())?;
    if !whole.is_empty() && s >= 60 {
        return Err(bad());
    }
    let frac_ms: u64 = format!("{frac:0<3}")[..3].parse().map_err(|_| bad())?;
    let total = (ms + s) * 1000 + frac_ms;
    u32::try_from(total).map_err(|_| bad())
}

/// **The trailer**, take 2, about 20 s: the opening line, mpv playing the
/// tour from YouTube at `tour` from `start` to `end`, at its own speed --
/// typed as `mpv --start=START --end=END https://youtu.be/ID`, yt-dlp
/// finding its stream, a real wait on the network, which [`Step::Picture`]
/// waits out -- held as long as the stretch plays, mpv ending by itself at
/// `end`, and the tour's address as the last line. The stretch is the one
/// from the README's jumps to the dial-up's lights, its times read off a
/// take of the tour. Refused unless `tour` is the tour's address
/// ([`tour_id`]) and `start` and `end` are times in it, `end` after `start`
/// by at most [`STRETCH_MAX_MS`]: nothing stands in for any of them. The
/// commands are built once and kept for the process's life, as the steps
/// hold `'static` text.
pub fn trailer(
    tour: Option<&str>,
    start: Option<&str>,
    end: Option<&str>,
) -> Result<Vec<Step>, String> {
    let tour = tour.ok_or(
        "the trailer plays the tour from YouTube and has no address for it: give --tour URL",
    )?;
    let id = tour_id(tour)?;
    let (Some(start), Some(end)) = (start, end) else {
        return Err(
            "the trailer plays a stretch of the tour and has not been told which: give --start \
             and --end, read off a take of the tour"
                .to_string(),
        );
    };
    let (from, to) = (tour_time(start)?, tour_time(end)?);
    if to <= from {
        return Err(format!("--end {end} is not after --start {start}"));
    }
    if to - from > STRETCH_MAX_MS {
        return Err(format!(
            "--start {start} to --end {end} is {:.1} s, past the trailer's {} s",
            f64::from(to - from) / 1000.0,
            STRETCH_MAX_MS / 1000
        ));
    }
    let keep = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    let play = keep(format!(
        "mpv --start={start} --end={end} https://youtu.be/{id}"
    ));
    let says = keep(format!("# the whole tour: youtu.be/{id}"));
    Ok(vec![
        Prompt,
        Hold(600),
        Run(OPENING),
        Prompt,
        Hold(400),
        Run(play),
        Picture,
        Hold(to - from),
        Prompt,
        Run(says),
        Prompt,
        Hold(1200),
    ])
}

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

/// The pause after a drag's first move and before its press, and after its
/// last move and before its release, in milliseconds; the moves between and
/// the gap from each to the next, a drag of 0.86 s in all.
pub const DRAG_PAUSE_MS: u64 = 80;
pub const DRAG_MOVES: u32 = 20;
pub const DRAG_GAP_MS: u64 = 35;

/// How far into its end cells a drag presses and lets go, in cells.
pub const DRAG_IN: f64 = 0.15;

/// **baud's window around its grid, in points**: the title strip's height
/// above the grid (`film::chrome::INSET`) and the padding round the grid
/// (`padding`'s default in `config.rs`), and the tour's grid, 80x24 --
/// both held to the app's by `the_window_geometry_is_the_apps` in
/// `baud-app/src/film/session.rs`.
pub const TITLE_PT: f64 = 28.0;
pub const PAD_PT: f64 = 8.0;
pub const GRID: (u16, u16) = (80, 24);

/// **A point inside `cell`**, `within` of a cell from its top left (0.5,
/// 0.5 its middle), in the screen's points, of the window whose frame is at
/// `origin` of `size`: the grid below the title strip and inside the
/// padding, each cell an equal part of what is left. A window larger than
/// its grid by a fraction of a cell puts the point off by that fraction
/// over 80 columns, at most.
pub fn in_window(
    cell: Cell,
    within: (f64, f64),
    origin: (f64, f64),
    size: (f64, f64),
) -> (f64, f64) {
    let w = (size.0 - 2.0 * PAD_PT) / f64::from(GRID.0);
    let h = (size.1 - TITLE_PT - 2.0 * PAD_PT) / f64::from(GRID.1);
    (
        origin.0 + PAD_PT + (f64::from(cell.col) + within.0) * w,
        origin.1 + TITLE_PT + PAD_PT + (f64::from(cell.row) + within.1) * h,
    )
}

/// The middle of a cell.
pub const MIDDLE: (f64, f64) = (0.5, 0.5);

/// **Where a drag presses and lets go**, within its end cells.
pub fn drag_ends() -> ((f64, f64), (f64, f64)) {
    ((DRAG_IN, 0.5), (1.0 - DRAG_IN, 0.5))
}

/// **`hello.txt`, baud defined**: one line in each of Hebrew, Arabic,
/// Chinese, Japanese, Korean and Hindi, each the word for baud in that
/// language, a colon and baud's own one-line definition, written for this
/// file: a unit of symbol rate, how many times a second the signal changes.
/// Then a line of emoji and the six languages' flags. The text is baud's
/// own; no definition is quoted.
///
/// **What it was checked against**, for its meaning and its words for
/// baud, none of which is shown: the Hebrew word as the Academy of the
/// Hebrew Language points it, a dagesh and a holam
/// (`[[source: hebrew-academy-baud]]`), and the Standards Institution of
/// Israel's term 95.06.34 (`[[source: sii-95-06-34-baud]]`); ITU-R
/// V.607-3, Annex 1, in ITU's Arabic and Chinese
/// (`[[source: itu-r-v607-3-ar]]`, `[[source: itu-r-v607-3-zh]]`); the
/// Seisenban Nihon Kokugo Daijiten (`[[source: kotobank-nikkoku-bo]]`);
/// the Standard Korean Language Dictionary, bo 21
/// (`[[source: stdict-bo-21]]`); and the Hindi word as the Commission for
/// Scientific and Technical Terminology gives it
/// (`[[source: cstt-glossary-computer-science]]`), with Mohan Sundar
/// Rajan's Doorsanchar Katha, National Book Trust
/// (`[[source: nbt-doorsanchar-katha]]`). Each is a research reference,
/// read and not kept.
///
/// - Hebrew: baud: a unit of measure of modulation rate -- the number of
///   times per second that the signal can change. The Academy's terms are
///   the word and its modulation rate; it has none for symbol rate.
/// - Arabic: baud: the unit of symbol rate, that is, the number of times
///   the signal changes in a second.
/// - Chinese: baud: the unit of symbol rate, that is, the number of times
///   the signal changes per second.
/// - Japanese: baud: the unit of modulation rate. It expresses how many
///   times the signal changes in one second.
/// - Korean: baud: a unit of symbol rate, showing how many times the
///   signal changes in one second.
/// - Hindi: baud: the unit of symbol rate, that is, how many times the
///   signal changes in one second.
/// - A pager, an arrow, signal bars and a stopwatch, a modem's signal
///   counted by the second; then the flags of Israel, Egypt, China, Japan,
///   South Korea and India.
pub const HELLO: &str = concat!(
    "בּוֹד: יחידת מידה לקצב אפנון – מספר הפעמים בשנייה שהאות יכול להשתנות.\n",
    "البود: وحدة معدل الرموز، أي عدد مرات تغير الإشارة في الثانية.\n",
    "波特：符号速率的单位，即信号每秒变化的次数。\n",
    "ボー：変調速度の単位。1秒間に信号が何回変わるかを表す。\n",
    "보: 심벌 속도의 단위로, 신호가 1초에 몇 번 바뀌는지를 나타낸다.\n",
    "बॉड: प्रतीक दर की इकाई, यानी सिग्नल एक सेकंड में कितनी बार बदलता है।\n",
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

/// **The word the session's `/` looks for in the README**, the tagline's.
pub const SEARCHED: &str = "character";

/// **How many rows Ctrl-d moves in helix here**: half the view's inner
/// height, its half-page move in helix 25.07.1's source, and helix's
/// status line and command line leave 22 of the window's 24 rows. A row is
/// at most one line of the file, so a press goes this far down it at most.
pub const HALF_PAGE: usize = 11;

/// **Whether `readme` still holds the scene**: helix finds [`SEARCHED`],
/// its smart case ignoring a capital, and the session's Ctrl-d presses end
/// inside the file. A README that loses the word would leave the film's
/// search with nothing to find.
pub fn readme_scene(readme: &str) -> Result<(), String> {
    let at = readme
        .lines()
        .position(|l| l.to_lowercase().contains(SEARCHED))
        .ok_or_else(|| {
            format!(
                "README.md no longer holds `{SEARCHED}`, which the film's `/{SEARCHED}` looks \
                 for in it: put the word back or change the search in SESSION"
            )
        })?;
    let presses = SESSION.iter().filter(|s| **s == Ctrl('d')).count();
    let lines = readme.lines().count();
    let end = at + presses * HALF_PAGE;
    if end >= lines {
        return Err(format!(
            "README.md has {lines} lines, and the film's {presses} Ctrl-d from `{SEARCHED}` on \
             line {} go to line {}, past its end",
            at + 1,
            end + 1
        ));
    }
    Ok(())
}

/// **The key a step presses, as the window's key reaches the core**, for
/// [`Escape`], [`Ctrl`] and the arrows: `Term::key` encodes it as the program on screen
/// asked. A control chord names its key by the unshifted character and
/// carries no text, as the app's `chord` builds it.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the film's player presses these, not the window's"
    )
)]
pub fn key_event(step: Step) -> Option<baud_core::KeyEvent> {
    use baud_domain::{Codepoint, KeyCode, Modifiers};
    match step {
        Escape => Some(baud_core::KeyEvent::press(KeyCode::Escape, Modifiers::NONE)),
        Ctrl(c) => Some(baud_core::KeyEvent::press(
            KeyCode::Char(Codepoint::from_char(c)),
            Modifiers::CTRL,
        )),
        Move(Arrow::Up) => Some(baud_core::KeyEvent::press(KeyCode::Up, Modifiers::NONE)),
        Move(Arrow::Down) => Some(baud_core::KeyEvent::press(KeyCode::Down, Modifiers::NONE)),
        _ => None,
    }
}

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
            Prompt | Scene(_) | Picture => 0.0,
            Run(t) => (t.chars().count() + 1) as f64 * KEY_GAP_MS,
            Keys(t) => t.chars().count() as f64 * KEY_GAP_MS,
            Line(t) => (t.chars().count() + 1) as f64 * KEY_GAP_MS,
            Escape | Ctrl(_) | Move(_) | Step::Copy(_) | Paste => KEY_GAP_MS,
            Hold(ms) | Peek(ms) => f64::from(ms),
            Step::Wheel { turns, .. } => {
                (WHEEL_SETTLE_MS + u64::from(turns.unsigned_abs()) * WHEEL_GAP_MS) as f64
            }
            Hover(_) | CmdClick(_) => WHEEL_SETTLE_MS as f64,
            Step::Drag { .. } => (2 * DRAG_PAUSE_MS + u64::from(DRAG_MOVES) * DRAG_GAP_MS) as f64,
        })
        .sum();
    Duration::from_secs_f64(ms / 1000.0)
}

/// One line of the plan for a step; `prompt` is the number a `Prompt`
/// step waits for.
pub fn describe(step: Step, prompt: usize) -> String {
    match step {
        Scene(n) => format!("scene {n}"),
        Prompt => format!("wait for prompt {prompt}"),
        Run(t) => format!("type `{t}`, wait for its echo, Return"),
        Keys(t) => format!("type `{t}`"),
        Line(t) => format!("type `{t}`, Return"),
        Escape => "press Escape".to_string(),
        Ctrl(c) => format!("press Ctrl-{c}"),
        Move(a) => format!("press {a:?}"),
        Hold(ms) => format!("hold {:.2} s", f64::from(ms) / 1000.0),
        Step::Wheel { turns, lines } => format!(
            "wheel {} turns of {lines} lines {} at the window's centre",
            turns.unsigned_abs(),
            if turns > 0 { "up" } else { "down" }
        ),
        Hover(c) => format!("rest the pointer on row {}, column {}", c.row, c.col),
        CmdClick(c) => format!("Command-click row {}, column {}", c.row, c.col),
        Drag { from, to } => format!(
            "drag from row {}, column {} to row {}, column {}",
            from.row, from.col, to.row, to.col
        ),
        Step::Copy(t) => format!("press Command-C, the selection `{t}`"),
        Paste => "press Command-V".to_string(),
        Peek(ms) => format!("hold Command {:.2} s", f64::from(ms) / 1000.0),
        Picture => "wait for the program's first picture".to_string(),
    }
}

/// **The events log's line for a step**, one for every step: `scene N` as
/// a scene begins, `prompt N` once the shell's prompt has come, `keys ...`
/// as typing begins, `key Escape`, `key Ctrl-d`, `key Down`, `key Command-C`
/// and `key Command-V` as each is pressed, `hold N ms` as a hold begins,
/// `peek N ms` as Command goes down, `picture` once one is shown, `wheel T
/// x L` once the wheel has turned, `hover R,C`, `command-click R,C` and
/// `drag R,C to R,C` once the pointer has done it, by row and column.
pub fn event(step: Step, prompt: usize) -> String {
    match step {
        Scene(n) => format!("scene {n}"),
        Prompt => format!("prompt {prompt}"),
        Run(t) | Keys(t) | Line(t) => format!("keys {t}"),
        Escape => "key Escape".to_string(),
        Ctrl(c) => format!("key Ctrl-{c}"),
        Move(a) => format!("key {a:?}"),
        Step::Copy(_) => "key Command-C".to_string(),
        Paste => "key Command-V".to_string(),
        Hold(ms) => format!("hold {ms} ms"),
        Peek(ms) => format!("peek {ms} ms"),
        Picture => "picture".to_string(),
        Step::Wheel { turns, lines } => format!("wheel {turns} x {lines}"),
        Hover(c) => format!("hover {},{}", c.row, c.col),
        CmdClick(c) => format!("command-click {},{}", c.row, c.col),
        Drag { from, to } => {
            format!("drag {},{} to {},{}", from.row, from.col, to.row, to.col)
        }
    }
}

/// The mark baud's zsh integration prints before each prompt
/// (`baud-app/src/shell/zshenv.zsh`): `ESC ] 133 ; A ;`, then the shell's
/// pid. The container's prompt writes it too, with nothing after the
/// semicolon (`nix/demo-container/profile`).
pub const PROMPT_MARK: &[u8] = b"\x1b]133;A;";

/// **The start of a kitty graphics command**, `ESC _ G`
/// (`[[source: kitty-graphics]]`): what [`Step::Picture`] waits for.
pub const PICTURE_MARK: &[u8] = b"\x1b_G";

/// Whether `log` shows a picture at or after `from`.
pub fn pictured(log: &[u8], from: usize) -> bool {
    let tail = log.get(from..).unwrap_or(&[]);
    tail.windows(PICTURE_MARK.len()).any(|w| w == PICTURE_MARK)
}

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

    /// **The storyline's tour, scene by scene**: each scene's number, the
    /// `#` line the storyline gives it or none, and what the scene types.
    #[allow(clippy::type_complexity)]
    const STORYLINE: [(u8, Option<&str>, &[Step]); 11] = [
        (
            6,
            Some("# now dial out. watch the lights."),
            &[Prompt, Run("ssh demo")],
        ),
        (7, None, &[Run("fastfetch")]),
        (
            8,
            Some("# watch the cursor."),
            &[
                Run("hx README.md"),
                Keys("4j"),
                Keys("4k"),
                Line("/character"),
                Ctrl('d'),
                Keys("ge"),
                Keys("gg"),
            ],
        ),
        (
            9,
            None,
            &[
                Line("/Nix"),
                Hover(NIX_AT),
                CmdClick(NIX_AT),
                Picture,
                Wheel {
                    turns: -1,
                    lines: 3,
                },
                Keys("q"),
                Line(":q"),
            ],
        ),
        (
            10,
            Some("# every language. right to left too."),
            &[Run("cat hello.txt")],
        ),
        (11, Some("# copy. paste."), &[Step::Copy("copy"), Paste]),
        (12, None, &[Run("btop"), Keys("q")]),
        (13, Some("# one more thing"), &[Run(FIREWORKS)]),
        (14, None, &[Run("exit")]),
        (
            15,
            Some("# the finale"),
            &[
                Run("sed -i '' s/plasma/ufo/ ~/.config/baud/config.toml"),
                Run("clear"),
            ],
        ),
        (
            16,
            Some("# that's baud. github.com/baud-terminal/baud"),
            &[Hold(2000)],
        ),
    ];

    /// **The tour is the storyline's scenes 6 to 16, in order, each with
    /// its `#` line**: the marks number 6 to 16, each scene's first command
    /// is its line where the storyline gives one and no comment where it
    /// gives none, each line is typed at a prompt and waited past, and each
    /// scene types what the storyline says it does, inside it. It opens on
    /// the launch's prompt. The control: a list with two scenes swapped is
    /// refused. Perturbation: swap scenes 9 and 10 in `SESSION` and the
    /// numbers' order fails; drop scene 15's line and its narration fails.
    #[test]
    fn the_tour_is_the_storylines_scenes_six_to_sixteen_in_order() {
        let got = scenes(SESSION);
        let numbers: Vec<u8> = got.iter().map(|s| s.0).collect();
        assert_eq!(numbers, (6..=16).collect::<Vec<u8>>(), "the scenes' order");
        assert_eq!(
            SESSION[..3],
            [Scene(6), Prompt, Hold(600)],
            "the tour opens on the launch's prompt"
        );
        for ((n, says, range), (want_n, want_says, types)) in got.iter().zip(STORYLINE) {
            assert_eq!(*n, want_n);
            assert_eq!(*says, want_says, "scene {n}'s line");
            let inside = &SESSION[range.clone()];
            if let Some(line) = want_says {
                let at = inside.iter().position(|s| *s == Run(line)).unwrap();
                assert_eq!(
                    inside.get(at + 1),
                    Some(&Prompt),
                    "scene {n}: its line's prompt"
                );
            }
            for step in types {
                assert!(inside.contains(step), "scene {n} has no {step:?}");
            }
        }
        // The control: the same check refuses a tour with two scenes swapped.
        let mut swapped = SESSION.to_vec();
        let (nine, ten) = (got[3].2.clone(), got[4].2.clone());
        let (a, b) = (
            SESSION[nine.clone()].to_vec(),
            SESSION[ten.clone()].to_vec(),
        );
        swapped.splice(ten, a);
        swapped.splice(nine, b);
        let order: Vec<u8> = scenes(&swapped).iter().map(|s| s.0).collect();
        assert_ne!(order, (6..=16).collect::<Vec<u8>>(), "the control");
    }

    /// **Everything the storyline must show is on screen**: a link
    /// clicked, the lights, the README's jumps, fastfetch, copy and paste,
    /// and the plasma trail down and up, each found in the tour's steps by
    /// [`shows`]. The controls: the tour without its drag shows no copy and
    /// paste; with its `4k` made `2k`, two rows, which snap, no arrow
    /// trail; and with the click on another cell than
    /// the pointer rested on, no link. Perturbation: delete the `Drag` from
    /// `SESSION` and the first assertion fails.
    #[test]
    fn every_must_show_item_is_in_some_step() {
        assert_eq!(shows(SESSION), Show::ALL, "what the tour shows");
        assert_eq!(
            [counted(DOWN, 'j'), counted(UP, 'k'), counted("j", 'j')],
            [Some(4), Some(4), Some(1)]
        );
        assert_eq!(counted("gg", 'j'), None, "another key");
        let no_drag: Vec<Step> = SESSION
            .iter()
            .copied()
            .filter(|s| !matches!(s, Drag { .. }))
            .collect();
        assert!(!shows(&no_drag).contains(&Show::CopyPaste), "the control");
        let quick: Vec<Step> = SESSION
            .iter()
            .copied()
            .map(|s| match s {
                Keys("4k") => Keys("2k"),
                s => s,
            })
            .collect();
        assert!(!shows(&quick).contains(&Show::ArrowTrail), "the control");
        let elsewhere: Vec<Step> = SESSION
            .iter()
            .copied()
            .map(|s| match s {
                CmdClick(c) => CmdClick(Cell {
                    row: c.row + 1,
                    col: c.col,
                }),
                s => s,
            })
            .collect();
        assert!(!shows(&elsewhere).contains(&Show::LinkClick), "the control");
    }

    /// **The trailer is refused without the tour's address and its
    /// stretch**: no address, another site's, an id of the wrong length; no
    /// start or no end; an end not after the start, a stretch past
    /// [`STRETCH_MAX_MS`], a time that is not one. Given them, in any of
    /// YouTube's forms, it opens on the tour's opening line, plays the
    /// address in mpv from the start to the end -- in its short form, no
    /// `?` or `&`, which zsh would read as a pattern or a background job --
    /// waits for its first frame, holds the stretch, and ends on the short
    /// form. Perturbation: let `trailer` take a missing end to a default and
    /// the end's assertion fails; type the address as given and the `?`
    /// check fails.
    #[test]
    fn the_trailer_refuses_without_the_tours_address() {
        let ok = "https://youtu.be/abcdefghijk";
        let (s, e) = (Some("1:23.5"), Some("1:33"));
        let err = trailer(None, s, e).unwrap_err();
        assert!(err.contains("--tour"), "{err}");
        for bad in [
            "",
            "https://example.com/watch?v=abcdefghijk",
            "http://youtu.be/abcdefghijk",
            "https://youtu.be/abcdefghij",
            "https://youtu.be/abcdefghij!",
        ] {
            assert!(trailer(Some(bad), s, e).is_err(), "{bad:?}");
        }
        for (start, end) in [(None, e), (s, None), (None, None)] {
            let err = trailer(Some(ok), start, end).unwrap_err();
            assert!(err.contains("--start") && err.contains("--end"), "{err}");
        }
        for (start, end) in [
            ("90", "90"),
            ("90", "80"),
            ("10", "30"),
            ("1:2x", "1:30"),
            ("1:75", "1:80"),
            ("83;rm", "90"),
            ("83.", "90"),
        ] {
            assert!(
                trailer(Some(ok), Some(start), Some(end)).is_err(),
                "{start}..{end}"
            );
        }
        assert_eq!(tour_time("83.5"), Ok(83_500));
        assert_eq!(tour_time("1:23.5"), Ok(83_500));
        assert_eq!(tour_time("0:01:23"), Ok(83_000));
        for url in [
            "https://www.youtube.com/watch?v=abc-EFG_123",
            "https://www.youtube.com/watch?v=abc-EFG_123&t=5s",
            "https://youtu.be/abc-EFG_123?si=AbCdEf0123",
            "https://youtu.be/abc-EFG_123",
        ] {
            assert_eq!(tour_id(url), Ok("abc-EFG_123"), "{url}");
        }
        let steps = trailer(Some("https://youtu.be/abc-EFG_123?si=AbCdEf0123"), s, e).unwrap();
        let play = steps
            .iter()
            .position(|s| *s == Run("mpv --start=1:23.5 --end=1:33 https://youtu.be/abc-EFG_123"))
            .expect("mpv plays the stretch of the address in its short form");
        for s in &steps {
            // A comment's `?` is no pattern: interactive_comments.
            let command = match s {
                Run(t) if !t.starts_with("# ") => *t,
                _ => continue,
            };
            assert!(
                !command.contains(['?', '&']),
                "zsh would read {command:?} otherwise"
            );
        }
        assert_eq!(steps[play + 1..play + 4], [Picture, Hold(9_500), Prompt]);
        assert_eq!(steps[2], Run(OPENING));
        assert!(steps.contains(&Run("# the whole tour: youtu.be/abc-EFG_123")));
        assert!(
            !steps.iter().any(|s| matches!(s, Scene(_))),
            "the trailer is no scene of the tour"
        );
        let fixed = duration(&steps).as_secs_f64() - 9.5;
        assert!(
            fixed + f64::from(STRETCH_MAX_MS) / 1000.0 < 20.5,
            "about 20 s at the longest, before the network: {fixed}"
        );
    }

    /// **A picture is a kitty graphics command after the typing**, an
    /// earlier one not counted. Perturbation: look from 0 and the second
    /// assertion fails.
    #[test]
    fn a_picture_is_looked_for_after_the_typing() {
        let log = b"\x1b_Ga=T;AAAA\x1b\\mpv x\r\n";
        assert!(pictured(log, 0));
        assert!(!pictured(log, 5), "the earlier picture does not count");
        let more = [&log[..], b"\x1b_Gm=1;BB\x1b\\"].concat();
        assert!(pictured(&more, log.len()));
    }

    /// **The tour waits for 18 prompts**: the launch, one after each of
    /// its 16 lines -- helix's after its `:q`, btop's after its `q` -- the
    /// ones typed in the container among them, whose shell marks its
    /// prompts as baud's does, and one after scene 11's pasted line.
    /// Perturbation: drop the `Prompt` after `cat hello.txt` and the count
    /// is 17.
    #[test]
    fn the_session_waits_for_a_prompt_after_every_command() {
        assert_eq!(prompts(SESSION), 18);
        let runs = SESSION.iter().filter(|s| matches!(s, Run(_))).count();
        assert_eq!(runs, 16);
        let at = SESSION.iter().position(|s| *s == Run("ssh demo")).unwrap();
        assert_eq!(
            &SESSION[at..at + 3],
            [Run("ssh demo"), Prompt, Hold(1500)],
            "into the container, to its own marked prompt"
        );
        let ran: Vec<&str> = SESSION
            .iter()
            .filter_map(|s| match s {
                Run(t) => Some(*t),
                _ => None,
            })
            .collect();
        let commands: Vec<&str> = ran.into_iter().filter(|t| !t.starts_with("# ")).collect();
        assert_eq!(
            commands,
            [
                "ssh demo",
                "fastfetch",
                "hx README.md",
                "cat hello.txt",
                "btop",
                FIREWORKS,
                "exit",
                SED_UFO,
                "clear"
            ]
        );
        assert_eq!(SESSION.last(), Some(&Hold(2000)), "the last line held 2 s");
    }

    /// **The finale turns the plasma trail to the saucer**: the config's
    /// trail is `plasma` (the config's own test says it is the word's one
    /// line), and nothing in the tour types over it before the finale's
    /// `sed`, which turns that word to `ufo` before `clear`, typed at
    /// baud's own shell after `exit`. Perturbation: put `liquid` back in
    /// `SED_UFO` and its assertion fails.
    #[test]
    fn the_finale_turns_the_plasma_trail_to_the_saucer() {
        assert_eq!(
            SED_UFO,
            "sed -i '' s/plasma/ufo/ ~/.config/baud/config.toml"
        );
        let at = |step: Step| SESSION.iter().position(|s| *s == step).unwrap();
        assert!(at(Run("exit")) < at(Run(SED_UFO)));
        assert!(at(Run(SED_UFO)) < at(Run("clear")));
        assert!(
            SESSION
                .iter()
                .all(|s| !matches!(s, Keys(k) if k.contains("plasma"))),
            "nothing types over the trail before the finale"
        );
        assert_eq!(FIREWORKS, "printf '\\e]1337;RequestAttention=fireworks\\a'");
    }

    /// **The README's scene is one helix, in the requested order**: `hx
    /// README.md`, `4j` and `4k`, `/character`, Ctrl-d
    /// three times, `ge`, `gg`, then the Nix link's `/Nix`, its click,
    /// Chawan's `q` and `:q`, then the shell's prompt; and every hold after
    /// a jump is past the 334 ms inside which a plasma lights nothing new.
    /// The control: no `Run` falls inside it, so it is one program.
    /// Perturbation: swap `ge` and `gg` and the slice fails; a 300 ms hold
    /// after a jump fails the holds' check.
    #[test]
    fn the_readme_scene_is_one_helix_in_its_order() {
        let at = |step: Step| SESSION.iter().position(|s| *s == step).unwrap();
        let open = at(Run("hx README.md"));
        let quit = at(Line(":q"));
        let moves: Vec<Step> = SESSION[open..=quit + 1]
            .iter()
            .copied()
            .filter(|s| !matches!(s, Hold(_) | Scene(_)))
            .collect();
        assert_eq!(
            moves,
            [
                Run("hx README.md"),
                Keys("4j"),
                Keys("4k"),
                Line("/character"),
                Ctrl('d'),
                Ctrl('d'),
                Ctrl('d'),
                Keys("ge"),
                Keys("gg"),
                Line("/Nix"),
                Hover(NIX_AT),
                CmdClick(NIX_AT),
                Picture,
                Wheel {
                    turns: -1,
                    lines: 3
                },
                Keys("q"),
                Line(":q"),
                Prompt,
            ]
        );
        let search = at(Line("/character"));
        for (i, s) in SESSION[search..at(Line("/Nix"))].iter().enumerate() {
            if let Hold(ms) = s {
                assert!(
                    *ms > REIGNITE_MS,
                    "step {} {s:?} is inside REIGNITE",
                    search + i
                );
            }
        }
        assert!(
            SESSION[open + 1..quit].iter().all(|s| !matches!(s, Run(_))),
            "one helix"
        );
    }

    /// **The README the film opens holds the scene**: `character` is found
    /// and three half pages from it stay inside the file. The controls: the
    /// README without the word fails and says which word; a README it ends
    /// short of fails and says how far; a capital still counts, as helix's
    /// smart case matches it. Perturbation: move the word below the last 33
    /// lines and the second check fails.
    #[test]
    fn the_readme_holds_the_scene() {
        const README: &str = include_str!("../../../README.md");
        assert_eq!(readme_scene(README), Ok(()));
        let lost = README.replace("character", "x").replace("Character", "x");
        let err = readme_scene(&lost).unwrap_err();
        assert!(err.contains("no longer holds `character`"), "{err}");
        assert!(err.contains("/character"), "{err}");
        let short = "a character\nb\n";
        let err = readme_scene(short).unwrap_err();
        assert!(err.contains("past its end"), "{err}");
        let long = format!("A CHARACTER\n{}", "b\n".repeat(HALF_PAGE * 3));
        assert_eq!(readme_scene(&long), Ok(()));
    }

    /// **Ctrl-d is what the core encodes for the key, not a byte typed**:
    /// `0x04` with no keyboard protocol, as ECMA-48's EOT sits under Ctrl-D
    /// in the VT100's keyboard, and kitty's `CSI 100 ; 5 u` once a program
    /// has pushed its disambiguate flag (`[[source: kitty-keyboard]]`), as
    /// helix does: there a byte typed would be a different report. Escape
    /// goes the same way. The control: Escape's own two forms differ too,
    /// and a step that presses nothing has no event. Perturbation: spell
    /// the key's codepoint as `0x04`, what `\u{4}` as text would do, and the
    /// flagged form fails.
    #[test]
    fn ctrl_d_is_encoded_by_the_core() {
        use baud_domain::{BytePos, ReadLen, WinSize};
        let ws = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let pressed = |first: &[u8], step: Step| {
            let mut owned = baud_core::Owned::new(&ws);
            let mut t = baud_core::Term::new(owned.storage(), &ws);
            t.write_at(BytePos::ZERO, first);
            t.feed(BytePos::ZERO, ReadLen::from_index(first.len()));
            t.clear_reply();
            assert!(t.key(key_event(step).expect("a key")), "encoded");
            t.reply().to_vec()
        };
        let flags = b"\x1b[>1u";
        assert_eq!(pressed(b"", Ctrl('d')), b"\x04".to_vec());
        assert_eq!(pressed(flags, Ctrl('d')), b"\x1b[100;5u".to_vec());
        assert_eq!(pressed(b"", Escape), b"\x1b".to_vec());
        assert_eq!(pressed(flags, Escape), b"\x1b[27u".to_vec());
        assert_eq!(pressed(b"", Move(Arrow::Down)), b"\x1b[B".to_vec());
        assert_eq!(pressed(b"", Move(Arrow::Up)), b"\x1b[A".to_vec());
        assert_eq!(key_event(Hold(1)), None);
        assert_eq!(key_event(Keys("d")), None);
    }

    /// **The Nix link is followed by a Command-click where the pointer
    /// rests, and the picture of the page waited for**: the hover and the
    /// click on [`NIX_AT`], the click a step of its own that presses no key
    /// here -- the player holds Command over the click -- logged as
    /// `command-click R,C` and planned as a hover's settle. The control: a
    /// hover presses nothing either. Perturbation: give `CmdClick` no line
    /// of its own and the event's check fails.
    #[test]
    fn the_nix_link_is_command_clicked_where_the_pointer_rests() {
        let at = SESSION.iter().position(|s| *s == CmdClick(NIX_AT)).unwrap();
        assert_eq!(
            SESSION[at - 2..at + 2],
            [Hover(NIX_AT), Hold(400), CmdClick(NIX_AT), Picture]
        );
        let c = CmdClick(Cell { row: 3, col: 7 });
        assert_eq!(event(c, 0), "command-click 3,7");
        assert!(describe(c, 0).contains("Command-click row 3, column 7"));
        assert_eq!(key_event(c), None);
        assert_eq!(
            key_event(Hover(Cell { row: 3, col: 7 })),
            None,
            "the control"
        );
        let d = duration(&[c]).as_secs_f64();
        assert!((d - WHEEL_SETTLE_MS as f64 / 1000.0).abs() < 1e-9, "{d}");
    }

    /// **The plan's duration is its holds, its keys and its wheel**: 1.5 s
    /// of holds, `ab` with its Return at 40 ms a key (0.12 s), `q` (0.04 s),
    /// `:q` and Return (0.12 s), and two wheels of 4 turns, each 30 ms
    /// then 4 x 50 ms (0.46 s): 2.24 s. The whole tour is about 40 s
    /// before its waits -- each program's start, the page Chawan loads.
    /// Perturbation: count a
    /// wheel's turns signed and the second wheel takes nothing off, which
    /// the total catches.
    /// **A hover and a drag take their time and say what they did**, and a
    /// cell lands in its place in a window of 80x24 cells of 10x20 points
    /// below the title strip and inside the padding. Perturbation: leave a
    /// drag's pauses out of `duration`; read `in_window`'s x from the row.
    #[test]
    fn a_hover_and_a_drag_are_planned() {
        let hover = Hover(Cell { row: 3, col: 7 });
        let drag = Drag {
            from: Cell { row: 1, col: 2 },
            to: Cell { row: 1, col: 4 },
        };
        let d = duration(&[hover, drag]).as_secs_f64();
        assert!((d - 0.89).abs() < 1e-9, "{d}");
        assert_eq!(event(hover, 0), "hover 3,7");
        assert_eq!(event(drag, 0), "drag 1,2 to 1,4");
        assert!(describe(hover, 0).contains("row 3, column 7"));
        assert!(describe(drag, 0).contains("column 4"));
        assert_eq!(key_event(hover), None);
        let size = (2.0 * PAD_PT + 800.0, TITLE_PT + 2.0 * PAD_PT + 480.0);
        let origin = (10.0, 20.0);
        let at = |c: Cell, w| in_window(c, w, origin, size);
        assert_eq!(at(Cell { row: 0, col: 0 }, MIDDLE), (23.0, 66.0));
        assert_eq!(at(Cell { row: 2, col: 79 }, (1.0, 1.0)), (818.0, 116.0));
        let (press, release) = drag_ends();
        assert!(press.0 < 0.5 && release.0 > 0.5 && press.0 > 0.0 && release.0 < 1.0);
    }

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
            (37.0..43.0).contains(&whole),
            "the tour is about 40 s before its waits: {whole}"
        );
        let esc = duration(&[Escape]).as_secs_f64();
        assert!((esc - 0.04).abs() < 1e-9, "Escape is one key: {esc}");
        let ctrl = duration(&[Ctrl('d')]).as_secs_f64();
        assert!((ctrl - 0.04).abs() < 1e-9, "Ctrl-d is one key: {ctrl}");
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
        assert_eq!(event(Ctrl('d'), 0), "key Ctrl-d");
        assert_eq!(event(Move(Arrow::Down), 0), "key Down");
        assert_eq!(event(Step::Copy("x"), 0), "key Command-C");
        assert_eq!(event(Peek(1500), 0), "peek 1500 ms");
        assert_eq!(describe(Ctrl('d'), 0), "press Ctrl-d");
        assert_eq!(lines[..2], ["scene 6", "prompt 1"]);
        assert_eq!(lines.last().map(String::as_str), Some("hold 2000 ms"));
    }

    /// **`hello.txt` is seven lines, baud defined in the six languages
    /// and the emoji**: each language's line is its word, a colon and
    /// baud's own definition -- the Hebrew word pointed as the Academy
    /// points it, a dagesh and a holam, so the font check meets them -- in
    /// the order Hebrew, Arabic, Chinese, Japanese, Korean, Hindi; the
    /// emoji line's pager and stopwatch are emoji, each with U+FE0F, which
    /// Noto Sans Symbols 2 would otherwise draw as text. Perturbation: put
    /// the quoted Chinese definition back and its line's check fails.
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
        assert!(lines[0].ends_with("מספר הפעמים בשנייה שהאות יכול להשתנות."));
        assert!(lines[1].ends_with("عدد مرات تغير الإشارة في الثانية."));
        assert_eq!(lines[2], "波特：符号速率的单位，即信号每秒变化的次数。");
        assert_eq!(
            lines[3],
            "ボー：変調速度の単位。1秒間に信号が何回変わるかを表す。"
        );
        assert_eq!(
            lines[4],
            "보: 심벌 속도의 단위로, 신호가 1초에 몇 번 바뀌는지를 나타낸다."
        );
        assert!(lines[5].ends_with("सिग्नल एक सेकंड में कितनी बार बदलता है।"));
        assert!(HELLO.contains("\u{1F4DF}\u{FE0F} "), "the pager as emoji");
        assert!(HELLO.contains("\u{23F1}\u{FE0F}"), "the stopwatch as emoji");
    }

    /// **Each line of `hello.txt` is one row of the demo's 80 columns**:
    /// written into baud's core at 80x24, each line, the emoji's too, ends
    /// on the row it began on, so no definition wraps. The control: the
    /// Japanese line, 27 wide characters and a digit, 55 columns, with
    /// fifteen more wide ones takes a second row.
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
        assert_eq!(
            rows(&format!("{japanese}{}", "語".repeat(15))),
            2,
            "the control"
        );
    }
}
