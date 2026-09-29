//! The theme files `[palette] light` and `dark` name (note Y, step 7).
//!
//! `config.rs`'s `apply` stays pure and stores the two strings; reading the
//! files is I/O and happens here, at launch, with its **own complaint list**,
//! so a file that fails is reported and leaves baud's own half in place
//! rather than stopping the terminal.
//!
//! Decided with Jev, with the rule written before asking: a bare name is
//! tried as given, then with `.yaml`, then `.yml` (Jev, 0.65); the reader is
//! chosen by extension -- `.yaml` and `.yml` are base16/base24, anything else
//! is a ghostty file (below 0.6, the written rule); a scheme whose polarity is
//! the other half's is used as named and reported (Jev, 0.93).

use std::path::{Path, PathBuf};

use baud_domain::ColorScheme;
use baud_domain::themefile::{self, Format, Scheme, SchemeErrorKind};
use baud_render::Theme;

/// Which half of the palette a file is named for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Half {
    Dark,
    Light,
}

impl Half {
    fn key(self) -> &'static str {
        match self {
            Half::Dark => "dark",
            Half::Light => "light",
        }
    }

    fn built_in(self) -> Theme {
        match self {
            Half::Dark => Theme::dark(),
            Half::Light => Theme::light(),
        }
    }

    fn scheme(self) -> ColorScheme {
        match self {
            Half::Dark => ColorScheme::Dark,
            Half::Light => ColorScheme::Light,
        }
    }
}

/// The two halves' schemes; `None` is baud's own.
#[derive(Clone, Copy, Default)]
pub struct Halves {
    pub dark: Option<Scheme>,
    pub light: Option<Scheme>,
}

impl Halves {
    pub fn get(&self, half: Half) -> Option<&Scheme> {
        match half {
            Half::Dark => self.dark.as_ref(),
            Half::Light => self.light.as_ref(),
        }
    }
}

/// Where a bare theme name is looked for: beside the config file in force,
/// and the config directories, read once from the environment.
pub struct Dirs {
    pub config: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub xdg: Option<PathBuf>,
}

impl Dirs {
    /// `config` is the file `Config::load` read, if one was.
    pub fn from_env(config: Option<&Path>) -> Dirs {
        Dirs {
            config: config.map(Path::to_path_buf),
            home: std::env::var_os("HOME").map(PathBuf::from),
            xdg: std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        }
    }
}

/// Both halves the config names, and what loading them had to say.
pub fn load_halves(light: Option<&str>, dark: Option<&str>, dirs: &Dirs) -> (Halves, Vec<String>) {
    let mut said = Vec::new();
    let halves = Halves {
        dark: dark.and_then(|s| load(s, Half::Dark, dirs, &mut said)),
        light: light.and_then(|s| load(s, Half::Light, dirs, &mut said)),
    };
    (halves, said)
}

/// The paths a value names, in the order they are tried. A value with a `/`
/// is a path, `~/` expanded from `HOME`; a bare name is looked up in the
/// `themes` directory beside the config file in force, then in
/// `$XDG_CONFIG_HOME/baud/themes` and `~/.config/baud/themes`, each directory
/// once, as given and then with `.yaml` and `.yml`.
pub fn candidates(spec: &str, dirs: &Dirs) -> Vec<PathBuf> {
    if spec.contains('/') {
        let path = match (spec.strip_prefix("~/"), &dirs.home) {
            (Some(rest), Some(home)) => home.join(rest),
            _ => PathBuf::from(spec),
        };
        return vec![path];
    }
    let mut roots: Vec<PathBuf> = Vec::new();
    let beside = dirs
        .config
        .as_deref()
        .and_then(Path::parent)
        .map(|p| p.join("themes"));
    let xdg = dirs.xdg.as_ref().map(|x| x.join("baud/themes"));
    let home = dirs.home.as_ref().map(|h| h.join(".config/baud/themes"));
    for root in [beside, xdg, home].into_iter().flatten() {
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    let mut out = Vec::new();
    for root in roots {
        for name in [
            spec.to_string(),
            format!("{spec}.yaml"),
            format!("{spec}.yml"),
        ] {
            out.push(root.join(name));
        }
    }
    out
}

/// The reader a file goes to: base16/base24 for `.yaml` and `.yml` in any
/// case, ghostty for anything else, no extension included.
fn is_tinted(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("yaml") || e.eq_ignore_ascii_case("yml"))
}

/// One half's scheme, or `None` with a complaint; notices about a scheme
/// that loaded go on `said` too.
fn load(spec: &str, half: Half, dirs: &Dirs, said: &mut Vec<String>) -> Option<Scheme> {
    let own = format!("using baud's own {} palette", half.key());
    let spec = spec.trim();
    if spec.is_empty() {
        said.push(format!("config: [palette] {} is empty; {own}", half.key()));
        return None;
    }
    let tried = candidates(spec, dirs);
    if tried.is_empty() {
        said.push(format!(
            "config: [palette] {} {spec:?}: no HOME or XDG_CONFIG_HOME to look in; {own}",
            half.key()
        ));
        return None;
    }
    // The first that exists; one that exists and cannot be read is said as
    // such, not skipped as though it were not there.
    let mut found = None;
    for p in &tried {
        match std::fs::read(p) {
            Ok(b) => {
                found = Some((p, b));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                said.push(format!(
                    "config: [palette] {} {} cannot be read ({e}); {own}",
                    half.key(),
                    p.display()
                ));
                return None;
            }
        }
    }
    let Some((path, bytes)) = found else {
        let list: Vec<_> = tried.iter().map(|p| p.display().to_string()).collect();
        said.push(format!(
            "config: [palette] {} {spec:?}: no theme file at {}; {own}",
            half.key(),
            list.join(", ")
        ));
        return None;
    };
    let at = format!("config: [palette] {} {}", half.key(), path.display());
    let read = if is_tinted(path) {
        themefile::base16(&bytes)
    } else {
        themefile::ghostty(&bytes, |line, key| {
            said.push(format!(
                "{at}: line {}: {} is not a colour and is ignored",
                line.0,
                String::from_utf8_lossy(key)
            ))
        })
    };
    let scheme = match read {
        Ok(s) => s,
        Err(e) => {
            said.push(format!(
                "{at}: line {}: {}; {own}",
                e.line.0,
                reason(e.kind)
            ));
            return None;
        }
    };
    notice(&at, half, &scheme, said);
    Some(scheme)
}

/// What a loaded scheme is worth saying: entries the file left to baud, a
/// declared variant its colours disagree with, a polarity that is the other
/// half's, and a ghostty file's entries past 15, which are not used.
fn notice(at: &str, half: Half, scheme: &Scheme, said: &mut Vec<String>) {
    let mut drawn = half.built_in();
    let unset = drawn.take_scheme(scheme).get_raw();
    if unset > 0 {
        said.push(format!(
            "{at}: {unset} of the sixteen, the page and the text are unset and stay baud's own"
        ));
    }
    let polarity = drawn.polarity();
    // What the file set of the two the rule reads, so a notice does not
    // credit the file with baud's own colour.
    let pair = match (scheme.page.is_some(), scheme.text.is_some()) {
        (true, true) => "its page and text read",
        (true, false) => "its page, with baud's own text, reads",
        (false, true) => "its text, on baud's own page, reads",
        (false, false) => "baud's own page and text read",
    };
    if let (Some(declared), Some(p)) = (scheme.declared, polarity) {
        if declared != p {
            said.push(format!(
                "{at}: declares variant {} but {pair} {}",
                name(declared),
                name(p)
            ));
        }
    }
    if polarity.is_some_and(|p| p != half.scheme()) {
        said.push(format!(
            "{at}: {pair} {}, and it is named as the {} half; used as named",
            polarity.map_or("", name),
            half.key()
        ));
    }
    if scheme.format == Format::Ghostty && scheme.extended.iter().any(Option::is_some) {
        said.push(format!(
            "{at}: entries 16-255 are not used; they stay xterm's"
        ));
    }
}

/// The theme a half draws: its built-in palette, the file named for it laid
/// over that, and the half's own dim, floor and exemptions. Both drivers call
/// this, so a pinned light theme dims by `dim_light` on either.
pub fn half_theme(
    half: Half,
    glass: &crate::config::Glass,
    floor: baud_render::Floor,
    halves: &Halves,
) -> Theme {
    let mut theme = half.built_in();
    if let Some(scheme) = halves.get(half) {
        theme.take_scheme(scheme);
    }
    theme.bg_alpha = baud_domain::Chan::from_unit(match half {
        Half::Dark => glass.dim_dark,
        Half::Light => glass.dim_light,
    });
    theme.app_bg = glass.app_backgrounds;
    theme.picture_bg = glass.pictures;
    theme.floor = floor;
    theme
}

fn name(s: ColorScheme) -> &'static str {
    match s {
        ColorScheme::Dark => "dark",
        ColorScheme::Light => "light",
    }
}

fn reason(kind: SchemeErrorKind) -> String {
    match kind {
        SchemeErrorKind::Hex(e) => format!("a colour that does not read ({e:?})"),
        SchemeErrorKind::Missing(slot) => format!("base{:02X} is missing", slot.0),
        SchemeErrorKind::Duplicate => "a key given twice".into(),
        SchemeErrorKind::UnknownSystem => "a scheme system that is not base16 or base24".into(),
        SchemeErrorKind::Syntax => "a line that is not a key and a value".into(),
        SchemeErrorKind::PaletteIndex => "a palette number that is not 0-255".into(),
        SchemeErrorKind::ColourName => "a colour name, which baud does not resolve yet".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/themes")
    }

    fn dirs(home: &Path) -> Dirs {
        Dirs {
            config: None,
            home: Some(home.into()),
            xdg: None,
        }
    }

    /// A home whose `.config/baud/themes` holds the fixtures.
    fn home(tag: &str) -> PathBuf {
        let h = std::env::temp_dir().join(format!("baud-palette-{}-{tag}", std::process::id()));
        let themes = h.join(".config/baud/themes");
        std::fs::create_dir_all(&themes).unwrap();
        for f in ["day.yaml", "sepia.yml", "broken.yaml", "night"] {
            std::fs::copy(fixtures().join(f), themes.join(f)).unwrap();
        }
        h
    }

    #[test]
    fn a_path_expands_its_tilde_and_a_bare_name_tries_three_files() {
        let d = Dirs {
            config: None,
            home: Some("/h".into()),
            xdg: Some("/x".into()),
        };
        assert_eq!(
            candidates("~/t/day.yaml", &d),
            [PathBuf::from("/h/t/day.yaml")]
        );
        assert_eq!(candidates("/abs/night", &d), [PathBuf::from("/abs/night")]);
        let want: Vec<PathBuf> = [
            "/x/baud/themes/day",
            "/x/baud/themes/day.yaml",
            "/x/baud/themes/day.yml",
            "/h/.config/baud/themes/day",
            "/h/.config/baud/themes/day.yaml",
            "/h/.config/baud/themes/day.yml",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        assert_eq!(candidates("day", &d), want);
    }

    /// The themes directory beside the config file in force is tried first,
    /// and a directory named twice is tried once (review, 2026-09-29).
    #[test]
    fn a_bare_name_looks_beside_the_config_in_force_first() {
        let d = Dirs {
            config: Some("/c/baud.toml".into()),
            home: Some("/h".into()),
            xdg: None,
        };
        let got = candidates("day", &d);
        assert_eq!(got[0], PathBuf::from("/c/themes/day"));
        assert_eq!(got.len(), 6, "{got:?}");
        let d = Dirs {
            config: Some("/x/baud/config.toml".into()),
            home: Some("/h".into()),
            xdg: Some("/x".into()),
        };
        let got = candidates("day", &d);
        assert_eq!(got[0], PathBuf::from("/x/baud/themes/day"));
        assert_eq!(got.len(), 6, "the XDG themes directory once: {got:?}");
        let none = Dirs {
            config: None,
            home: None,
            xdg: None,
        };
        assert!(candidates("day", &none).is_empty());
    }

    #[test]
    fn the_extension_chooses_the_reader() {
        assert!(is_tinted(Path::new("a/day.yaml")));
        assert!(is_tinted(Path::new("sepia.yml")));
        assert!(!is_tinted(Path::new("night")));
        assert!(!is_tinted(Path::new("night.conf")));
        assert!(
            is_tinted(Path::new("Day.YAML")),
            "the case of the extension does not matter"
        );
        assert!(is_tinted(Path::new("day.Yml")));
    }

    /// Each fixture, by bare name and by path, lands and says what it must.
    #[test]
    fn the_halves_load_and_say_what_they_must() {
        let h = home("load");
        let (halves, said) = load_halves(Some("day"), Some("night"), &dirs(&h));
        let light = halves.light.expect("day loads");
        assert_eq!(light.format, Format::Base16);
        let dark = halves.dark.expect("night loads, partial");
        assert_eq!(dark.format, Format::Ghostty);
        let joined = said.join("\n");
        assert!(
            joined.contains("line 13: font-family is not a colour"),
            "{joined}"
        );
        assert!(
            joined.contains("8 of the sixteen, the page and the text are unset"),
            "{joined}"
        );
        assert!(joined.contains("entries 16-255 are not used"), "{joined}");
        assert_eq!(said.len(), 3, "day says nothing: {joined}");
        let _ = std::fs::remove_dir_all(h);
    }

    #[test]
    fn a_declared_variant_and_a_half_that_disagree_are_reported() {
        let h = home("disagree");
        let (halves, said) = load_halves(Some("sepia"), Some("day.yaml"), &dirs(&h));
        assert!(
            halves.light.is_some() && halves.dark.is_some(),
            "both used as named"
        );
        let joined = said.join("\n");
        assert!(
            joined.contains("sepia.yml: declares variant dark but its page and text read light"),
            "{joined}"
        );
        assert!(
            joined.contains(
                "day.yaml: its page and text read light, and it is named as the dark half"
            ),
            "{joined}"
        );
        assert_eq!(said.len(), 2, "{joined}");
        let _ = std::fs::remove_dir_all(h);
    }

    #[test]
    fn a_file_that_fails_leaves_baud_own_half() {
        let h = home("fail");
        let (halves, said) = load_halves(Some("broken"), Some("nowhere"), &dirs(&h));
        assert!(halves.light.is_none() && halves.dark.is_none());
        let joined = said.join("\n");
        assert!(
            joined.contains(
                "broken.yaml: line 20: base0F is missing; using baud's own light palette"
            ),
            "{joined}"
        );
        assert!(
            joined.contains("[palette] dark \"nowhere\": no theme file at"),
            "{joined}"
        );
        let (_, said) = load_halves(Some("  "), None, &dirs(&h));
        assert_eq!(
            said,
            ["config: [palette] light is empty; using baud's own light palette"]
        );
        let _ = std::fs::remove_dir_all(h);
    }

    /// The first file that exists wins: XDG before HOME.
    #[test]
    fn the_first_existing_candidate_is_read() {
        let h = home("first");
        let x = h.join("xdg");
        std::fs::create_dir_all(x.join("baud/themes")).unwrap();
        std::fs::copy(fixtures().join("sepia.yml"), x.join("baud/themes/day.yaml")).unwrap();
        let d = Dirs {
            config: None,
            home: Some(h.clone()),
            xdg: Some(x),
        };
        let (_, said) = load_halves(Some("day"), None, &d);
        assert!(
            said.join("")
                .contains("xdg/baud/themes/day.yaml: declares variant dark"),
            "{said:?}"
        );
        let _ = std::fs::remove_dir_all(h);
    }

    /// A file that is there and cannot be read says so; with nowhere to
    /// look, the message says that, not an empty list.
    #[test]
    fn an_unreadable_file_and_nowhere_to_look_are_said_as_such() {
        let h = home("unreadable");
        std::fs::create_dir_all(h.join(".config/baud/themes/dir.yaml")).unwrap();
        let (halves, said) = load_halves(Some("~/.config/baud/themes/dir.yaml"), None, &dirs(&h));
        assert!(halves.light.is_none());
        assert!(
            said.join("").contains("dir.yaml cannot be read"),
            "{said:?}"
        );
        let none = Dirs {
            config: None,
            home: None,
            xdg: None,
        };
        let (_, said) = load_halves(None, Some("night"), &none);
        assert_eq!(
            said,
            [
                "config: [palette] dark \"night\": no HOME or XDG_CONFIG_HOME to look in; using baud's own dark palette"
            ]
        );
        let _ = std::fs::remove_dir_all(h);
    }

    /// A polarity notice says which of page and text the file set.
    #[test]
    fn a_polarity_notice_says_what_came_from_baud() {
        let h = home("page-only");
        let t = h.join(".config/baud/themes/pale");
        std::fs::write(&t, "background = #ffffff\n").unwrap();
        let (_, said) = load_halves(None, Some("pale"), &dirs(&h));
        let joined = said.join("\n");
        assert!(
            joined.contains(
                "its page, with baud's own text, reads light, and it is named as the dark half"
            ),
            "{joined}"
        );
        let _ = std::fs::remove_dir_all(h);
    }

    /// The theme a half draws: the built-in half, the file over it, and the
    /// half's own dim -- one function both drivers call, so a pinned light
    /// theme on Linux dims by `dim_light` as macOS does.
    #[test]
    fn a_half_is_its_file_over_its_own_and_dims_as_itself() {
        let glass = crate::config::Glass::defaults();
        let floor = baud_render::Floor::DEFAULT;
        let day = themefile::base16(&std::fs::read(fixtures().join("day.yaml")).unwrap()).unwrap();
        let halves = Halves {
            dark: None,
            light: Some(day),
        };
        let light = half_theme(Half::Light, &glass, floor, &halves);
        let mut want = Theme::light();
        want.take_scheme(&day);
        assert_eq!(
            (light.palette, light.fg, light.bg),
            (want.palette, want.fg, want.bg)
        );
        assert_eq!(
            light.bg_alpha,
            baud_domain::Chan::from_unit(glass.dim_light)
        );
        let dark = half_theme(Half::Dark, &glass, floor, &halves);
        assert_eq!(
            dark.palette,
            Theme::dark().palette,
            "the light file stays in its half"
        );
        assert_eq!(dark.bg_alpha, baud_domain::Chan::from_unit(glass.dim_dark));
        assert_eq!(
            (dark.floor, dark.app_bg, dark.picture_bg),
            (floor, glass.app_backgrounds, glass.pictures)
        );
    }
}
