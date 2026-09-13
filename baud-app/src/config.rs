//! The terminal's configuration: which faces, what size, how translucent.
//!
//! A small, dependency-free reader for a subset of TOML -- `key = value`,
//! string and integer scalars, and a bracketed list of strings that may span
//! lines. baud owns its stack rather than pulling `serde` and a TOML crate for
//! three keys; the subset is enough for a font list and two numbers, and a key
//! it does not understand is ignored rather than fatal, so a newer config does
//! not break an older binary.
//!
//! The file is looked up at `$BAUD_CONFIG`, then `$XDG_CONFIG_HOME/baud/
//! config.toml`, then `~/.config/baud/config.toml`, then `./config.toml`. When
//! none is found or a field is absent, the defaults are the built-in Menlo
//! stack -- so the app runs with no config at all.

/// A face is a file and, for a collection (`.ttc`), which face in it: the
/// path may carry `@N`, so `Menlo.ttc@0`.
pub struct FaceSpec {
    pub path: String,
    pub index: u32,
}

pub struct Config {
    /// The fallback stack, first face first.
    pub faces: Vec<FaceSpec>,
    /// Points; pixels are this times the backing scale.
    pub font_size: f32,
    /// The window background's opacity, 0 clear to 255 opaque.
    pub background_alpha: u8,
}

impl Config {
    /// The built-in defaults: the system mono, its colour emoji, and its
    /// symbols for the braille and box drawing Menlo lacks.
    pub fn defaults() -> Config {
        Config {
            faces: [
                "/System/Library/Fonts/Menlo.ttc@0",
                "/System/Library/Fonts/Apple Color Emoji.ttc@0",
                "/System/Library/Fonts/Apple Symbols.ttf",
            ]
            .iter()
            .map(|s| FaceSpec::parse(s))
            .collect(),
            font_size: 13.0,
            background_alpha: 0xD8,
        }
    }

    /// The config from the first file that exists, over the defaults; the
    /// defaults themselves when none is found.
    pub fn load() -> Config {
        let mut c = Config::defaults();
        if let Some(text) = read_first() {
            c.apply(&text);
        }
        c
    }

    fn apply(&mut self, text: &str) {
        for (key, value) in fields(text) {
            match key.as_str() {
                "font_size" => {
                    if let Ok(v) = value.trim().parse::<f32>() {
                        self.font_size = v;
                    }
                }
                "background_alpha" => {
                    if let Ok(v) = value.trim().parse::<u8>() {
                        self.background_alpha = v;
                    }
                }
                "fonts" => {
                    let faces: Vec<_> =
                        strings(&value).iter().map(|s| FaceSpec::parse(s)).collect();
                    if !faces.is_empty() {
                        self.faces = faces;
                    }
                }
                _ => {}
            }
        }
    }
}

impl FaceSpec {
    fn parse(s: &str) -> FaceSpec {
        match s
            .rsplit_once('@')
            .and_then(|(p, i)| Some((p, i.parse().ok()?)))
        {
            Some((path, index)) => FaceSpec {
                path: path.to_string(),
                index,
            },
            None => FaceSpec {
                path: s.to_string(),
                index: 0,
            },
        }
    }
}

fn read_first() -> Option<String> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let xdg = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from);
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("BAUD_CONFIG") {
        paths.push(p.into());
    }
    if let Some(x) = xdg {
        paths.push(x.join("baud/config.toml"));
    }
    if let Some(h) = &home {
        paths.push(h.join(".config/baud/config.toml"));
    }
    paths.push("config.toml".into());
    paths
        .into_iter()
        .find_map(|p| std::fs::read_to_string(p).ok())
}

/// `key = value` pairs. A value runs to the end of the line, or, when it opens
/// a `[`, to the matching `]` however many lines that takes. Comments (`#`)
/// outside a string and blank lines are skipped. Section headers (`[name]` on
/// their own line) are ignored -- the keys are flat.
fn fields(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let line = strip_comment(line);
        let trimmed = line.trim();
        if trimmed.is_empty() || (trimmed.starts_with('[') && trimmed.ends_with(']')) {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let mut value = value.trim().to_string();
        if value.starts_with('[') && !value.contains(']') {
            // A list across lines: gather until the closing bracket.
            while let Some(next) = lines.peek() {
                let n = strip_comment(next);
                value.push(' ');
                value.push_str(n.trim());
                lines.next();
                if n.contains(']') {
                    break;
                }
            }
        }
        out.push((key.trim().to_string(), value));
    }
    out
}

/// Everything before an unquoted `#`.
fn strip_comment(line: &str) -> &str {
    let mut in_str = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '"' => in_str = !in_str,
            '#' if !in_str => return &line[..i],
            _ => {}
        }
    }
    line
}

/// The quoted strings in a value, whether a lone `"x"` or a `["a", "b"]` list.
fn strings(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = value.char_indices();
    while let Some((i, ch)) = chars.next() {
        if ch == '"' {
            let rest = &value[i + 1..];
            if let Some(end) = rest.find('"') {
                out.push(rest[..end].to_string());
                // Advance past the closing quote.
                for _ in 0..rest[..=end].chars().count() {
                    chars.next();
                }
            }
        }
    }
    out
}
