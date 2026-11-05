//! **The scratch home, as data**: every file the shell, the programs and
//! baud read during the session, and baud's whole environment. Nothing of
//! the person's own -- no shell startup file, no program's config --
//! reaches the film.

use std::path::PathBuf;

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

/// helix's own defaults: a file of its own, so the person's is not read.
const HELIX: &str = "# helix's defaults, for the README session\n";

/// **The zsh startup file.** `PATH` is set here and not only in the
/// environment: baud starts a login shell, and on macOS `/etc/zprofile`
/// runs path_helper, which reorders `PATH`, before `.zshrc` is read. Its
/// `ssh` reads the scratch home's ssh config (`container::ZSH_SSH`).
fn zshrc(path: &str) -> String {
    format!(
        "export PATH={path}\nPS1='{}'\n{}",
        crate::session::PS1,
        crate::container::ZSH_SSH
    )
}

/// **baud's config**, as the session shows it in helix: 22 lines, no more
/// than the rows helix gives a file in a 24-row window, so the session's
/// jumps between its first line and its last cross most of the screen;
/// `trail = "plasma"` the last line, and the word nowhere else, so `ge`
/// lands on it and `/plasma` selects it.
///
/// - `font_size` first, which `BAUD_SESSION_POINTS` replaces, then the
///   fonts one a line: baud's own by family name, `Proto Hebrew Mono C`
///   and `Symbols Nerd Font Mono`, then the pinned open faces by paths
///   beside this file, `fonts/<file>`, so no path of the machine is on
///   screen.
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
/// - `[cursor]`: the cursor still, so the first and last frames match,
///   and the plasma trail, which the session changes to fire.
///
/// The title strip above the grid stays the window's material: no key
/// makes it solid (`accessible` in `baud-app/src/app.rs`).
pub fn config(faces: &[&str]) -> String {
    let mut fonts = vec![
        "\"Proto Hebrew Mono C\"".to_string(),
        "\"Symbols Nerd Font Mono\"".to_string(),
    ];
    fonts.extend(faces.iter().map(|f| format!("\"fonts/{f}\"")));
    let list: String = fonts.iter().map(|f| format!("  {f},\n")).collect();
    format!("font_size = {POINTS}\nfonts = [\n{list}]\n{TABLES}")
}

/// [`config`]'s tables: the transition, the window solid and its size and
/// font fixed, and the cursor, its trail last.
const TABLES: &str = "[screen]\ntransition = \"on\"\n[glass]\ndim_dark = 1.0\ndim_light = 1.0\n\
                      [font]\nchanges = \"off\"\n[window]\noperations = \"off\"\n\
                      [cursor]\nblink = false\ntrail = \"plasma\"\n";

/// **The photo the picture shot draws**: the elementary OS wallpaper
/// "Canazei Granite Ridges", an Unsplash photograph published before 8 June
/// 2017 and so CC0, as elementary's `debian/copyright` records
/// (`THIRD_PARTY.md`), fetched once by its URL at the wallpapers' tag
/// `8.0.0` into the faces' cache and refused unless it hashes to the pin
/// `scripts/readme-capture.sh` checks it by.
pub const PHOTO: crate::fonts::Pinned = crate::fonts::Pinned {
    name: "Canazei Granite Ridges",
    url: "https://raw.githubusercontent.com/elementary/wallpapers/8.0.0/backgrounds/Canazei%20Granite%20Ridges.jpg",
    sha256: "0eff9cd7fe92aabb8937644ca154436ba926310808dd99a2902cdde335b0868c",
    draws: "the picture shot",
};

/// **The photo's width as the session's working directory holds it**, in
/// pixels: 50 of the film's 17-pixel columns at 14 points, the width the
/// earlier take's `--place` gave it, which kitten draws a pixel a pixel.
/// Its 5472-pixel original took kitten a second to decode
/// (`scripts/readme-capture.sh`).
pub const PHOTO_WIDTH: u32 = 850;

/// Where the photo is, under the scratch root: the shell's working
/// directory, so the command names it bare.
pub const PHOTO_AT: &str = "home/baud/canazei.jpg";

/// **Every text file of the scratch directory**, each a path under the
/// root and its bytes. The faces are copied in beside the config; they are
/// the pinned files, the same bytes on every machine.
pub fn files(path: &str, faces: &[&str]) -> Vec<(String, Vec<u8>)> {
    vec![
        ("home/.zshrc".to_string(), zshrc(path).into_bytes()),
        (
            "home/.config/helix/config.toml".to_string(),
            HELIX.as_bytes().to_vec(),
        ),
        (
            "home/baud/hello.txt".to_string(),
            crate::session::HELLO.as_bytes().to_vec(),
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

    /// **The config lists baud's own faces by name and the pinned ones by
    /// paths beside it, one a line**, at 14 points, the transition on, the
    /// window solid and the cursor still with its plasma trail last, in 22
    /// lines -- the rows helix gives a file in 24 -- with "plasma" on the
    /// last alone; the zsh file sets the prompt, the `PATH` and ssh's
    /// config; and baud's environment is its list, no `XDG_*` among it.
    /// Perturbation: a blank line after the fonts makes 23 lines, and a
    /// `trail_scroll` after the trail ends the file on another line.
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
        assert!(c.starts_with("font_size = 14\nfonts = [\n"), "{c}");
        assert!(c.contains(
            "  \"Proto Hebrew Mono C\",\n  \"Symbols Nerd Font Mono\",\n  \"fonts/NotoColorEmoji.ttf\",\n"
        ));
        assert_eq!(c.lines().count(), 22, "{c}");
        assert_eq!(c.lines().last(), Some("trail = \"plasma\""));
        assert_eq!(c.matches("plasma").count(), 1, "the word once");
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
            "[cursor]\nblink = false\ntrail = \"plasma\"\n",
        ] {
            assert!(c.contains(table), "{table}:\n{c}");
        }
        let z = zshrc("/a/bin:/usr/bin");
        assert_eq!(
            z,
            "export PATH=/a/bin:/usr/bin\nPS1='%F{cyan}baud%f %F{magenta}\u{276F}%f '\n\
             alias ssh='ssh -F ~/.ssh/config'\n"
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
            "clock_format = \"\"",
            "save_config_on_exit = false",
        ] {
            assert!(BTOP.lines().any(|l| l == line), "{line}");
        }
        let clock = BTOP.replace("clock_format = \"\"", "clock_format = \"%X\"");
        assert!(!clock.lines().any(|l| l == "clock_format = \"\""));
    }
}
