//! **The scratch home, as data**: every file the shell, the programs and
//! baud read during the session, and baud's whole environment. Nothing of
//! the person's own -- no shell startup file, no program's config --
//! reaches the film.

use std::path::{Path, PathBuf};

/// **The grid the window opens at: 80 columns by 24 rows, the VT100's
/// screen, at 14 points.** The README shows the GIF 776 px wide whatever
/// the window's size, so the text's size there is set by the columns and
/// not by the points: about 776 / (0.6 x 80), 16 px, the README's own body
/// size, where 110 columns gave 11.5. The points set only the film's
/// pixels.
pub const GRID: &str = "80x24";
pub const POINTS: u32 = 14;

/// baud's config under the home, as the session's helix opens it.
pub const CONFIG: &str = ".config/baud/config.toml";

/// **A new directory in `tmp`, by its physical path.** macOS's `/var` is a
/// symbolic link to `/private/var`, and a terminal's `TMPDIR` is under `/var`.
/// A shell told no `PWD`, as the window's is, finds the resolved path, and
/// starship and zsh spell a path from `HOME` only when `HOME` is a prefix of
/// it: under a `HOME` in the other spelling the prompt showed the path,
/// `⋯/baud-demo-18837-0/home/baud`, where `~/baud` belongs, and the session's
/// leak check refused the take. So every path the session is given is
/// resolved here, once, and all of them share it.
pub fn fresh_root(tmp: &Path) -> Result<PathBuf, String> {
    let tmp = std::fs::canonicalize(tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    for n in 0..100 {
        let dir = tmp.join(format!("baud-demo-{}-{n}", std::process::id()));
        if std::fs::create_dir(&dir).is_ok() {
            return Ok(dir);
        }
    }
    Err(format!("no new directory in {}", tmp.display()))
}

/// Where everything lives under the scratch root.
pub struct Layout {
    pub root: PathBuf,
}

impl Layout {
    pub fn home(&self) -> PathBuf {
        self.root.join("home")
    }
    /// Where the shell starts: under the home, so the window's title reads
    /// `~/baud` rather than a path of the machine's.
    pub fn work(&self) -> PathBuf {
        self.home().join("baud")
    }
    pub fn tmp(&self) -> PathBuf {
        self.root.join("tmp")
    }
    /// The pinned faces, beside the config, which names them by paths
    /// relative to itself.
    pub fn fonts(&self) -> PathBuf {
        self.home().join(".config/baud/fonts")
    }
    /// baud's config, where baud looks for it in the scratch home with no
    /// `BAUD_CONFIG` and no `XDG_CONFIG_HOME` (`config::lookup`), and the
    /// file the session edits in helix.
    pub fn config(&self) -> PathBuf {
        self.home().join(CONFIG)
    }
    pub fn pty_log(&self) -> PathBuf {
        self.root.join("pty.log")
    }
    pub fn baud_log(&self) -> PathBuf {
        self.root.join("baud.log")
    }
}

/// The shell's `PATH`: the programs' own directories first, then the
/// system's.
pub fn path(bins: &[String]) -> String {
    let mut all: Vec<&str> = bins.iter().map(String::as_str).collect();
    all.extend(["/usr/bin", "/bin", "/usr/sbin", "/sbin"]);
    all.join(":")
}

/// **helix's own defaults but soft wrap**: a file of its own, so the
/// person's is not read. With no wrap, `/character` scrolled the whole README
/// sideways: helix searches as each key is typed, `/ch` lands on `scheme` at
/// the right edge of the README's fourth line, and the offset stays when the
/// search moves on, so the first headless take read "e terminal with
/// character". The same file the container's helix reads
/// (`nix/demo-container/default.nix`).
pub const HELIX: &str = include_str!("../../../nix/demo-container/helix.toml");

/// **The zsh startup file.** `PATH` is set here and not only in the
/// environment: baud starts a login shell, and on macOS `/etc/zprofile`
/// runs path_helper, which reorders `PATH`, before `.zshrc` is read. The
/// prompt is starship's, the pinned one on that `PATH`
/// (`container::STARSHIP_NIXPKGS`), drawn from baud's preset as the scratch
/// home holds it ([`STARSHIP`]), its cache in the scratch home too.
/// `YAZI_CONFIG_HOME` names the folder of yazi's config there ([`YAZI`]),
/// so yazi reads that one whatever else its lookup would find; set here,
/// by `$HOME`, as starship's are, since a path under the scratch home spelt
/// out in baud's environment would hold the `/home/` the names check
/// refuses. `CHA_DIR` names Chawan's the same way ([`CHAWAN`]), and
/// `EDITOR` is helix, which yazi's `edit` opener runs for a text file
/// (`${EDITOR:-vi} %s` in yazi 26.9.1's preset). `~/bin` ([`BIN`]), where
/// the demo puts `demo-open`, comes first on the `PATH`, by `$HOME` too.
/// Its `ssh` reads the scratch home's ssh config (`container::ZSH_SSH`).
/// `interactive_comments` is set, so the tour's narration, each a line
/// beginning `#` typed at the prompt, is a comment the shell skips: zsh has
/// the option off in an interactive shell, where `# are you ready for
/// this?` would be run as a command named `#` and its `?` read as a
/// pattern.
fn zshrc(path: &str) -> String {
    format!(
        "export PATH={path}\nsetopt interactive_comments\n\
         export STARSHIP_CONFIG=\"$HOME/{STARSHIP}\"\n\
         export STARSHIP_CACHE=\"$HOME/.cache/starship\"\neval \"$(starship init zsh)\"\n\
         export YAZI_CONFIG_HOME=\"$HOME/{YAZI}\"\nexport CHA_DIR=\"$HOME/{CHAWAN}\"\n\
         export EDITOR=hx\nexport PATH=\"$HOME/{BIN}:$PATH\"\n{}",
        crate::container::ZSH_SSH
    )
}

/// **Where the scratch home holds the tour's own programs**, `~/bin`:
/// `demo-open` (`baud-app/examples/demo-open.rs`).
pub const BIN: &str = "bin";

/// **mpv's config in the scratch home**, `~/.config/mpv/mpv.conf`, so the
/// trailer's `mpv URL`, and the README film's that replaces it, is typed
/// with no option: its video in kitty's graphics protocol, each frame sent
/// in the stream itself (`vo-kitty-use-shm=no`, mpv's default, written
/// out: mpv 0.41 named its shared memory without the leading `/` the
/// protocol asks for, which baud refuses, as kitty does), no audio, which
/// the takes do not film, and no status line, whose writes from its own
/// thread landed inside a frame's chunks in 0.41 and cut them
/// (`tests/recordings/README.md`, `kitty/mpv-logo-40x6.raw`). The demo's
/// mpv (`run::MPV_REV`) holds the fixes to both, and the takes film no
/// status line either way.
pub const MPV_CONF: &str = "# mpv for the tour and the trailer: video in kitty's protocol, in the stream\n\
                            vo=kitty\nvo-kitty-use-shm=no\naudio=no\nreally-quiet\n";

/// **Where the scratch home holds yazi's config**, which
/// `YAZI_CONFIG_HOME` names ([`zshrc`]), so yazi reads no config of the
/// person's.
pub const YAZI: &str = ".config/yazi";

/// **yazi's config**, `yazi.toml`, for yazi 26.9.1: a GIF opens in kitten,
/// `kitten icat --hold`, which sends the GIF's frames for baud to animate
/// and waits for a key -- not mpv, which would draw a transparent GIF on
/// black. The opener blocks, so yazi gives kitten the screen until it
/// exits, and names the file by `%s`, which yazi splices in escaped: it
/// runs an opener as `sh -c` with no arguments
/// (`yazi-scheduler/src/process/shell.rs`, `yazi-fs/src/splatter.rs`), so a
/// `"$@"` would name nothing. The rule is put before yazi's own by
/// `prepend_rules`, where `rules` would replace them
/// (`yazi-config/src/open/open.rs`), and selects by `url`, which with
/// `mime` is all a rule's selector reads (`yazi-config/src/selector.rs`).
pub const YAZI_TOML: &str = "# yazi for the tour: a GIF opens in kitten, which sends its frames for baud to animate\n\
                             [opener]\ngif = [{ run = \"kitten icat --hold %s\", block = true }]\n\
                             [open]\nprepend_rules = [{ url = \"*.gif\", use = \"gif\" }]\n";

/// **Where the scratch home holds Chawan's config**, which `CHA_DIR`
/// names ([`zshrc`]) -- the first place Chawan 0.4.4 looks after a `-C`,
/// and `~/.config/chawan` the next, with no `XDG_CONFIG_HOME` in baud's
/// environment, so a `cha` started by baud rather than by the shell reads
/// the same file (`[[source: chawan-0-4-4-config]]`).
pub const CHAWAN: &str = ".config/chawan";

/// **Chawan's config**, `config.toml`: its defaults but a page's pictures
/// drawn, `buffer.images = true`, which is off by default, with
/// `display.image-mode = "auto"`, Chawan's own detection, which finds
/// kitty's protocol in baud (`[[source: chawan-0-4-4-config]]`).
pub const CHAWAN_TOML: &str = "# Chawan for the tour: its defaults, a page's pictures drawn\n\
                               [buffer]\nimages = true\n\
                               [display]\nimage-mode = \"auto\"\n";

/// **The copy of the repository yazi opens**, under the working directory
/// ([`REPO_AT`]), each path at the take's commit (`git archive HEAD`): the
/// README and every file of the repository it links to, so each link a
/// click follows finds its file. A path ending in `/` is a folder, whole.
pub const REPO_COPY: [&str; 20] = [
    "README.md",
    "CODE_OF_CONDUCT.md",
    "LICENSE",
    "LICENSES/",
    "THIRD_PARTY.md",
    "assets/logo/baud-lockup-dark-tight.svg",
    "assets/logo/baud-lockup-light-tight.svg",
    "assets/readme/logo-dark.gif",
    "assets/readme/logo-light.gif",
    "budgets.toml",
    "docs/README.md",
    "docs/embedding.md",
    "docs/guide/configuration.md",
    "docs/guide/features.md",
    "docs/guide/install.md",
    "docs/guide/licensing.md",
    "docs/guide/status.md",
    "docs/roadmap.md",
    "docs/vt-coverage.md",
    "scripts/bounded",
];

/// Where the repository's copy goes, under the working directory: `cd
/// baud` from `~/baud`.
pub const REPO_AT: &str = "baud";

/// **The files of the repository `readme` links to**, each once, in
/// order: the targets of its markdown links, `](...)`, and of its HTML
/// `src`, `srcset` and `href` attributes, without a web address, a
/// fragment or a `mailto:`. For the test that holds [`REPO_COPY`] to the
/// README.
#[cfg(test)]
pub fn linked(readme: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for open in ["](", "src=\"", "srcset=\"", "href=\""] {
        let close = if open == "](" { ')' } else { '"' };
        let mut rest = readme;
        while let Some(at) = rest.find(open) {
            rest = &rest[at + open.len()..];
            let end = rest.find(close).unwrap_or(rest.len());
            let value = &rest[..end];
            let candidates: Vec<&str> = match open {
                "srcset=\"" => value
                    .split(',')
                    .filter_map(|c| c.split_whitespace().next())
                    .collect(),
                _ => vec![value],
            };
            for c in candidates {
                let c = c.split('#').next().unwrap_or("").trim();
                if c.is_empty() || c.contains("://") || c.starts_with("mailto:") {
                    continue;
                }
                if !out.iter().any(|o| o == c) {
                    out.push(c.to_string());
                }
            }
        }
    }
    out
}

/// Where the scratch home holds baud's starship preset, the repository's
/// `assets/starship/baud.toml` byte for byte ([`PRESET`]).
pub const STARSHIP: &str = ".config/starship.toml";

/// **baud's starship preset**, the file the repository ships.
pub const PRESET: &str = include_str!("../../../assets/starship/baud.toml");

/// **The repository's README as this example was built**, for the checks
/// made before a take: the word the tour's search finds and the files it
/// links to. The take itself shows the README at its commit, `HEAD`.
pub const README: &str = include_str!("../../../README.md");

/// **baud's config**: 27 lines, `trail = "plasma"` the last, and the word
/// nowhere else, so the finale's `sed` changes that line alone.
///
/// - `font_size` first, which `BAUD_SESSION_POINTS` replaces, then the
///   fonts: baud's own by family name on the list's first line, `baud Mono`
///   and `Symbols Nerd Font Mono`, then the pinned open
///   faces one a line, by paths beside this file, `fonts/<file>`, so no
///   path of the machine is on screen.
/// - No `[images]` table: baud's default answer to kitten's detection,
///   the Primary DA `?1;2c`, is one it takes, so `kitten icat` runs with no
///   option on baud's defaults.
/// - `[screen] transition = "on"`: a full-screen program slides in and
///   out, as btop does.
/// - The window solid, each by its own key
///   (`docs/guide/configuration.md`): `[glass] dim_dark = 1.0` and
///   `dim_light = 1.0`, the background "opaque" under either appearance,
///   so nothing behind the window shows through the grid -- the page
///   `baud-app/src/film.rs` films on; `[font] changes = "off"`, so no
///   program changes the font or its size; `[window] operations = "off"`,
///   so no program moves, resizes or full-screens the window, and it
///   keeps the grid it opened at.
/// - The links ([`LINKS`]): a Command-click on a web address opens it in
///   Chawan, `cha`, in a baud window of its own over the window clicked,
///   its pictures drawn whatever config Chawan finds from baud's
///   environment; one on a file path in `~/bin/demo-open`
///   (`baud-app/examples/demo-open.rs`) there, a GIF in kitten and the
///   rest in helix. `~/` is baud's own home, the scratch home, and the
///   program is named under it rather than by its absolute path, which
///   names this machine's temporary folder. Relative paths are read too,
///   beside the three prefixes read by default, so a README's
///   `assets/readme/logo-dark.gif` in helix is read against the folder
///   the shell reported, the repo copy's root in the README film; over
///   ssh, as in the tour, no path is looked up.
/// - `[cursor]`: the cursor blinking as baud's default blinks it,
///   `blink = "auto"`, at baud's shell's prompt and at the server's over
///   ssh -- `false` held it still in the whole film, the server's prompt
///   among it -- and the plasma trail, which the finale changes to the
///   saucer. A program that asks for a steady cursor, as helix does, has
///   it steady until it gives the cursor back.
///
/// The title strip above the grid stays the window's material: no key
/// makes it solid (`accessible` in `baud-app/src/app.rs`).
pub fn config(faces: &[&str]) -> String {
    let list: String = faces
        .iter()
        .map(|f| format!("  \"fonts/{f}\",\n"))
        .collect();
    format!(
        "font_size = {POINTS}\nfonts = [\"baud Mono\", \"Symbols Nerd Font Mono\",\n\
         {list}]\n{TABLES}{LINKS}{CURSOR}"
    )
}

/// [`config`]'s tables: the transition, the window solid and its size and
/// font fixed.
const TABLES: &str = "[screen]\ntransition = \"on\"\n[glass]\ndim_dark = 1.0\ndim_light = 1.0\n\
                      [font]\nchanges = \"off\"\n[window]\noperations = \"off\"\n";

/// **The config's link rules**, `links.toml` beside this file, which
/// baud's own test of the demo reads through baud's config reader
/// (`film::session`).
pub const LINKS: &str = include_str!("links.toml");

/// The cursor, its trail last.
const CURSOR: &str = "[cursor]\nblink = \"auto\"\ntrail = \"plasma\"\n";

/// **Every text file of the scratch directory**, each a path under the
/// root and its bytes. The faces are copied in beside the config; they are
/// the pinned files, the same bytes on every machine.
pub fn files(path: &str, faces: &[&str]) -> Vec<(String, Vec<u8>)> {
    vec![
        ("home/.zshrc".to_string(), zshrc(path).into_bytes()),
        (format!("home/{STARSHIP}"), PRESET.as_bytes().to_vec()),
        (
            "home/.config/helix/config.toml".to_string(),
            HELIX.as_bytes().to_vec(),
        ),
        (
            "home/.config/mpv/mpv.conf".to_string(),
            MPV_CONF.as_bytes().to_vec(),
        ),
        (
            format!("home/{YAZI}/yazi.toml"),
            YAZI_TOML.as_bytes().to_vec(),
        ),
        (
            format!("home/{CHAWAN}/config.toml"),
            CHAWAN_TOML.as_bytes().to_vec(),
        ),
        (format!("home/{CONFIG}"), config(faces).into_bytes()),
    ]
}

/// **baud's environment, and nothing else**, as `env -i` gives it -- no
/// `XDG_CONFIG_HOME` or other `XDG_*` directory among it, which baud reads
/// before `HOME` (`config::lookup`) and helix reads too, so neither reads
/// the person's config: the scratch home and zsh's startup directory, zsh,
/// the `PATH`, the locale,
/// the scratch temporary directory, `NOSYSZSHRC` (which a nix-darwin
/// `/etc/zshrc` honours), and baud's own: the grid, the window on the
/// primary display, which the recorder films, the motion and scroller, its
/// log and the pty log the session watches. No `BAUD_CONFIG`: baud finds
/// its config where it looks with none, `~/.config/baud/config.toml`
/// ([`CONFIG`]), the file the session edits.
pub fn env(at: &Layout, path: &str) -> Vec<(String, String)> {
    let s = |p: PathBuf| p.to_string_lossy().into_owned();
    [
        ("HOME", s(at.home())),
        ("ZDOTDIR", s(at.home())),
        ("SHELL", "/bin/zsh".to_string()),
        ("PATH", path.to_string()),
        ("LANG", "en_US.UTF-8".to_string()),
        ("TMPDIR", s(at.tmp())),
        ("NOSYSZSHRC", "1".to_string()),
        ("BAUD_GRID", GRID.to_string()),
        ("BAUD_SCREEN", "primary".to_string()),
        ("BAUD_MOTION", "full".to_string()),
        ("BAUD_SCROLLER", "overlay".to_string()),
        ("BAUD_LOG", "info".to_string()),
        ("BAUD_PTYLOG", s(at.pty_log())),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::tests::qz;
    use crate::names::{env_named, named, named_outside, public_path};

    /// **The scratch home names nothing of the machine**: every file it
    /// writes, the zsh startup file with the programs' nix paths in its
    /// `PATH` among them, and baud's environment, the scratch root aside. The
    /// control: a home whose `.zshrc` named the user's home would be
    /// caught.
    #[test]
    fn the_scratch_home_names_nothing_of_the_machine() {
        let n = qz();
        let public = public_path(env!("CARGO_PKG_REPOSITORY"));
        let path = path(&[
            "/nix/store/px5j-openssh-10.5p1/bin".to_string(),
            "/nix/store/ayzh-helix-25.07.1/bin".to_string(),
        ]);
        let faces = ["NotoColorEmoji.ttf", "NotoSansCJKsc-Regular.otf"];
        for (file, bytes) in files(&path, &faces) {
            let found = named_outside(&bytes, &n, &[&public]);
            assert_eq!(found, Vec::<String>::new(), "{file}");
        }
        let at = Layout {
            root: PathBuf::from("/private/qz/T/baud-demo-4242"),
        };
        let root = at.root.to_string_lossy().into_owned();
        assert_eq!(env_named(&env(&at, &path), &root, &n), []);
        let leaky = zshrc("/Users/me/bin:/usr/bin");
        assert!(!named(leaky.as_bytes(), &n).is_empty());
    }

    /// **Every path the session is given is physical, whatever spells the
    /// temporary directory**: a root made through a real symbolic link is the
    /// resolved path, `HOME`, `ZDOTDIR`, `TMPDIR` and `BAUD_PTYLOG` all under
    /// it, and a shell in the working directory told no `PWD`, as the window's
    /// is, reads a `PWD` and a `pwd -P` under `HOME`, which is what makes the
    /// prompt `~/baud`. Perturbation: join the link's spelling without
    /// resolving it, and `HOME` is no prefix of what `pwd -P` prints, the run
    /// that showed `⋯/baud-demo-18837-0/home/baud`.
    #[test]
    fn every_path_the_session_is_given_is_physical() {
        let base = std::env::temp_dir().join(format!("demo-physical-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let real = base.join("real");
        std::fs::create_dir_all(&real).unwrap();
        let link = base.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let resolved = real.canonicalize().unwrap();

        let root = fresh_root(&link).unwrap();
        assert!(root.starts_with(&resolved), "{}", root.display());
        assert_eq!(root.canonicalize().unwrap(), root);

        let at = Layout { root: root.clone() };
        std::fs::create_dir_all(at.work()).unwrap();
        let e = env(&at, "/a/bin");
        let var = |k: &str| {
            let (_, v) = e.iter().find(|(key, _)| key == k).unwrap();
            PathBuf::from(v)
        };
        for k in ["HOME", "ZDOTDIR", "TMPDIR", "BAUD_PTYLOG"] {
            assert!(var(k).starts_with(&root), "{k}: {}", var(k).display());
        }
        let out = std::process::Command::new("/bin/sh")
            .args(["-c", "pwd -P; printf '%s\n' \"$PWD\""])
            .env_clear()
            .env("HOME", var("HOME"))
            .current_dir(at.work())
            .output()
            .unwrap();
        let said = String::from_utf8(out.stdout).unwrap();
        let lines: Vec<&str> = said.lines().collect();
        assert_eq!(lines.len(), 2, "{said}");
        for line in lines {
            assert!(Path::new(line).starts_with(var("HOME")), "{line}");
        }
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// **The config lists baud's own faces by name and the pinned ones by
    /// paths beside it**, at 14 points, the transition on, the window solid,
    /// the links in their windows, and the cursor blinking with
    /// its plasma trail last, in 27 lines, with "plasma" on the last alone,
    /// the line the finale's `sed` changes; the zsh file sets the `PATH`,
    /// comments at the prompt, starship's prompt from the preset in the
    /// scratch home, and ssh's config; and baud's environment is its list,
    /// no `XDG_*` among it. Perturbation: a blank line after the
    /// fonts makes 28 lines, and a `trail_scroll` after the trail ends the
    /// file on another line.
    #[test]
    fn the_config_and_the_shell_say_what_the_session_needs() {
        let faces = [
            "NotoColorEmoji.ttf",
            "NotoSansCJKsc-Regular.otf",
            "NotoNaskhArabic-Regular.ttf",
            "NotoSansDevanagari-Regular.ttf",
            "NotoSansSymbols2-Regular.ttf",
        ];
        let c = config(&faces);
        assert!(
            c.starts_with(
                "font_size = 14\nfonts = [\"baud Mono\", \"Symbols Nerd Font Mono\",\n  \
                 \"fonts/NotoColorEmoji.ttf\",\n"
            ),
            "{c}"
        );
        assert_eq!(c.lines().count(), 27, "{c}");
        assert!(
            !c.contains("[images]"),
            "baud's defaults, no [images] table: {c}"
        );
        assert_eq!(c.lines().last(), Some("trail = \"plasma\""));
        assert_eq!(c.matches("plasma").count(), 1, "the word once");
        // The finale's `sed`, its one substitution, changes the last line
        // alone; the control, a second `plasma`, would change two.
        assert!(crate::session::SED_UFO.contains(" s/plasma/ufo/ "));
        let after = c.replacen("plasma", "ufo", 1);
        assert_eq!(after.lines().last(), Some("trail = \"ufo\""));
        let changed = |a: &str, b: &str| a.lines().zip(b.lines()).filter(|(x, y)| x != y).count();
        assert_eq!(changed(&c, &after), 1, "one line changed");
        let twice = format!("{c}# plasma\n");
        assert_eq!(
            changed(&twice, &twice.replace("plasma", "ufo")),
            2,
            "the control"
        );
        assert!(
            c.lines().all(|l| l.chars().count() < 60),
            "no line runs off"
        );
        // The window solid, each key in its own table: opaque under
        // either appearance, and the font and the window no program's.
        for table in [
            "[screen]\ntransition = \"on\"\n",
            "[glass]\ndim_dark = 1.0\ndim_light = 1.0\n",
            "[font]\nchanges = \"off\"\n",
            "[window]\noperations = \"off\"\n",
            "[link.path]\nforms = [\"/\", \"~/\", \"./\", \"relative\"]\n\
             action = \"window\"\ncommand = [\"~/bin/demo-open\", \"{path}\"]\n",
            "[link.https]\naction = \"window\"\n\
             command = [\"cha\", \"-o\", \"buffer.images=true\", \"{url}\"]\n",
            "[cursor]\nblink = \"auto\"\ntrail = \"plasma\"\n",
        ] {
            assert!(c.contains(table), "{table}:\n{c}");
        }
        let z = zshrc("/a/bin:/usr/bin");
        assert_eq!(
            z,
            "export PATH=/a/bin:/usr/bin\n\
             setopt interactive_comments\n\
             export STARSHIP_CONFIG=\"$HOME/.config/starship.toml\"\n\
             export STARSHIP_CACHE=\"$HOME/.cache/starship\"\n\
             eval \"$(starship init zsh)\"\n\
             export YAZI_CONFIG_HOME=\"$HOME/.config/yazi\"\n\
             export CHA_DIR=\"$HOME/.config/chawan\"\n\
             export EDITOR=hx\n\
             export PATH=\"$HOME/bin:$PATH\"\n\
             alias ssh='ssh -F ~/.ssh/config'\n"
        );
        let preset = files("/a/bin", &faces)
            .into_iter()
            .find(|(f, _)| f == "home/.config/starship.toml")
            .expect("the preset in the scratch home");
        let helix = files("/a/bin", &faces)
            .into_iter()
            .find(|(f, _)| f == "home/.config/helix/config.toml")
            .expect("helix's config in the scratch home");
        assert!(
            String::from_utf8_lossy(&helix.1).contains("[editor.soft-wrap]\nenable = true\n"),
            "no sideways scroll of the README"
        );
        assert!(
            files("/a/bin", &faces)
                .iter()
                .all(|(f, _)| !f.starts_with("home/baud/")),
            "the working directory's files come from the repository at HEAD"
        );
        assert_eq!(
            preset.1,
            std::fs::read(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../assets/starship/baud.toml"
            ))
            .unwrap(),
            "the repository's preset, byte for byte"
        );
        let at = Layout {
            root: PathBuf::from("/t/r"),
        };
        let e = env(&at, "/a/bin");
        let keys: Vec<&str> = e.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            [
                "HOME",
                "ZDOTDIR",
                "SHELL",
                "PATH",
                "LANG",
                "TMPDIR",
                "NOSYSZSHRC",
                "BAUD_GRID",
                "BAUD_SCREEN",
                "BAUD_MOTION",
                "BAUD_SCROLLER",
                "BAUD_LOG",
                "BAUD_PTYLOG"
            ]
        );
        assert!(!keys.iter().any(|k| k.starts_with("XDG_")), "{keys:?}");
        assert!(e.contains(&("HOME".to_string(), "/t/r/home".to_string())));
        assert_eq!(
            at.config(),
            PathBuf::from("/t/r/home/.config/baud/config.toml")
        );
        assert_eq!(at.fonts(), PathBuf::from("/t/r/home/.config/baud/fonts"));
        assert!(e.contains(&("BAUD_GRID".to_string(), "80x24".to_string())));
        assert!(e.contains(&("BAUD_SCREEN".to_string(), "primary".to_string())));
        let windows = include_str!("../../src/windows.rs");
        assert!(windows.contains("pub const SCREEN: &str = \"BAUD_SCREEN\";"));
        assert!(windows.contains("Some(v) if v == \"primary\" => Ok(Screen::Primary),"));
    }

    /// **yazi reads the scratch home's config alone, and opens a GIF with
    /// kitten**: the shell's `YAZI_CONFIG_HOME` names the folder its
    /// `yazi.toml` is written to, so no config of the person's is read; the
    /// opener blocks, so yazi gives kitten the screen until a key ends
    /// `--hold`, and names the files by `%s`, which yazi 26.9.1 splices in
    /// escaped -- it runs an opener as `sh -c` with no arguments, so a
    /// `"$@"` would name none -- and the rule is prepended to yazi's own by
    /// `url`, the key its rule selector reads beside `mime`. The control: a
    /// variable naming another folder than the file's is caught.
    /// Perturbation: spell the opener `"$@"` and its assertion fails.
    #[test]
    fn yazi_reads_the_scratch_config_and_opens_a_gif_with_kitten() {
        let written = files("/a/bin", &[])
            .into_iter()
            .find(|(f, _)| f.ends_with("/yazi.toml"))
            .expect("yazi's config in the scratch home");
        assert_eq!(written.0, "home/.config/yazi/yazi.toml");
        assert_eq!(written.1, YAZI_TOML.as_bytes());
        assert!(
            YAZI_TOML
                .contains("[opener]\ngif = [{ run = \"kitten icat --hold %s\", block = true }]\n")
        );
        assert!(
            YAZI_TOML.contains("[open]\nprepend_rules = [{ url = \"*.gif\", use = \"gif\" }]\n")
        );
        assert!(!YAZI_TOML.contains("$@") && !YAZI_TOML.contains("name ="));
        let folder = |z: &str| {
            z.lines()
                .find_map(|l| l.strip_prefix("export YAZI_CONFIG_HOME=\"$HOME/"))
                .map(|d| format!("home/{}/yazi.toml", d.trim_end_matches('"')))
        };
        let z = zshrc("/a/bin");
        assert_eq!(folder(&z), Some(written.0.clone()));
        let other = z.replace(".config/yazi", ".yazi");
        assert_ne!(folder(&other), Some(written.0), "the control");
        assert!(
            z.lines().any(|l| l == "export EDITOR=hx"),
            "Enter edits in helix"
        );
    }

    /// **Chawan reads the scratch home's config alone**: the shell's
    /// `CHA_DIR` names the folder its `config.toml` is written to, which is
    /// also `~/.config/chawan`, where Chawan looks next with no
    /// `XDG_CONFIG_HOME`, so a `cha` baud starts reads it too; and it draws
    /// a page's pictures, `buffer.images` on, by the protocol it detects.
    /// The control: a variable naming another folder is caught.
    #[test]
    fn chawan_reads_the_scratch_config() {
        let written = files("/a/bin", &[])
            .into_iter()
            .find(|(f, _)| f.ends_with("/chawan/config.toml"))
            .expect("Chawan's config in the scratch home");
        assert_eq!(written.0, "home/.config/chawan/config.toml");
        assert_eq!(written.1, CHAWAN_TOML.as_bytes());
        let folder = |z: &str| {
            z.lines()
                .find_map(|l| l.strip_prefix("export CHA_DIR=\"$HOME/"))
                .map(|d| format!("home/{}/config.toml", d.trim_end_matches('"')))
        };
        let z = zshrc("/a/bin");
        assert_eq!(folder(&z), Some(written.0.clone()));
        assert_ne!(
            folder(&z.replace(".config/chawan", ".chawan")),
            Some(written.0),
            "the control"
        );
        assert!(CHAWAN_TOML.contains("[buffer]\nimages = true\n"));
        assert!(CHAWAN_TOML.contains("[display]\nimage-mode = \"auto\"\n"));
    }

    /// **The repository's copy is the README and every file it links
    /// to**, exactly: [`REPO_COPY`] is the README and [`linked`]'s list of
    /// it, each once, a folder named once with its `/`; and each is in the
    /// repository. The controls: a README with one more relative link, or
    /// with a web address, a fragment and a `srcset` of two, reads as it
    /// should. Perturbation: drop a path from `REPO_COPY` and the
    /// equality fails.
    #[test]
    fn the_repository_copy_is_the_readme_and_what_it_links_to() {
        let mut want = linked(README);
        want.push("README.md".to_string());
        want.sort();
        let mut have: Vec<String> = REPO_COPY.iter().map(|s| s.to_string()).collect();
        have.sort();
        assert_eq!(have, want);
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        for p in REPO_COPY {
            assert!(root.join(p).exists(), "{p}");
        }
        assert_eq!(
            linked(
                "[a](docs/a.md) [w](https://x.org/y) [f](b.md#top) \
                 <img srcset=\"c.gif 1x, d.gif 2x\"> [m](mailto:a@b) [a2](docs/a.md)"
            ),
            ["docs/a.md", "b.md", "c.gif", "d.gif"]
        );
    }

    /// The btop configs `scripts/readme-film.sh` and
    /// `scripts/readme-capture.sh` write, each the heredoc that follows
    /// its `btop.conf"`.
    fn script_btop(script: &str) -> &str {
        let open = "btop.conf\" <<'EOF'\n";
        assert_eq!(script.matches(open).count(), 1, "one btop config");
        let body = &script[script.find(open).unwrap() + open.len()..];
        &body[..body.find("\nEOF\n").expect("the heredoc ends") + 1]
    }

    /// **What a btop config would show of the machine**, by the keys
    /// btop 1.4.7 reads (`[[source: btop-config-147]]`) and the container's
    /// 1.4.6 reads alike (`[[source: btop-config-146]]`): each key once,
    /// since the last of two wins, and spelt as btop spells it, since it
    /// skips a key it does not know and keeps the default for a bool it
    /// cannot read.
    fn btop_shows(conf: &str) -> Vec<&'static str> {
        let mut keys: Vec<(&str, &str)> = Vec::new();
        let mut twice = false;
        for line in conf.lines() {
            let (k, v) = line.split_once(" = ").expect("key = value");
            twice |= keys.iter().any(|&(seen, _)| seen == k);
            keys.push((k, v));
        }
        let is =
            |k: &str, want: &[&str]| keys.iter().any(|&(key, v)| key == k && want.contains(&v));
        let off = ["false", "False"];
        let mut shown = Vec::new();
        if !is("shown_boxes", &["\"cpu\""]) {
            shown.push("a box but the cpu box");
        }
        let named = keys.iter().any(|&(k, v)| {
            k == "custom_cpu_name" && v.len() > 2 && v.starts_with('"') && v.ends_with('"')
        });
        if !named {
            shown.push("the chip's name");
        }
        if !is("show_gpu_info", &["\"Off\""]) {
            shown.push("the GPU's memory");
        }
        if !is("show_uptime", &off) {
            shown.push("the uptime");
        }
        if !is("show_cpu_freq", &off) {
            shown.push("the CPU's frequency");
        }
        if !is("show_disks", &off) {
            shown.push("the disks");
        }
        if twice {
            shown.push("a key given twice");
        }
        shown
    }

    /// **btop shows nothing of the machine but its load**, in all three
    /// configs that run it: the container's and the two scripts'. The load
    /// average is not among what this holds out, because btop 1.4.7 draws
    /// it in the cpu box whatever the config: `Cpu::draw` writes
    /// "Load avg:" once the core list ends, behind no key
    /// (`[[source: btop-draw-147]]`). The controls:
    /// the config before this one, which showed the mem box and the chip;
    /// an empty name, which btop reads as no name; a misspelt key, which
    /// btop skips; a bool btop cannot read; and a key given twice.
    #[test]
    fn btop_shows_nothing_of_the_machine() {
        let film = include_str!("../../../scripts/readme-film.sh");
        let capture = include_str!("../../../scripts/readme-capture.sh");
        for (who, conf) in [
            ("container", BTOP),
            ("readme-film.sh", script_btop(film)),
            ("readme-capture.sh", script_btop(capture)),
        ] {
            assert_eq!(btop_shows(conf), Vec::<&str>::new(), "{who}");
        }
        let before = "color_theme = \"Default\"\nshown_boxes = \"cpu mem\"\nupdate_ms = 100\n";
        assert_eq!(
            btop_shows(before),
            [
                "a box but the cpu box",
                "the chip's name",
                "the GPU's memory",
                "the uptime",
                "the CPU's frequency",
                "the disks"
            ]
        );
        let unnamed = BTOP.replace("custom_cpu_name = \"CPU\"", "custom_cpu_name = \"\"");
        assert_eq!(btop_shows(&unnamed), ["the chip's name"]);
        let misspelt = BTOP.replace("show_uptime", "show_uptim");
        assert_eq!(btop_shows(&misspelt), ["the uptime"]);
        let unread = BTOP.replace("show_cpu_freq = false", "show_cpu_freq = no");
        assert_eq!(btop_shows(&unread), ["the CPU's frequency"]);
        let twice = format!("{BTOP}show_uptime = true\n");
        assert_eq!(btop_shows(&twice), ["a key given twice"]);
    }

    /// **The container's btop, naming nothing of the machine but its
    /// load**, the file `nix/demo-container/default.nix` installs: its cpu
    /// box alone, each key as btop describes it (`[[source:
    /// btop-config-146]]`):
    ///
    /// - `shown_boxes = "cpu"`: "Manually set which boxes to show". Not
    ///   `proc`, which names the user and every command line, nor `net`,
    ///   which names the address, nor `mem`, whose "Total:" is the memory's
    ///   size and whose disks are listed by volume name and size, nor `gpu0`.
    /// - `custom_cpu_name = "CPU"`: "Custom cpu model name, empty string to
    ///   disable"; empty, btop titles the core list with the chip's name.
    /// - `show_gpu_info = "Off"`, `show_uptime`, `show_cpu_freq`,
    ///   `show_disks` and `show_battery` false, and no clock.
    /// - `check_temp` and `show_coretemp` false: "Show cpu temperature"
    ///   and "Show temperatures for cpu cores also if check_temp is True
    ///   and sensors has been found", so no temperature is drawn whether or
    ///   not the VM has a sensor.
    ///
    /// On the terminal's background, and drawn every 100 ms so its graphs
    /// fill in the seconds it is on screen.
    const BTOP: &str = include_str!("../../../nix/demo-container/btop.conf");

    /// **The container's btop draws no battery and no clock**: in the
    /// container both would be the VM's, and the clock is drawn by default
    /// at the top of the cpu box -- `show_battery` "Show battery stats in
    /// top right if battery is present", `clock_format` "Draw a clock at top
    /// of screen ... empty string to disable" (`[[source: btop-config-146]]`)
    /// -- and it is never told to save its settings over the file. The
    /// control: the default clock.
    #[test]
    fn the_container_s_btop_draws_no_battery_and_no_clock() {
        for line in [
            "show_battery = false",
            "check_temp = false",
            "show_coretemp = false",
            "clock_format = \"\"",
            "save_config_on_exit = false",
        ] {
            assert!(BTOP.lines().any(|l| l == line), "{line}");
        }
        let clock = BTOP.replace("clock_format = \"\"", "clock_format = \"%X\"");
        assert!(!clock.lines().any(|l| l == "clock_format = \"\""));
    }
}
