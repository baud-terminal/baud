//! **Where the README's GIF came from, written beside it.** `--record`
//! writes the provenance file beside the GIF, its name the GIF's with
//! `.provenance.toml` added: the commit of baud that was built and
//! filmed and whether its tree was clean, the command, every step of the
//! session with its time since the launch, the nixpkgs revision and each
//! tool's store path, the SHA-256 of each pinned file, the raw film and the
//! GIF, the recorder's and the encoder's arguments and the macOS version.
//! `--verify <gif>` reads it back and checks the GIF's SHA-256 and that the
//! commit is in this repository, on any system with git and `shasum` or
//! `sha256sum`.
//!
//! The file is TOML, written and read here by a reader of the subset it
//! writes -- tables, basic strings, numbers and lists of strings -- so the
//! example adds no dependency.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// One value: a string, a number as written, or a list of strings.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Str(String),
    Num(String),
    List(Vec<String>),
}

/// The file: its tables in order, the top one named `""`, each with its
/// keys in order.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Doc {
    pub tables: Vec<(String, Vec<(String, Value)>)>,
}

impl Doc {
    /// Sets `key` in `table`, the table added at the end when it is new.
    pub fn set(&mut self, table: &str, key: &str, v: Value) {
        let at = match self.tables.iter().position(|(n, _)| n == table) {
            Some(i) => i,
            None => {
                self.tables.push((table.to_string(), Vec::new()));
                self.tables.len() - 1
            }
        };
        let keys = &mut self.tables[at].1;
        match keys.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = v,
            None => keys.push((key.to_string(), v)),
        }
    }

    pub fn str(&mut self, table: &str, key: &str, s: impl Into<String>) {
        self.set(table, key, Value::Str(s.into()));
    }

    pub fn num(&mut self, table: &str, key: &str, n: impl ToString) {
        self.set(table, key, Value::Num(n.to_string()));
    }

    pub fn list(&mut self, table: &str, key: &str, items: &[String]) {
        self.set(table, key, Value::List(items.to_vec()));
    }

    pub fn get(&self, table: &str, key: &str) -> Option<&Value> {
        self.tables
            .iter()
            .find(|(n, _)| n == table)
            .and_then(|(_, keys)| keys.iter().find(|(k, _)| k == key))
            .map(|(_, v)| v)
    }

    /// A string or a number's text.
    pub fn text_of(&self, table: &str, key: &str) -> Option<&str> {
        match self.get(table, key)? {
            Value::Str(s) | Value::Num(s) => Some(s),
            Value::List(_) => None,
        }
    }

    /// **The file's text**: `comment` as `#` lines, then the top table's
    /// keys, then each table under its header.
    pub fn text(&self, comment: &[&str]) -> String {
        let mut out = String::new();
        for line in comment {
            out.push_str(if line.is_empty() { "#" } else { "# " });
            out.push_str(line);
            out.push('\n');
        }
        for (name, keys) in &self.tables {
            if !name.is_empty() {
                out.push_str(&format!("\n[{}]\n", key_text(name)));
            } else if !comment.is_empty() && !keys.is_empty() {
                out.push('\n');
            }
            for (k, v) in keys {
                out.push_str(&key_text(k));
                out.push_str(" = ");
                match v {
                    Value::Str(s) => out.push_str(&quote(s)),
                    Value::Num(n) => out.push_str(n),
                    Value::List(items) if items.is_empty() => out.push_str("[]"),
                    Value::List(items) => {
                        out.push_str("[\n");
                        for item in items {
                            out.push_str("  ");
                            out.push_str(&quote(item));
                            out.push_str(",\n");
                        }
                        out.push(']');
                    }
                }
                out.push('\n');
            }
        }
        out
    }
}

fn bare(k: &str) -> bool {
    !k.is_empty()
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn key_text(k: &str) -> String {
    if bare(k) { k.to_string() } else { quote(k) }
}

/// A TOML basic string: `"` and `\` escaped, and every control character.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// **The subset [`Doc::text`] writes, read back**: `#` comments, `[table]`
/// headers, and `key = value` with a basic string, a number or a list of
/// basic strings, a key bare or quoted. Anything else is refused, as is a
/// table or a key given twice.
pub fn read(text: &str) -> Result<Doc, String> {
    let c: Vec<char> = text.chars().collect();
    let mut r = Reader { c: &c, i: 0 };
    let mut doc = Doc::default();
    let mut table = String::new();
    loop {
        r.blank();
        let Some(ch) = r.peek() else { break };
        if ch == '[' {
            r.i += 1;
            r.space();
            let name = r.key()?;
            r.space();
            r.expect(']')?;
            r.end_of_line()?;
            if doc.tables.iter().any(|(n, _)| *n == name) {
                return Err(format!("[{name}] twice"));
            }
            doc.tables.push((name.clone(), Vec::new()));
            table = name;
            continue;
        }
        let key = r.key()?;
        r.space();
        r.expect('=')?;
        r.space();
        let v = r.value()?;
        r.end_of_line()?;
        if doc.get(&table, &key).is_some() {
            return Err(format!("{key} twice in [{table}]"));
        }
        doc.set(&table, &key, v);
    }
    Ok(doc)
}

struct Reader<'a> {
    c: &'a [char],
    i: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }

    fn line(&self) -> usize {
        self.c[..self.i.min(self.c.len())]
            .iter()
            .filter(|&&c| c == '\n')
            .count()
            + 1
    }

    fn err(&self, what: &str) -> String {
        format!("line {}: {what}", self.line())
    }

    /// Spaces and tabs.
    fn space(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.i += 1;
        }
    }

    /// Spaces, newlines and comments.
    fn blank(&mut self) {
        loop {
            match self.peek() {
                Some(' ' | '\t' | '\n' | '\r') => self.i += 1,
                Some('#') => {
                    while !matches!(self.peek(), None | Some('\n')) {
                        self.i += 1;
                    }
                }
                _ => return,
            }
        }
    }

    fn expect(&mut self, want: char) -> Result<(), String> {
        if self.peek() == Some(want) {
            self.i += 1;
            Ok(())
        } else {
            Err(self.err(&format!("expected {want:?}")))
        }
    }

    /// Spaces, an optional comment, then a newline or the end.
    fn end_of_line(&mut self) -> Result<(), String> {
        self.space();
        if self.peek() == Some('#') {
            while !matches!(self.peek(), None | Some('\n')) {
                self.i += 1;
            }
        }
        if self.peek() == Some('\r') {
            self.i += 1;
        }
        match self.peek() {
            None => Ok(()),
            Some('\n') => {
                self.i += 1;
                Ok(())
            }
            Some(_) => Err(self.err("more after the value")),
        }
    }

    fn key(&mut self) -> Result<String, String> {
        if self.peek() == Some('"') {
            return self.string();
        }
        let from = self.i;
        while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            self.i += 1;
        }
        if self.i == from {
            return Err(self.err("expected a key"));
        }
        Ok(self.c[from..self.i].iter().collect())
    }

    fn value(&mut self) -> Result<Value, String> {
        match self.peek() {
            Some('"') => self.string().map(Value::Str),
            Some('[') => {
                self.i += 1;
                let mut items = Vec::new();
                loop {
                    self.blank();
                    match self.peek() {
                        Some(']') => {
                            self.i += 1;
                            return Ok(Value::List(items));
                        }
                        Some('"') => items.push(self.string()?),
                        _ => return Err(self.err("a list holds strings")),
                    }
                    self.blank();
                    match self.peek() {
                        Some(',') => self.i += 1,
                        Some(']') => {}
                        _ => return Err(self.err("expected , or ]")),
                    }
                }
            }
            Some(c) if c.is_ascii_digit() || c == '-' || c == '+' => {
                let from = self.i;
                while matches!(self.peek(), Some(c) if c.is_ascii_digit() || "+-._eE".contains(c)) {
                    self.i += 1;
                }
                Ok(Value::Num(self.c[from..self.i].iter().collect()))
            }
            _ => Err(self.err("expected a string, a number or a list")),
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(self.err("a string never ends"));
            };
            self.i += 1;
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let Some(e) = self.peek() else {
                        return Err(self.err("a string never ends"));
                    };
                    self.i += 1;
                    match e {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'u' | 'U' => {
                            let n = if e == 'u' { 4 } else { 8 };
                            let hex: String = self
                                .c
                                .get(self.i..self.i + n)
                                .unwrap_or(&[])
                                .iter()
                                .collect();
                            let ch = u32::from_str_radix(&hex, 16)
                                .ok()
                                .filter(|_| hex.len() == n)
                                .and_then(char::from_u32)
                                .ok_or_else(|| self.err("a bad \\u escape"))?;
                            self.i += n;
                            out.push(ch);
                        }
                        _ => return Err(self.err("an unknown escape")),
                    }
                }
                c if (c as u32) < 0x20 && c != '\t' || c as u32 == 0x7f => {
                    return Err(self.err("a control character in a string"));
                }
                c => out.push(c),
            }
        }
    }
}

/// **The repository this example is in**, the workspace root above
/// `crates/baud-app/`: where `--record` reads the commit and `--verify` looks it
/// up.
pub fn repository() -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    here.parent()
        .and_then(|p| p.parent())
        .unwrap_or(here)
        .to_path_buf()
}

/// The provenance file of a GIF: the GIF's path with `.provenance.toml`
/// added.
pub fn path_of(gif: &Path) -> PathBuf {
    let mut p = gif.as_os_str().to_owned();
    p.push(".provenance.toml");
    PathBuf::from(p)
}

/// **The repository's state**: the commit checked out and `git status`'s
/// lines, untracked files included -- empty when the tree is clean.
#[derive(Clone, Debug, PartialEq)]
pub struct Tree {
    pub commit: String,
    pub dirty: Vec<String>,
}

fn git(root: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("git: {e}"))
}

/// A full commit name: forty lowercase hexadecimal digits.
pub fn is_commit_name(s: &str) -> bool {
    s.len() == 40
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// **What is checked out at `root`, and what differs from it**: `HEAD`'s
/// commit, and `git status --porcelain` with every untracked file listed,
/// whatever the person's git configuration hides by default.
pub fn tree(root: &Path) -> Result<Tree, String> {
    let head = git(root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    let commit = String::from_utf8_lossy(&head.stdout).trim().to_string();
    if !head.status.success() || !is_commit_name(&commit) {
        return Err(format!(
            "{} is not a git checkout with a commit",
            root.display()
        ));
    }
    let status = git(
        root,
        &[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--ignore-submodules=none",
        ],
    )?;
    if !status.status.success() {
        return Err("git status failed".to_string());
    }
    let dirty = String::from_utf8_lossy(&status.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    Ok(Tree { commit, dirty })
}

/// **A recording is made of a clean tree**, so the commit it names is what
/// was built and filmed; a dirty one is refused unless `allow_dirty`, and
/// then the file says so and lists what differed.
pub fn admit(t: &Tree, allow_dirty: bool) -> Result<(), String> {
    if t.dirty.is_empty() || allow_dirty {
        return Ok(());
    }
    let shown: Vec<&str> = t.dirty.iter().take(8).map(String::as_str).collect();
    Err(format!(
        "the tree differs from {} in {} path{} ({}{}), so the commit would not be what was \
         filmed: commit or stash them, or pass --allow-dirty, which the provenance file then \
         records",
        &t.commit[..12],
        t.dirty.len(),
        if t.dirty.len() == 1 { "" } else { "s" },
        shown.join("; "),
        if t.dirty.len() > shown.len() {
            "; ..."
        } else {
            ""
        }
    ))
}

/// Whether `commit` is a commit object of the repository at `root`.
pub fn commit_exists(root: &Path, commit: &str) -> bool {
    is_commit_name(commit)
        && git(root, &["cat-file", "-e", &format!("{commit}^{{commit}}")])
            .map(|o| o.status.success())
            .unwrap_or(false)
}

/// Whether `commit` is `HEAD` or one of its ancestors.
pub fn is_ancestor(root: &Path, commit: &str) -> bool {
    is_commit_name(commit)
        && git(root, &["merge-base", "--is-ancestor", commit, "HEAD"])
            .map(|o| o.status.success())
            .unwrap_or(false)
}

/// **A file's SHA-256**, by `shasum -a 256`, or `sha256sum` where there is
/// no `shasum`.
pub fn sha256(file: &Path) -> Result<String, String> {
    for (cmd, args) in [("shasum", &["-a", "256"][..]), ("sha256sum", &[][..])] {
        let Ok(out) = Command::new(cmd).args(args).arg(file).output() else {
            continue;
        };
        if !out.status.success() {
            return Err(format!("{cmd} could not read {}", file.display()));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let hex = text.split_whitespace().next().unwrap_or("");
        if hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(hex.to_ascii_lowercase());
        }
        return Err(format!("{cmd} said {text:?}"));
    }
    Err("neither shasum nor sha256sum is on PATH".to_string())
}

/// **Whether `gif` is the GIF its provenance file describes**: the file
/// beside it names this GIF, its SHA-256 and length match, and the commit
/// it names is in the repository at `root`. Returns what was checked, a
/// line each; a mismatch is an error.
pub fn verify(gif: &Path, root: &Path) -> Result<Vec<String>, String> {
    let prov = path_of(gif);
    let text = fs::read_to_string(&prov).map_err(|e| format!("{}: {e}", prov.display()))?;
    let doc = read(&text).map_err(|e| format!("{}: {e}", prov.display()))?;
    let field = |t: &str, k: &str| {
        doc.text_of(t, k)
            .map(str::to_string)
            .ok_or(format!("{} has no {k} in [{t}]", prov.display()))
    };
    let name = field("gif", "file")?;
    let want = field("gif", "sha256")?;
    let bytes = field("gif", "bytes")?;
    let commit = field("baud", "commit")?;
    let mut said = Vec::new();
    let own = gif.file_name().map(|n| n.to_string_lossy().into_owned());
    if own.as_deref() != Some(name.as_str()) {
        return Err(format!(
            "the provenance file describes {name}, not {}",
            gif.display()
        ));
    }
    let got = sha256(gif)?;
    let len = fs::metadata(gif)
        .map_err(|e| format!("{}: {e}", gif.display()))?
        .len();
    if got != want || len.to_string() != bytes {
        return Err(format!(
            "{} is {len} bytes with SHA-256 {got}; its provenance file describes {bytes} bytes \
             with {want}: not the GIF that was recorded",
            gif.display()
        ));
    }
    said.push(format!(
        "gif:     {name}, {len} bytes, SHA-256 {got}, as recorded"
    ));
    if !commit_exists(root, &commit) {
        return Err(format!(
            "the commit it was recorded at, {commit}, is not in the repository at {}",
            root.display()
        ));
    }
    said.push(format!(
        "commit:  {commit} is in this repository{}",
        if is_ancestor(root, &commit) {
            ", and HEAD descends from it"
        } else {
            ", and HEAD does not descend from it"
        }
    ));
    said.push(format!(
        "tree:    {}",
        doc.text_of("baud", "tree").unwrap_or("not recorded")
    ));
    Ok(said)
}

/// **The encoder's settings, exactly**: gifski at quality 80 with its lossy
/// and motion quality at 100 -- a lower setting of either leaves faint
/// traces of earlier screens behind later ones -- 30 frames a second and
/// 1164 px wide, about three quarters of the 1572 px window at 12 pt.
pub const GIFSKI: [&str; 10] = [
    "--quality",
    "80",
    "--lossy-quality",
    "100",
    "--motion-quality",
    "100",
    "--fps",
    "30",
    "--width",
    "1164",
];

/// The frame rate the film is sampled at, gifski's `--fps`.
pub const FPS: u32 = 30;

/// **What the provenance file records**, every value from the run that
/// made the GIF.
pub struct Facts {
    /// The GIF's file name.
    pub gif: String,
    /// The command, as it would be typed: `cargo run`, the profile's flags,
    /// `--example demo --` and the example's arguments.
    pub command: String,
    /// The directory it was run from, from the repository's root.
    pub from: String,
    pub tree: Tree,
    pub macos: String,
    pub gif_sha256: String,
    pub gif_bytes: u64,
    pub gif_frames: usize,
    pub gif_seconds: f64,
    pub raw_sha256: String,
    pub raw_bytes: u64,
    pub raw_seconds: f64,
    /// Where the raw film is kept, in words: it is not in the repository.
    pub raw_kept: String,
    /// The recorder's line naming the display, its scale and size.
    pub display: String,
    /// baud's window in points, as the window server gave it.
    pub window: String,
    /// The crop, in the film's pixels: `crop=W:H:X:Y`.
    pub crop: String,
    pub recorder_sha256: String,
    pub compile: Vec<String>,
    pub swiftc: Vec<String>,
    pub recorder_args: Vec<String>,
    pub recorder_said: Vec<String>,
    pub frames_args: Vec<String>,
    pub gifski_args: Vec<String>,
    pub gifski_inputs: String,
    pub nixpkgs: String,
    pub tools: Vec<(String, String)>,
    pub pins: Vec<(String, String)>,
    pub events: Vec<String>,
}

/// **The provenance file's tables**, from [`Facts`].
pub fn document(f: &Facts) -> Doc {
    let mut d = Doc::default();
    d.str("", "made", &f.command);
    d.str("", "from", &f.from);
    d.str("baud", "commit", &f.tree.commit);
    d.str(
        "baud",
        "tree",
        if f.tree.dirty.is_empty() {
            "clean"
        } else {
            "dirty, recorded with --allow-dirty"
        },
    );
    d.list("baud", "dirty", &f.tree.dirty);
    d.str("baud", "macos", &f.macos);
    d.str("gif", "file", &f.gif);
    d.str("gif", "sha256", &f.gif_sha256);
    d.num("gif", "bytes", f.gif_bytes);
    d.num("gif", "frames", f.gif_frames);
    d.num("gif", "seconds", format!("{:.3}", f.gif_seconds));
    d.str("raw", "sha256", &f.raw_sha256);
    d.num("raw", "bytes", f.raw_bytes);
    d.num("raw", "seconds", format!("{:.3}", f.raw_seconds));
    d.str("raw", "kept", &f.raw_kept);
    d.str("raw", "display", &f.display);
    d.str("raw", "window", &f.window);
    d.str("raw", "crop", &f.crop);
    d.str("recorder", "source", "tools/scripts/trail-record.swift");
    d.str("recorder", "sha256", &f.recorder_sha256);
    d.list("recorder", "compile", &f.compile);
    d.list("recorder", "swiftc", &f.swiftc);
    d.list("recorder", "args", &f.recorder_args);
    d.list("recorder", "said", &f.recorder_said);
    d.list("encoder", "frames", &f.frames_args);
    d.list("encoder", "gifski", &f.gifski_args);
    d.str("encoder", "gifski_inputs", &f.gifski_inputs);
    d.str("nixpkgs", "revision", &f.nixpkgs);
    for (attr, path) in &f.tools {
        d.str("tools", attr, path);
    }
    for (file, sha) in &f.pins {
        d.str("sha256", file, sha);
    }
    d.list("session", "events", &f.events);
    d
}

/// The comment the file opens with.
pub fn header(gif: &str) -> Vec<String> {
    vec![
        format!("How {gif} was made: the demo's session played in baud's window, filmed"),
        "and encoded by the one command below. Every value is the run's own.".to_string(),
        String::new(),
        format!("Check it: cargo run --example demo -- --verify <path to {gif}>"),
    ]
}

/// **The features the tour's MP4 is filmed with**: `take`, whose journal
/// of what baud showed the MP4's timeline is made from
/// (`crates/baud-app/src/take.rs`); every other take builds baud as it ships.
pub const TAKE_FEATURES: [&str; 1] = ["take"];

/// **The films' licences, for the reviewer**, whose last two sections hold
/// the credit lines for the video's descriptions.
pub const LICENSES: &str = include_str!("../../../../docs/assets/demo/LICENSES.md");

/// The heading of [`LICENSES`]'s credit line, every version's.
pub const CREDIT_HEADING: &str = "## The credit line for the video's description";

/// The heading of [`LICENSES`]'s keyboard sounds' credit line, the clicks
/// version's alone (`sound.rs`).
pub const CLICKS_HEADING: &str =
    "## The keyboard sounds' credit line, for the clicks version's description";

/// **The credit line**, the one fenced block under [`CREDIT_HEADING`] in
/// `licences`, without its fences.
pub fn credit_line(licences: &str) -> Result<&str, String> {
    block_under(licences, CREDIT_HEADING)
}

/// **The keyboard sounds' credit line**, the one fenced block under
/// [`CLICKS_HEADING`].
pub fn clicks_credit(licences: &str) -> Result<&str, String> {
    block_under(licences, CLICKS_HEADING)
}

/// The one ```` ```text ```` block of the section `heading` opens, which
/// ends at the next `## ` heading or the file's end, without its fences.
fn block_under<'a>(licences: &'a str, heading: &str) -> Result<&'a str, String> {
    let at = licences
        .find(heading)
        .ok_or_else(|| format!("docs/assets/demo/LICENSES.md has no section {heading:?}"))?;
    let rest = &licences[at + heading.len()..];
    let rest = &rest[..rest.find("\n## ").unwrap_or(rest.len())];
    let open = rest
        .find("```text\n")
        .ok_or_else(|| format!("{heading:?} has no ```text block"))?;
    let body = &rest[open + "```text\n".len()..];
    let close = body
        .find("\n```")
        .ok_or_else(|| format!("{heading:?}'s block is not closed"))?;
    let line = body[..close].trim();
    if line.is_empty() || body[close + 4..].contains("```") {
        return Err(format!("{heading:?} must hold one block, and it not empty"));
    }
    Ok(line)
}

/// **What the tour's MP4 record holds**, every value from the take that
/// made it.
pub struct Tour {
    /// The MP4's file name, and its timeline's.
    pub mp4: String,
    pub timeline: String,
    pub command: String,
    pub from: String,
    pub tree: Tree,
    pub macos: String,
    pub mp4_sha256: String,
    pub mp4_bytes: u64,
    pub frames: usize,
    pub fps: u32,
    pub seconds: f64,
    pub timeline_sha256: String,
    pub nixpkgs: String,
    pub tools: Vec<(String, String)>,
    pub pins: Vec<(String, String)>,
    pub events: Vec<String>,
}

/// **The end card**: which baud the video shows, in words for the video's
/// description -- the commit filmed, whether its tree was clean, and the
/// features it was built with.
pub fn end_card(tree: &Tree) -> String {
    let short: String = tree.commit.chars().take(12).collect();
    let state = match tree.dirty.is_empty() {
        true => "its tree clean",
        false => "its tree dirty, filmed with --allow-dirty",
    };
    format!(
        "Filmed from baud at commit {short}, {state}, built with --features {}.",
        TAKE_FEATURES.join(",")
    )
}

/// **`NAME.mp4.provenance.toml`'s document**: the commit, its tree, the
/// features, the command, the MP4's and its timeline's SHA-256, the tools
/// and pins, every step with its time, and the video's description -- the
/// end card and the credit line ([`credit_line`]) -- once for each of the
/// video's two versions (`sound.rs`): the silent one's, and the clicks
/// one's, which alone ends with the keyboard sounds' credit line
/// ([`clicks_credit`]).
pub fn tour_document(f: &Tour, credits: &str, clicks: &str) -> Doc {
    let mut d = Doc::default();
    d.str("", "made", &f.command);
    d.str("", "from", &f.from);
    d.str("baud", "commit", &f.tree.commit);
    d.str(
        "baud",
        "tree",
        if f.tree.dirty.is_empty() {
            "clean"
        } else {
            "dirty, recorded with --allow-dirty"
        },
    );
    d.list("baud", "dirty", &f.tree.dirty);
    let features: Vec<String> = TAKE_FEATURES.iter().map(|s| s.to_string()).collect();
    d.list("baud", "features", &features);
    d.str("baud", "macos", &f.macos);
    d.str("mp4", "file", &f.mp4);
    d.str("mp4", "sha256", &f.mp4_sha256);
    d.num("mp4", "bytes", f.mp4_bytes);
    d.num("mp4", "frames", f.frames);
    d.num("mp4", "fps", f.fps);
    d.num("mp4", "seconds", format!("{:.3}", f.seconds));
    d.str("timeline", "file", &f.timeline);
    d.str("timeline", "sha256", &f.timeline_sha256);
    d.str("video", "end_card", end_card(&f.tree));
    let silent = format!("{}\n\n{credits}", end_card(&f.tree));
    d.str(
        "video",
        "description_clicks",
        format!("{silent}\n\n{clicks}"),
    );
    d.str("video", "description_silent", silent);
    d.str("nixpkgs", "revision", &f.nixpkgs);
    for (attr, path) in &f.tools {
        d.str("tools", attr, path);
    }
    for (file, sha) in &f.pins {
        d.str("sha256", file, sha);
    }
    d.list("session", "events", &f.events);
    d
}

/// The comment the tour's record opens with.
pub fn tour_header(mp4: &str) -> Vec<String> {
    vec![
        format!("How {mp4} was made: the tour played in baud's window, filmed and"),
        "encoded by the one command below; the MP4 itself is uploaded, not kept".to_string(),
        "here. Every value is the take's own.".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::named;
    use crate::names::tests::qz;

    fn sample() -> Doc {
        let mut d = Doc::default();
        d.str(
            "",
            "made",
            "cargo run --release --example demo -- --record a \"b\\c\".gif",
        );
        d.str("baud", "commit", "0123456789abcdef0123456789abcdef01234567");
        d.list("baud", "dirty", &[]);
        d.num("gif", "bytes", 17_000_000);
        d.num("gif", "seconds", "25.933");
        d.str("gif", "prompt", "baud \u{276F} \tand\nmore\u{1b}[0m");
        d.str("sha256", "NotoColorEmoji.ttf", "72a6");
        d.list(
            "session",
            "events",
            &[
                "0.000 baud pid 7".to_string(),
                "-0.652 recorder: \"ready\"".to_string(),
            ],
        );
        d
    }

    /// **What is written is read back, value for value**: quotes,
    /// backslashes, a tab, a newline, an escape character and the prompt's
    /// `❯`, a quoted key, a number, an empty list and a list of two, under
    /// a comment. Perturbation: drop the escape of `"` in `quote` and the
    /// reader stops mid-string, which the round trip catches.
    #[test]
    fn the_file_reads_back_what_it_wrote() {
        let d = sample();
        let text = d.text(&["a comment", "", "and a third line"]);
        assert!(
            text.starts_with("# a comment\n#\n# and a third line\n"),
            "{text}"
        );
        assert!(text.contains("\"NotoColorEmoji.ttf\" = \"72a6\""), "{text}");
        assert!(text.contains("\\u001B[0m"), "{text}");
        assert_eq!(read(&text), Ok(d));
    }

    /// **The reader refuses what the writer never writes**: an unended
    /// string, a bad escape, a table or key given twice, a bare word, text
    /// after a value. And it takes what a person might add: a trailing
    /// comment, a blank line, a list on one line.
    #[test]
    fn the_reader_refuses_what_it_does_not_know() {
        for bad in [
            "a = \"x",
            "a = \"\\q\"",
            "[t]\n[t]\n",
            "a = \"x\"\na = \"y\"\n",
            "a = yes\n",
            "a = \"x\" \"y\"\n",
            "a = [1]\n",
            "a = \"\\u12\"\n",
        ] {
            assert!(read(bad).is_err(), "{bad:?}");
        }
        let ok = read("# c\n\n[gif]\nfile = \"h.gif\" # the GIF\nall = [\"a\", \"b\"]\nn = 3\n")
            .unwrap();
        assert_eq!(ok.text_of("gif", "file"), Some("h.gif"));
        assert_eq!(ok.text_of("gif", "n"), Some("3"));
        assert_eq!(
            ok.get("gif", "all"),
            Some(&Value::List(vec!["a".to_string(), "b".to_string()]))
        );
    }

    fn facts() -> Facts {
        let s = |v: &str| v.to_string();
        Facts {
            gif: s("trailer.gif"),
            command: s(
                "cargo run --release --example demo -- --record docs/assets/demo/trailer.gif",
            ),
            from: s("."),
            tree: Tree {
                commit: s("0123456789abcdef0123456789abcdef01234567"),
                dirty: Vec::new(),
            },
            macos: s("27.0"),
            gif_sha256: s("ab"),
            gif_bytes: 17,
            gif_frames: 780,
            gif_seconds: 26.0,
            raw_sha256: s("cd"),
            raw_bytes: 99,
            raw_seconds: 26.01,
            raw_kept: s("in cargo's target directory, demo/trailer.raw.mp4"),
            display: s("display 1728x1117 pt at 2.0x: 3456x2234 px, 30 applications left out"),
            window: s("471,161 786x572 pt"),
            crop: s("crop=1572:1144:942:322"),
            recorder_sha256: s("ef"),
            compile: vec![s("swiftc"), s("-o"), s("<scratch>/trail-record")],
            swiftc: vec![s("Apple Swift version 6.3")],
            recorder_args: vec![s("display"), s("<scratch>/baud.pid")],
            recorder_said: vec![s("first frame")],
            frames_args: vec![s("ffmpeg"), s("-i"), s("<scratch>/raw.mp4")],
            gifski_args: GIFSKI.iter().map(|a| a.to_string()).collect(),
            gifski_inputs: s("<scratch>/frames/000001.png to 000780.png, in order"),
            nixpkgs: s("319e4cebef10cbb636c6ea392137b52ad105336a"),
            tools: vec![(s("gifski"), s("/nix/store/x-gifski-1.34.0"))],
            pins: vec![(s("NotoColorEmoji.ttf"), s("72a6"))],
            events: vec![
                s("-0.700 recorder ready"),
                s("0.000 baud pid 7"),
                s("1.000 hold 700 ms"),
            ],
        }
    }

    /// **The provenance names nothing of the machine, and the names check
    /// over it can fire**: the file built from a run's facts holds none of
    /// a made-up machine's names, scratch paths written as `<scratch>`;
    /// the same file with the GIF given by a path in the home holds one.
    /// And it reads back to the commit, the GIF's hash and a clean tree.
    #[test]
    fn the_provenance_names_nothing_of_the_machine() {
        let n = qz();
        let f = facts();
        let text = document(&f).text(&["How trailer.gif was made"]);
        assert_eq!(named(text.as_bytes(), &n), Vec::<String>::new(), "{text}");
        let back = read(&text).unwrap();
        assert_eq!(back.text_of("baud", "commit"), Some(f.tree.commit.as_str()));
        assert_eq!(back.text_of("baud", "tree"), Some("clean"));
        assert_eq!(back.text_of("gif", "bytes"), Some("17"));
        let leaky = Facts {
            command: "cargo run --example demo -- --record /Users/me/trailer.gif".to_string(),
            ..facts()
        };
        assert!(!named(document(&leaky).text(&[]).as_bytes(), &n).is_empty());
        let dirty = Facts {
            tree: Tree {
                commit: f.tree.commit.clone(),
                dirty: vec![" M README.md".to_string()],
            },
            ..facts()
        };
        let d = document(&dirty);
        assert_eq!(
            d.text_of("baud", "tree"),
            Some("dirty, recorded with --allow-dirty")
        );
    }

    /// A scratch git repository with one commit, its configuration the
    /// test's own, not the person's.
    pub fn repo(name: &str) -> (PathBuf, String) {
        let dir = std::env::temp_dir().join(format!("baud-demo-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let run = |args: &[&str]| {
            let ok = Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.org")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.org")
                .stdout(Stdio::null())
                .status()
                .unwrap();
            assert!(ok.success(), "git {args:?}");
        };
        run(&["init", "-q"]);
        fs::write(dir.join("a.txt"), "a\n").unwrap();
        run(&["add", "a.txt"]);
        run(&["-c", "commit.gpgsign=false", "commit", "-q", "-m", "a"]);
        let head = tree(&dir).unwrap().commit;
        (dir, head)
    }

    /// **A dirty tree is refused, and allowed only when asked**: a clean
    /// checkout passes; a changed file and an untracked one are each
    /// listed and refused, the refusal naming `--allow-dirty`; allowed, it
    /// passes and the file records it. Perturbation: list status without
    /// `--untracked-files=all` under a configuration that hides them and
    /// the untracked file passes; drop the check of `allow_dirty` and the
    /// changed tree is refused even when allowed.
    #[test]
    fn a_dirty_tree_is_refused_unless_allowed() {
        let (dir, head) = repo("tree");
        let clean = tree(&dir).unwrap();
        assert_eq!(
            clean,
            Tree {
                commit: head.clone(),
                dirty: Vec::new()
            }
        );
        assert_eq!(admit(&clean, false), Ok(()));
        fs::write(dir.join("a.txt"), "b\n").unwrap();
        let changed = tree(&dir).unwrap();
        assert_eq!(changed.dirty, [" M a.txt"]);
        let said = admit(&changed, false).unwrap_err();
        assert!(
            said.contains("a.txt") && said.contains("--allow-dirty"),
            "{said}"
        );
        assert_eq!(admit(&changed, true), Ok(()));
        Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["checkout", "-q", "a.txt"])
            .status()
            .unwrap();
        fs::create_dir_all(dir.join("new")).unwrap();
        fs::write(dir.join("new/b.txt"), "b\n").unwrap();
        let untracked = Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["config", "status.showUntrackedFiles", "no"])
            .status()
            .unwrap();
        assert!(untracked.success());
        assert_eq!(tree(&dir).unwrap().dirty, ["?? new/b.txt"]);
        assert!(admit(&tree(&dir).unwrap(), false).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    /// **`--verify` takes the GIF its file describes and nothing else**: a
    /// GIF and its provenance pass; one byte of the GIF changed fails, as
    /// does a commit the repository does not have and a provenance file
    /// naming another GIF. The SHA-256 is the vector `shasum` gives for
    /// these bytes, written here, so the check is not the hash checking
    /// itself. Perturbation: compare only the length and the changed byte
    /// passes.
    #[test]
    fn verify_takes_the_gif_it_describes_and_nothing_else() {
        let (dir, head) = repo("verify");
        let gif = dir.join("trailer.gif");
        fs::write(&gif, b"GIF89a not really a GIF\n").unwrap();
        let sha = "874773806b0ff68997016c684f2b176828a47c32c01c9e6195752059e5099465";
        assert_eq!(sha256(&gif).unwrap(), sha);
        let write = |name: &str, commit: &str| {
            let mut d = Doc::default();
            d.str("baud", "commit", commit);
            d.str("baud", "tree", "clean");
            d.str("gif", "file", name);
            d.str("gif", "sha256", sha);
            d.num("gif", "bytes", 24);
            fs::write(path_of(&gif), d.text(&[])).unwrap();
        };
        write("trailer.gif", &head);
        let said = verify(&gif, &dir).unwrap();
        assert!(said[0].contains("as recorded"), "{said:?}");
        assert!(said[1].contains("HEAD descends from it"), "{said:?}");
        let mut bytes = fs::read(&gif).unwrap();
        bytes[10] ^= 1;
        fs::write(&gif, &bytes).unwrap();
        let err = verify(&gif, &dir).unwrap_err();
        assert!(err.contains("not the GIF that was recorded"), "{err}");
        bytes[10] ^= 1;
        fs::write(&gif, &bytes).unwrap();
        assert!(verify(&gif, &dir).is_ok(), "restored, it passes again");
        write("trailer.gif", "0123456789abcdef0123456789abcdef01234567");
        assert!(
            verify(&gif, &dir)
                .unwrap_err()
                .contains("not in the repository")
        );
        write("other.gif", &head);
        assert!(
            verify(&gif, &dir)
                .unwrap_err()
                .contains("describes other.gif")
        );
        let _ = fs::remove_dir_all(&dir);
    }
    /// **The tour's record names the commit, the feature and the credits**:
    /// the commit and `features = ["take"]` under `[baud]`, the end card in
    /// both versions' descriptions, then the credit line, which is the one
    /// block under LICENSES.md's heading; the clicks version's alone then
    /// ends with the keyboard sounds' credit, the licence's own copyright
    /// line in it, and the silent version's names no keyboard sound; and
    /// the file reads back. The controls: a section with two blocks, or
    /// none, is refused, and each heading's block is its own section's, not
    /// the next one's. Perturbations: leave the feature out of the end card,
    /// or give the silent version the clicks credit, and an assertion fails.
    #[test]
    fn the_tour_record_names_the_commit_the_feature_and_the_credits() {
        let credits = credit_line(LICENSES).expect("the credit line");
        assert!(credits.contains("JetBrains Mono"), "{credits}");
        let clicks = clicks_credit(LICENSES).expect("the keyboard sounds' credit line");
        assert!(
            clicks.contains("Copyright (c) 2026 Hải Nguyễn")
                && clicks.contains("Mechvibes")
                && clicks.contains("MIT License"),
            "{clicks}"
        );
        assert!(!credits.contains("Mechvibes") && !clicks.contains("JetBrains"));
        let tree = Tree {
            commit: "0123456789abcdef0123456789abcdef01234567".to_string(),
            dirty: Vec::new(),
        };
        let f = Tour {
            mp4: "tour.mp4".to_string(),
            timeline: "tour.timeline.tsv".to_string(),
            command: "cargo run --release --example demo -- --record docs/assets/demo/tour.mp4"
                .to_string(),
            from: ".".to_string(),
            tree: tree.clone(),
            macos: "27.0".to_string(),
            mp4_sha256: "ab".repeat(32),
            mp4_bytes: 10,
            frames: 600,
            fps: 60,
            seconds: 10.0,
            timeline_sha256: "cd".repeat(32),
            nixpkgs: "rev".to_string(),
            tools: Vec::new(),
            pins: Vec::new(),
            events: vec!["0.000 scene 1".to_string()],
        };
        let text = tour_document(&f, credits, clicks).text(&["x"]);
        let back = read(&text).expect("it reads back");
        assert_eq!(back.text_of("baud", "commit"), Some(tree.commit.as_str()));
        assert_eq!(
            back.get("baud", "features"),
            Some(&Value::List(vec!["take".to_string()]))
        );
        let card = end_card(&tree);
        assert!(
            card.contains("0123456789ab") && card.contains("--features take"),
            "{card}"
        );
        let silent = back.text_of("video", "description_silent").unwrap();
        assert!(silent.starts_with(&card) && silent.ends_with(credits));
        assert!(!silent.contains("Keyboard sounds"), "{silent}");
        let with = back.text_of("video", "description_clicks").unwrap();
        assert_eq!(with, format!("{silent}\n\n{clicks}"));
        let two = format!("{CREDIT_HEADING}\n\n```text\na\n```\n\n```text\nb\n```\n");
        assert!(credit_line(&two).is_err(), "the control: two blocks");
        assert!(credit_line(CREDIT_HEADING).is_err(), "the control: none");
        let next =
            format!("{CLICKS_HEADING}\n\nnone here\n\n{CREDIT_HEADING}\n\n```text\na\n```\n");
        assert!(
            clicks_credit(&next).is_err(),
            "the control: the next section's block"
        );
        assert_eq!(credit_line(&next), Ok("a"));
    }
}
