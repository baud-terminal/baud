//! The theme files `[palette] light` and `dark` name (note Y, step 7).
//!
//! `config.rs`'s `apply` stays pure and stores the two strings; reading the
//! files is I/O and happens here, at launch, with its **own complaint list**,
//! so a file that fails is reported and leaves baud's own half in place
//! rather than stopping the terminal.
//!
//! **The content chooses the reader** (note AR; the owner's call of
//! 2026-10-17): [`themefile::detect::classify`] gives one verdict and only
//! that verdict's reader runs. Every load says what the file was read as and
//! the line that decided; a file named for another format is read by its
//! content and the name reported (Jev, O1, 0.90/0.92, decisions.jsonl
//! 680/681). A bare name is tried as given, then with `.yaml`, then `.yml`
//! (the owner, 2026-10-17, keeping Jev's 0.65 order); a scheme whose
//! polarity is the other half's is used as named and reported (Jev, 0.93).

use std::path::{Path, PathBuf};

use baud_domain::ColorScheme;
use baud_domain::themefile::detect::{self, Grammar, Kind, Mark, NotText, Shape, Unknown, Verdict};
use baud_domain::themefile::{self, Cpp, Format, Scheme, SchemeErrorKind, Skipped};
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
    /// The config's selection colours, laid over either half's file.
    pub selection: crate::config::Selection,
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
            home: crate::config::absolute_dir(std::env::var_os("HOME")),
            xdg: crate::config::absolute_dir(std::env::var_os("XDG_CONFIG_HOME")),
        }
    }
}

/// Both halves the config names, and what loading them had to say.
pub fn load_halves(light: Option<&str>, dark: Option<&str>, dirs: &Dirs) -> (Halves, Vec<String>) {
    let mut said = Vec::new();
    let halves = Halves {
        dark: dark.and_then(|s| load(s, Half::Dark, dirs, &mut said)),
        light: light.and_then(|s| load(s, Half::Light, dirs, &mut said)),
        selection: crate::config::Selection::default(),
    };
    (halves, said)
}

/// The paths a value names, in the order they are tried. A value with a `/`
/// is a path: `~/` expanded from `HOME`, and none when there is no `HOME`;
/// a relative one beside the config file in force, the owner's call of
/// 2026-10-13, not under the directory baud was started in. A bare name is
/// looked up in the `themes` directory beside the config file in force,
/// then in `$XDG_CONFIG_HOME/baud/themes` and `~/.config/baud/themes`, each
/// directory once, as given and then with `.yaml` and `.yml`.
pub fn candidates(spec: &str, dirs: &Dirs) -> Vec<PathBuf> {
    if spec.contains('/') {
        if let Some(rest) = spec.strip_prefix("~/") {
            return dirs.home.iter().map(|home| home.join(rest)).collect();
        }
        let path = Path::new(spec);
        let beside = dirs.config.as_deref().and_then(Path::parent);
        return vec![match beside {
            Some(dir) if path.is_relative() => dir.join(path),
            _ => path.to_path_buf(),
        }];
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

/// The formats a file's extension names, for the cross-check: the name
/// never chooses the reader, and one the content disagrees with is
/// reported. `None` for a name that implies nothing, no extension included.
fn implied(path: &Path) -> Option<(&'static str, &'static [Kind])> {
    let name = path.file_name()?.to_str()?;
    if name.eq_ignore_ascii_case(".Xresources") || name.eq_ignore_ascii_case(".Xdefaults") {
        return Some((name_of_dotfile(name), &[Kind::Xresources]));
    }
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "yaml" => (
            ".yaml",
            &[
                Kind::Tinted,
                Kind::Tinted8,
                Kind::ColorNN,
                Kind::AlacrittyYaml,
            ],
        ),
        "yml" => (
            ".yml",
            &[
                Kind::Tinted,
                Kind::Tinted8,
                Kind::ColorNN,
                Kind::AlacrittyYaml,
            ],
        ),
        "conf" => (".conf", &[Kind::Kitty]),
        "toml" => (".toml", &[Kind::Alacritty, Kind::Wezterm]),
        "ini" => (".ini", &[Kind::Foot]),
        "itermcolors" => (".itermcolors", &[Kind::ITermPlist]),
        "json" => (".json", &[Kind::WindowsTerminal]),
        "xresources" => (".Xresources", &[Kind::Xresources]),
        "xdefaults" => (".Xdefaults", &[Kind::Xresources]),
        _ => return None,
    })
}

fn name_of_dotfile(name: &str) -> &'static str {
    if name.eq_ignore_ascii_case(".Xresources") {
        ".Xresources"
    } else {
        ".Xdefaults"
    }
}

/// What a format is called in a report.
fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Tinted => "a base16 or base24 scheme",
        Kind::Ghostty => "a ghostty theme",
        Kind::PlainList => "a list of 16 colours",
        Kind::Kitty => "a kitty theme",
        Kind::Foot => "a foot theme",
        Kind::Alacritty => "an alacritty theme",
        Kind::AlacrittyYaml => "an alacritty YAML config",
        Kind::ITermPlist => "an iTerm2 colour preset",
        Kind::WindowsTerminal => "a Windows Terminal scheme",
        Kind::Xresources => "X resources",
        Kind::Tinted8 => "a tinted8 scheme",
        Kind::ColorNN => "a color_01-color_16 YAML scheme (Gogh, iTerm2-Color-Schemes)",
        Kind::Wezterm => "a wezterm scheme",
    }
}

/// What a read scheme is called: the reader's own answer, where it gives
/// one (base16 or base24).
fn format_name(f: Format) -> &'static str {
    match f {
        Format::Base16 => "a base16 scheme",
        Format::Base24 => "a base24 scheme",
        Format::Ghostty => "a ghostty theme",
        Format::Kitty => "a kitty theme",
        Format::Foot => "a foot theme",
        Format::Alacritty => "an alacritty theme",
        Format::AlacrittyYaml => "an alacritty YAML config",
        Format::ITermPlist => "an iTerm2 colour preset",
        Format::WindowsTerminal => "a Windows Terminal scheme",
        Format::PlainList => "a list of 16 colours",
        Format::Xresources => "X resources",
        Format::Empty => "an empty file",
    }
}

fn grammar_name(g: Grammar) -> &'static str {
    match g {
        Grammar::Colon => "`key: value` lines (YAML)",
        Grammar::Equals => "`key = value` lines",
        Grammar::Space => "`key value` lines",
        Grammar::Resource => "`*.resource: value` lines (X resources)",
        Grammar::Bare => "a word alone on a line",
        Grammar::Other => "a line that is no theme grammar",
    }
}

fn shape_name(s: Shape) -> &'static str {
    match s {
        Shape::Grammar(g) => grammar_name(g),
        Shape::Kind(k) => kind_name(k),
    }
}

/// `line N: key`, for a report.
fn at_line(m: Mark<'_>) -> String {
    format!("line {}: {}", m.line.0, String::from_utf8_lossy(m.key))
}

/// An `#include "name"`: an absolute name as it is, else beside the
/// including file, as cpp looks for a quoted name first.
fn included(from: &Path, name: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    let name = Path::new(std::ffi::OsStr::from_bytes(name));
    match from.parent() {
        Some(dir) if name.is_relative() => dir.join(name),
        _ => name.to_path_buf(),
    }
}

/// Why a file was not read, from what was seen in it.
fn refusal(v: Verdict<'_>) -> String {
    match v {
        Verdict::Is(k, m) => format!(
            "{} ({}), which baud does not read",
            kind_name(k),
            at_line(m)
        ),
        Verdict::Mixed(a, ma, b, mb) => format!(
            "two formats in one file: {} at line {} and {} at line {}",
            shape_name(a),
            ma.line.0,
            shape_name(b),
            mb.line.0
        ),
        Verdict::Unknown(u) => match u {
            Unknown::HexLines(n) => {
                format!("{} lines of #rrggbb, where a list of colours has 16", n.0)
            }
            Unknown::Xml => "XML that is not an iTerm2 colour preset (no Ansi 0 Color)".into(),
            Unknown::Json => "JSON that is not a Windows Terminal scheme".into(),
            Unknown::JsonSchemes(m) => format!(
                "a Windows Terminal settings file holding several schemes ({}); save the one to use as its own file",
                at_line(m)
            ),
            Unknown::Sections(m) => format!("sections no theme format uses ({})", at_line(m)),
            Unknown::FootRemovedSection(m) => format!(
                "foot's [colors] section ({}), which foot no longer reads; name it [colors-dark] or [colors-light]",
                at_line(m)
            ),
            Unknown::FootOutsideSection(m) => format!(
                "foot colour keys outside a [colors-dark] or [colors-light] section ({})",
                at_line(m)
            ),
            Unknown::Yaml(m) => format!(
                "YAML with none of system, base00-base17, scheme.system, color_01 or colors ({})",
                at_line(m)
            ),
            Unknown::TintedSystem(m) => format!(
                "a tinted scheme whose scheme.system is not tinted8 ({})",
                at_line(m)
            ),
            Unknown::TintedSlotsInEquals(m) => format!(
                "base16 slot names in `key = value` syntax ({}); base16 and base24 are YAML, `key: value`",
                at_line(m)
            ),
            Unknown::Lines(g, m) => format!(
                "{} with no colour key a theme format has ({})",
                grammar_name(g),
                at_line(m)
            ),
        },
        Verdict::NotText(NotText::BinaryPlist) => {
            "a binary plist; save the iTerm2 preset as XML".into()
        }
        Verdict::Empty => "an empty file".into(),
    }
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
    let verdict = detect::classify(&bytes);
    let body = detect::body(&bytes);
    // Only the verdict's reader runs; what it ignored is said after what
    // the file was read as.
    let mut ignored = Vec::new();
    // The included file a refusal is in, for X resources.
    let mut failed_in = None;
    let (read, mark) = match verdict {
        Verdict::Is(Kind::Tinted, m) => (themefile::base16(body), Some(m)),
        Verdict::Is(Kind::Ghostty, m) => (
            themefile::ghostty(body, |line, key| {
                ignored.push(format!(
                    "{at}: line {}: {} is not a colour and is ignored",
                    line.0,
                    String::from_utf8_lossy(key)
                ))
            }),
            Some(m),
        ),
        Verdict::Is(
            k @ (Kind::Kitty
            | Kind::Foot
            | Kind::Alacritty
            | Kind::AlacrittyYaml
            | Kind::ITermPlist
            | Kind::WindowsTerminal),
            m,
        ) => {
            let mut skip = |line: themefile::Line, key: &[u8], why: Skipped| {
                ignored.push(skipped(&at, k, line, key, why));
            };
            let read = match k {
                Kind::Kitty => themefile::kitty(body, &mut skip),
                Kind::Foot => themefile::foot(body, &mut skip),
                Kind::Alacritty => themefile::alacritty(body, &mut skip),
                Kind::AlacrittyYaml => themefile::alacritty_yaml(body, &mut skip),
                Kind::ITermPlist => themefile::iterm(body, &mut skip),
                _ => themefile::windows_terminal(body, &mut skip),
            };
            (read, Some(m))
        }
        Verdict::Is(Kind::PlainList, m) => (themefile::plain_list(body), Some(m)),
        Verdict::Is(Kind::Xresources, m) => {
            let root = path.clone();
            let read = themefile::xresources(
                body,
                &root,
                |from: &PathBuf, name: &[u8]| {
                    let p = included(from, name);
                    std::fs::read(&p).ok().map(|b| (p, b))
                },
                |file: &PathBuf, line, key: &[u8], why| {
                    let at = if *file == root {
                        at.clone()
                    } else {
                        format!("{at}: in {}", file.display())
                    };
                    ignored.push(skipped(&at, Kind::Xresources, line, key, why));
                },
            )
            .map_err(|(file, e)| {
                failed_in = file;
                e
            });
            (read, Some(m))
        }
        Verdict::Empty => (Ok(Scheme::empty(Format::Empty)), None),
        other => {
            said.push(format!("{at}: {}; {own}", refusal(other)));
            return None;
        }
    };
    let why = mark
        .map(|m| format!(" ({})", at_line(m)))
        .unwrap_or_default();
    let scheme = match read {
        Ok(s) => s,
        Err(e) => {
            let name = match verdict {
                Verdict::Is(k, _) => kind_name(k),
                _ => format_name(Format::Empty),
            };
            let why = mark
                .map(|m| format!(", {}", at_line(m)))
                .unwrap_or_default();
            let inside = failed_in
                .map(|f| format!(" in {}", f.display()))
                .unwrap_or_default();
            said.push(format!(
                "{at}: line {}{inside}: {} (read as {name}{why}); {own}",
                e.line.0,
                reason(e.kind)
            ));
            return None;
        }
    };
    said.push(format!("{at}: read as {}{why}", format_name(scheme.format)));
    said.append(&mut ignored);
    if let (Verdict::Is(k, _), Some((ext, kinds))) = (verdict, implied(path)) {
        if !kinds.contains(&k) {
            said.push(format!(
                "{at}: named {ext}, but the content is {}",
                kind_name(k)
            ));
        }
    }
    notice(&at, half, &scheme, said);
    Some(scheme)
}

/// A line a reader passed over, said with why.
fn skipped(at: &str, k: Kind, line: themefile::Line, key: &[u8], why: Skipped) -> String {
    let key = String::from_utf8_lossy(key);
    let terminal = match k {
        Kind::Kitty => "kitty",
        Kind::Foot => "foot",
        Kind::Alacritty | Kind::AlacrittyYaml => "alacritty",
        Kind::ITermPlist => "iTerm2",
        Kind::WindowsTerminal => "Windows Terminal",
        Kind::Xresources => "xterm",
        _ => "its terminal",
    };
    let what = match why {
        Skipped::NotAColour => format!("{key} is not a colour and is ignored"),
        Skipped::Refused => {
            format!("{key} has a value {terminal} does not read either, and is ignored")
        }
        Skipped::Section => format!("{key} holds no colours and is ignored"),
        Skipped::Syntax if matches!(k, Kind::Xresources) => {
            format!("{key} has no colon, so xrdb passes it over; ignored")
        }
        Skipped::Syntax => format!("{key} is not a {terminal} setting and is ignored"),
        Skipped::Unheld => format!(
            "{key} is one of {terminal}'s own colours, which a theme here has no place for; ignored"
        ),
        Skipped::Clipped => format!("{key} is outside sRGB and is clipped, as macOS clips it"),
        Skipped::CellRelative => {
            format!(
                "{key} names the cell's own colour, which a theme here cannot hold; baud's stays"
            )
        }
        Skipped::Program => {
            let program = key
                .split(['.', '*'])
                .find(|c| !c.is_empty())
                .unwrap_or_default();
            format!(
                "{key} is for {program} alone, and baud reads only the names every client reads (*name or *.name); ignored"
            )
        }
        Skipped::NoFile => {
            format!("#include \"{key}\" names a file that cannot be read; passed over")
        }
        Skipped::Nested => format!(
            "#include \"{key}\" is nested more than {} files deep, where X stops; not read",
            themefile::XRM_DEPTH
        ),
        Skipped::Directive => {
            format!(
                "#{key} is a C preprocessor directive baud does not run; passed over, as cpp passes it"
            )
        }
    };
    format!("{at}: line {}: {what}", line.0)
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
    // base16 and base24 name 16-21 by slot, and that is their mapping, not
    // a choice the file made.
    let numbered = matches!(
        scheme.format,
        Format::Ghostty
            | Format::Kitty
            | Format::Foot
            | Format::Alacritty
            | Format::AlacrittyYaml
            | Format::Xresources
    );
    if numbered && scheme.extended.iter().any(Option::is_some) {
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
    theme.selection = halves.selection.page.over(theme.selection);
    theme.selection_text = halves.selection.text.over(theme.selection_text);
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
        SchemeErrorKind::NoName => "no name, which a Windows Terminal scheme requires".into(),
        SchemeErrorKind::Count => "not the sixteen colours the format requires".into(),
        SchemeErrorKind::NotUtf8 => "a byte that is not UTF-8, which the format requires".into(),
        SchemeErrorKind::Cpp(c) => format!(
            "{}, which baud's C preprocessor subset does not run",
            match c {
                Cpp::If => "#if",
                Cpp::Elif => "#elif",
                Cpp::MacroArguments => "a #define with arguments",
                Cpp::AngleInclude => "#include <...>, which searches the system's include path",
                Cpp::IncludeName => "an #include with no quoted file name",
                Cpp::NoName => "a directive with no macro name",
                Cpp::Unmatched => "an #else or #endif with no #ifdef open",
                Cpp::Unterminated => "an #ifdef or /* comment the file does not close",
                Cpp::Nesting => "conditionals nested more than 64 deep",
                Cpp::Macros => "more #define text than baud holds",
                Cpp::Long => "a directive longer than 4096 bytes",
            }
        ),
        SchemeErrorKind::Pair => {
            "a second theme: the file holds a dark and a light one, so save the one for this half alone".into()
        }
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

    /// **A relative theme path is read beside the config file**, the
    /// owner's call of 2026-10-13, not under the directory baud started
    /// in. An absolute path is kept and `~/` is still home; a `~/` with no
    /// home to expand is looked for nowhere, rather than as a directory
    /// named `~` under the working directory.
    #[test]
    fn a_relative_theme_path_resolves_against_the_config_files_directory() {
        let d = Dirs {
            config: Some("/c/baud/config.toml".into()),
            home: Some("/h".into()),
            xdg: None,
        };
        assert_eq!(
            candidates("themes/night.yaml", &d),
            [PathBuf::from("/c/baud/themes/night.yaml")]
        );
        assert_eq!(
            candidates("../shared/day", &d),
            [PathBuf::from("/c/baud/../shared/day")]
        );
        assert_eq!(candidates("/abs/night", &d), [PathBuf::from("/abs/night")]);
        assert_eq!(
            candidates("~/t/day.yaml", &d),
            [PathBuf::from("/h/t/day.yaml")]
        );
        let homeless = Dirs { home: None, ..d };
        assert!(
            candidates("~/t/day.yaml", &homeless).is_empty(),
            "{:?}",
            candidates("~/t/day.yaml", &homeless)
        );
    }

    /// **The name only cross-checks the content**, which chooses the
    /// reader: base16 YAML named `.conf` is read as base16 and the name
    /// reported; the extension's case does not matter, and a name that
    /// implies nothing is not reported.
    #[test]
    fn the_content_chooses_the_reader_and_the_name_only_cross_checks() {
        let tinted: &[Kind] = &[
            Kind::Tinted,
            Kind::Tinted8,
            Kind::ColorNN,
            Kind::AlacrittyYaml,
        ];
        assert_eq!(implied(Path::new("a/day.yaml")), Some((".yaml", tinted)));
        assert_eq!(implied(Path::new("Day.YAML")), Some((".yaml", tinted)));
        assert_eq!(implied(Path::new("sepia.Yml")), Some((".yml", tinted)));
        assert_eq!(implied(Path::new("night")), None);
        assert_eq!(implied(Path::new("colors.txt")), None);
        assert_eq!(
            implied(Path::new("t/.Xresources")),
            Some((".Xresources", &[Kind::Xresources][..]))
        );
        let h = home_with("day-conf", &[]);
        let t = h.join(".config/baud/themes/day.conf");
        std::fs::copy(fixtures().join("day.yaml"), &t).unwrap();
        let (halves, said) = load_halves(None, Some("day.conf"), &dirs(&h));
        let said = said.join("\n");
        assert_eq!(
            halves.dark.map(|s| s.format),
            Some(Format::Base16),
            "{said}"
        );
        assert!(
            said.contains("day.conf: named .conf, but the content is a base16 or base24 scheme"),
            "{said}"
        );
        let _ = std::fs::remove_dir_all(h);
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
        assert!(
            joined.contains("night: read as a ghostty theme (line 2: palette)"),
            "{joined}"
        );
        assert!(
            joined.contains("day.yaml: read as a base16 scheme (line 1: system)"),
            "{joined}"
        );
        assert_eq!(said.len(), 5, "day says only what it was read as: {joined}");
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
        assert_eq!(said.len(), 4, "two read-as lines and two notices: {joined}");
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
                "broken.yaml: line 20: base0F is missing (read as a base16 or base24 scheme, line 1: system); using baud's own light palette"
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

    /// A home whose `.config/baud/themes` holds the named fixtures.
    fn home_with(tag: &str, files: &[&str]) -> PathBuf {
        let h = std::env::temp_dir().join(format!("baud-palette-{}-{tag}", std::process::id()));
        let themes = h.join(".config/baud/themes");
        std::fs::create_dir_all(&themes).unwrap();
        for f in files {
            std::fs::copy(fixtures().join(f), themes.join(f)).unwrap();
        }
        h
    }

    /// One fixture loaded as the dark half: the scheme and what was said.
    fn dark_of(tag: &str, file: &str) -> (Option<Scheme>, String) {
        let h = home_with(tag, &[file]);
        let (halves, said) = load_halves(None, Some(file), &dirs(&h));
        let _ = std::fs::remove_dir_all(h);
        (halves.dark, said.join("\n"))
    }

    fn rgb(hex: &str) -> baud_domain::Color {
        themefile::hex(hex.as_bytes()).unwrap()
    }

    /// **A double-quoted alacritty file never draws its search colour as
    /// the page.** Read line by line as ghostty, its section headers were
    /// ignored and the last `background =`, `[colors.search.matches]`'s
    /// `#ffff00`, became the page (note AR, section 1.3).
    #[test]
    fn an_alacritty_file_never_draws_its_search_colour_as_the_page() {
        let (dark, said) = dark_of("alacritty", "alacritty.toml");
        assert_ne!(
            dark.and_then(|s| s.page),
            Some(rgb("#ffff00")),
            "the search match colour is the page: {said}"
        );
        assert!(said.contains("an alacritty theme"), "{said}");
        let colours = dark.map(|s| Scheme {
            format: Format::Empty,
            ..s
        });
        assert_eq!(colours, Some(fixture_night()), "{said}");
        assert!(
            said.contains("line 16: background is one of alacritty's own colours"),
            "the search colours are said, not drawn: {said}"
        );
    }

    /// **alacritty's YAML config, before 0.13, is read too**, with `0x`
    /// colours and an `indexed_colors` entry.
    #[test]
    fn an_alacritty_yaml_config_reads_as_the_colours_it_carries() {
        let (got, said) = dark_of("alacritty-yml", "alacritty.yml");
        let colours = got.map(|s| Scheme {
            format: Format::Empty,
            ..s
        });
        let mut want = fixture_night();
        want.extended[0] = Some(rgb("#ff00ff"));
        assert_eq!(colours, Some(want), "{said}");
        assert!(
            said.contains("read as an alacritty YAML config (line 4: primary)"),
            "{said}"
        );
    }

    /// **A base16 file with no extension is read as base16**, with the
    /// colours of the same file named `.yaml`.
    #[test]
    fn a_base16_file_without_an_extension_is_read_as_base16() {
        let (dawn, said) = dark_of("dawn", "dawn");
        let day = themefile::base16(&std::fs::read(fixtures().join("day.yaml")).unwrap()).unwrap();
        assert_eq!(dawn, Some(day), "{said}");
        assert!(said.contains("read as a base16 scheme"), "{said}");
    }

    /// **A ghostty theme named `.yaml` is read as ghostty**, and the name
    /// that says otherwise is reported (Jev, O1, decisions.jsonl 680/681).
    #[test]
    fn a_ghostty_theme_named_yaml_is_read_as_ghostty_and_the_name_reported() {
        let (mislabelled, said) = dark_of("night-yaml", "night.yaml");
        let (night, _) = dark_of("night-plain", "night");
        assert!(night.is_some());
        assert_eq!(mislabelled, night, "{said}");
        assert!(said.contains("read as a ghostty theme"), "{said}");
        assert!(said.contains("named .yaml"), "{said}");
    }

    /// Each other format is named for what it is, never read as a
    /// ghostty file whose lines are all ignored or all comments.
    #[test]
    fn every_other_format_is_named_for_what_it_is() {
        for (file, name) in [
            ("tinted8.yaml", "a tinted8 scheme"),
            ("Xresources", "X resources"),
        ] {
            let (_, said) = dark_of(file, file);
            assert!(said.contains(name), "{file}: {said}");
            assert!(
                !said.contains("is not a colour and is ignored"),
                "{file}: {said}"
            );
            assert!(!said.contains("are unset"), "{file}: {said}");
        }
    }

    /// Two grammars in one file are refused naming both lines; tinted's
    /// slot names in ghostty's grammar are refused saying so (Jev, O10).
    #[test]
    fn a_mixed_file_and_tinted_slots_in_equals_syntax_are_refused() {
        let (mixed, said) = dark_of("mixed", "mixed");
        assert!(mixed.is_none(), "{said}");
        assert!(
            said.contains("two formats in one file: `key: value` lines (YAML) at line 1 and `key = value` lines at line 5"),
            "{said}"
        );
        let (eq, said) = dark_of("slots-eq", "slots-eq");
        assert!(eq.is_none(), "{said}");
        assert!(said.contains("base16 slot names"), "{said}");
    }

    /// The dark scheme every other terminal's fixture carries
    /// (`tests/themes/README.md`), each in its own spelling.
    fn fixture_night() -> Scheme {
        let ansi = [
            "#1d1f21", "#cc6666", "#b5bd68", "#f0c674", "#81a2be", "#b294bb", "#8abeb7", "#c5c8c6",
            "#666666", "#d54e53", "#b9ca4a", "#e7c547", "#7aa6da", "#c397d8", "#70c0b1", "#eaeaea",
        ];
        let mut s = Scheme::empty(Format::Empty);
        s.ansi = ansi.map(|h| Some(rgb(h)));
        s.page = Some(rgb("#161719"));
        s.text = Some(rgb("#c5c8c7"));
        s.cursor = Some(rgb("#aeafad"));
        s.cursor_text = Some(rgb("#1d1f22"));
        s.selection = Some(rgb("#373b41"));
        s.selection_text = Some(rgb("#c5c8c8"));
        s
    }

    /// **kitty's and foot's own theme files are read**, each to the colours
    /// it carries, and each says what it was read as.
    #[test]
    fn kitty_and_foot_files_read_as_the_colours_they_carry() {
        for (file, why) in [
            ("kitty.conf", "read as a kitty theme (line 3: foreground)"),
            ("foot.ini", "read as a foot theme (line 2: colors-dark)"),
        ] {
            let (got, said) = dark_of(file, file);
            let colours = got.map(|s| Scheme {
                format: Format::Empty,
                ..s
            });
            assert_eq!(colours, Some(fixture_night()), "{file}: {said}");
            assert!(said.contains(why), "{file}: {said}");
            assert!(!said.contains("ignored"), "{file}: {said}");
        }
    }

    /// **iTerm2's presets and Windows Terminal's schemes are read**, each
    /// to the colours it carries; Windows Terminal has no cursor text or
    /// selection text colour.
    #[test]
    fn iterm2_and_windows_terminal_files_read_as_the_colours_they_carry() {
        let (got, said) = dark_of("iterm", "night.itermcolors");
        let colours = got.map(|s| Scheme {
            format: Format::Empty,
            ..s
        });
        assert_eq!(colours, Some(fixture_night()), "{said}");
        assert!(
            said.contains("read as an iTerm2 colour preset (line 6: Ansi 0 Color)"),
            "{said}"
        );
        assert!(
            said.contains("Bold Color is one of iTerm2's own colours"),
            "{said}"
        );
        let (got, said) = dark_of("wt", "windows-terminal.json");
        let colours = got.map(|s| Scheme {
            format: Format::Empty,
            ..s
        });
        let mut want = fixture_night();
        want.cursor_text = None;
        want.selection_text = None;
        assert_eq!(colours, Some(want), "{said}");
        assert!(
            said.contains("read as a Windows Terminal scheme (line 16: brightBlack)"),
            "{said}"
        );
    }

    /// **A list of sixteen colours is read** as ANSI 0 to 15, as pywal's
    /// `colors` file holds them; its page and text stay baud's own (Jev,
    /// O11), and the notice says so.
    #[test]
    fn a_list_of_sixteen_colours_reads_as_ansi_0_to_15() {
        let (got, said) = dark_of("colors", "colors");
        let mut want = fixture_night();
        (want.page, want.text) = (None, None);
        (
            want.cursor,
            want.cursor_text,
            want.selection,
            want.selection_text,
        ) = (None, None, None, None);
        let colours = got.map(|s| Scheme {
            format: Format::Empty,
            ..s
        });
        assert_eq!(colours, Some(want), "{said}");
        assert!(
            said.contains("read as a list of 16 colours (line 1: #1d1f21)"),
            "{said}"
        );
        assert!(
            said.contains("2 of the sixteen, the page and the text are unset"),
            "{said}"
        );
    }

    /// **A file is read as bytes wherever its format is**: a ghostty
    /// theme with a Latin-1 comment reads with the colours base read from
    /// it, since ghostty takes a line as bytes and a comment is passed
    /// over whole (`[[source: ghostty-args-d9840f3]]` 1450-1466).
    #[test]
    fn a_ghostty_theme_with_a_latin_1_comment_reads_as_base_read_it() {
        let bytes = b"# Th\xe8me d'\xe9t\xe9\npalette = 1=#cc6666\nbackground = #1d1f21\n";
        let h = home_with("latin1", &[]);
        std::fs::write(h.join(".config/baud/themes/latin1"), bytes).unwrap();
        let (halves, said) = load_halves(None, Some("latin1"), &dirs(&h));
        let _ = std::fs::remove_dir_all(h);
        let said = said.join("\n");
        let base = themefile::ghostty(bytes, |_, _| {}).unwrap();
        assert_eq!(halves.dark, Some(base), "{said}");
        assert!(said.contains("read as a ghostty theme"), "{said}");
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
            selection: crate::config::Selection::default(),
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

    /// **The config's selection colours win over a theme file's, and a
    /// file's over reverse video** (Jev, 0.97 both orders: a key set after
    /// the theme wins, as in ghostty and kitty); `"none"` takes a file's
    /// colour away. The file here is iTerm2's, which names both.
    #[test]
    fn the_configs_selection_wins_over_the_files_and_none_takes_it_away() {
        use crate::config::{Selection, SelectionPick};
        let glass = crate::config::Glass::defaults();
        let floor = baud_render::Floor::DEFAULT;
        let (night, _) = dark_of("iterm-sel", "night.itermcolors");
        let file = |selection| Halves {
            dark: night,
            light: None,
            selection,
        };
        let drawn = |h: &Halves, half| {
            let t = half_theme(half, &glass, floor, h);
            (t.selection, t.selection_text)
        };
        // fixture_night's selection and selected text.
        let from_file = (
            Some(baud_render::Argb(0xFF_37_3B_41)),
            Some(baud_render::Argb(0xFF_C5_C8_C8)),
        );
        assert_eq!(drawn(&file(Selection::default()), Half::Dark), from_file);
        assert_eq!(
            drawn(&file(Selection::default()), Half::Light),
            (None, None),
            "a half with no file is reverse video"
        );
        let mine = baud_render::Argb::rgb(
            baud_domain::Chan::from_u8(1),
            baud_domain::Chan::from_u8(2),
            baud_domain::Chan::from_u8(3),
        );
        let config = Selection {
            page: SelectionPick::Colour(mine),
            text: SelectionPick::None,
        };
        assert_eq!(drawn(&file(config), Half::Dark), (Some(mine), None));
        assert_eq!(
            drawn(&file(config), Half::Light),
            (Some(mine), None),
            "over both halves"
        );
    }

    /// **X resources are read as xrdb loads them**: a tinted file through
    /// its `#define`s, as `cpp -P -DCOLOR` gives it.
    #[test]
    fn a_tinted_xresources_file_reads_as_cpp_gives_it() {
        let (got, said) = dark_of("xres-tinted", "base16-0x96f.Xresources");
        let s = got.unwrap_or_else(|| panic!("{said}"));
        assert_eq!(s.page, Some(rgb("#262427")), "{said}");
        assert_eq!(s.ansi[1], Some(rgb("#ff7272")), "{said}");
        assert!(
            said.contains("read as X resources (line 22: *foreground)"),
            "{said}"
        );
        assert!(!said.contains("ignored"), "{said}");
    }

    /// **A file of urxvt's entries alone loads empty, naming urxvt**: its
    /// names are for that program, and baud reads only every client's.
    #[test]
    fn an_xresources_file_for_urxvt_alone_loads_empty_naming_it() {
        let (got, said) = dark_of("xres-urxvt", "urxvt.Xresources");
        let s = got.unwrap_or_else(|| panic!("{said}"));
        assert_eq!(
            Scheme {
                format: Format::Empty,
                ..s
            },
            Scheme::empty(Format::Empty)
        );
        assert!(
            said.contains("line 2: URxvt*foreground is for URxvt alone"),
            "{said}"
        );
        assert!(
            said.contains("line 5: urxvt*color1 is for urxvt alone"),
            "{said}"
        );
        assert!(
            said.contains("18 of the sixteen, the page and the text are unset"),
            "{said}"
        );
    }

    /// **An `#include` beside the file is read**, its macro used after it.
    #[test]
    fn an_xresources_include_beside_the_file_is_read() {
        let h = home_with("xres-include", &["xres-main", "xres-colours"]);
        let (halves, said) = load_halves(None, Some("xres-main"), &dirs(&h));
        let _ = std::fs::remove_dir_all(h);
        let said = said.join("\n");
        let s = halves.dark.unwrap_or_else(|| panic!("{said}"));
        assert_eq!(s.page, Some(rgb("#161719")), "{said}");
        assert_eq!(s.text, Some(rgb("#c5c8c7")), "{said}");
        assert_eq!(s.cursor, Some(rgb("#aeafad")), "{said}");
    }

    /// **An empty or comments-only file loads empty**, setting nothing and
    /// saying so (Jev decided load_empty_with_notice 0.98 / 0.98,
    /// decisions.jsonl 710/711): TOML's grammar allows an empty document,
    /// and ghostty, kitty, foot and alacritty each load one and set nothing.
    #[test]
    fn an_empty_or_comments_only_file_loads_empty_with_the_unset_notice() {
        for (tag, text) in [
            ("empty", ""),
            ("comments", "# a theme to come\n\n! later\n"),
        ] {
            let h = home_with(&format!("empty-{tag}"), &[]);
            std::fs::write(h.join(".config/baud/themes/blank"), text).unwrap();
            let (halves, said) = load_halves(None, Some("blank"), &dirs(&h));
            let _ = std::fs::remove_dir_all(h);
            let said = said.join("\n");
            assert_eq!(
                halves.dark,
                Some(Scheme::empty(Format::Empty)),
                "{tag}: {said}"
            );
            assert!(said.contains("read as an empty file"), "{tag}: {said}");
            assert!(
                said.contains(
                    "18 of the sixteen, the page and the text are unset and stay baud's own"
                ),
                "{tag}: {said}"
            );
        }
    }
}
