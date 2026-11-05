//! **Shell integration through the app's own spawn, with real shells.**
//! Each test starts zsh, bash or fish as a login shell through
//! [`Pty::fork_given`], the spawn a window makes, in a scratch `HOME` whose
//! startup files are the test's, with `[shell] integration` on or off;
//! types into it as a keyboard would; feeds what it writes into a
//! terminal, answering its queries; and reads the screen, the prompts and
//! the words drawn (`baud_render::Reorder`, as `baud-render`'s word-wrap
//! tests draw them). A shell not on `PATH` -- CI's runners carry bash and
//! seldom zsh or fish -- is skipped with the reason printed, and so is a
//! bash baud does not mark (macOS's own).
//!
//! **Waits are bounded and never sleep**: each one polls the master until
//! what it waits for has been written, and fails after [`WAIT`] saying what
//! the shell wrote.

use super::{Got, Outbox, Parent, Pty, make_nonblocking, read_master, text_area, write_some};
use crate::shell::Integration;
use baud_core::{Owned, Term};
use baud_domain::{BytePos, ExitCode, ExitStatus, PackedCell, ReadLen, Row, Rows, WinSize};
use baud_font::Px;
use baud_render::{Reorder, WordWrap};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The longest a wait runs: far past what a shell takes to start here.
const WAIT: Duration = Duration::from_secs(30);

/// Forty columns and twelve rows: the paragraph wraps three times.
const WS: WinSize = WinSize {
    ws_col: 40,
    ws_row: 12,
};

/// The paragraph each shell prints, from a variable its startup file
/// sets: no word longer than a line, long enough to wrap three times at
/// forty columns.
const PARAGRAPH: &str = "Every finished paragraph is drawn at its words, so a word \
                         that does not fit at the end of a line moves whole to the next";

/// What a shell's line editor writes each time it starts reading a line:
/// bracketed paste on, as zsh, bash 5.1 and later and fish all turn it.
const READS: &[u8] = b"\x1b[?2004h";

/// The prompts the stock startup files set, blanks trimmed: zsh's `%# `,
/// bash's `$ `, fish's `> `.
const PROMPTS: [&str; 3] = ["%", "$", ">"];

/// How many times the shell has written something ([`Session::written`]).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Times(usize);

impl Times {
    const NONE: Times = Times(0);
    const ONCE: Times = Times(1);

    fn next(self) -> Times {
        Times(self.0 + 1)
    }
}

/// Whether the shell is still on the other end of the pty.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stream {
    Open,
    Closed,
}

/// The shells, by the name `$SHELL` gives each.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sh {
    Zsh,
    Bash,
    Fish,
}

impl Sh {
    fn name(self) -> &'static str {
        match self {
            Sh::Zsh => "zsh",
            Sh::Bash => "bash",
            Sh::Fish => "fish",
        }
    }

    /// The shell on `PATH`, or why the test is skipped.
    fn find(self) -> Result<PathBuf, String> {
        let path =
            super::resolve(self.name()).ok_or_else(|| format!("no {} on PATH", self.name()))?;
        let path = PathBuf::from(path);
        if self == Sh::Bash
            && cfg!(target_os = "macos")
            && std::fs::canonicalize(&path).is_ok_and(|p| p == Path::new("/bin/bash"))
        {
            return Err("the only bash on PATH is macOS's own, which baud does not mark".into());
        }
        Ok(path)
    }
}

/// **A scratch directory of a test's own**, with a `home` and a `tmp`:
/// emptied first, and removed when it goes out of scope, a failing test's
/// included.
struct Scratch(PathBuf);

impl std::ops::Deref for Scratch {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The scratch directory `tag` names, for this test process: one left
/// by a killed process of the same pid is removed first.
fn scratch(tag: &str) -> Scratch {
    let dir = std::env::temp_dir().join(format!("baud-shell-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).expect("a scratch home");
    std::fs::create_dir_all(dir.join("tmp")).expect("a scratch tmp");
    Scratch(dir)
}

/// `text` written to `rel` under `dir`, its directories made.
fn put(dir: &Path, rel: &str, text: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().expect("a parent")).expect("a directory");
    std::fs::write(p, text).expect("a file");
}

/// **The startup files of a stock shell**, as a user who has set a
/// prompt and a variable has them: no marks of their own. Each also
/// appends its name to `$HOME/ran`, so a test can read which ran.
fn stock(sh: Sh, home: &Path) {
    match sh {
        Sh::Zsh => {
            for f in [".zshenv", ".zprofile", ".zlogin"] {
                put(home, f, &format!("print -r -- {f} >> $HOME/ran\n"));
            }
            put(
                home,
                ".zshrc",
                &format!(
                    "print -r -- .zshrc >> $HOME/ran\nPS1='%# '\nFROM_RC=yes\nP='{PARAGRAPH}'\n"
                ),
            );
        }
        Sh::Bash => {
            put(
                home,
                ".bash_profile",
                "echo .bash_profile >> \"$HOME/ran\"\n. \"$HOME/.bashrc\"\n",
            );
            put(
                home,
                ".bashrc",
                &format!("echo .bashrc >> \"$HOME/ran\"\nPS1='$ '\nFROM_RC=yes\nP='{PARAGRAPH}'\n"),
            );
        }
        Sh::Fish => {
            put(
                home,
                ".config/fish/config.fish",
                &format!(
                    "echo config.fish >> $HOME/ran\nset -g fish_greeting\n\
                     function fish_prompt; echo -n '> '; end\nset -g FROM_RC yes\nset -g P '{PARAGRAPH}'\n"
                ),
            );
        }
    }
}

/// The parent's environment a test's baud has: the scratch `HOME` and
/// `TMPDIR` (where the scripts are written), this `PATH`, a UTF-8 locale,
/// `SHELL`, and `more`.
fn parent(dir: &Path, shell: &Path, more: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    let os = |s: &str| OsString::from(s);
    let mut env = vec![
        (os("HOME"), dir.join("home").into_os_string()),
        (os("TMPDIR"), dir.join("tmp").into_os_string()),
        (os("PATH"), std::env::var_os("PATH").unwrap_or_default()),
        (os("LANG"), os("en_US.UTF-8")),
        (os("SHELL"), shell.as_os_str().to_owned()),
        (os("USER"), os("baud")),
        (os("LOGNAME"), os("baud")),
    ];
    env.extend(more.iter().map(|(k, v)| (os(k), os(v))));
    env
}

/// **A shell on a pty, its output fed into a terminal.**
struct Session {
    master: crate::sys::Fd,
    term: Term<'static>,
    outbox: Outbox,
    /// Every byte the shell wrote, in order: what a failure shows.
    seen: Vec<u8>,
    stream: Stream,
}

impl Session {
    /// `$SHELL` in `env` started as a window starts it, under
    /// `integration`, at [`WS`].
    fn start(env: Vec<(OsString, OsString)>, integration: Integration) -> Session {
        Session::start_at(&WS, env, integration)
    }

    /// [`Session::start`] at `ws`.
    fn start_at(ws: &WinSize, env: Vec<(OsString, OsString)>, integration: Integration) -> Session {
        let (_, master) = Pty::fork_given(
            text_area(ws, Px::of(8), Px::of(16)),
            None,
            env,
            || None,
            Parent::Other,
            None,
            &integration.into(),
        )
        .expect("forkpty");
        make_nonblocking(master);
        let owned: &'static mut Owned = Box::leak(Box::new(Owned::new(ws)));
        let term = Term::new(owned.storage(), ws);
        Session {
            master,
            term,
            outbox: Outbox::default(),
            seen: Vec::new(),
            stream: Stream::Open,
        }
    }

    /// Everything the shell has written by now, fed; the terminal's
    /// answers sent.
    fn pump(&mut self) {
        let mut buf = [0u8; 4096];
        loop {
            match read_master(self.master, &mut buf) {
                Got::Bytes(b) => {
                    let n = b.len();
                    self.seen.extend_from_slice(b);
                    self.term.write_at(BytePos::ZERO, &buf[..n]);
                    self.term.feed(BytePos::ZERO, ReadLen::from_index(n));
                    self.outbox.push(self.term.reply());
                    self.term.clear_reply();
                }
                Got::Nothing => break,
                Got::Closed => {
                    self.stream = Stream::Closed;
                    break;
                }
            }
        }
        let _ = write_some(self.master, &mut self.outbox);
    }

    /// Pumps until `done` holds, or fails after [`WAIT`] naming `what`.
    fn until(&mut self, what: &str, mut done: impl FnMut(&Session) -> bool) {
        let deadline = Instant::now() + WAIT;
        loop {
            self.pump();
            if done(self) {
                return;
            }
            assert!(
                self.stream == Stream::Open,
                "the shell ended before {what}: {}",
                self.tail()
            );
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "never {what}: {}", self.tail());
            let mut p = crate::sys::PollFd {
                fd: self.master,
                events: crate::sys::POLLIN,
                revents: 0,
            };
            let ms = core::ffi::c_int::try_from(left.as_millis()).unwrap_or(core::ffi::c_int::MAX);
            // SAFETY: one initialised record and its count.
            unsafe { crate::sys::poll(&mut p, 1, ms) };
        }
    }

    /// How many times the shell has written `needle`.
    fn written(&self, needle: &[u8]) -> Times {
        Times(
            self.seen
                .windows(needle.len())
                .filter(|w| *w == needle)
                .count(),
        )
    }

    /// Waits for the line editor to be reading its `n`th line, its prompt
    /// drawn: bracketed paste turned on `n` times, and the cursor just
    /// past the prompt on a row that holds nothing else. fish 4 turns it
    /// on before it asks the terminal what it needs and draws the prompt.
    fn reading(&mut self, n: Times) {
        self.until(&format!("line {} is read", n.0), |s| {
            let (y, x) = s.term.cursor();
            let row = s.screen().get(y.index()).cloned().unwrap_or_default();
            s.written(READS) >= n && PROMPTS.contains(&row.as_str()) && x.index() == row.len() + 1
        });
    }

    /// `line` typed, and Return.
    fn line(&mut self, line: &str) {
        self.outbox.push(line.as_bytes());
        self.outbox.push(b"\r");
        let _ = write_some(self.master, &mut self.outbox);
    }

    /// `exit` typed, and the end of the stream waited for: the shell has
    /// written what it writes as it exits (bash its history) and gone.
    fn finish(mut self) {
        self.line("exit");
        self.until("the shell exits", |s| s.stream == Stream::Closed);
    }

    /// `line` typed at the prompt, and the next prompt waited for.
    fn run(&mut self, line: &str) {
        let n = self.written(READS);
        self.line(line);
        self.reading(n.next());
    }

    /// The end of what the shell wrote, for a failure's message.
    fn tail(&self) -> String {
        let from = self.seen.len().saturating_sub(1500);
        format!("{:?}", String::from_utf8_lossy(&self.seen[from..]))
    }

    /// Every row of the live screen as stored, blanks trimmed.
    fn screen(&self) -> Vec<String> {
        let vp = self.term.view(Rows::ZERO);
        (0..vp.rows().index())
            .map(|y| text(vp.row(Row::from_index(y))))
            .collect()
    }

    /// Every row as the word wrap draws the window, or `None` where it
    /// draws the rows as stored.
    fn drawn(&self) -> Option<Vec<String>> {
        let mut o = Reorder::new();
        o.set_word_wrap(WordWrap::On);
        o.lay_out(&self.term, Rows::ZERO);
        o.orders().display()?;
        let vp = o.view(&self.term, Rows::ZERO);
        Some(
            (0..vp.rows().index())
                .map(|y| {
                    let r = Row::from_index(y);
                    let mut out = vec![PackedCell::EMPTY; vp.cols().index()];
                    o.orders().permute(r, vp.row(r), &mut out);
                    text(&out)
                })
                .collect(),
        )
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: the descriptor forkpty returned, owned by this session
        // alone; closing it hangs the shell up.
        drop(unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(self.master) });
    }
}

/// A row's text, blanks trimmed.
fn text(cells: &[PackedCell]) -> String {
    cells
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .and_then(|p| char::from_u32(p.value()))
                .unwrap_or(' ')
        })
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// Whether `lines`, read as words, are the paragraph's, in order and each
/// whole: a word split across two lines reads as two that are not its.
fn words_whole(lines: &[String]) -> bool {
    lines
        .join(" ")
        .split_whitespace()
        .eq(PARAGRAPH.split_whitespace())
}

/// The rows from the one the paragraph starts on to the prompt after it.
fn paragraph(lines: &[String]) -> Vec<String> {
    let Some(first) = lines.iter().position(|l| l.starts_with("Every finished")) else {
        return Vec::new();
    };
    lines[first..]
        .iter()
        .take_while(|l| !l.is_empty() && !matches!(l.chars().next(), Some('%' | '$' | '>')))
        .cloned()
        .collect()
}

/// The command each shell prints the paragraph with, and a screenful of
/// lines before it.
fn print_paragraph(sh: Sh) -> &'static str {
    match sh {
        Sh::Zsh => "print -r -- $P",
        Sh::Bash => "printf '%s\\n' \"$P\"",
        Sh::Fish => "echo $P",
    }
}

fn lines_first(sh: Sh) -> &'static str {
    match sh {
        Sh::Zsh => "print -l {1..30}",
        Sh::Bash => "for i in {1..30}; do echo $i; done",
        Sh::Fish => "for i in (seq 30); echo $i; end",
    }
}

/// `sh` started stock in a scratch home under `integration`, at its first
/// prompt; `None`, said, when the shell is not here.
fn stock_session(sh: Sh, tag: &str, integration: Integration) -> Option<(Session, Scratch)> {
    let shell = match sh.find() {
        Ok(p) => p,
        Err(why) => {
            eprintln!("skipped: {why}");
            return None;
        }
    };
    let dir = scratch(tag);
    stock(sh, &dir.join("home"));
    let mut s = Session::start(parent(&dir, &shell, &[]), integration);
    s.reading(Times::ONCE);
    Some((s, dir))
}

/// **A stock shell's paragraph is drawn at its words once its prompt is
/// back**, on the live screen, with integration on: the shell's startup
/// files send no marks, baud's do. The terminal's own cut splits a word
/// (the control); the window as the word wrap draws it breaks every line
/// at a space, every word whole and in order. With integration off the
/// same shell's paragraph stays as the terminal cut it while it is on the
/// live screen, which is what every shell got before baud marked them.
fn a_stock_paragraph_is_drawn_at_its_words(sh: Sh) {
    for integration in [Integration::On, Integration::Off] {
        let tag = format!("words-{}-{integration:?}", sh.name());
        let Some((mut s, _dir)) = stock_session(sh, &tag, integration) else {
            return;
        };
        s.run(lines_first(sh));
        s.run(print_paragraph(sh));
        let stored = paragraph(&s.screen());
        assert!(
            !stored.is_empty() && !words_whole(&stored),
            "the control: the terminal's cut splits a word: {:?}",
            s.screen()
        );
        if sh == Sh::Fish && fish_marks_itself(&s.seen) {
            // fish 4 marks its own prompts, and baud adds none to them.
            assert!(
                s.written(b";aid=") == Times::NONE,
                "no mark of baud's beside fish's own: {}",
                s.tail()
            );
        }
        match integration {
            Integration::On => {
                let drawn = s.drawn().unwrap_or_else(|| {
                    panic!(
                        "{}: the paragraph is re-broken: zone {:?} row {:?} screen {:?}",
                        sh.name(),
                        s.term.primary_zone(),
                        s.term.prompt_row(),
                        s.screen()
                    )
                });
                let lines = paragraph(&drawn);
                assert!(words_whole(&lines), "every word whole, in order: {lines:?}");
            }
            // fish 4 marks its own prompts, baud's integration or none.
            Integration::Off if sh == Sh::Fish && fish_marks_itself(&s.seen) => {
                let lines = s.drawn().map_or(stored.clone(), |d| paragraph(&d));
                assert!(words_whole(&lines), "fish's own marks: {lines:?}");
            }
            Integration::Off => {
                let lines = s.drawn().map_or(stored.clone(), |d| paragraph(&d));
                assert_eq!(
                    lines, stored,
                    "drawn as the terminal cut it with integration off"
                );
            }
        }
        s.finish();
    }
}

#[test]
fn a_stock_zsh_paragraph_is_drawn_at_its_words() {
    a_stock_paragraph_is_drawn_at_its_words(Sh::Zsh);
}

#[test]
fn a_stock_bash_paragraph_is_drawn_at_its_words() {
    a_stock_paragraph_is_drawn_at_its_words(Sh::Bash);
}

#[test]
fn a_stock_fish_paragraph_is_drawn_at_its_words() {
    a_stock_paragraph_is_drawn_at_its_words(Sh::Fish);
}

/// **Each command's exit status reaches the prompt it ran from**: `false`,
/// `sh -c 'exit 3'` and `true`, read back from the terminal's prompt
/// log as the exit-status marks read it. An empty Enter after them is a
/// prompt no command ran from, read as cancelled, as the text has a `D`
/// with no `C` read.
fn exit_codes_reach_the_marks(sh: Sh) {
    let Some((mut s, _dir)) = stock_session(sh, &format!("status-{}", sh.name()), Integration::On)
    else {
        return;
    };
    for line in ["false", "sh -c 'exit 3'", "true", ""] {
        s.run(line);
    }
    let statuses: Vec<ExitStatus> = s.term.prompts().map(|p| p.status).collect();
    let want = [
        ExitStatus::Exited(ExitCode::of(1)),
        ExitStatus::Exited(ExitCode::of(3)),
        ExitStatus::Exited(ExitCode::of(0)),
    ];
    assert!(
        statuses.windows(3).any(|w| w == want),
        "{}: the statuses in order: {statuses:?}: {}",
        sh.name(),
        s.tail()
    );
    // fish 4's own marks read an empty Enter as the same prompt drawn
    // again; baud's send the `D` the text asks for.
    if !(sh == Sh::Fish && fish_marks_itself(&s.seen)) {
        let last_two = &statuses[statuses.len().saturating_sub(2)..];
        assert_eq!(
            last_two,
            [ExitStatus::Cancelled, ExitStatus::Unreported],
            "{}: an empty Enter, and the prompt waiting: {statuses:?}",
            sh.name()
        );
    }
    s.finish();
}

#[test]
fn zsh_exit_codes_reach_the_marks() {
    exit_codes_reach_the_marks(Sh::Zsh);
}

#[test]
fn bash_exit_codes_reach_the_marks() {
    exit_codes_reach_the_marks(Sh::Bash);
}

#[test]
fn fish_exit_codes_reach_the_marks() {
    exit_codes_reach_the_marks(Sh::Fish);
}

/// **With integration off, nothing is injected**: no mark at all comes
/// from a stock shell, and its environment names none of baud's
/// directories.
fn nothing_is_injected_with_integration_off(sh: Sh) {
    let Some((mut s, dir)) = stock_session(sh, &format!("off-{}", sh.name()), Integration::Off)
    else {
        return;
    };
    s.run("env");
    let seen = String::from_utf8_lossy(&s.seen).into_owned();
    if sh == Sh::Fish && fish_marks_itself(&s.seen) {
        // fish 4 marks its own prompts; baud's carry an `aid`, fish's none.
        assert!(!seen.contains("133;A;aid="), "no mark of baud's: {seen:?}");
    } else {
        assert!(!seen.contains("\x1b]133;"), "no mark: {seen:?}");
    }
    let tmp = dir.join("tmp");
    for borrowed in ["\nZDOTDIR=", "\nENV=", "BAUD_"] {
        assert!(!seen.contains(borrowed), "no {borrowed:?} in: {seen:?}");
    }
    assert!(
        std::fs::read_dir(&tmp).expect("tmp").next().is_none(),
        "no script was written"
    );
    s.finish();
}

/// Whether a fish marks its own prompts, as fish 4 does: an `A` with
/// kitty's `click_events`.
fn fish_marks_itself(seen: &[u8]) -> bool {
    let own = b"\x1b]133;A;click_events=";
    seen.windows(own.len()).any(|w| w == own)
}

#[test]
fn zsh_gets_nothing_with_integration_off() {
    nothing_is_injected_with_integration_off(Sh::Zsh);
}

#[test]
fn bash_gets_nothing_with_integration_off() {
    nothing_is_injected_with_integration_off(Sh::Bash);
}

#[test]
fn fish_gets_nothing_with_integration_off() {
    nothing_is_injected_with_integration_off(Sh::Fish);
}

/// What a shell says of itself, written to `$HOME/state` by one command:
/// the startup files that ran, in order, a variable the user's file set,
/// its environment sorted, and its options.
///
/// **bash's state holds what the shell itself writes to stderr -- its
/// builtins, and a DEBUG trap's first firing before the line's first
/// command, which is why that command is a builtin -- and nothing a
/// program it starts writes there**: under job control, the process bash
/// forks to run a program now and then reports, before the program runs,
/// that it could not put itself in a process group of its own -- `child
/// setpgid (N to N): Operation not permitted` -- in either arm, and in a
/// bash no baud started (measured on macOS through `script(1)` under
/// load: 32 of 9000 forks with job control on, none of 9000 with it off).
/// With every stderr in the state, that line failed 5 of 420 runs: 3 as
/// a line one arm's state had and the other's did not, 2 as the first
/// line of the startup files' list; the programs' stderr now goes to the
/// terminal.
fn state_command(sh: Sh) -> &'static str {
    match sh {
        Sh::Zsh => {
            "{ cat ~/ran; print -r -- rc=$FROM_RC zdotdir=${ZDOTDIR-unset} \
             histfile=${HISTFILE-unset}; env | LC_ALL=C sort; setopt; } > ~/state"
        }
        Sh::Bash => {
            "{ { while IFS= read -r l; do printf '%s\\n' \"$l\"; done < ~/ran; \
             echo rc=$FROM_RC; } 2>&1; env | LC_ALL=C sort; { shopt -p; set +o; \
             declare -p HISTFILE; history 2; trap -p; } 2>&1; } > ~/state"
        }
        Sh::Fish => {
            "begin; cat ~/ran; echo rc=$FROM_RC; env | LC_ALL=C sort; status is-login; \
             and echo login; end > ~/state"
        }
    }
}

/// The state a shell started under `integration` in `dir` writes, its
/// `ran` log cleared first; `None` when the shell is not here.
fn state_of(sh: Sh, dir: &Path, more: &[(&str, &str)], integration: Integration) -> Option<String> {
    let shell = sh.find().ok()?;
    let home = dir.join("home");
    let _ = std::fs::remove_file(home.join("ran"));
    let _ = std::fs::remove_file(home.join("state"));
    if sh == Sh::Bash {
        // The history each arm loads, as the last arm left it rewritten.
        put(&home, ".bash_history", "echo from-history\n");
    }
    let mut s = Session::start(parent(dir, &shell, more), integration);
    s.reading(Times::ONCE);
    s.run(state_command(sh));
    let state = std::fs::read_to_string(home.join("state")).expect("the state");
    s.finish();
    Some(state)
}

/// **The user's own startup files run as they do without baud, and the
/// shell is left as it would be**: with the same scratch home, the shell
/// started with integration off and on writes the same list of startup
/// files run, in the same order, the variable one of them set, the same
/// environment and the same options -- bash's `shopt` and `set -o`, zsh's
/// `setopt` -- and bash the same `HISTFILE`, history and traps. `more` is added
/// to the parent's environment of both, and `rc` to the end of the
/// user's own `.zshrc`, `.bashrc` or `config.fish`.
fn startup_runs_as_without_baud(
    sh: Sh,
    tag: &str,
    more: &[(&str, &str)],
    want_ran: &[&str],
    rc: &str,
) {
    if let Err(why) = sh.find() {
        eprintln!("skipped: {why}");
        return;
    }
    let dir = scratch(tag);
    stock(sh, &dir.join("home"));
    let main = match sh {
        Sh::Zsh => ".zshrc",
        Sh::Bash => ".bashrc",
        Sh::Fish => ".config/fish/config.fish",
    };
    let home = dir.join("home");
    let text = std::fs::read_to_string(home.join(main)).expect("the startup file");
    put(&home, main, &format!("{text}{rc}"));
    for (k, v) in more {
        if *k == "ZDOTDIR" {
            // The user's startup files where their ZDOTDIR names.
            let zdot = Path::new(v);
            stock(sh, zdot);
            for f in [".zshenv", ".zprofile", ".zshrc", ".zlogin"] {
                let _ = std::fs::remove_file(dir.join("home").join(f));
            }
            let _ = zdot;
        }
    }
    let off = state_of(sh, &dir, more, Integration::Off).expect("the shell");
    let on = state_of(sh, &dir, more, Integration::On).expect("the shell");
    let ran: Vec<&str> = off.lines().take(want_ran.len()).collect();
    assert_eq!(ran, want_ran, "the control: the startup files ran: {off}");
    assert!(
        off.contains("rc=yes"),
        "the control: the user's file set its variable"
    );
    if sh == Sh::Bash {
        assert!(
            off.contains("echo from-history") && off.contains(".bash_history"),
            "the control: bash read ~/.bash_history: {off}"
        );
    }
    if on != off {
        let diff: Vec<String> = on
            .lines()
            .filter(|l| !off.lines().any(|o| o == *l))
            .map(|l| format!("+ {l}"))
            .chain(
                off.lines()
                    .filter(|l| !on.lines().any(|o| o == *l))
                    .map(|l| format!("- {l}")),
            )
            .collect();
        panic!(
            "{}: the shell differs with integration on: {diff:#?}",
            sh.name()
        );
    }
}

#[test]
fn zsh_startup_runs_as_without_baud() {
    startup_runs_as_without_baud(
        Sh::Zsh,
        "start-zsh",
        &[],
        &[".zshenv", ".zprofile", ".zshrc", ".zlogin"],
        "",
    );
}

/// The same, with the user's own `ZDOTDIR` naming where their files are.
#[test]
fn zsh_startup_runs_as_without_baud_from_the_users_zdotdir() {
    let zdot = scratch("zdot");
    let z = zdot.to_str().expect("a UTF-8 path").to_string();
    startup_runs_as_without_baud(
        Sh::Zsh,
        "start-zsh-zdotdir",
        &[("ZDOTDIR", &z)],
        &[".zshenv", ".zprofile", ".zshrc", ".zlogin"],
        "",
    );
}

/// bash's login chain, and an `ENV` the user had, which bash outside
/// POSIX mode never reads, kept and still unread.
#[test]
fn bash_startup_runs_as_without_baud() {
    let shrc = scratch("env");
    let env = shrc.join("shrc");
    std::fs::write(&env, "echo ENV >> \"$HOME/ran\"\n").expect("an ENV file");
    let e = env.to_str().expect("a UTF-8 path").to_string();
    startup_runs_as_without_baud(
        Sh::Bash,
        "start-bash",
        &[("ENV", &e)],
        &[".bash_profile", ".bashrc", "rc=yes"],
        "",
    );
}

/// The same with the prompts exported, as a Python virtual environment's
/// `activate` exports `PS1`: what a command is given holds the user's
/// prompts, and no mark of baud's -- `PS0`, which bash leaves unset, not
/// in it at all -- and no trap is left set.
#[test]
fn bash_startup_runs_as_without_baud_with_its_prompts_exported() {
    startup_runs_as_without_baud(
        Sh::Bash,
        "start-bash-export",
        &[],
        &[".bash_profile", ".bashrc", "rc=yes"],
        "export PS1 PS2 PS0\n",
    );
}

/// The same for zsh.
#[test]
fn zsh_startup_runs_as_without_baud_with_its_prompts_exported() {
    startup_runs_as_without_baud(
        Sh::Zsh,
        "start-zsh-export",
        &[],
        &[".zshenv", ".zprofile", ".zshrc", ".zlogin"],
        "export PS1 PS2\n",
    );
}

#[test]
fn fish_startup_runs_as_without_baud() {
    startup_runs_as_without_baud(Sh::Fish, "start-fish", &[], &["config.fish", "rc=yes"], "");
}

/// **A completion list stays on the screen of a marked shell**, which
/// needs the input mark (`B`) after the prompt: bash, after two Tabs,
/// prints the list of a directory's files below the line and the prompt
/// again below the list, and zsh does when the list does not fit below
/// its prompt, after asking whether to show it when it is taller than the
/// window, which `y` answers. With the prompt's
/// mark sent again and no `C` or `D` between, a shell that marked no
/// input lost the list (`tests/completion/`). The list's names are still
/// on the screen above the prompt the shell drew again, the line as
/// typed on it.
fn a_completion_list_stays(sh: Sh, files: std::ops::RangeInclusive<usize>, tabs: &str, last: &str) {
    let Some((mut s, dir)) = stock_session(sh, &format!("list-{}", sh.name()), Integration::On)
    else {
        return;
    };
    let d = dir.join("d");
    for i in files.clone() {
        put(&d, &format!("file{i:02}"), "");
    }
    s.run(lines_first(sh));
    let typed = format!("ls {}/file", d.display());
    s.outbox.push(typed.as_bytes());
    s.outbox.push(tabs.as_bytes());
    let _ = write_some(s.master, &mut s.outbox);
    let asks = |s: &Session| String::from_utf8_lossy(&s.seen).contains("do you wish to see all");
    s.until("the list, or the question", |s| {
        asks(s) || s.screen().iter().any(|r| r.contains(last))
    });
    if asks(&s) {
        s.outbox.push(b"y");
        let _ = write_some(s.master, &mut s.outbox);
    }
    s.until("the list and the prompt again under it", |s| {
        let screen = s.screen();
        let (y, _) = s.term.cursor();
        let prompt_again = screen
            .get(y.index())
            .is_some_and(|r| r.ends_with(typed.rsplit('/').next().unwrap_or("")) && r.len() > 2);
        prompt_again && screen[..y.index()].iter().any(|r| r.contains(last))
    });
    let screen = s.screen();
    let (y, _) = s.term.cursor();
    let end = *files.end();
    let names = [end - 2, end - 1, end].map(|i| format!("file{i:02}"));
    for name in names {
        assert!(
            screen[..y.index()].iter().any(|r| r.contains(&name)),
            "{}: {name} is still on the screen above the prompt: {screen:?}",
            sh.name()
        );
    }
    s.outbox.push(b"\x15");
    s.finish();
}

#[test]
fn a_zsh_completion_list_stays() {
    a_completion_list_stays(Sh::Zsh, 1..=50, "\t", "file50");
}

#[test]
fn a_bash_completion_list_stays() {
    a_completion_list_stays(Sh::Bash, 1..=12, "\t\t", "file12");
}

/// **zsh's new-user setup runs as it does without baud**: zsh loads its
/// newuser module just before `$ZDOTDIR/.zshenv`, and with baud's
/// directory in `ZDOTDIR` the module would find baud's file there and
/// never offer `zsh-newuser-install` to a user with no startup files.
/// With a home holding none, in a window of 80 by 24 -- the setup runs
/// in no fewer than 72 by 15 -- the setup's first words appear with
/// integration off (the control) and on alike. A zsh that ships no
/// newuser script -- macOS's -- shows it in neither, and is skipped.
#[test]
fn zsh_offers_its_new_user_setup_as_without_baud() {
    let shell = match Sh::Zsh.find() {
        Ok(p) => p,
        Err(why) => return eprintln!("skipped: {why}"),
    };
    const OFFER: &str = "This is the Z Shell configuration function for new users";
    let mut offered = Vec::new();
    for integration in [Integration::Off, Integration::On] {
        let dir = scratch(&format!("newuser-{integration:?}"));
        let ws = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let mut s = Session::start_at(&ws, parent(&dir, &shell, &[]), integration);
        let shown = |s: &Session| String::from_utf8_lossy(&s.seen).contains(OFFER);
        s.until("the setup's offer, or the prompt", |s| {
            shown(s) || s.written(READS) >= Times::ONCE
        });
        offered.push(shown(&s));
        // `q`: "Quit and do nothing", then the shell's own prompt.
        if shown(&s) {
            s.outbox.push(b"q");
            let _ = write_some(s.master, &mut s.outbox);
        }
        s.until("the prompt", |s| s.written(READS) >= Times::ONCE);
        s.finish();
    }
    if offered == [false, false] {
        return eprintln!("skipped: this zsh ships no new-user script");
    }
    assert_eq!(
        offered,
        [true, true],
        "offered with integration off, then on"
    );
}

/// **A user's startup files that already mark the prompt keep their own
/// marks, and baud adds none**: marks in the prompts, as
/// `tests/prompt/bashrc` and WezTerm's zsh integration send them, and
/// marks sent only from a hook function of the user's own. Every mark the
/// shell writes is the user's -- none carries baud's `aid` -- the user's
/// `theirs` among them, and the marker is unset.
fn already_marked_keeps_its_own(sh: Sh, rc: &str, file: &str, theirs: &[u8]) {
    let shell = match sh.find() {
        Ok(p) => p,
        Err(why) => return eprintln!("skipped: {why}"),
    };
    let dir = scratch(&format!("marked-{}-{}", sh.name(), theirs.len()));
    put(&dir.join("home"), file, rc);
    let mut s = Session::start(parent(&dir, &shell, &[]), Integration::On);
    s.until("the user's mark", |s| s.written(theirs) >= Times::ONCE);
    s.until("the line is read", |s| s.written(READS) >= Times::ONCE);
    let n = s.written(READS);
    s.line(match sh {
        Sh::Fish => "set -q BAUD_SHELL_INTEGRATION; or echo marker=unset",
        _ => "echo marker=${BAUD_SHELL_INTEGRATION-unset}",
    });
    s.until("the next line is read", |s| s.written(READS) > n);
    // bash turns bracketed paste on before it draws the prompt.
    s.until("the control: the user's mark at the next prompt", |s| {
        s.written(theirs) >= Times(2)
    });
    let seen = String::from_utf8_lossy(&s.seen).into_owned();
    assert!(
        seen.contains("marker=unset"),
        "the marker is unset: {seen:?}"
    );
    assert!(!seen.contains(";aid="), "no mark of baud's: {seen:?}");
    s.finish();
}

#[test]
fn a_zsh_that_marks_keeps_its_own() {
    already_marked_keeps_its_own(
        Sh::Zsh,
        "PS1=$'%{\\e]133;P;k=i\\a%}%# %{\\e]133;B\\a%}'\n\
         precmd() { print -n '\\e]133;A\\a' }\n\
         preexec() { print -n '\\e]133;C\\a' }\n",
        ".zshrc",
        b"\x1b]133;B",
    );
}

/// The same, the marks sent from a hook of the user's alone.
#[test]
fn a_zsh_hook_that_marks_keeps_its_own() {
    already_marked_keeps_its_own(
        Sh::Zsh,
        "PS1='%# '\nmarks() { print -n '\\e]133;A\\a' }\nprecmd_functions+=(marks)\n",
        ".zshrc",
        b"\x1b]133;A\x07",
    );
}

#[test]
fn a_bash_that_marks_keeps_its_own() {
    already_marked_keeps_its_own(
        Sh::Bash,
        "PS1='\\[\\e]133;A\\a\\]$ \\[\\e]133;B\\a\\]'\nPS0='\\e]133;C\\a'\n\
         PROMPT_COMMAND='printf \"\\e]133;D;%s\\a\" \"$?\"'\n",
        ".bash_profile",
        b"\x1b]133;B",
    );
}

/// The same, the marks sent from a function `PROMPT_COMMAND` names.
#[test]
fn a_bash_hook_that_marks_keeps_its_own() {
    already_marked_keeps_its_own(
        Sh::Bash,
        "PS1='$ '\nmarks() { printf '\\e]133;A\\a'; }\nPROMPT_COMMAND=marks\n",
        ".bash_profile",
        b"\x1b]133;A\x07",
    );
}

/// fish 3, the marks sent from a handler of the prompt's event; fish 4
/// gets nothing added either way.
#[test]
fn a_fish_handler_that_marks_keeps_its_own() {
    already_marked_keeps_its_own(
        Sh::Fish,
        "set -g fish_greeting\nfunction fish_prompt; echo -n '> '; end\n\
         function marks --on-event fish_prompt; printf '\\e]133;A\\a'; end\n",
        ".config/fish/config.fish",
        b"\x1b]133;A\x07",
    );
}

/// **A startup file that unsets the marker keeps baud's marks out**: the
/// opt-out a user's zsh or bash startup file has, the marker being set
/// before they run.
fn unsetting_the_marker_keeps_marks_out(sh: Sh, rc: &str, file: &str) {
    let shell = match sh.find() {
        Ok(p) => p,
        Err(why) => return eprintln!("skipped: {why}"),
    };
    let dir = scratch(&format!("optout-{}", sh.name()));
    put(&dir.join("home"), file, rc);
    let mut s = Session::start(parent(&dir, &shell, &[]), Integration::On);
    s.reading(Times::ONCE);
    s.run("echo saw=$SAW");
    let seen = String::from_utf8_lossy(&s.seen).into_owned();
    assert!(
        seen.contains("saw=1"),
        "the control: the file saw the marker: {seen:?}"
    );
    assert!(!seen.contains("\x1b]133;"), "no mark: {seen:?}");
    s.finish();
}

#[test]
fn a_zshrc_that_unsets_the_marker_gets_no_marks() {
    unsetting_the_marker_keeps_marks_out(
        Sh::Zsh,
        "PS1='%# '\nSAW=$BAUD_SHELL_INTEGRATION\nunset BAUD_SHELL_INTEGRATION\n",
        ".zshrc",
    );
}

#[test]
fn a_bash_profile_that_unsets_the_marker_gets_no_marks() {
    unsetting_the_marker_keeps_marks_out(
        Sh::Bash,
        "PS1='$ '\nSAW=$BAUD_SHELL_INTEGRATION\nunset BAUD_SHELL_INTEGRATION\n",
        ".bash_profile",
    );
}

/// **What shell integration hands a startup to put back never passes on
/// from a parent**: a baud started from a shell that kept one of them, or
/// exported the marker, gives its own shell none of them.
#[test]
fn the_helpers_never_pass_from_a_parent() {
    let parent: Vec<(OsString, OsString)> = crate::shell::HELPERS
        .iter()
        .map(|k| (OsString::from(k), OsString::from("leaked")))
        .chain([(OsString::from("KEPT"), OsString::from("yes"))])
        .collect();
    let env: Vec<String> = super::child_env(parent, None)
        .iter()
        .map(|e| e.to_string_lossy().into_owned())
        .collect();
    assert!(
        env.contains(&"KEPT=yes".to_string()),
        "the control: {env:?}"
    );
    for k in crate::shell::HELPERS {
        assert!(
            !env.iter().any(|e| e.starts_with(&format!("{k}="))),
            "{k} passed on: {env:?}"
        );
    }
}

/// **A command is run as it is**: `BAUD_CMD`'s `/bin/sh -c`, with
/// integration on and `$SHELL` naming zsh, gets no argument and no
/// variable of shell integration's.
#[test]
fn a_command_gets_nothing() {
    let dir = scratch("command");
    let parent = parent(&dir, Path::new("/bin/zsh"), &[]);
    let exec =
        super::Exec::of(Some("true".into()), &parent, Integration::On, None).expect("an exec");
    assert!(exec.injection.is_none());
    let argv: Vec<String> = exec
        .argv
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(argv, ["sh", "-c", "true"]);
    // The control: the same parent's login shell is given its start.
    let shell = super::Exec::of(None, &parent, Integration::On, None).expect("an exec");
    assert!(shell.injection.is_some());
}

/// **A user's `set -u` meets nothing unset in baud's hooks**: bash with
/// it on in `.bash_profile` and `PS0` unset, as bash starts, runs a
/// command and draws its prompts with every mark and no error.
#[test]
fn bash_under_set_u_keeps_its_marks() {
    let shell = match Sh::Bash.find() {
        Ok(p) => p,
        Err(why) => return eprintln!("skipped: {why}"),
    };
    let dir = scratch("nounset");
    put(&dir.join("home"), ".bash_profile", "set -u\nPS1='$ '\n");
    let mut s = Session::start(parent(&dir, &shell, &[]), Integration::On);
    s.reading(Times::ONCE);
    s.run("false");
    let seen = String::from_utf8_lossy(&s.seen).into_owned();
    assert!(!seen.contains("unbound variable"), "no error: {seen:?}");
    for mark in ["\x1b]133;A;aid=", "\x1b]133;C", "\x1b]133;D;1;aid="] {
        assert!(seen.contains(mark), "{mark:?} sent: {seen:?}");
    }
    s.finish();
}

/// The lines of `text` with each run of equal lines kept once: a prompt
/// drawn again in place logs its line again.
fn runs(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.lines().collect();
    lines.dedup();
    lines
}

/// What `sh`, started under each of integration off and on in a scratch
/// home holding `files`, writes to `$HOME/log` while it runs `lines`, a
/// prompt between each: off's first, the control.
fn logged(sh: Sh, tag: &str, files: &[(&str, &str)], lines: &[&str]) -> Option<[String; 2]> {
    let shell = match sh.find() {
        Ok(p) => p,
        Err(why) => {
            eprintln!("skipped: {why}");
            return None;
        }
    };
    let log = |integration: Integration| {
        let dir = scratch(&format!("{tag}-{integration:?}"));
        for (file, text) in files {
            put(&dir.join("home"), file, text);
        }
        let mut s = Session::start(parent(&dir, &shell, &[]), integration);
        s.reading(Times::ONCE);
        for line in lines {
            s.run(line);
        }
        s.finish();
        std::fs::read_to_string(dir.join("home").join("log")).expect("the log")
    };
    Some([log(Integration::Off), log(Integration::On)])
}

/// **A fish prompt reads the status fish left it**: a `fish_prompt` that
/// logs `$status` and `$pipestatus` logs the same with integration on as
/// off, after `false` and after `false | true` -- `1 1`, then `0 1 0`.
#[test]
fn a_fish_prompt_reads_the_status_as_without_baud() {
    let Some([off, on]) = logged(
        Sh::Fish,
        "fish-status",
        &[(
            ".config/fish/config.fish",
            "set -g fish_greeting\n\
             function fish_prompt; echo \"$status $pipestatus\" >> $HOME/log; echo -n '> '; end\n",
        )],
        &["false", "false | true"],
    ) else {
        return;
    };
    let off = runs(&off);
    assert!(
        off.ends_with(&["1 1", "0 1 0"]),
        "the control: the statuses logged: {off:?}"
    );
    assert_eq!(runs(&on), off, "with integration on");
}

/// **`funcsave fish_prompt` saves the user's own prompt**, fish's way to
/// keep one: with the prompt in `functions/fish_prompt.fish`, as
/// `funcsave` writes it, saving it leaves the file as it was, and a prompt
/// defined at the command line and then saved is written as it was
/// defined. Integration off is the control, and on writes the same.
#[test]
fn funcsave_saves_the_users_own_fish_prompt() {
    const MINE: &str = "function fish_prompt\n    echo -n '> '\nend\n";
    let file = ".config/fish/functions/fish_prompt.fish";
    let saves = "funcsave fish_prompt; cat ~/.config/fish/functions/fish_prompt.fish >> ~/log";
    let Some([off, on]) = logged(
        Sh::Fish,
        "funcsave",
        &[
            (".config/fish/config.fish", "set -g fish_greeting\n"),
            (file, MINE),
        ],
        &[saves, "function fish_prompt; echo -n '$ '; end", saves],
    ) else {
        return;
    };
    assert!(
        off.starts_with(MINE) && off[MINE.len()..].contains("echo -n '$ '"),
        "the control: the file as it was, then the new prompt: {off:?}"
    );
    assert_eq!(on, off, "saved with integration on");
}

/// **The user's own `PROMPT_COMMAND` reads the command's status**: one
/// that logs `$?` and `PIPESTATUS` logs the same with integration on as
/// off, after `false` and after `false | true` -- `1 1`, then `0 1 0` --
/// set in `.bash_profile` as `rc` spells it.
fn bash_prompt_command_reads_the_status(tag: &str, rc: &str) {
    let profile = format!("PS1='$ '\n{rc}\n");
    let Some([off, on]) = logged(
        Sh::Bash,
        tag,
        &[(".bash_profile", &profile)],
        &["false", "false | true"],
    ) else {
        return;
    };
    let off = runs(&off);
    assert!(
        off.ends_with(&["1 1", "0 1 0"]),
        "the control: the statuses logged: {off:?}"
    );
    assert_eq!(runs(&on), off, "with integration on");
}

#[test]
fn a_bash_prompt_command_reads_the_status_as_without_baud() {
    bash_prompt_command_reads_the_status(
        "pc-scalar",
        "PROMPT_COMMAND='echo \"$? ${PIPESTATUS[*]}\" >> ~/log'",
    );
}

/// The same, `PROMPT_COMMAND` an array, as bash 5.1 and later take it.
#[test]
fn a_bash_prompt_command_array_reads_the_status_as_without_baud() {
    bash_prompt_command_reads_the_status(
        "pc-array",
        "PROMPT_COMMAND=('echo \"$? ${PIPESTATUS[*]}\" >> ~/log')",
    );
}

/// The same set in `.bash_profile` and exported, which keeps it a string:
/// the user's own entry reads the command's `$?`.
#[test]
fn an_exported_bash_prompt_command_reads_the_status_as_without_baud() {
    let Some([off, on]) = logged(
        Sh::Bash,
        "pc-exported",
        &[(
            ".bash_profile",
            "PS1='$ '\nexport PROMPT_COMMAND='echo \"$?\" >> ~/log'\n",
        )],
        &["false", "sh -c 'exit 3'"],
    ) else {
        return;
    };
    let off = runs(&off);
    assert!(
        off.ends_with(&["1", "3"]),
        "the control: the statuses logged: {off:?}"
    );
    assert_eq!(runs(&on), off, "with integration on");
}

/// **A line's commands read `$_`, `$?` and `PIPESTATUS` as they were, and
/// a user's own DEBUG trap runs for them as it does without baud**, with
/// `PS1` exported, so baud's trap takes its marks out before each line.
/// With no trap of the user's, the line after `false` reads `$?` as 1, the
/// line after `echo a b` reads `$_` as `b`, the line after `false | true`
/// reads `PIPESTATUS` as `1 0`, and `trap -p DEBUG` shows none; under
/// `extdebug`, where a DEBUG trap that fails skips its command, the line
/// after `false` still runs. With a trap of the user's, it logs each of
/// those commands with the `$?` before it and is the trap `trap -p` shows;
/// it also runs for the commands of baud's `PROMPT_COMMAND` entries, as it
/// would for any entry, and those lines, which name baud's functions, are
/// left out. Integration off is the control, and on logs the same.
#[test]
fn a_bash_debug_trap_runs_as_without_baud() {
    // Each read is the first command of its line, which baud's trap
    // fires before.
    let lines = [
        "false",
        "echo \"after $?\" >> ~/log",
        "echo a b >/dev/null",
        "echo \"last $_\" >> ~/log",
        "false | true",
        "echo \"pipe ${PIPESTATUS[*]}\" >> ~/log",
        "false",
        "echo ran >> ~/log",
        "trap -p DEBUG >> ~/log",
    ];
    let mine = |log: &str| {
        log.lines()
            .filter(|l| !l.contains("__baud"))
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    let cases = [
        (
            "debug-none",
            "",
            &["after 1", "last b", "pipe 1 0", "ran"][..],
        ),
        (
            "debug-extdebug",
            "shopt -s extdebug\n",
            &["after 1", "last b", "pipe 1 0", "ran"][..],
        ),
        (
            "debug-trap",
            "trap 'echo \"trap $? $BASH_COMMAND\" >> ~/log' DEBUG\n",
            &[
                "trap 1 echo \"after $?\" >> ~/log",
                "after 1",
                "trap -- 'echo \"trap $? $BASH_COMMAND\" >> ~/log' DEBUG",
            ][..],
        ),
    ];
    for (tag, trap, want) in cases {
        let profile = format!("PS1='$ '\nexport PS1\n{trap}");
        let Some([off, on]) = logged(Sh::Bash, tag, &[(".bash_profile", &profile)], &lines) else {
            return;
        };
        let off = mine(&off);
        for w in want {
            assert!(off.iter().any(|l| l == w), "the control: {w:?} in {off:?}");
        }
        assert_eq!(mine(&on), off, "{tag}: with integration on");
    }
}

/// **bash-preexec runs as it does without baud**: the real one, 0.6.0 and
/// 0.7.0 (`fixtures/bash-preexec/`), set up as its README and Atuin's
/// say -- copied to `~/.bash-preexec.sh` and sourced last -- with a
/// `preexec` and a `precmd` that log, after no `PROMPT_COMMAND`, a
/// string, an array and an exported string of the user's. It installs
/// itself at the first prompt, from an entry of `PROMPT_COMMAND` that
/// rewrites it and sets the DEBUG trap; the trap then takes the first
/// command after its own last entry for the user's. With integration
/// off and on the log is the same: `preexec` for every line, the first
/// included, `precmd` at every prompt, the user's entries with the `$?`
/// they read without baud, and `trap -p DEBUG` showing the trap it set
/// (none on bash 5.3 and later, where 0.7.0 runs preexec from `PS0`).
#[test]
fn bash_preexec_runs_as_without_baud() {
    let lines = [
        "echo one",
        "false",
        "false | true",
        "trap -p DEBUG >> ~/log",
    ];
    let shapes = [
        ("none", ""),
        (
            "string",
            "PROMPT_COMMAND='echo \"pc $? ${PIPESTATUS[*]}\" >> ~/log'\n",
        ),
        (
            "array",
            "PROMPT_COMMAND=('echo \"pc $? ${PIPESTATUS[*]}\" >> ~/log')\n",
        ),
        (
            "exported",
            "export PROMPT_COMMAND='echo \"pc $?\" >> ~/log'\n",
        ),
    ];
    for version in ["0.6.0", "0.7.0"] {
        let lib = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/bash-preexec")
            .join(format!("bash-preexec-{version}"));
        let lib = std::fs::read_to_string(&lib).expect("the bash-preexec fixture");
        for (shape, rc) in shapes {
            let profile = format!(
                "PS1='$ '\n{rc}[[ -f ~/.bash-preexec.sh ]] && source ~/.bash-preexec.sh\n\
                 preexec() {{ echo \"preexec $1\" >> ~/log; }}\n\
                 precmd() {{ echo \"precmd $?\" >> ~/log; }}\n"
            );
            let tag = format!("bp-{version}-{shape}");
            let Some([off, on]) = logged(
                Sh::Bash,
                &tag,
                &[(".bash_profile", &profile), (".bash-preexec.sh", &lib)],
                &lines,
            ) else {
                return;
            };
            for line in lines {
                let want = format!("preexec {line}");
                assert!(
                    off.lines().any(|l| l == want),
                    "the control, {tag}: {want:?} in {off:?}"
                );
            }
            assert!(
                off.lines().filter(|l| l.starts_with("precmd ")).count() > lines.len(),
                "the control, {tag}: precmd at every prompt: {off:?}"
            );
            assert_eq!(on, off, "{tag}: with integration on");
        }
    }
}
