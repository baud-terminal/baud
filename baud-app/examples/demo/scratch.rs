//! **The scratch home, as data**: every file the shell, the programs and
//! baud read during the session, and baud's whole environment. Nothing of
//! the person's own -- no shell startup file, no program's config --
//! reaches the film.

use std::path::PathBuf;

/// The grid the window opens at: 110 columns by 33 rows at 12 points, the
/// largest type at which btop shows both its boxes whole on an 18-core Mac
/// (`scripts/readme-film.sh`).
pub const GRID: &str = "110x33";
pub const POINTS: u32 = 12;

/// The repository's README, which the session prints and opens in helix.
pub const README: &str = include_str!("../../../README.md");

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
    pub fn fonts(&self) -> PathBuf {
        self.root.join("fonts")
    }
    pub fn config(&self) -> PathBuf {
        self.root.join("baud.toml")
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

/// btop with neither its process list, which names the user and every
/// command line, nor its network box, which names the address; on the
/// terminal's background, and drawn every 100 ms so its graphs fill in the
/// seconds it is on screen.
const BTOP: &str = "\
color_theme = \"Default\"
theme_background = false
truecolor = true
shown_boxes = \"cpu mem\"
update_ms = 100
";

/// helix's own defaults: a file of its own, so the person's is not read.
const HELIX: &str = "# helix's defaults, for the README session\n";

/// **The zsh startup file.** `PATH` is set here and not only in the
/// environment: baud starts a login shell, and on macOS `/etc/zprofile`
/// runs path_helper, which reorders `PATH`, before `.zshrc` is read.
fn zshrc(path: &str) -> String {
    format!("export PATH={path}\nPS1='{}'\n", crate::session::PS1)
}

/// **baud's config**: 12 points, the cursor still, and the fonts --
/// baud's own by family name, `Proto Hebrew Mono C` with its bold and
/// italic and `Symbols Nerd Font Mono`, then the pinned open faces by
/// paths beside this file, `fonts/<file>`.
pub fn config(faces: &[&str]) -> String {
    let mut fonts = vec![
        "\"Proto Hebrew Mono C\"".to_string(),
        "\"Symbols Nerd Font Mono\"".to_string(),
    ];
    fonts.extend(faces.iter().map(|f| format!("\"fonts/{f}\"")));
    format!(
        "font_size = {POINTS}\nfonts = [{}]\n\n[cursor]\nblink = false\n",
        fonts.join(", ")
    )
}

/// **Every text file of the scratch directory**, each a path under the
/// root and its bytes. The photo and the faces are copied in beside them;
/// they are the pinned files, the same bytes on every machine.
pub fn files(path: &str, faces: &[&str]) -> Vec<(String, Vec<u8>)> {
    vec![
        ("home/.zshrc".to_string(), zshrc(path).into_bytes()),
        (
            "home/.config/btop/btop.conf".to_string(),
            BTOP.as_bytes().to_vec(),
        ),
        (
            "home/.config/helix/config.toml".to_string(),
            HELIX.as_bytes().to_vec(),
        ),
        (
            "home/baud/README.md".to_string(),
            README.as_bytes().to_vec(),
        ),
        (
            "home/baud/hello.txt".to_string(),
            crate::session::HELLO.as_bytes().to_vec(),
        ),
        ("baud.toml".to_string(), config(faces).into_bytes()),
    ]
}

/// **baud's environment, and nothing else**, as `env -i` gives it: the
/// scratch home and zsh's startup directory, zsh, the `PATH`, the locale,
/// the scratch temporary directory, `NOSYSZSHRC` (which a nix-darwin
/// `/etc/zshrc` honours), and baud's own: the grid, the config, the
/// motion and scroller, its log and the pty log the session watches.
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
        ("BAUD_CONFIG", s(at.config())),
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
    /// `PATH` among them and the README with the repository's address
    /// taken out, and baud's environment, the scratch root aside. The
    /// control: a home whose `.zshrc` named the user's home would be
    /// caught.
    #[test]
    fn the_scratch_home_names_nothing_of_the_machine() {
        let n = qz();
        let public = public_path(env!("CARGO_PKG_REPOSITORY"));
        let path = path(&[
            "/nix/store/lq4p-btop-1.4.7/bin".to_string(),
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

    /// **The config lists baud's own faces by name and the pinned ones by
    /// paths beside it**, at 12 points with the cursor still, and the
    /// zsh file sets the prompt and the `PATH`.
    #[test]
    fn the_config_and_the_shell_say_what_the_session_needs() {
        let c = config(&["NotoColorEmoji.ttf"]);
        assert!(c.starts_with("font_size = 12\n"), "{c}");
        assert!(c.contains(
            "fonts = [\"Proto Hebrew Mono C\", \"Symbols Nerd Font Mono\", \"fonts/NotoColorEmoji.ttf\"]"
        ));
        assert!(c.ends_with("[cursor]\nblink = false\n"), "{c}");
        let z = zshrc("/a/bin:/usr/bin");
        assert_eq!(
            z,
            "export PATH=/a/bin:/usr/bin\nPS1='%F{cyan}baud%f %F{magenta}\u{276F}%f '\n"
        );
        let e = env(
            &Layout {
                root: PathBuf::from("/t/r"),
            },
            "/a/bin",
        );
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
                "BAUD_CONFIG",
                "BAUD_MOTION",
                "BAUD_SCROLLER",
                "BAUD_LOG",
                "BAUD_PTYLOG"
            ]
        );
        assert!(e.contains(&("HOME".to_string(), "/t/r/home".to_string())));
        assert!(e.contains(&("BAUD_GRID".to_string(), "110x33".to_string())));
    }
}
