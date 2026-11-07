//! **Nothing the session is given names this machine.** The rule is the
//! film tool's (`machine_names` and `named` in `baud-app/src/film.rs`,
//! which this example cannot call, the package having no library): the
//! user's name, the host's, the home and temporary directories and the
//! roots such paths start with, matched as fixed strings. Here it adds the
//! computer's two names, which `scripts/readme-film.sh` hands the film
//! tool, since this example reads them itself.

use std::path::Path;

/// **The machine's names**: `USER`, `LOGNAME`, `HOME`, `TMPDIR` and the
/// temporary directory, the roots `/Users/`, `/home/`, `/private/` and
/// `/var/folders`, and each of `hosts` -- the host name, the computer's
/// names -- with its first label. Names under three bytes are dropped, as
/// the film tool drops them: too short to tell from text.
pub fn machine_names(env: &[(String, String)], temp: &str, hosts: &[String]) -> Vec<String> {
    let mut names: Vec<String> = env
        .iter()
        .filter(|(k, _)| ["USER", "LOGNAME", "HOME", "TMPDIR"].contains(&k.as_str()))
        .map(|(_, v)| v.clone())
        .chain(std::iter::once(temp.to_string()))
        .map(|v| v.trim_end_matches('/').to_string())
        .collect();
    names.extend(
        ["/Users/", "/home/", "/private/", "/var/folders"]
            .iter()
            .map(|s| s.to_string()),
    );
    for h in hosts {
        let h = h.trim();
        names.push(h.to_string());
        names.extend(h.split('.').next().map(str::to_string));
    }
    names.retain(|n| n.len() >= 3);
    names.sort();
    names.dedup();
    names
}

/// **Every spelling this process can tell of a directory**: as given and as
/// the file system resolves it, neither with a slash at its end. A program
/// can print either: under `/var/folders` as its `TMPDIR` says, or
/// under `/private/var/folders` as `getcwd` does.
pub fn spellings(dir: &Path) -> Vec<String> {
    let bare = |p: &Path| p.to_string_lossy().trim_end_matches('/').to_string();
    let mut all = vec![bare(dir)];
    if let Ok(real) = std::fs::canonicalize(dir) {
        all.push(bare(&real));
    }
    all.dedup();
    all
}

/// **The temporary directory and the scratch root, each in every spelling**:
/// `root` is in `temp`, reached through `temp`'s spelling or its own.
pub fn scratch_names(temp: &Path, root: &Path) -> Vec<String> {
    let mut all = spellings(temp);
    all.extend(spellings(root));
    if let Some(leaf) = root.file_name() {
        all.extend(spellings(&temp.join(leaf)));
    }
    all
}

/// **`names` and the scratch root's own name, for what the session printed
/// alone.** A prompt truncates a path to its last folders, so a path holding
/// neither a root of the machine's nor the temporary directory still shows
/// `baud-demo-18837-0`. Not one of [`machine_names`]: the scratch directory
/// is the one path the environment may carry ([`env_named`]).
pub fn with_scratch_leaf(mut names: Vec<String>, root: &Path) -> Vec<String> {
    names.extend(root.file_name().map(|l| l.to_string_lossy().into_owned()));
    names
}

/// **Which of `names` the bytes hold**, as fixed strings.
pub fn named(bytes: &[u8], names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter(|n| bytes.windows(n.len()).any(|w| w == n.as_bytes()))
        .cloned()
        .collect()
}

/// `bytes` with every `cut` taken out.
fn without(bytes: &[u8], cut: &[u8]) -> Vec<u8> {
    if cut.is_empty() {
        return bytes.to_vec();
    }
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(cut) {
            i += cut.len();
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

/// **The repository's own address on its forge**, `owner/name`, from the
/// package's `repository` URL: `https://github.com/baud-terminal/baud` is
/// `baud-terminal/baud`. It is public, and the README the session prints
/// carries it, so [`named_outside`] takes it out before looking: its
/// owner's handle can hold the user's name.
pub fn public_path(repository: &str) -> String {
    let rest = repository.split_once("://").map_or(repository, |(_, r)| r);
    rest.split_once('/')
        .map_or("", |(_, path)| path)
        .trim_end_matches('/')
        .to_string()
}

/// **Which of `names` the bytes hold outside each of `public`**: every
/// occurrence of each is taken out first.
pub fn named_outside(bytes: &[u8], names: &[String], public: &[&str]) -> Vec<String> {
    let mut b = bytes.to_vec();
    for p in public {
        b = without(&b, p.as_bytes());
    }
    named(&b, names)
}

/// **Which of `names` an environment holds, the scratch directory aside**:
/// every variable that must name a file -- `HOME`, `ZDOTDIR`, `TMPDIR`,
/// `BAUD_PTYLOG` -- names one under `root`, a fresh directory in the
/// system's temporary one, so `root` itself is the one path of the machine
/// the session carries. Each occurrence of it is taken out before the
/// names are looked for, and `root`'s own last component must name
/// nothing. Returns each variable and the names it holds.
pub fn env_named(
    env: &[(String, String)],
    root: &str,
    names: &[String],
) -> Vec<(String, Vec<String>)> {
    let leaf = root.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let mut found = Vec::new();
    let own = named(leaf.as_bytes(), names);
    if !own.is_empty() {
        found.push(("the scratch directory's name".to_string(), own));
    }
    for (k, v) in env {
        let hit = named_outside(v.as_bytes(), names, &[root]);
        if !hit.is_empty() {
            found.push((k.clone(), hit));
        }
    }
    found
}

#[cfg(test)]
pub mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// A made-up machine: user `qzuser` at home in `/Users/me`, host
    /// `qzhost.local`, computer `Qz Mac`, names no text of the repository
    /// holds.
    pub fn qz() -> Vec<String> {
        machine_names(
            &env(&[
                ("USER", "qzuser"),
                ("HOME", "/Users/me"),
                ("TMPDIR", "/private/qz/T/"),
                ("PATH", "/usr/bin"),
            ]),
            "/private/qz/T/",
            &["qzhost.local".to_string(), "Qz Mac".to_string()],
        )
    }

    /// **The names are the user, the home, the temporary directory, the
    /// roots and the hosts**, and each, written, is found: the probe can
    /// fire. PATH is not a name.
    #[test]
    fn the_names_are_the_machines_and_each_is_found() {
        let n = qz();
        for want in [
            "qzuser",
            "/Users/me",
            "/private/qz/T",
            "/var/folders",
            "/private/",
            "qzhost.local",
            "qzhost",
            "Qz Mac",
        ] {
            assert!(n.contains(&want.to_string()), "{want} in {n:?}");
        }
        assert!(!n.contains(&"/usr/bin".to_string()));
        for name in &n {
            assert!(named(name.as_bytes(), &n).contains(name), "{name}");
        }
    }

    /// **The repository's address is taken out, and nothing else**: a
    /// README line naming `qzuser/baud` on the forge passes when that is
    /// the repository, and the same user name anywhere else is still
    /// found. Perturbation: take nothing out and the README line names
    /// the user.
    #[test]
    fn the_repositorys_address_is_public_and_nothing_else_is() {
        assert_eq!(
            public_path("https://github.com/baud-terminal/baud"),
            "baud-terminal/baud"
        );
        assert_eq!(public_path("https://example.org/a/b/"), "a/b");
        let n = qz();
        let readme = b"nix run github:qzuser/baud\ngit clone https://github.com/qzuser/baud";
        assert_eq!(
            named_outside(readme, &n, &["qzuser/baud"]),
            Vec::<String>::new()
        );
        assert!(!named(readme, &n).is_empty(), "the control: it is there");
        let home = b"see /Users/me/qzuser/baud for the file";
        assert!(!named_outside(home, &n, &["qzuser/baud"]).is_empty());
    }

    /// **The environment names nothing but the scratch directory**: every
    /// path under the root passes, a variable naming the user's home or
    /// the host fails, and a root whose own name holds the user's fails.
    /// Perturbation: skip taking the root out and every path-valued
    /// variable fails, as `/private/` is a root of the machine's.
    #[test]
    fn the_environment_names_nothing_but_the_scratch_directory() {
        let n = qz();
        let root = "/private/qz/T/baud-demo-4242";
        let clean = env(&[
            ("HOME", "/private/qz/T/baud-demo-4242/home"),
            ("TMPDIR", "/private/qz/T/baud-demo-4242/tmp"),
            ("PATH", "/nix/store/x-btop-1.4.7/bin:/usr/bin:/bin"),
            ("LANG", "en_US.UTF-8"),
        ]);
        assert_eq!(env_named(&clean, root, &n), []);
        let leak = env(&[
            ("HOME", "/private/qz/T/baud-demo-4242/home"),
            ("EDITOR", "/Users/me/bin/vi"),
            ("HOSTNAME", "qzhost"),
        ]);
        let found = env_named(&leak, root, &n);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].0, "EDITOR");
        assert_eq!(found[1].0, "HOSTNAME");
        let named_root = "/private/qz/T/qzuser-demo";
        assert_eq!(env_named(&[], named_root, &n).len(), 1);
    }

    /// **The search names both spellings of a temporary directory reached
    /// through a symbolic link, and of the scratch root in it**: a program
    /// can print the one its `TMPDIR` says or the one `getcwd` does.
    /// Perturbation: keep only the spelling given, and the resolved one is
    /// not a name.
    #[test]
    fn the_search_names_both_spellings_of_a_linked_temporary_directory() {
        let base = std::env::temp_dir().join(format!("demo-spellings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let real = base.join("real");
        std::fs::create_dir_all(&real).unwrap();
        let link = base.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let resolved = real.canonicalize().unwrap();
        let root = crate::scratch::fresh_root(&link).unwrap();
        let leaf = root.file_name().unwrap().to_owned();
        let n = scratch_names(&link, &root);
        for want in [
            link.clone(),
            link.join(&leaf),
            resolved.clone(),
            resolved.join(&leaf),
        ] {
            let want = want.to_string_lossy().into_owned();
            assert!(n.contains(&want), "{want} in {n:?}");
            let printed = format!("\x1b[0m{want}/home/baud \u{276F} ");
            assert!(named(printed.as_bytes(), &n).contains(&want), "{want}");
        }
        std::fs::remove_dir_all(&base).unwrap();
        let slash = spellings(Path::new("/no/such/dir/"));
        assert_eq!(slash, ["/no/such/dir"], "as given, its slash off");
    }

    /// **A truncated path is found by the scratch directory's own name, in
    /// what the session printed alone**: the prompt of the take that showed
    /// the path holds no root once `/home/` is past its cut. The control: the
    /// machine's names alone do not find it, and they must not, as the
    /// environment names the directory. Perturbation: add nothing, and the
    /// truncated path passes.
    #[test]
    fn a_truncated_path_is_found_by_the_scratch_directory_s_name() {
        let root = Path::new("/private/qz/T/baud-demo-18837-0");
        let printed = "\u{22EF}/baud-demo-18837-0/baud \u{276F} ".as_bytes();
        assert_eq!(named(printed, &qz()), Vec::<String>::new(), "the control");
        assert_eq!(
            named(printed, &with_scratch_leaf(qz(), root)),
            ["baud-demo-18837-0"]
        );
        let clean = "~/baud \u{276F} ".as_bytes();
        assert_eq!(
            named(clean, &with_scratch_leaf(qz(), root)),
            Vec::<String>::new()
        );
    }

    /// **The search for what must never be filmed fires, and refuses to
    /// search without its controls** (`xtask::forbidden`, which
    /// `--record` and `--headless` run over the session's bytes): a list of
    /// the made-up machine's names, a clean file passes (0), the user's
    /// name coloured letter by letter is found once the escapes are out
    /// (1), and a list with a label that has no control, or a control its
    /// pattern does not find, searches nothing (2).
    #[test]
    fn the_forbidden_search_fires_and_needs_its_controls() {
        let dir = std::env::temp_dir().join(format!("demo-forbidden-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let w = |name: &str, bytes: &[u8]| {
            let p = dir.join(name);
            std::fs::write(&p, bytes).unwrap();
            p
        };
        let list = w(
            "list",
            b"# made up\nuser::(?i:qzuser)\nhost::qzhost\\.local\n",
        );
        let controls = w("controls", b"user\tQZUSER\nhost\tat qzhost.local\n");
        let clean = w("clean", b"baud@demo:~$ fastfetch\r\n");
        let coloured = w("coloured", b"\x1b[1mqz\x1b[0m\x1b[33muser\x1b[0m@demo\n");
        let code = |list: &std::path::Path, controls: &std::path::Path, file: &std::path::Path| {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            xtask::forbidden::search(list, controls, &[file.to_path_buf()], &mut out, &mut err)
                .expect("the files are there")
                .exit()
                .0
        };
        assert_eq!(code(&list, &controls, &clean), 0);
        assert_eq!(
            code(&list, &controls, &coloured),
            1,
            "found past its escapes"
        );
        let one = w("one", b"user\tQZUSER\n");
        assert_eq!(code(&list, &one, &clean), 2, "a label with no control");
        let wrong = w("wrong", b"user\tsomeone\nhost\tqzhost.local\n");
        assert_eq!(code(&list, &wrong, &clean), 2, "a control not found");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
