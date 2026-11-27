//! `cargo xtask forbidden --git GIT_DIR PATTERNS CONTROLS [--range A..B]
//! [--identity 'NAME <EMAIL>'] [--tsv OUT]`: a repository searched with the
//! list, every label's control found first exactly as for files (exit 2).
//!
//! What is searched, a label counted once for each thing it is found in:
//!
//! - each ref's name;
//! - each object's path, as `git rev-list --objects` names it;
//! - each commit whole, its headers and its message;
//! - each blob.
//!
//! A commit or blob is searched as its bytes; a blob that begins `1f 8b` also
//! as the gzip member it holds, when that inflates; and each of those that
//! holds an ESC, or a NUL in its first 4096 bytes, again with the terminal's
//! escapes taken out. Trees and tags are searched by their path alone.
//!
//! The objects are those every ref reaches, or with `--range A..B` those
//! `A..B` reaches; the ref names are searched either way. `--identity`
//! requires each commit's author and committer, name and address, to be it.
//!
//! stdout holds one summary line. stderr holds, for each label found, its
//! count by kind, and the count of author and committer lines that are not
//! the identity; neither quotes what matched. `--tsv OUT` writes one row a
//! hit, `label<TAB>kind<TAB>detail`, in the list's order and, within a label,
//! refs, then paths, then objects in `rev-list`'s order. The kind is `ref`,
//! `path`, `commit` or `blob`, and the detail the ref's name, the path, the
//! commit's id to 12 digits, or the blob's id to 12 digits, a space and its
//! path. A ref's or a path's detail is what matched: the file is written
//! only where it is asked for.
//!
//! Exit 0 when nothing is found; 1 on a hit, an author or committer that is
//! not the identity, a list that cannot be used or a git that fails; 2 when
//! the controls refuse, before git is run.

use std::borrow::Cow;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use regex::bytes::RegexSet;

use super::{ESC, Pattern, armed};
use crate::features_since::Exit;
use crate::tarball;

const USAGE: &str = "usage: cargo xtask forbidden --git GIT_DIR PATTERNS CONTROLS \
                     [--range A..B] [--identity 'NAME <EMAIL>'] [--tsv OUT]";

/// The bytes looked at for a NUL, from a view's start.
const NUL_REACH: usize = 4096;

/// No deflate stream inflates to more than 1032 times its length, so a gzip
/// trailer that claims more is not inflated.
const MOST_INFLATED: usize = 1032;

/// The command's words.
struct Args {
    dir: PathBuf,
    list: PathBuf,
    ctl: PathBuf,
    range: Option<String>,
    identity: Option<Vec<u8>>,
    tsv: Option<PathBuf>,
}

impl Args {
    fn parse(args: &[&str]) -> Option<Args> {
        let (mut range, mut identity, mut tsv) = (None, None, None);
        let mut words = Vec::new();
        let mut it = args.iter();
        while let Some(&word) = it.next() {
            match word {
                "--range" => range = Some(it.next()?.to_string()),
                "--identity" => identity = Some(it.next()?.as_bytes().to_vec()),
                "--tsv" => tsv = Some(PathBuf::from(it.next()?)),
                _ => words.push(word),
            }
        }
        let [dir, list, ctl] = words.as_slice() else {
            return None;
        };
        Some(Args {
            dir: PathBuf::from(*dir),
            list: PathBuf::from(*list),
            ctl: PathBuf::from(*ctl),
            range,
            identity,
            tsv,
        })
    }
}

/// Where a hit was found.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Ref,
    Path,
    Commit,
    Blob,
}

impl Kind {
    const ALL: [Kind; 4] = [Kind::Ref, Kind::Path, Kind::Commit, Kind::Blob];

    fn name(self) -> &'static str {
        match self {
            Kind::Ref => "ref",
            Kind::Path => "path",
            Kind::Commit => "commit",
            Kind::Blob => "blob",
        }
    }
}

/// One hit: where, and the detail its row carries.
struct Row {
    kind: Kind,
    detail: Vec<u8>,
}

/// The hits of one label, in the order found.
struct Label {
    name: String,
    rows: Vec<Row>,
}

/// Every label's hits.
#[derive(Default)]
struct Found {
    labels: Vec<Label>,
}

impl Found {
    fn add(&mut self, name: &str, kind: Kind, detail: &[u8]) {
        let row = Row {
            kind,
            detail: detail.to_vec(),
        };
        match self.labels.iter_mut().find(|l| l.name == name) {
            Some(l) => l.rows.push(row),
            None => self.labels.push(Label {
                name: name.to_string(),
                rows: vec![row],
            }),
        }
    }

    fn rows(&self, name: &str) -> &[Row] {
        self.labels
            .iter()
            .find(|l| l.name == name)
            .map_or(&[][..], |l| l.rows.as_slice())
    }
}

/// An object `rev-list --objects` names, and the path it names it at.
struct Object {
    oid: Vec<u8>,
    path: Vec<u8>,
}

/// The bytes of `git --git-dir DIR ARGS...`, or what it said on failing.
fn git(dir: &Path, args: &[&str]) -> io::Result<Vec<u8>> {
    let o = Command::new("git")
        .arg("--git-dir")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .output()?;
    if !o.status.success() {
        return Err(io::Error::other(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr).trim()
        )));
    }
    Ok(o.stdout)
}

/// The lines of `text`, cut at `\n`, `\r\n` and `\r`.
fn lines(text: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let (mut start, mut at) = (0, 0);
    while at < text.len() {
        match text[at] {
            b'\n' => {
                out.push(&text[start..at]);
                at += 1;
                start = at;
            }
            b'\r' => {
                out.push(&text[start..at]);
                at += 1;
                if text.get(at) == Some(&b'\n') {
                    at += 1;
                }
                start = at;
            }
            _ => at += 1,
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// The objects in `rev-list`'s order, each once, at the first path it has.
fn objects(dir: &Path, range: Option<&str>) -> io::Result<Vec<Object>> {
    let listed = git(dir, &["rev-list", "--objects", range.unwrap_or("--all")])?;
    let mut out: Vec<Object> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in lines(&listed) {
        let (oid, path) = match line.iter().position(|&b| b == b' ') {
            Some(at) => (&line[..at], &line[at + 1..]),
            None => (line, &b""[..]),
        };
        if seen.insert(oid.to_vec()) {
            out.push(Object {
                oid: oid.to_vec(),
                path: path.to_vec(),
            });
        }
    }
    Ok(out)
}

/// `git cat-file --batch`, asked one object at a time.
struct Batch {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}

impl Batch {
    fn start(dir: &Path) -> io::Result<Batch> {
        let mut child = Command::new("git")
            .arg("--git-dir")
            .arg(dir)
            .args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        let input = child.stdin.take();
        let output = child
            .stdout
            .take()
            .map(BufReader::new)
            .ok_or_else(|| io::Error::other("git cat-file: no stdout"))?;
        Ok(Batch {
            child,
            input,
            output,
        })
    }

    /// The object's type and its bytes.
    fn object(&mut self, oid: &[u8]) -> io::Result<(Vec<u8>, Vec<u8>)> {
        let input = self
            .input
            .as_mut()
            .ok_or_else(|| io::Error::other("git cat-file: no stdin"))?;
        input.write_all(oid)?;
        input.write_all(b"\n")?;
        input.flush()?;
        let mut head = Vec::new();
        self.output.read_until(b'\n', &mut head)?;
        let words: Vec<&[u8]> = head.trim_ascii_end().split(|&b| b == b' ').collect();
        let bad = || {
            io::Error::other(format!(
                "git cat-file: {}",
                String::from_utf8_lossy(head.trim_ascii_end())
            ))
        };
        let [_, kind, size] = words.as_slice() else {
            return Err(bad());
        };
        let size: usize = std::str::from_utf8(size)
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or_else(bad)?;
        let mut data = vec![0; size];
        self.output.read_exact(&mut data)?;
        let mut newline = [0; 1];
        self.output.read_exact(&mut newline)?;
        Ok((kind.to_vec(), data))
    }

    fn finish(mut self) -> io::Result<()> {
        drop(self.input.take());
        self.child.wait()?;
        Ok(())
    }
}

/// The gzip member a blob holds, when it inflates and its trailer claims
/// no more than deflate can give.
fn gunzip(data: &[u8]) -> Option<Vec<u8>> {
    let tail = data.len().checked_sub(4)?;
    let claimed = <[u8; 4]>::try_from(&data[tail..]).ok()?;
    let claimed = usize::try_from(u32::from_le_bytes(claimed)).ok()?;
    if claimed > data.len().saturating_mul(MOST_INFLATED) {
        return None;
    }
    tarball::gunzip(data).ok()
}

/// What a commit or blob is searched as.
fn views(data: &[u8]) -> Vec<Cow<'_, [u8]>> {
    let mut out = vec![Cow::Borrowed(data)];
    if data.starts_with(&[0x1f, 0x8b]) {
        if let Some(inflated) = gunzip(data) {
            out.push(Cow::Owned(inflated));
        }
    }
    let mut stripped: Vec<Cow<'_, [u8]>> = Vec::new();
    for view in &out {
        let bytes: &[u8] = view;
        if bytes.contains(&0x1b) || bytes[..bytes.len().min(NUL_REACH)].contains(&0) {
            stripped.push(Cow::Owned(ESC.replace_all(bytes, &b""[..]).into_owned()));
        }
    }
    out.extend(stripped);
    out
}

/// The labels found in any view of `data`, each once.
fn labels<'p>(pats: &'p [Pattern], joined: &RegexSet, data: &[u8]) -> Vec<&'p str> {
    let mut hit: Vec<&str> = Vec::new();
    for view in views(data) {
        if !joined.is_match(&view) {
            continue;
        }
        for p in pats {
            if !hit.contains(&p.label()) && p.finds(&view) {
                hit.push(p.label());
            }
        }
    }
    hit
}

/// The name and address of each author and committer line of a commit's
/// headers: the line after its first space, less its last two words.
fn signers(commit: &[u8]) -> Vec<&[u8]> {
    let head = commit
        .windows(2)
        .position(|w| w == b"\n\n")
        .map_or(commit, |at| &commit[..at]);
    head.split(|&b| b == b'\n')
        .filter(|l| l.starts_with(b"author ") || l.starts_with(b"committer "))
        .map(|l| {
            let after = l.splitn(2, |&b| b == b' ').nth(1).unwrap_or_default();
            after.rsplitn(3, |&b| b == b' ').last().unwrap_or(after)
        })
        .collect()
}

/// The first 12 digits of an object's id.
fn short(oid: &[u8]) -> &[u8] {
    &oid[..oid.len().min(12)]
}

/// `cargo xtask forbidden --git ...`, the words after `--git`.
pub fn run(args: &[&str], out: &mut impl Write, err: &mut impl Write) -> io::Result<Exit> {
    let Some(a) = Args::parse(args) else {
        writeln!(err, "{USAGE}")?;
        return Ok(Exit(1));
    };
    let (pats, joined) = match armed(&a.list, &a.ctl, err)? {
        Ok(armed) => armed,
        Err(refused) => return Ok(refused.exit()),
    };
    let mut found = Found::default();

    let refs = git(&a.dir, &["for-each-ref", "--format=%(refname)"])?;
    let refs: Vec<&[u8]> = refs
        .split(u8::is_ascii_whitespace)
        .filter(|r| !r.is_empty())
        .collect();
    for r in &refs {
        for p in &pats {
            if p.finds(r) {
                found.add(p.label(), Kind::Ref, r);
            }
        }
    }

    let objects = objects(&a.dir, a.range.as_deref())?;
    for o in objects.iter().filter(|o| !o.path.is_empty()) {
        for p in &pats {
            if p.finds(&o.path) {
                found.add(p.label(), Kind::Path, &o.path);
            }
        }
    }

    let (mut commits, mut blobs, mut strange) = (0, 0, 0);
    let mut batch = Batch::start(&a.dir)?;
    for o in &objects {
        let (kind, data) = batch.object(&o.oid)?;
        match &kind[..] {
            b"commit" => {
                commits += 1;
                if let Some(identity) = &a.identity {
                    strange += signers(&data)
                        .iter()
                        .filter(|&&w| w != identity.as_slice())
                        .count();
                }
                for name in labels(&pats, &joined, &data) {
                    found.add(name, Kind::Commit, short(&o.oid));
                }
            }
            b"blob" => {
                blobs += 1;
                for name in labels(&pats, &joined, &data) {
                    let detail = [short(&o.oid), b" ".as_slice(), o.path.as_slice()].concat();
                    found.add(name, Kind::Blob, &detail);
                }
            }
            _ => {}
        }
    }
    batch.finish()?;

    let mut hits = 0;
    let mut tsv = Vec::new();
    for p in &pats {
        let rows = found.rows(p.label());
        if rows.is_empty() {
            continue;
        }
        hits += rows.len();
        let counts: Vec<String> = Kind::ALL
            .iter()
            .map(|&k| (k, rows.iter().filter(|r| r.kind == k).count()))
            .filter(|&(_, n)| n > 0)
            .map(|(k, n)| format!("{} {n}", k.name()))
            .collect();
        writeln!(
            err,
            "readme-forbidden: {}: {} hits ({})",
            p.label(),
            rows.len(),
            counts.join(", ")
        )?;
        for r in rows {
            writeln!(
                tsv,
                "{}\t{}\t{}",
                p.label(),
                r.kind.name(),
                String::from_utf8_lossy(&r.detail)
            )?;
        }
    }
    if strange > 0 {
        writeln!(
            err,
            "readme-forbidden: {strange} author or committer lines are not the identity"
        )?;
    }
    if let Some(path) = &a.tsv {
        fs::write(path, &tsv)
            .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))?;
    }
    writeln!(
        out,
        "readme-forbidden: {} patterns, each found in its control; \
         {} refs, {commits} commits, {blobs} blobs, {hits} hits",
        pats.len(),
        refs.len()
    )?;
    Ok(Exit(if hits > 0 || strange > 0 { 1 } else { 0 }))
}
