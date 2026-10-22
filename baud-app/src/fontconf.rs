//! **fontconfig's font directories, read from its own configuration**, with
//! no libfontconfig: on Linux the configuration is what says which fonts are
//! installed, and where one is found its directories are the scan's list
//! (Jev's lean, C_CONF_ONLY, 0.12/0.46 then 0.80/0.84). Read from fontconfig
//! 2.15.0's source, which the user documentation (`[[source: fontconfig-user]]`)
//! describes more narrowly in places:
//!
//! * **The file.** `FONTCONFIG_FILE`, else `fonts.conf`; a name that is not
//!   absolute is looked for along `FONTCONFIG_PATH` and then the configuration
//!   directory, one that starts with `~` is `HOME` and the rest
//!   (`[[source: fontconfig-fccfg-2-15-0]]` `FcConfigGetFilename`,
//!   `FcConfigGetPath`). The directory is `SYSCONFDIR/fonts`
//!   (`[[source: fontconfig-configure-2-15-0]]`, `--with-baseconfigdir`);
//!   baud takes `/etc/fonts`, and a fontconfig built for another directory is
//!   reached through the two variables, as fontconfig itself reads them.
//! * **`<include>`** reads a file, or a directory's files whose names begin
//!   with an ASCII digit and end in `.conf`, in byte order; a file already
//!   read, by its real path, is not read again; `prefix="xdg"` puts the name
//!   under `$XDG_CONFIG_HOME` (`[[source: fontconfig-fcxml-2-15-0]]`
//!   `FcParseInclude`, `FcConfigParseAndLoadDir`, `_FcConfigParse`). The
//!   user's own `fonts.conf` is reached this way, through the system's
//!   `conf.d/50-user.conf` (`[[source: fontconfig-50-user-conf-2-15-0]]`).
//! * **`<dir>`** adds a directory. `prefix="xdg"` adds it under
//!   `$XDG_DATA_HOME` and under every `$XDG_DATA_DIRS` entry, `"relative"`
//!   under the including file's own directory, `"default"` or `"cwd"` (or
//!   none) as written, relative to the working directory (`FcParseDir`
//!   and the prefix rules it calls). A leading `~` is `HOME` and the rest,
//!   and the name is made absolute and canonical
//!   (`[[source: fontconfig-fcstr-2-15-0]]` `FcStrCopyFilename`,
//!   `FcStrCanonAbsoluteFilename`). A directory added again moves to the end
//!   (`FcStrSetAddFilenamePairWithSalt`: "Override maps with new one if
//!   exists"). `<remap-dir>` adds its directory the same way, and
//!   `<reset-dirs/>` removes every directory added so far.
//! * **The variables** are read as fontconfig reads them: an `XDG_*_HOME`
//!   that is set counts even when empty, and with none set, `HOME` and
//!   `/.local/share` or `/.config`. A `<dir prefix="xdg">` under an empty
//!   `XDG_DATA_HOME` is its name alone, made absolute against the working
//!   directory, as `FcStrBuildFilename` writes no `/` after an empty first
//!   part; an `<include prefix="xdg">` under an empty `XDG_CONFIG_HOME` is
//!   `/` and its name, as `FcParseInclude` writes the `/` itself.
//!
//! The XML is read by a small reader of its own that knows elements,
//! attributes, character data, the five predefined entities and character
//! references, comments, CDATA, processing instructions and a DOCTYPE; at the
//! first thing it cannot read it stops, keeping what came before. That is
//! fontconfig's reading only of a file included with `ignore_missing="yes"`,
//! as `conf.d` and the user's `fonts.conf` are: its streaming parser has
//! acted on what came before, and the error is not passed up.
//!
//! **Two departures, open.** fontconfig drops a whole configuration that
//! fails -- a malformed root file, or an `<include>` without
//! `ignore_missing` that is missing or malformed -- and loads
//! `FcInitFallbackConfig` instead (`[[source: fontconfig-fcinit-2-15-0]]`);
//! baud keeps what it read before the failure. And when no `fonts.conf` is
//! found, that fallback reads its default directories, `$XDG_DATA_HOME/fonts`,
//! the system's `conf.d` and the user's `fontconfig/conf.d` and
//! `fontconfig/fonts.conf`; baud uses its fixed list and reads no
//! configuration.

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// The configuration directory baud looks in: `SYSCONFDIR/fonts` with the
/// `/etc` a system's own fontconfig reads.
pub const CONFIG_DIR: &str = "/etc/fonts";

/// What fontconfig reads from the process to find and read its files.
pub struct Env {
    pub home: Option<OsString>,
    pub xdg_data_home: Option<OsString>,
    pub xdg_data_dirs: Option<OsString>,
    pub xdg_config_home: Option<OsString>,
    /// `FONTCONFIG_FILE`.
    pub file: Option<OsString>,
    /// `FONTCONFIG_PATH`.
    pub path: Option<OsString>,
    pub config_dir: PathBuf,
    pub cwd: Option<PathBuf>,
}

impl Env {
    /// This process's.
    pub fn from_process() -> Env {
        let var = std::env::var_os;
        Env {
            home: var("HOME"),
            xdg_data_home: var("XDG_DATA_HOME"),
            xdg_data_dirs: var("XDG_DATA_DIRS"),
            xdg_config_home: var("XDG_CONFIG_HOME"),
            file: var("FONTCONFIG_FILE"),
            path: var("FONTCONFIG_PATH"),
            config_dir: CONFIG_DIR.into(),
            cwd: std::env::current_dir().ok(),
        }
    }
}

/// **The font directories fontconfig's configuration names**, in its
/// order; `None` when no configuration file is found, where fontconfig
/// would load its fallback configuration (one of the module's open
/// departures).
pub fn dirs(env: &Env) -> Option<Vec<PathBuf>> {
    let name = env.file.clone().unwrap_or_else(|| "fonts.conf".into());
    let mut load = Load {
        env,
        read: HashSet::new(),
        dirs: Vec::new(),
    };
    let root = load.filename(&name)?;
    load.parse(&root);
    Some(load.dirs)
}

/// A reading of the configuration in progress.
struct Load<'e> {
    env: &'e Env,
    /// The files and directories read, by real path.
    read: HashSet<PathBuf>,
    dirs: Vec<PathBuf>,
}

/// `a` and `b` joined by one `/`, as `FcParseInclude` and
/// `FcConfigXdgDataHome` join theirs, with the separator written whatever
/// `a` is.
fn join(a: &OsStr, b: &OsStr) -> OsString {
    let mut s = a.to_os_string();
    s.push("/");
    s.push(b);
    s
}

/// `a` and `b` as `FcStrBuildFilename` joins them: a `/` between them only
/// once something is written, so an empty `a` leaves `b` as it is.
fn build_filename(a: &OsStr, b: &OsStr) -> OsString {
    if a.is_empty() {
        b.to_os_string()
    } else {
        join(a, b)
    }
}

/// `name` without its first byte, a `~`.
fn after_tilde(name: &OsStr) -> &OsStr {
    use std::os::unix::ffi::OsStrExt;
    OsStr::from_bytes(&name.as_bytes()[1..])
}

fn starts_with_tilde(name: &OsStr) -> bool {
    use std::os::unix::ffi::OsStrExt;
    name.as_bytes().first() == Some(&b'~')
}

impl Load<'_> {
    /// `FcConfigGetFilename`: the file `name` names, if it exists.
    fn filename(&self, name: &OsStr) -> Option<PathBuf> {
        let exists = |p: PathBuf| std::fs::metadata(&p).is_ok().then_some(p);
        if Path::new(name).is_absolute() {
            return exists(name.into());
        }
        if starts_with_tilde(name) {
            let mut s = self.env.home.clone()?;
            s.push(after_tilde(name));
            return exists(s.into());
        }
        let mut search: Vec<PathBuf> = self
            .env
            .path
            .iter()
            .flat_map(|p| std::env::split_paths(p).collect::<Vec<_>>())
            .collect();
        search.push(self.env.config_dir.clone());
        search
            .into_iter()
            .find_map(|d| exists(join(d.as_os_str(), name).into()))
    }

    /// `_FcConfigParse`: the file or directory at `path`, once.
    fn parse(&mut self, path: &Path) {
        let Ok(real) = path.canonicalize() else {
            return;
        };
        if !self.read.insert(real.clone()) {
            return;
        }
        if real.is_dir() {
            self.parse_dir(&real);
            return;
        }
        let Ok(bytes) = std::fs::read(&real) else {
            return;
        };
        for e in Reader::new(&bytes).elements() {
            self.element(&e, &real);
        }
    }

    /// `FcConfigParseAndLoadDir`: every `[0-9]*.conf` in `dir`, in byte
    /// order.
    fn parse_dir(&mut self, dir: &Path) {
        use std::os::unix::ffi::OsStrExt;
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut names: Vec<OsString> = rd
            .flatten()
            .map(|e| e.file_name())
            .filter(|n| {
                let b = n.as_bytes();
                b.len() > 5 && b[0].is_ascii_digit() && b.ends_with(b".conf")
            })
            .collect();
        names.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
        for n in names {
            self.parse(&dir.join(n));
        }
    }

    /// What one ended element does, read in `file`.
    fn element(&mut self, e: &Element, file: &Path) {
        match e.name.as_str() {
            "dir" => self.add_dir(&e.text, e.attr("prefix"), file),
            // A remap needs its `as-path`; without one fontconfig says so and
            // ignores the element.
            "remap-dir" if e.attr("as-path").is_some() => {
                self.add_dir(&e.text, e.attr("prefix"), file);
            }
            "reset-dirs" => self.dirs.clear(),
            "include" => self.include(&e.text, e.attr("prefix")),
            _ => {}
        }
    }

    /// `FcParseInclude`.
    fn include(&mut self, text: &str, prefix: Option<&str>) {
        let name: OsString = match prefix {
            Some("xdg") => join(
                &self.xdg_home(&self.env.xdg_config_home, ".config"),
                text.as_ref(),
            ),
            _ => text.into(),
        };
        if let Some(p) = self.filename(&name) {
            self.parse(&p);
        }
    }

    /// `$XDG_..._HOME` when set, empty or not; else `HOME` (or nothing) and
    /// `/` + `fallback`.
    fn xdg_home(&self, var: &Option<OsString>, fallback: &str) -> OsString {
        var.clone().unwrap_or_else(|| {
            join(
                self.env.home.as_deref().unwrap_or(OsStr::new("")),
                fallback.as_ref(),
            )
        })
    }

    /// `FcParseDir`: the directories `text` names under `prefix`.
    fn add_dir(&mut self, text: &str, prefix: Option<&str>, file: &Path) {
        if text.is_empty() {
            return;
        }
        let path = OsStr::new(text);
        let names: Vec<OsString> = match prefix {
            Some("xdg") => {
                let mut v = vec![build_filename(
                    &self.xdg_home(&self.env.xdg_data_home, ".local/share"),
                    path,
                )];
                v.extend(self.xdg_data_dirs().iter().map(|d| build_filename(d, path)));
                v
            }
            Some("relative") => match file.parent() {
                Some(dir) => vec![build_filename(dir.as_os_str(), path)],
                None => Vec::new(),
            },
            _ => vec![path.to_os_string()],
        };
        for n in names {
            if n.is_empty() || (starts_with_tilde(&n) && self.env.home.is_none()) {
                continue;
            }
            let Some(p) = self.canonical(&n) else {
                continue;
            };
            self.dirs.retain(|d| *d != p);
            self.dirs.push(p);
        }
    }

    /// `FcConfigXdgDataDirs`: `$XDG_DATA_DIRS`'s entries, trailing slashes
    /// trimmed, or `/usr/local/share` and `/usr/share` when it is unset.
    fn xdg_data_dirs(&self) -> Vec<OsString> {
        use std::os::unix::ffi::OsStrExt;
        let Some(v) = &self.env.xdg_data_dirs else {
            return vec!["/usr/local/share".into(), "/usr/share".into()];
        };
        v.as_bytes()
            .split(|&b| b == b':')
            .filter(|s| !s.is_empty())
            .map(|s| {
                let mut s = s;
                while s.len() > 1 && s.ends_with(b"/") {
                    s = &s[..s.len() - 1];
                }
                OsStr::from_bytes(s).to_os_string()
            })
            .collect()
    }

    /// `FcStrCopyFilename`: a leading `~` made `HOME`, the name made
    /// absolute against the working directory, then `//`, `/.` and `/..`
    /// folded and a final `/` trimmed.
    fn canonical(&self, name: &OsStr) -> Option<PathBuf> {
        use std::os::unix::ffi::OsStrExt;
        let full: OsString = if starts_with_tilde(name) {
            let mut s = self.env.home.clone()?;
            s.push(after_tilde(name));
            s
        } else {
            name.to_os_string()
        };
        let full = if full.as_bytes().first() == Some(&b'/') {
            full
        } else {
            join(self.env.cwd.as_deref()?.as_os_str(), &full)
        };
        let mut parts: Vec<&[u8]> = Vec::new();
        for seg in full.as_bytes().split(|&b| b == b'/') {
            match seg {
                b"" | b"." => {}
                b".." => {
                    parts.pop();
                }
                s => parts.push(s),
            }
        }
        let mut out = Vec::new();
        for p in parts {
            out.push(b'/');
            out.extend_from_slice(p);
        }
        if out.is_empty() {
            out.push(b'/');
        }
        Some(PathBuf::from(OsStr::from_bytes(&out)))
    }
}

/// One element the reader has seen end: its name, its attributes, and the
/// character data directly inside it.
struct Element {
    name: String,
    attrs: Vec<(String, String)>,
    text: String,
}

impl Element {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// A small XML reader over one document.
struct Reader<'b> {
    /// What is left to read.
    rest: &'b [u8],
}

impl<'b> Reader<'b> {
    fn new(src: &'b [u8]) -> Reader<'b> {
        Reader { rest: src }
    }

    fn rest(&self) -> &'b [u8] {
        self.rest
    }

    /// Past the next `end`, or `None` when the document has none.
    fn skip_past(&mut self, end: &[u8]) -> Option<()> {
        let i = self.rest().windows(end.len()).position(|w| w == end)?;
        self.rest = &self.rest[i + end.len()..];
        Some(())
    }

    /// Every element, in the order each ends; the elements read before
    /// anything malformed.
    fn elements(mut self) -> Vec<Element> {
        let mut done = Vec::new();
        let mut open: Vec<Element> = Vec::new();
        while !self.rest.is_empty() {
            let rest = self.rest();
            if rest.starts_with(b"<!--") {
                if self.skip_past(b"-->").is_none() {
                    break;
                }
            } else if rest.starts_with(b"<![CDATA[") {
                let body = &rest[9..];
                self.rest = body;
                if self.skip_past(b"]]>").is_none() {
                    break;
                }
                let inside = &body[..body.len() - self.rest.len() - 3];
                if let Some(e) = open.last_mut() {
                    e.text.push_str(&String::from_utf8_lossy(inside));
                }
            } else if rest.starts_with(b"<?") {
                if self.skip_past(b"?>").is_none() {
                    break;
                }
            } else if rest.starts_with(b"<!") {
                if self.doctype().is_none() {
                    break;
                }
            } else if rest.starts_with(b"</") {
                self.rest = &self.rest[2..];
                let Some(name) = self.name() else { break };
                self.space();
                if self.rest().first() != Some(&b'>') {
                    break;
                }
                self.rest = &self.rest[1..];
                match open.pop() {
                    Some(e) if e.name == name => done.push(e),
                    _ => break,
                }
            } else if rest.starts_with(b"<") {
                self.rest = &self.rest[1..];
                let Some((e, empty)) = self.start() else {
                    break;
                };
                if empty {
                    done.push(e);
                } else {
                    open.push(e);
                }
            } else {
                let len = rest.iter().position(|&b| b == b'<').unwrap_or(rest.len());
                self.rest = &rest[len..];
                let Some(text) = unescape(&rest[..len]) else {
                    break;
                };
                if let Some(e) = open.last_mut() {
                    e.text.push_str(&text);
                }
            }
        }
        done
    }

    /// A `<!DOCTYPE ...>`, with any internal subset in brackets.
    fn doctype(&mut self) -> Option<()> {
        let mut depth = 0usize;
        while let Some(&b) = self.rest().first() {
            self.rest = &self.rest[1..];
            match b {
                b'[' => depth += 1,
                b']' => depth = depth.checked_sub(1)?,
                b'>' if depth == 0 => return Some(()),
                _ => {}
            }
        }
        None
    }

    fn space(&mut self) {
        while self.rest().first().is_some_and(u8::is_ascii_whitespace) {
            self.rest = &self.rest[1..];
        }
    }

    /// An element or attribute name.
    fn name(&mut self) -> Option<String> {
        let rest = self.rest();
        let len = rest
            .iter()
            .position(|&b| b.is_ascii_whitespace() || b"/>=<\"'".contains(&b))
            .unwrap_or(rest.len());
        if len == 0 {
            return None;
        }
        self.rest = &self.rest[len..];
        Some(String::from_utf8_lossy(&rest[..len]).into_owned())
    }

    /// A start tag after its `<`: the element, and whether it is empty.
    fn start(&mut self) -> Option<(Element, bool)> {
        let name = self.name()?;
        let mut attrs = Vec::new();
        loop {
            self.space();
            match self.rest().first()? {
                b'>' => {
                    self.rest = &self.rest[1..];
                    let e = Element {
                        name,
                        attrs,
                        text: String::new(),
                    };
                    return Some((e, false));
                }
                b'/' => {
                    if self.rest().get(1) != Some(&b'>') {
                        return None;
                    }
                    self.rest = &self.rest[2..];
                    let e = Element {
                        name,
                        attrs,
                        text: String::new(),
                    };
                    return Some((e, true));
                }
                _ => {
                    let key = self.name()?;
                    self.space();
                    if self.rest().first() != Some(&b'=') {
                        return None;
                    }
                    self.rest = &self.rest[1..];
                    self.space();
                    let quote = *self.rest().first()?;
                    if quote != b'"' && quote != b'\'' {
                        return None;
                    }
                    self.rest = &self.rest[1..];
                    let len = self.rest().iter().position(|&b| b == quote)?;
                    let value = unescape(&self.rest[..len])?;
                    self.rest = &self.rest[len + 1..];
                    attrs.push((key, value));
                }
            }
        }
    }
}

/// Character data with its references read: the five predefined entities
/// and `&#N;` / `&#xN;`; `None` for any other reference.
fn unescape(b: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(b);
    let mut out = String::with_capacity(s.len());
    let mut rest: &str = &s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i + 1..];
        let end = rest.find(';')?;
        let r = &rest[..end];
        let c = match r {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let n = match r.strip_prefix("#x") {
                    Some(h) => u32::from_str_radix(h, 16).ok()?,
                    None => r.strip_prefix('#')?.parse().ok()?,
                };
                char::from_u32(n)?
            }
        };
        out.push(c);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory of this test's own, canonical (macOS puts the
    /// temporary directory behind a symbolic link).
    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("baud-b145-fc-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    fn write(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn env(root: &Path) -> Env {
        Env {
            home: Some(root.join("home").into()),
            xdg_data_home: None,
            xdg_data_dirs: None,
            xdg_config_home: None,
            file: None,
            path: None,
            config_dir: root.join("etc/fonts"),
            cwd: Some(root.join("cwd")),
        }
    }

    fn shown(root: &Path, dirs: &[PathBuf]) -> Vec<String> {
        let r = root.display().to_string();
        dirs.iter()
            .map(|d| d.display().to_string().replace(&r, "R"))
            .collect()
    }

    /// The system file as fontconfig 2.15.0's `fonts.conf.in` lays it out,
    /// and its `conf.d/50-user.conf` as that release ships it.
    fn system(root: &Path) {
        write(
            &root.join("etc/fonts/fonts.conf"),
            r#"<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
<!-- Font directory list <dir>/commented/out</dir> -->
	<dir>/usr/share/fonts</dir>
	<dir prefix="xdg">fonts</dir>
	<!-- the following element will be removed in the future -->
	<dir>~/.fonts</dir>
	<include ignore_missing="yes">conf.d</include>
</fontconfig>
"#,
        );
        write(
            &root.join("etc/fonts/conf.d/50-user.conf"),
            r#"<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
  <description>Load per-user customization files</description>
	<include ignore_missing="yes" prefix="xdg">fontconfig/conf.d</include>
	<include ignore_missing="yes" prefix="xdg">fontconfig/fonts.conf</include>
	<include ignore_missing="yes" deprecated="yes">~/.fonts.conf.d</include>
	<include ignore_missing="yes" deprecated="yes">~/.fonts.conf</include>
</fontconfig>
"#,
        );
    }

    /// **The system file, its `conf.d` in order, the user's file through
    /// `50-user.conf`, and every prefix**: `xdg` under `$XDG_DATA_HOME`'s
    /// default and both default `$XDG_DATA_DIRS` entries, `~` under `HOME`,
    /// `relative` beside the including file, none relative to the working
    /// directory; a directory added again moves to the end; a `conf.d` file
    /// whose name does not begin with a digit or end in `.conf` is not
    /// read; a file included twice is read once; entities and CDATA are
    /// character data, and a commented-out element is no element.
    #[test]
    fn the_configuration_names_the_directories() {
        let root = scratch("all");
        system(&root);
        write(
            &root.join("etc/fonts/conf.d/10-relative.conf"),
            r#"<fontconfig><dir prefix="relative">rel</dir><dir>plain/./x/../y/</dir></fontconfig>"#,
        );
        write(
            &root.join("etc/fonts/conf.d/60-again.conf"),
            r#"<fontconfig>
  <dir>/usr/share/fonts</dir>
  <remap-dir as-path="/fonts">/srv/fonts</remap-dir>
  <remap-dir>/no/as-path</remap-dir>
  <include>conf.d</include>
</fontconfig>"#,
        );
        write(
            &root.join("etc/fonts/conf.d/x.conf"),
            "<fontconfig><dir>/not-a-digit</dir></fontconfig>",
        );
        write(
            &root.join("etc/fonts/conf.d/7-skip.conf.bak"),
            "<fontconfig><dir>/bak</dir></fontconfig>",
        );
        write(
            &root.join("home/.config/fontconfig/fonts.conf"),
            "<fontconfig><dir>~/Fonts &amp; more</dir><dir><![CDATA[/cdata<dir>]]></dir></fontconfig>",
        );
        let got = dirs(&env(&root)).expect("a configuration");
        assert_eq!(
            shown(&root, &got),
            [
                "R/home/.local/share/fonts",
                "/usr/local/share/fonts",
                "R/home/.fonts",
                "R/etc/fonts/conf.d/rel",
                "R/cwd/plain/y",
                "R/home/Fonts & more",
                "/cdata<dir>",
                "/usr/share/fonts",
                "/srv/fonts",
            ]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// **`<reset-dirs/>` leaves only what follows it**: a user's file that
    /// resets keeps the system's directories out.
    #[test]
    fn reset_dirs_keeps_only_what_follows() {
        let root = scratch("reset");
        system(&root);
        write(
            &root.join("home/.config/fontconfig/fonts.conf"),
            "<fontconfig><reset-dirs /><dir>/mine</dir></fontconfig>",
        );
        assert_eq!(dirs(&env(&root)).unwrap(), [PathBuf::from("/mine")]);
        let _ = std::fs::remove_dir_all(root);
    }

    /// **The variables, as fontconfig reads them**: an `XDG_DATA_HOME` or
    /// `XDG_CONFIG_HOME` that is set counts even when empty, the data home
    /// as no prefix at all and the configuration home as `/`;
    /// `XDG_DATA_DIRS`' entries lose their trailing slashes; with no
    /// `HOME`, a `~` directory is skipped and the data home is
    /// `/.local/share`.
    #[test]
    fn the_variables_as_fontconfig_reads_them() {
        let root = scratch("vars");
        system(&root);
        let mut e = env(&root);
        e.xdg_data_home = Some(root.join("data").into());
        e.xdg_data_dirs = Some(format!("/a/:{}//:", root.join("b").display()).into());
        e.xdg_config_home = Some(root.join("cfg").into());
        write(
            &root.join("cfg/fontconfig/fonts.conf"),
            "<fontconfig><dir>/from-xdg-config</dir></fontconfig>",
        );
        assert_eq!(
            shown(&root, &dirs(&e).unwrap()),
            [
                "/usr/share/fonts",
                "R/data/fonts",
                "/a/fonts",
                "R/b/fonts",
                "R/home/.fonts",
                "/from-xdg-config",
            ]
        );
        e.xdg_data_home = Some("".into());
        assert_eq!(
            shown(&root, &dirs(&e).unwrap())[1],
            "R/cwd/fonts",
            "set and empty"
        );
        write(
            &root.join("etc/fonts/fontconfig/fonts.conf"),
            "<fontconfig><dir>/beside-the-system-file</dir></fontconfig>",
        );
        e.xdg_config_home = Some("".into());
        assert!(
            !shown(&root, &dirs(&e).unwrap()).contains(&"/beside-the-system-file".to_string()),
            "set and empty: the include is /fontconfig/fonts.conf"
        );
        let mut e = env(&root);
        e.home = None;
        assert_eq!(
            shown(&root, &dirs(&e).unwrap()),
            [
                "/.local/share/fonts",
                "/usr/local/share/fonts",
                "/usr/share/fonts"
            ]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// **Which file is read**: `FONTCONFIG_FILE` when it names one, a
    /// name looked for along `FONTCONFIG_PATH` before the configuration
    /// directory, and no configuration at all when none is found.
    #[test]
    fn the_file_fontconfig_reads() {
        let root = scratch("file");
        system(&root);
        write(
            &root.join("other.conf"),
            "<fontconfig><dir>/other</dir></fontconfig>",
        );
        write(
            &root.join("path/fonts.conf"),
            "<fontconfig><dir>/on-the-path</dir></fontconfig>",
        );
        let mut e = env(&root);
        e.file = Some(root.join("other.conf").into());
        assert_eq!(dirs(&e).unwrap(), [PathBuf::from("/other")]);
        let mut e = env(&root);
        e.path = Some(root.join("path").into());
        assert_eq!(dirs(&e).unwrap(), [PathBuf::from("/on-the-path")]);
        let mut e = env(&root);
        e.config_dir = root.join("nowhere");
        assert_eq!(dirs(&e), None);
        let _ = std::fs::remove_dir_all(root);
    }

    /// **What comes before a malformed part is kept**: an end tag that
    /// closes the wrong element stops the reading, and an unknown entity
    /// does too. fontconfig reads an `ignore_missing` include this way; a
    /// root file like this one it drops whole for its fallback
    /// configuration, an open departure (the module's doc).
    #[test]
    fn a_malformed_file_keeps_what_came_before() {
        let root = scratch("bad");
        let mut e = env(&root);
        e.file = Some(root.join("bad.conf").into());
        write(
            &root.join("bad.conf"),
            "<fontconfig><dir>/a</dir><dir>/b</fontconfig><dir>/c</dir>",
        );
        assert_eq!(dirs(&e).unwrap(), [PathBuf::from("/a")]);
        write(
            &root.join("bad.conf"),
            "<fontconfig><dir>/a</dir><dir>/b&nbsp;</dir><dir>/c</dir></fontconfig>",
        );
        assert_eq!(dirs(&e).unwrap(), [PathBuf::from("/a")]);
        let _ = std::fs::remove_dir_all(root);
    }
}
