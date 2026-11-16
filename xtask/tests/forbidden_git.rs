//! `cargo xtask forbidden --git`: a repository's ref names, paths, commits
//! and blobs searched with the list, and the rows its `--tsv` writes. Every
//! list, name and repository here is invented.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use xtask::features_since::Exit;
use xtask::forbidden;
use xtask::tarball::crc32;

static NEXT: AtomicUsize = AtomicUsize::new(0);

const LIST: &[u8] = b"# made up\nuser::(?i:qzuser)\nhost::qzhost\\.local\n";
const CONTROLS: &[u8] = b"user\tQZUSER\nhost\tat qzhost.local\n";

const ANN: (&str, &str) = ("Ann Example", "ann@example.invalid");
const IDENTITY: &str = "Ann Example <ann@example.invalid>";

/// What a run printed, the rows it wrote, and how it exited.
#[derive(PartialEq, Eq, Debug)]
struct Ran {
    exit: Exit,
    out: String,
    err: String,
    tsv: Option<String>,
}

/// A scratch folder holding a bare repository, removed on drop.
struct Repo(PathBuf);

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// `git` with no configuration but the identity and the date given.
fn git(who: (&str, &str), args: &[&str], input: &[u8]) -> String {
    let (name, email) = who;
    let mut child = Command::new("git")
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", name)
        .env("GIT_AUTHOR_EMAIL", email)
        .env("GIT_COMMITTER_NAME", name)
        .env("GIT_COMMITTER_EMAIL", email)
        .env("GIT_AUTHOR_DATE", "1700000000 +0000")
        .env("GIT_COMMITTER_DATE", "1700000000 +0000")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("git");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("input");
    let o = child.wait_with_output().expect("git");
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8(o.stdout)
        .expect("utf-8")
        .trim()
        .to_string()
}

/// A tree's line for a file.
fn file(name: &str, oid: &str) -> String {
    format!("100644 blob {oid}\t{name}\n")
}

/// A tree's line for a folder.
fn folder(name: &str, oid: &str) -> String {
    format!("040000 tree {oid}\t{name}\n")
}

impl Repo {
    fn new() -> Repo {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let d =
            std::env::temp_dir().join(format!("xtask-forbidden-git-{}-{n}", std::process::id()));
        fs::create_dir_all(&d).expect("scratch");
        let r = Repo(d);
        let at = r.git_dir();
        git(ANN, &["init", "--quiet", "--bare", at.as_str()], b"");
        r
    }

    fn git_dir(&self) -> String {
        self.0.join("repo.git").display().to_string()
    }

    /// `git --git-dir REPO ARGS`, by `who`.
    fn git_as(&self, who: (&str, &str), args: &[&str], input: &[u8]) -> String {
        let at = self.git_dir();
        let words: Vec<&str> = ["--git-dir", at.as_str()]
            .into_iter()
            .chain(args.iter().copied())
            .collect();
        git(who, &words, input)
    }

    fn blob(&self, bytes: &[u8]) -> String {
        self.git_as(ANN, &["hash-object", "-w", "--stdin"], bytes)
    }

    fn tree(&self, lines: &[String]) -> String {
        self.git_as(ANN, &["mktree"], lines.concat().as_bytes())
    }

    fn commit_as(&self, who: (&str, &str), tree: &str, parents: &[&str], msg: &str) -> String {
        let mut args = vec!["commit-tree", tree];
        for p in parents {
            args.extend(["-p", *p]);
        }
        self.git_as(who, &args, msg.as_bytes())
    }

    fn commit(&self, tree: &str, parents: &[&str], msg: &str) -> String {
        self.commit_as(ANN, tree, parents, msg)
    }

    fn set_ref(&self, name: &str, oid: &str) {
        self.git_as(ANN, &["update-ref", name, oid], b"");
    }

    /// One commit of `files` at the top of its tree, on `refs/heads/main`.
    fn main(&self, files: &[(&str, &[u8])], msg: &str) -> String {
        let lines: Vec<String> = files
            .iter()
            .map(|(name, bytes)| file(name, &self.blob(bytes)))
            .collect();
        let c = self.commit(&self.tree(&lines), &[], msg);
        self.set_ref("refs/heads/main", &c);
        c
    }

    /// The search of this repository with `list`, its controls and `extra`
    /// words, its rows written beside it.
    fn scan(&self, list: &[u8], controls: &[u8], extra: &[&str]) -> Ran {
        self.scan_at(&self.git_dir(), list, controls, extra)
    }

    /// The same search of the repository at `gd`.
    fn scan_at(&self, gd: &str, list: &[u8], controls: &[u8], extra: &[&str]) -> Ran {
        let write = |name: &str, bytes: &[u8]| {
            let p = self.0.join(name);
            fs::write(&p, bytes).expect("write");
            p.display().to_string()
        };
        let (l, c) = (write("LIST", list), write("CONTROLS", controls));
        let tsv = self.0.join("hits.tsv");
        let t = tsv.display().to_string();
        let mut args = vec!["--git", gd, l.as_str(), c.as_str(), "--tsv", t.as_str()];
        args.extend(extra);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let exit = forbidden::run(&args, &mut out, &mut err).expect("no error");
        Ran {
            exit,
            out: String::from_utf8(out).expect("utf-8"),
            err: String::from_utf8(err).expect("utf-8"),
            tsv: fs::read_to_string(&tsv).ok(),
        }
    }
}

/// The summary line, for the list's two patterns.
fn summary(counts: &str) -> String {
    format!("readme-forbidden: 2 patterns, each found in its control; {counts}\n")
}

fn short(oid: &str) -> &str {
    &oid[..12]
}

/// A gzip member of two stored deflate blocks, so the two halves never
/// stand side by side in its bytes.
fn gzip(first: &[u8], second: &[u8]) -> Vec<u8> {
    let mut gz = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff];
    for (header, part) in [(0, first), (1, second)] {
        gz.push(header);
        let len = u16::try_from(part.len()).expect("short");
        gz.extend(len.to_le_bytes());
        gz.extend((!len).to_le_bytes());
        gz.extend(part);
    }
    let whole = [first, second].concat();
    gz.extend(crc32(&whole).0.to_le_bytes());
    gz.extend(u32::try_from(whole.len()).expect("short").to_le_bytes());
    gz
}

/// **A clean repository passes**: the summary, no label, an empty TSV, 0.
#[test]
fn a_clean_repository_passes() {
    let r = Repo::new();
    r.main(&[("notes.txt", b"hello\n")], "first\n");
    assert_eq!(
        r.scan(LIST, CONTROLS, &[]),
        Ran {
            exit: Exit(0),
            out: summary("1 refs, 1 commits, 1 blobs, 0 hits"),
            err: String::new(),
            tsv: Some(String::new()),
        }
    );
}

/// **A blob's hit names its label and counts its kind, never what matched**;
/// its row is the blob's short id and its path.
#[test]
fn a_hit_in_a_blob_is_its_id_and_path() {
    let r = Repo::new();
    r.main(&[("notes.txt", b"x qzuser y\n")], "first\n");
    let b = r.blob(b"x qzuser y\n");
    let ran = r.scan(LIST, CONTROLS, &[]);
    assert_eq!(
        ran,
        Ran {
            exit: Exit(1),
            out: summary("1 refs, 1 commits, 1 blobs, 1 hits"),
            err: "readme-forbidden: user: 1 hits (blob 1)\n".to_string(),
            tsv: Some(format!("user\tblob\t{} notes.txt\n", short(&b))),
        }
    );
    assert!(!ran.out.contains("qzuser") && !ran.err.contains("qzuser"));
}

/// **The escapes come out only of a view holding an ESC, or a NUL in its
/// first 4096 bytes**: a BEL between the halves of a word stays in a blob
/// that holds neither.
#[test]
fn escapes_come_out_of_a_view_with_an_esc_or_an_early_nul() {
    let pad = |n: usize| vec![b'a'; n];
    let cases: [(Vec<u8>, Exit); 5] = [
        (b"qz\x1b[1muser".to_vec(), Exit(1)),
        (b"qz\x07user".to_vec(), Exit(0)),
        (b"qz\x07user\x00".to_vec(), Exit(1)),
        // The NUL at offset 4095, then at 4096.
        (
            [&b"qz\x07user"[..], &pad(4088)[..], &b"\x00"[..]].concat(),
            Exit(1),
        ),
        (
            [&b"qz\x07user"[..], &pad(4089)[..], &b"\x00"[..]].concat(),
            Exit(0),
        ),
    ];
    for (bytes, exit) in cases {
        let r = Repo::new();
        r.main(&[("f", &bytes)], "first\n");
        assert_eq!(
            r.scan(LIST, CONTROLS, &[]).exit,
            exit,
            "{:?}",
            &bytes[..bytes.len().min(16)]
        );
    }
}

/// **A gzip blob is searched inflated**, when it inflates: a CRC that does
/// not match, or a length past what deflate can give, is no view.
#[test]
fn a_gzip_blob_is_searched_inflated() {
    let gz = gzip(b"qz", b"user");
    let r = Repo::new();
    r.main(&[("log.gz", &gz)], "first\n");
    let b = r.blob(&gz);
    assert_eq!(
        r.scan(LIST, CONTROLS, &[]).tsv,
        Some(format!("user\tblob\t{} log.gz\n", short(&b)))
    );

    let at = gz.len() - 8;
    let mut bad_crc = gz.clone();
    bad_crc[at] ^= 1;
    let mut too_long = gz;
    too_long[at + 4..].copy_from_slice(&u32::MAX.to_le_bytes());
    for bytes in [bad_crc, too_long] {
        let r = Repo::new();
        r.main(&[("log.gz", &bytes)], "first\n");
        assert_eq!(r.scan(LIST, CONTROLS, &[]).exit, Exit(0));
    }
}

/// **A path is searched once for each object at it**: the folder and the
/// file under it, each a row whose detail is the path.
#[test]
fn a_path_is_a_hit_of_its_own() {
    let r = Repo::new();
    let inner = r.tree(&[file("notes.txt", &r.blob(b"clean\n"))]);
    let c = r.commit(&r.tree(&[folder("qzuser", &inner)]), &[], "first\n");
    r.set_ref("refs/heads/main", &c);
    let ran = r.scan(LIST, CONTROLS, &[]);
    assert_eq!(
        ran,
        Ran {
            exit: Exit(1),
            out: summary("1 refs, 1 commits, 1 blobs, 2 hits"),
            err: "readme-forbidden: user: 2 hits (path 2)\n".to_string(),
            tsv: Some("user\tpath\tqzuser\nuser\tpath\tqzuser/notes.txt\n".to_string()),
        }
    );
    assert!(!ran.err.contains("qzuser"));
}

/// **A commit is searched whole**: its message, and its headers, where a
/// label found in both the author and the committer is one row.
#[test]
fn a_commit_is_searched_with_its_headers() {
    let r = Repo::new();
    let tree = r.tree(&[file("f", &r.blob(b"clean\n"))]);
    let by_message = r.commit(&tree, &[], "fix for QZUSER\n");
    let by_header = r.commit_as(("Qzuser", "q@example.invalid"), &tree, &[], "plain\n");
    r.set_ref("refs/heads/main", &by_message);
    r.set_ref("refs/heads/other", &by_header);
    let ran = r.scan(LIST, CONTROLS, &[]);
    assert_eq!(ran.exit, Exit(1));
    assert_eq!(ran.err, "readme-forbidden: user: 2 hits (commit 2)\n");
    let mut rows: Vec<&str> = ran.tsv.as_deref().expect("tsv").lines().collect();
    rows.sort_unstable();
    let mut want = [
        format!("user\tcommit\t{}", short(&by_message)),
        format!("user\tcommit\t{}", short(&by_header)),
    ];
    want.sort_unstable();
    assert_eq!(rows, want);
}

/// **The rows come in the list's order, and within a label as found**:
/// refs, then paths, then objects.
#[test]
fn rows_come_by_label_then_refs_paths_and_objects() {
    let r = Repo::new();
    let c = r.main(&[("qzuser.txt", b"qzuser\n")], "first\n");
    r.set_ref("refs/tags/qzhost.local", &c);
    let b = r.blob(b"qzuser\n");
    assert_eq!(
        r.scan(LIST, CONTROLS, &[]),
        Ran {
            exit: Exit(1),
            out: summary("2 refs, 1 commits, 1 blobs, 3 hits"),
            err: "readme-forbidden: user: 2 hits (path 1, blob 1)\n\
                  readme-forbidden: host: 1 hits (ref 1)\n"
                .to_string(),
            tsv: Some(format!(
                "user\tpath\tqzuser.txt\n\
                 user\tblob\t{} qzuser.txt\n\
                 host\tref\trefs/tags/qzhost.local\n",
                short(&b)
            )),
        }
    );
}

/// **An object at two paths is one object**, its row at the first path.
#[test]
fn an_object_at_two_paths_is_searched_once() {
    let r = Repo::new();
    r.main(&[("a.txt", b"qzuser\n"), ("b.txt", b"qzuser\n")], "first\n");
    let b = r.blob(b"qzuser\n");
    let ran = r.scan(LIST, CONTROLS, &[]);
    assert_eq!(ran.out, summary("1 refs, 1 commits, 1 blobs, 1 hits"));
    assert_eq!(ran.tsv, Some(format!("user\tblob\t{} a.txt\n", short(&b))));
}

/// **`--range A..B` searches what B adds to A**, and the ref names still.
#[test]
fn a_range_searches_only_what_it_adds() {
    let r = Repo::new();
    let old = r.blob(b"qzuser\n");
    let first = r.commit(&r.tree(&[file("a.txt", &old)]), &[], "first\n");
    let tree = r.tree(&[file("a.txt", &old), file("b.txt", &r.blob(b"clean\n"))]);
    let second = r.commit(&tree, &[first.as_str()], "second\n");
    r.set_ref("refs/heads/main", &second);

    let range = format!("{first}..{second}");
    let ran = r.scan(LIST, CONTROLS, &["--range", &range]);
    assert_eq!(ran.exit, Exit(0));
    assert_eq!(ran.out, summary("1 refs, 1 commits, 1 blobs, 0 hits"));

    let all = r.scan(LIST, CONTROLS, &[]);
    assert_eq!(all.exit, Exit(1));
    assert_eq!(all.out, summary("1 refs, 2 commits, 2 blobs, 1 hits"));
}

/// **`--identity` holds every author and committer to it**, and says how
/// many lines are not, never who they are.
#[test]
fn every_author_and_committer_is_the_identity() {
    let r = Repo::new();
    r.main(&[("f", b"clean\n")], "first\n");
    let ran = r.scan(LIST, CONTROLS, &["--identity", IDENTITY]);
    assert_eq!(ran.exit, Exit(0));
    assert_eq!(ran.err, "");

    let r = Repo::new();
    let tree = r.tree(&[file("f", &r.blob(b"clean\n"))]);
    let c = r.commit_as(
        ("Bob Example", "bob@example.invalid"),
        &tree,
        &[],
        "first\n",
    );
    r.set_ref("refs/heads/main", &c);
    let ran = r.scan(LIST, CONTROLS, &["--identity", IDENTITY]);
    assert_eq!(
        ran,
        Ran {
            exit: Exit(1),
            out: summary("1 refs, 1 commits, 1 blobs, 0 hits"),
            err: "readme-forbidden: 2 author or committer lines are not the identity\n".to_string(),
            tsv: Some(String::new()),
        }
    );
}

/// **The controls come first**: a label with none searches nothing, so a
/// folder that is no repository is never opened, and no TSV is written.
#[test]
fn a_label_with_no_control_searches_no_repository() {
    let r = Repo::new();
    let missing = r.0.join("missing.git").display().to_string();
    let ran = r.scan_at(
        &missing,
        b"user::qzuser\npass::qzpass\n",
        b"user\tqzuser\n",
        &[],
    );
    assert_eq!(
        ran,
        Ran {
            exit: Exit(2),
            out: String::new(),
            err: "readme-forbidden: pass: no control\n".to_string(),
            tsv: None,
        }
    );
}

/// **A folder that is no repository is an error**, which `main` turns into
/// exit 1.
#[test]
fn a_folder_that_is_no_repository_is_an_error() {
    let r = Repo::new();
    let list = r.0.join("LIST");
    let ctl = r.0.join("CONTROLS");
    fs::write(&list, LIST).expect("write");
    fs::write(&ctl, CONTROLS).expect("write");
    let missing = r.0.join("missing.git").display().to_string();
    let (l, c) = (list.display().to_string(), ctl.display().to_string());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert!(forbidden::run(&["--git", &missing, &l, &c], &mut out, &mut err).is_err());
}

/// **The usage**: the words after `--git` that are not three, or an option
/// with no value.
#[test]
fn words_that_are_not_three_are_the_usage() {
    let usage = "usage: cargo xtask forbidden --git GIT_DIR PATTERNS CONTROLS \
                 [--range A..B] [--identity 'NAME <EMAIL>'] [--tsv OUT]\n";
    let cases: [&[&str]; 4] = [
        &["--git"],
        &["--git", "d", "l"],
        &["--git", "d", "l", "c", "x"],
        &["--git", "d", "l", "c", "--range"],
    ];
    for args in cases {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let exit = forbidden::run(args, &mut out, &mut err).expect("no error");
        assert_eq!(
            (exit, String::from_utf8(err).expect("utf-8")),
            (Exit(1), usage.to_string()),
            "{args:?}"
        );
    }
}
