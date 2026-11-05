//! **baud's starship preset, `assets/starship/baud.toml`: every key one
//! starship 1.27 reads, every colour baud's, and the guide that ships it.**
//!
//! The preset is checked offline against starship 1.27.0's own schema,
//! pinned in `docs/research/sources/starship-1.27.0-config-schema.json`
//! (`[[source: starship-1270-config-schema]]`): starship reports an
//! unknown key or a value of the wrong type only as a warning on stderr
//! and then draws its default for it, so a misspelt key reads as a prompt
//! that is quietly not the preset. The check here is the schema's own
//! rules -- `properties`, `additionalProperties`, `type`, `items`, `$ref`,
//! `anyOf`, `minimum` -- read with a small JSON and TOML reader, because
//! the workspace takes no parser for either.
//!
//! The palettes are held to baud's colours, so the preset cannot drift
//! from them: `amber` is the brand's amber on the dark page and the
//! light theme's amber on the light one, and every other name is the
//! theme entry its comment names, at the contrast the comment states.

use std::fs;
use std::path::PathBuf;

use baud_render::{Argb, BaudCursor, Theme};

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn read(rel: &str) -> String {
    fs::read_to_string(workspace().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

const PRESET: &str = "assets/starship/baud.toml";
const SCHEMA: &str = "docs/research/sources/starship-1.27.0-config-schema.json";
const GUIDE: &str = "docs/guide/starship.md";

/// **The light theme's amber, `#986100`**: the brand's amber darkened in
/// OKLCH lightness alone, its hue kept, to read 4.52:1 on the light page,
/// the value baud's light cursor is drawn in ([`BaudCursor::light`]), so
/// the ❯ and the block beside it are one colour.
const LIGHT_AMBER: u32 = 0x98_6100;

// ---------------------------------------------------------------- JSON

#[derive(Debug, Clone, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(kv) => kv.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

struct JsonReader<'a> {
    s: &'a [u8],
    i: usize,
}

impl JsonReader<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }

    fn eat(&mut self, b: u8) {
        self.ws();
        assert_eq!(
            self.s.get(self.i),
            Some(&b),
            "JSON: {} at {}",
            b as char,
            self.i
        );
        self.i += 1;
    }

    fn value(&mut self) -> Json {
        self.ws();
        match self.s[self.i] {
            b'{' => {
                self.i += 1;
                let mut kv = Vec::new();
                self.ws();
                if self.s[self.i] == b'}' {
                    self.i += 1;
                    return Json::Obj(kv);
                }
                loop {
                    self.ws();
                    let Json::Str(k) = self.value() else {
                        panic!("JSON: a key that is not a string at {}", self.i)
                    };
                    self.eat(b':');
                    kv.push((k, self.value()));
                    self.ws();
                    match self.s[self.i] {
                        b',' => self.i += 1,
                        b'}' => {
                            self.i += 1;
                            return Json::Obj(kv);
                        }
                        c => panic!("JSON: {} at {}", c as char, self.i),
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.s[self.i] == b']' {
                    self.i += 1;
                    return Json::Arr(items);
                }
                loop {
                    items.push(self.value());
                    self.ws();
                    match self.s[self.i] {
                        b',' => self.i += 1,
                        b']' => {
                            self.i += 1;
                            return Json::Arr(items);
                        }
                        c => panic!("JSON: {} at {}", c as char, self.i),
                    }
                }
            }
            b'"' => Json::Str(self.string()),
            b't' => self.word("true", Json::Bool(true)),
            b'f' => self.word("false", Json::Bool(false)),
            b'n' => self.word("null", Json::Null),
            _ => {
                let start = self.i;
                while self.i < self.s.len()
                    && matches!(
                        self.s[self.i],
                        b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
                    )
                {
                    self.i += 1;
                }
                let text = std::str::from_utf8(&self.s[start..self.i]).expect("ASCII");
                Json::Num(
                    text.parse()
                        .unwrap_or_else(|_| panic!("JSON number {text:?}")),
                )
            }
        }
    }

    fn word(&mut self, w: &str, v: Json) -> Json {
        assert!(
            self.s[self.i..].starts_with(w.as_bytes()),
            "JSON at {}",
            self.i
        );
        self.i += w.len();
        v
    }

    fn hex4(&mut self) -> u32 {
        let h = std::str::from_utf8(&self.s[self.i..self.i + 4]).expect("ASCII");
        self.i += 4;
        u32::from_str_radix(h, 16).expect("\\u escape")
    }

    fn string(&mut self) -> String {
        self.i += 1;
        let mut out = Vec::new();
        loop {
            let c = self.s[self.i];
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).expect("UTF-8"),
                b'\\' => {
                    let e = self.s[self.i];
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4();
                            let cp = if (0xD800..0xDC00).contains(&hi) {
                                assert_eq!(&self.s[self.i..self.i + 2], b"\\u", "a lone surrogate");
                                self.i += 2;
                                let lo = self.hex4();
                                0x1_0000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else {
                                hi
                            };
                            char::from_u32(cp).expect("a scalar value")
                        }
                        _ => panic!("JSON escape \\{}", e as char),
                    };
                    let mut b = [0; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
                }
                _ => out.push(c),
            }
        }
    }
}

fn json(text: &str) -> Json {
    let mut r = JsonReader {
        s: text.as_bytes(),
        i: 0,
    };
    let v = r.value();
    r.ws();
    assert_eq!(r.i, r.s.len(), "JSON: text after the value");
    v
}

// ---------------------------------------------------------------- TOML

/// The TOML the preset and the guide's snippets are written in: tables,
/// arrays of tables, bare and quoted keys, basic, literal and multi-line
/// strings, integers, booleans and one-line arrays. Anything else panics,
/// so a construct this reader does not know cannot pass unread.
#[derive(Debug, Clone, PartialEq)]
enum Toml {
    Str(String),
    Int(i64),
    Bool(bool),
    Arr(Vec<Toml>),
    Table(Vec<(String, Toml)>),
}

impl Toml {
    fn get(&self, key: &str) -> Option<&Toml> {
        match self {
            Toml::Table(kv) => kv.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn str(&self) -> &str {
        match self {
            Toml::Str(s) => s,
            other => panic!("not a string: {other:?}"),
        }
    }
}

/// The table at `path` under `root`, made if missing; an array of tables
/// is entered at its last element.
fn table_at<'a>(root: &'a mut Toml, path: &[String]) -> &'a mut Vec<(String, Toml)> {
    let mut t = root;
    for key in path {
        let Toml::Table(kv) = t else {
            panic!("TOML: {key} under a value")
        };
        if !kv.iter().any(|(k, _)| k == key) {
            kv.push((key.clone(), Toml::Table(Vec::new())));
        }
        let at = kv.iter().position(|(k, _)| k == key).expect("just made");
        t = &mut kv[at].1;
        if let Toml::Arr(items) = t {
            t = items.last_mut().expect("an array of tables has a table");
        }
    }
    match t {
        Toml::Table(kv) => kv,
        other => panic!("TOML: not a table: {other:?}"),
    }
}

fn keys(spec: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = spec.trim();
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('"') {
            let end = r.find('"').expect("a quoted key ends");
            out.push(r[..end].to_string());
            rest = r[end + 1..].trim_start();
        } else {
            let end = rest.find('.').unwrap_or(rest.len());
            out.push(rest[..end].trim().to_string());
            rest = &rest[end..];
        }
        rest = rest.strip_prefix('.').unwrap_or(rest).trim_start();
    }
    assert!(out.iter().all(|k| !k.is_empty()), "TOML key {spec:?}");
    out
}

struct TomlReader<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> TomlReader<'a> {
    fn rest(&self) -> &'a str {
        &self.s[self.i..]
    }

    fn blank(&mut self) {
        let r = self.rest();
        self.i += r.len() - r.trim_start_matches([' ', '\t']).len();
    }

    fn value(&mut self) -> Toml {
        self.blank();
        let r = self.rest();
        if let Some(body) = r.strip_prefix("\"\"\"") {
            let end = body.find("\"\"\"").expect("a multi-line string ends");
            self.i += 3 + end + 3;
            let raw = body[..end].strip_prefix('\n').unwrap_or(&body[..end]);
            // A backslash at a line's end joins the next line, its
            // leading blanks dropped.
            let mut joined = String::new();
            let mut lines = raw.split('\n').peekable();
            while let Some(line) = lines.next() {
                match line.strip_suffix('\\') {
                    Some(l) => {
                        joined.push_str(l);
                        if let Some(next) = lines.peek_mut() {
                            *next = next.trim_start();
                        }
                    }
                    None => {
                        joined.push_str(line);
                        if lines.peek().is_some() {
                            joined.push('\n');
                        }
                    }
                }
            }
            return Toml::Str(unescape(&joined));
        }
        if let Some(body) = r.strip_prefix('"') {
            let mut end = 0;
            let b = body.as_bytes();
            while b[end] != b'"' {
                end += if b[end] == b'\\' { 2 } else { 1 };
            }
            self.i += 1 + end + 1;
            return Toml::Str(unescape(&body[..end]));
        }
        if let Some(body) = r.strip_prefix('\'') {
            let end = body.find('\'').expect("a literal string ends");
            self.i += 1 + end + 1;
            return Toml::Str(body[..end].to_string());
        }
        if r.starts_with('[') {
            self.i += 1;
            let mut items = Vec::new();
            loop {
                self.blank();
                if self.rest().starts_with(']') {
                    self.i += 1;
                    return Toml::Arr(items);
                }
                items.push(self.value());
                self.blank();
                if self.rest().starts_with(',') {
                    self.i += 1;
                }
            }
        }
        for (w, v) in [("true", true), ("false", false)] {
            if r.starts_with(w) {
                self.i += w.len();
                return Toml::Bool(v);
            }
        }
        let end = r
            .find(|c: char| !(c.is_ascii_digit() || c == '_' || c == '-' || c == '+'))
            .unwrap_or(r.len());
        assert!(end > 0, "TOML: a value this reader does not know: {r:?}");
        self.i += end;
        let digits: String = r[..end].chars().filter(|&c| c != '_').collect();
        Toml::Int(
            digits
                .parse()
                .unwrap_or_else(|_| panic!("TOML integer {digits:?}")),
        )
    }
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(u @ ('u' | 'U')) => {
                let n = if u == 'u' { 4 } else { 8 };
                let h: String = it.by_ref().take(n).collect();
                let cp = u32::from_str_radix(&h, 16).expect("\\u escape");
                out.push(char::from_u32(cp).expect("a scalar value"));
            }
            other => panic!("TOML escape \\{other:?}"),
        }
    }
    out
}

/// Where a comment starts on `line`, outside any string.
fn comment_at(line: &str) -> usize {
    let (mut quote, mut escaped) = (None, false);
    for (i, c) in line.char_indices() {
        match (quote, c) {
            (Some('"'), '\\') if !escaped => {
                escaped = true;
                continue;
            }
            (Some(q), c) if c == q && !escaped => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, '#') => return i,
            _ => {}
        }
        escaped = false;
    }
    line.len()
}

fn toml(text: &str) -> Toml {
    let mut root = Toml::Table(Vec::new());
    let mut path: Vec<String> = Vec::new();
    let mut r = TomlReader { s: text, i: 0 };
    while r.i < text.len() {
        let line_end = r.rest().find('\n').map_or(text.len(), |n| r.i + n);
        let line = &text[r.i..line_end];
        let code = line[..comment_at(line)].trim();
        if code.is_empty() {
            r.i = (line_end + 1).min(text.len());
            continue;
        }
        if let Some(spec) = code.strip_prefix("[[").and_then(|c| c.strip_suffix("]]")) {
            let full = keys(spec);
            let (last, parent) = full.split_last().expect("a name");
            let kv = table_at(&mut root, parent);
            match kv.iter_mut().find(|(k, _)| k == last) {
                Some((_, Toml::Arr(items))) => items.push(Toml::Table(Vec::new())),
                Some(_) => panic!("TOML: {spec} is not an array of tables"),
                None => kv.push((last.clone(), Toml::Arr(vec![Toml::Table(Vec::new())]))),
            }
            path = full;
            r.i = (line_end + 1).min(text.len());
            continue;
        }
        if let Some(spec) = code.strip_prefix('[').and_then(|c| c.strip_suffix(']')) {
            path = keys(spec);
            table_at(&mut root, &path);
            r.i = (line_end + 1).min(text.len());
            continue;
        }
        let eq = code
            .find('=')
            .unwrap_or_else(|| panic!("TOML: not key = value: {code:?}"));
        let key = keys(&code[..eq]);
        r.i += line.find('=').expect("the same =") + 1;
        let v = r.value();
        let rest_end = r.rest().find('\n').map_or(text.len(), |n| r.i + n);
        let tail = &text[r.i..rest_end];
        assert!(
            tail[..comment_at(tail)].trim().is_empty(),
            "TOML: text after the value of {key:?}: {tail:?}"
        );
        r.i = (rest_end + 1).min(text.len());
        let mut full = path.clone();
        full.extend(key);
        let (last, parent) = full.split_last().expect("a key");
        let kv = table_at(&mut root, parent);
        assert!(
            !kv.iter().any(|(k, _)| k == last),
            "TOML: {last} given twice"
        );
        kv.push((last.clone(), v));
    }
    root
}

// -------------------------------------------------------------- schema

/// Every way `value` at `path` breaks `schema`, under `root`'s `$defs`.
fn check(value: &Toml, schema: &Json, root: &Json, path: &str, errors: &mut Vec<String>) {
    let Json::Obj(rules) = schema else {
        if *schema == Json::Bool(false) {
            errors.push(format!("{path}: not allowed"));
        }
        return;
    };
    for (rule, arg) in rules {
        match rule.as_str() {
            "$ref" => {
                let Json::Str(r) = arg else { panic!("$ref") };
                let name = r.strip_prefix("#/$defs/").expect("a local $ref");
                let def = root
                    .get("$defs")
                    .and_then(|d| d.get(name))
                    .expect("the $ref's def");
                check(value, def, root, path, errors);
            }
            "anyOf" | "oneOf" => {
                let Json::Arr(branches) = arg else {
                    panic!("{rule}")
                };
                let fits = branches
                    .iter()
                    .filter(|b| {
                        let mut e = Vec::new();
                        check(value, b, root, path, &mut e);
                        e.is_empty()
                    })
                    .count();
                let ok = if rule == "anyOf" {
                    fits >= 1
                } else {
                    fits == 1
                };
                if !ok {
                    errors.push(format!("{path}: fits {fits} of the {rule} branches"));
                }
            }
            "type" => {
                let names: Vec<&str> = match arg {
                    Json::Str(t) => vec![t.as_str()],
                    Json::Arr(ts) => ts
                        .iter()
                        .map(|t| match t {
                            Json::Str(t) => t.as_str(),
                            _ => panic!("type"),
                        })
                        .collect(),
                    _ => panic!("type"),
                };
                let is = match value {
                    Toml::Str(_) => "string",
                    Toml::Int(_) => "integer",
                    Toml::Bool(_) => "boolean",
                    Toml::Arr(_) => "array",
                    Toml::Table(_) => "object",
                };
                if !names
                    .iter()
                    .any(|&n| n == is || (n == "number" && is == "integer"))
                {
                    errors.push(format!("{path}: a {is} where the schema wants {names:?}"));
                }
            }
            "properties" | "additionalProperties" => {
                let Toml::Table(kv) = value else { continue };
                if rule == "additionalProperties" {
                    let named = schema.get("properties");
                    for (k, v) in kv {
                        if named.and_then(|p| p.get(k)).is_none() {
                            check(v, arg, root, &format!("{path}.{k}"), errors);
                        }
                    }
                } else {
                    for (k, v) in kv {
                        if let Some(s) = arg.get(k) {
                            check(v, s, root, &format!("{path}.{k}"), errors);
                        }
                    }
                }
            }
            "items" => {
                if let Toml::Arr(items) = value {
                    for (i, v) in items.iter().enumerate() {
                        check(v, arg, root, &format!("{path}[{i}]"), errors);
                    }
                }
            }
            "minimum" | "maximum" => {
                let (Toml::Int(n), Json::Num(bound)) = (value, arg) else {
                    continue;
                };
                let n = *n as f64;
                if (rule == "minimum" && n < *bound) || (rule == "maximum" && n > *bound) {
                    errors.push(format!("{path}: {n} past the {rule} {bound}"));
                }
            }
            "required" => {
                let Json::Arr(names) = arg else {
                    panic!("required")
                };
                for n in names {
                    let Json::Str(n) = n else { panic!("required") };
                    if value.get(n).is_none() {
                        errors.push(format!("{path}: no {n}"));
                    }
                }
            }
            // Annotations: they describe, and constrain nothing.
            "default" | "description" | "format" | "title" | "$schema" | "$defs" => {}
            other => panic!("{path}: a schema rule this check does not read: {other}"),
        }
    }
}

fn schema() -> Json {
    json(&read(SCHEMA))
}

fn errors_of(text: &str, schema: &Json) -> Vec<String> {
    let mut errors = Vec::new();
    check(&toml(text), schema, schema, "", &mut errors);
    errors
}

/// **The preset is a starship 1.27 config**: every key one the schema
/// names and every value of its type. The negative control: the same file
/// with a key starship does not have and a battery threshold written as a
/// string gives exactly those two errors. Perturbation: misspell
/// `truncate_to_repo` and the first check fails.
#[test]
fn the_preset_is_valid_against_starship_s_schema() {
    let schema = schema();
    assert_eq!(
        schema.get("title"),
        Some(&Json::Str("FullConfig".to_string())),
        "the pinned file is starship's root schema"
    );
    let preset = read(PRESET);
    assert_eq!(errors_of(&preset, &schema), Vec::<String>::new());
    let bad = format!(
        "bogus_key = 1\n{}",
        preset.replace("threshold = 10", "threshold = \"10\"")
    );
    let found = errors_of(&bad, &schema);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(
        found.iter().any(|e| e.starts_with(".bogus_key")),
        "{found:?}"
    );
    assert!(found.iter().any(|e| e.contains("threshold")), "{found:?}");
}

/// **Every `$name` in the format is a module starship has**, so a
/// misspelt module is not dropped from the prompt in silence; and the
/// format draws no `$fill` and no `$line_break`, which pad or add a row.
#[test]
fn the_format_names_only_modules_starship_has() {
    let schema = schema();
    let props = schema.get("properties").expect("properties");
    let preset = toml(&read(PRESET));
    let format = preset.get("format").expect("a format").str();
    let mut names = Vec::new();
    let mut rest = format;
    while let Some(i) = rest.find('$') {
        rest = &rest[i + 1..];
        let end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        names.push(&rest[..end]);
    }
    assert!(names.len() >= 10, "{names:?}");
    for n in &names {
        assert!(props.get(n).is_some(), "${n} is no module of starship 1.27");
    }
    for unwanted in ["fill", "line_break", "all"] {
        assert!(!names.contains(&unwanted), "${unwanted}");
    }
    assert!(format.ends_with("$character"), "the ❯ last");
}

/// **One palette line, and no aliases**: the guide's switch to the light
/// palette edits the line `palette = "baud-dark"`, so it is there exactly
/// once, and the preset carries no `aliases`, which would rename the
/// person's own hosts or users.
#[test]
fn the_palette_line_is_there_once_and_nothing_is_aliased() {
    let text = read(PRESET);
    assert_eq!(
        text.lines()
            .filter(|l| *l == "palette = \"baud-dark\"")
            .count(),
        1
    );
    fn no_aliases(t: &Toml, at: &str) {
        if let Toml::Table(kv) = t {
            for (k, v) in kv {
                assert!(!k.ends_with("aliases"), "{at}.{k}");
                no_aliases(v, &format!("{at}.{k}"));
            }
        }
    }
    no_aliases(&toml(&text), "");
}

fn argb(hex: &str) -> u32 {
    let h = hex.strip_prefix('#').expect("a hex colour");
    assert_eq!(h.len(), 6, "{hex}");
    u32::from_str_radix(h, 16).expect("hex")
}

fn rgb(c: Argb) -> u32 {
    c.0 & 0xFF_FFFF
}

fn lin(v: u32) -> f64 {
    let v = f64::from(v) / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn lum(c: u32) -> f64 {
    0.2126 * lin(c >> 16 & 0xFF) + 0.7152 * lin(c >> 8 & 0xFF) + 0.0722 * lin(c & 0xFF)
}

/// WCAG 2's contrast ratio.
fn contrast(a: u32, b: u32) -> f64 {
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// The entry starship's colour `name` is among the terminal's sixteen.
fn entry(name: &str) -> usize {
    let base = [
        "black", "red", "green", "yellow", "blue", "purple", "cyan", "white",
    ];
    let (bright, n) = match name.strip_prefix("bright-") {
        Some(n) => (8, n),
        None => (0, name),
    };
    bright
        + base
            .iter()
            .position(|b| *b == n)
            .unwrap_or_else(|| panic!("{name}"))
}

/// Each `name = "value"` of the `[palettes.<which>]` table, with the
/// comment lines right above it.
fn palette_lines(text: &str, which: &str) -> Vec<(String, String, String)> {
    let head = format!("[palettes.{which}]");
    let body = &text[text.find(&head).expect("the palette") + head.len()..];
    let mut out = Vec::new();
    let mut said = String::new();
    for line in body.lines() {
        if line.starts_with('[') {
            break;
        }
        if let Some(c) = line.strip_prefix('#') {
            said.push_str(c);
            said.push(' ');
        } else if let Some((k, v)) = line.split_once(" = ") {
            out.push((
                k.to_string(),
                v.trim_matches('"').to_string(),
                std::mem::take(&mut said),
            ));
        }
    }
    out
}

/// The first `N.NN:1` in `said`.
fn ratio(said: &str) -> f64 {
    let at = said
        .find(":1")
        .unwrap_or_else(|| panic!("no ratio in {said:?}"));
    let start = said[..at]
        .rfind(|c: char| !(c.is_ascii_digit() || c == '.'))
        .map_or(0, |i| i + 1);
    said[start..at]
        .parse()
        .unwrap_or_else(|_| panic!("{said:?}"))
}

/// The first `#rrggbb` in `said`.
fn hex_in(said: &str) -> u32 {
    let at = said
        .find('#')
        .unwrap_or_else(|| panic!("no colour in {said:?}"));
    argb(&said[at..at + 7])
}

/// **Each palette is baud's colours, at the contrast its comments state.**
/// `amber` is the brand's amber on the dark page -- the brand script's
/// `AMBER` and the lockup's -- and [`LIGHT_AMBER`] on the light page;
/// every other name is one of the terminal's sixteen, the entry its
/// comment names, which is the value baud's theme draws for that entry,
/// at the ratio the comment gives on that theme's page. The control: the
/// light page's yellow, which the comment says sits too near the amber,
/// is a different entry from what `note` names there.
#[test]
fn the_palettes_are_baud_s_colours() {
    let text = read(PRESET);
    let brand = read("scripts/make-brand-pictures.py");
    let line = brand
        .lines()
        .find(|l| l.starts_with("AMBER = \""))
        .expect("the brand script's AMBER");
    let amber = argb(line.trim_start_matches("AMBER = \"").trim_end_matches('"'));
    assert_eq!(
        amber,
        rgb(baud_render::splash::DARK.amber),
        "the script's and the lockup's"
    );
    // The cursor's amber on each page, the ❯'s neighbour.
    assert_eq!(rgb(BaudCursor::dark().fill), amber, "the dark cursor");
    assert_eq!(
        rgb(BaudCursor::light().fill),
        LIGHT_AMBER,
        "the light cursor"
    );
    for (which, theme, want_amber) in [
        ("baud-dark", Theme::dark(), amber),
        ("baud-light", Theme::light(), LIGHT_AMBER),
    ] {
        let page = rgb(theme.bg);
        let lines = palette_lines(&text, which);
        let names: Vec<&str> = lines.iter().map(|(k, _, _)| k.as_str()).collect();
        assert_eq!(
            names,
            ["amber", "fail", "quiet", "change", "note"],
            "{which}"
        );
        for (name, value, said) in &lines {
            let drawn = if name == "amber" {
                let v = argb(value);
                assert_eq!(v, want_amber, "{which} amber");
                v
            } else {
                let i = entry(value);
                let drawn = rgb(theme.palette[i]);
                assert!(
                    said.contains(&format!("entry {i},")),
                    "{which} {name}: {said}"
                );
                assert_eq!(hex_in(said), drawn, "{which} {name} is entry {i}");
                drawn
            };
            let c = contrast(drawn, page);
            assert!(
                // The comment rounds to two places: 4.515 is written 4.52.
                (c - ratio(said)).abs() <= 0.0051,
                "{which} {name}: {c:.3}:1 against the comment's {said}"
            );
            assert!(c >= 4.5, "{which} {name} reads {c:.2}:1");
        }
    }
    let light = Theme::light();
    assert_ne!(entry("yellow"), entry("bright-purple"));
    assert_eq!(
        rgb(light.palette[entry("yellow")]),
        0x6f_5500,
        "the control"
    );
}

/// **The guide ships it**: the page names the file, `STARSHIP_CONFIG`,
/// both palettes and the starship it needs, with both advisories, and
/// says no Nerd Font is needed; the documentation's index links it; and
/// every TOML block it offers is starship 1.27 config too.
#[test]
fn the_guide_ships_the_preset_and_its_snippets_are_valid() {
    let guide = read(GUIDE);
    for needed in [
        "assets/starship/baud.toml",
        "STARSHIP_CONFIG",
        "baud-light",
        "baud-dark",
        "starship config palette",
        "1.27",
        "GHSA-679r-jwc5-r5m3",
        "GHSA-x486-6jjc-phfp",
        "Nerd Font",
    ] {
        assert!(guide.contains(needed), "{GUIDE} does not say {needed}");
    }
    let index = read("docs/README.md");
    assert!(
        index.contains("](guide/starship.md)"),
        "docs/README.md links the page"
    );
    let schema = schema();
    let mut blocks = 0;
    let mut rest = guide.as_str();
    while let Some(i) = rest.find("```toml\n") {
        rest = &rest[i + 8..];
        let end = rest.find("```").expect("the block ends");
        assert_eq!(
            errors_of(&rest[..end], &schema),
            Vec::<String>::new(),
            "{}",
            &rest[..end]
        );
        blocks += 1;
        rest = &rest[end..];
    }
    assert!(blocks >= 2, "the opt-ins, as TOML: {blocks}");
}
