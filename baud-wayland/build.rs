//! **The interface tables, generated from the pinned protocol XMLs at build
//! time.** `docs/research/sources/wayland.xml` and the `wp-*.xml`,
//! `xdg-system-bell-v1.xml` and `xdg-activation-v1.xml` beside it are the
//! sources the manifest pins by hash; nothing here is transcribed by
//! hand. The XML this reads is the restricted shape `wayland-scanner`
//! reads -- `interface`, `request`, `event`, `arg`, `enum`, `entry`,
//! `description` -- so the reader is a few dozen lines rather than a
//! dependency, and a file that does not parse fails the build rather than
//! producing a table with a hole in it.

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// The pinned protocol files, in the order their globals are usually
/// advertised; the order fixes the interface indices.
const FILES: &[&str] = &[
    "wayland.xml",
    "wp-xdg-shell.xml",
    "wp-linux-dmabuf-v1.xml",
    "wp-presentation-time.xml",
    "wp-viewporter.xml",
    "wp-fractional-scale-v1.xml",
    "wp-xdg-decoration-unstable-v1.xml",
    "wp-fifo-v1.xml",
    "wp-commit-timing-v1.xml",
    "wp-linux-drm-syncobj-v1.xml",
    "wp-tearing-control-v1.xml",
    "wp-cursor-shape-v1.xml",
    "wp-text-input-unstable-v3.xml",
    "wp-primary-selection-unstable-v1.xml",
    "xdg-system-bell-v1.xml",
    "xdg-activation-v1.xml",
];

#[derive(Debug)]
struct Arg {
    kind: String,
    interface: Option<String>,
    nullable: bool,
}

#[derive(Debug)]
struct Msg {
    name: String,
    since: u32,
    destructor: bool,
    args: Vec<Arg>,
}

#[derive(Debug)]
struct Entry {
    name: String,
    value: String,
}

#[derive(Debug)]
struct Enum {
    name: String,
    bitfield: bool,
    entries: Vec<Entry>,
}

#[derive(Debug)]
struct Interface {
    name: String,
    version: u32,
    requests: Vec<Msg>,
    events: Vec<Msg>,
    enums: Vec<Enum>,
}

/// A tag: its name and its attributes, from `<` to `>`. Comments,
/// declarations and text are skipped by the caller.
struct Tag {
    name: String,
    attrs: Vec<(String, String)>,
    closing: bool,
}

impl Tag {
    fn attr(&self, k: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(a, _)| a == k)
            .map(|(_, v)| v.as_str())
    }
}

fn parse_tag(s: &str) -> Tag {
    let s = s.trim();
    let closing = s.starts_with('/');
    let body = s.trim_start_matches('/').trim_end_matches('/').trim();
    let (name, rest) = match body.find(char::is_whitespace) {
        Some(i) => (&body[..i], &body[i..]),
        None => (body, ""),
    };
    let mut attrs = Vec::new();
    let mut rest = rest.trim();
    while !rest.is_empty() {
        let eq = rest.find('=').expect("attribute without a value");
        let key = rest[..eq].trim().to_string();
        let after = rest[eq + 1..].trim_start();
        let quote = after.chars().next().expect("attribute without a quote");
        assert!(quote == '"' || quote == '\'', "attribute quote");
        let end = after[1..].find(quote).expect("unterminated attribute") + 1;
        let value = unescape(&after[1..end]);
        attrs.push((key, value));
        rest = after[end + 1..].trim_start();
    }
    Tag {
        name: name.to_string(),
        attrs,
        closing,
    }
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn parse_file(xml: &str, file: &str) -> Vec<Interface> {
    let mut out: Vec<Interface> = Vec::new();
    let mut msgs: Option<(bool, Msg)> = None;
    let mut en: Option<Enum> = None;
    let mut i = 0;
    while let Some(lt) = xml[i..].find('<') {
        let at = i + lt;
        let rest = &xml[at..];
        if rest.starts_with("<!--") {
            let end = rest.find("-->").expect("unterminated comment") + 3;
            i = at + end;
            continue;
        }
        if rest.starts_with("<?") {
            let end = rest.find("?>").expect("unterminated declaration") + 2;
            i = at + end;
            continue;
        }
        let gt = rest.find('>').expect("unterminated tag");
        let tag = parse_tag(&rest[1..gt]);
        i = at + gt + 1;
        match (tag.name.as_str(), tag.closing) {
            ("interface", false) => out.push(Interface {
                name: tag.attr("name").expect("interface name").to_string(),
                version: tag
                    .attr("version")
                    .expect("interface version")
                    .parse()
                    .unwrap(),
                requests: Vec::new(),
                events: Vec::new(),
                enums: Vec::new(),
            }),
            ("request", false) | ("event", false) => {
                assert!(msgs.is_none(), "{file}: nested message");
                msgs = Some((
                    tag.name == "request",
                    Msg {
                        name: tag.attr("name").expect("message name").to_string(),
                        since: tag.attr("since").map_or(1, |s| s.parse().unwrap()),
                        destructor: tag.attr("type") == Some("destructor"),
                        args: Vec::new(),
                    },
                ));
            }
            ("request", true) | ("event", true) => {
                let (is_request, m) = msgs.take().expect("message close without open");
                let iface = out.last_mut().expect("message outside an interface");
                match is_request {
                    true => iface.requests.push(m),
                    false => iface.events.push(m),
                }
            }
            ("arg", false) => {
                let (_, m) = msgs.as_mut().expect("arg outside a message");
                assert!(tag.attr("name").is_some(), "{file}: arg without a name");
                m.args.push(Arg {
                    kind: tag.attr("type").expect("arg type").to_string(),
                    interface: tag.attr("interface").map(str::to_string),
                    nullable: tag.attr("allow-null") == Some("true"),
                });
            }
            ("enum", false) => {
                en = Some(Enum {
                    name: tag.attr("name").expect("enum name").to_string(),
                    bitfield: tag.attr("bitfield") == Some("true"),
                    entries: Vec::new(),
                })
            }
            ("enum", true) => {
                let e = en.take().expect("enum close without open");
                out.last_mut()
                    .expect("enum outside an interface")
                    .enums
                    .push(e);
            }
            ("entry", false) => {
                let e = en.as_mut().expect("entry outside an enum");
                e.entries.push(Entry {
                    name: tag.attr("name").expect("entry name").to_string(),
                    value: tag.attr("value").expect("entry value").to_string(),
                });
            }
            _ => {}
        }
    }
    assert!(msgs.is_none() && en.is_none(), "{file}: unclosed element");
    out
}

fn ident(s: &str) -> String {
    match s {
        "type" | "move" | "fn" | "self" | "as" | "in" | "loop" | "match" | "ref" | "mod" => {
            format!("r#{s}")
        }
        _ => s.to_string(),
    }
}

fn shout(s: &str) -> String {
    let u = s.to_ascii_uppercase();
    match u.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        true => format!("N{u}"),
        false => u,
    }
}

fn emit(ifaces: &[Interface]) -> String {
    let mut o = String::new();
    writeln!(
        o,
        "// Generated by build.rs from the pinned protocol XMLs. Do not edit."
    )
    .unwrap();
    writeln!(o, "use crate::table::{{ArgKind, Interface, Msg}};").unwrap();
    for (idx, i) in ifaces.iter().enumerate() {
        writeln!(o, "#[allow(dead_code, non_upper_case_globals)]").unwrap();
        writeln!(o, "pub mod {} {{", ident(&i.name)).unwrap();
        writeln!(o, "    use super::*;").unwrap();
        writeln!(o, "    pub const NAME: &str = \"{}\";", i.name).unwrap();
        writeln!(o, "    pub const VERSION: u32 = {};", i.version).unwrap();
        writeln!(o, "    pub const INDEX: usize = {idx};").unwrap();
        for (kind, list) in [("request", &i.requests), ("event", &i.events)] {
            writeln!(o, "    pub mod {kind} {{").unwrap();
            for (op, m) in list.iter().enumerate() {
                writeln!(o, "        pub const {}: u16 = {op};", shout(&m.name)).unwrap();
            }
            writeln!(o, "    }}").unwrap();
        }
        for e in &i.enums {
            writeln!(o, "    pub mod {} {{", ident(&e.name)).unwrap();
            writeln!(o, "        pub const BITFIELD: bool = {};", e.bitfield).unwrap();
            for en in &e.entries {
                writeln!(
                    o,
                    "        pub const {}: u32 = {};",
                    shout(&en.name),
                    en.value
                )
                .unwrap();
            }
            writeln!(o, "    }}").unwrap();
        }
        for (name, list) in [("REQUESTS", &i.requests), ("EVENTS", &i.events)] {
            writeln!(o, "    pub static {name}: &[Msg] = &[").unwrap();
            for m in list {
                write!(
                    o,
                    "        Msg {{ name: \"{}\", since: {}, destructor: {}, args: &[",
                    m.name, m.since, m.destructor
                )
                .unwrap();
                for a in &m.args {
                    let k = match a.kind.as_str() {
                        "int" => "ArgKind::Int".to_string(),
                        "uint" => "ArgKind::Uint".to_string(),
                        "fixed" => "ArgKind::Fixed".to_string(),
                        "string" => format!("ArgKind::Str {{ nullable: {} }}", a.nullable),
                        "object" => format!("ArgKind::Object {{ nullable: {} }}", a.nullable),
                        "new_id" => match &a.interface {
                            Some(n) => format!("ArgKind::NewId {{ interface: Some(\"{n}\") }}"),
                            None => "ArgKind::NewId { interface: None }".to_string(),
                        },
                        "array" => "ArgKind::Array".to_string(),
                        "fd" => "ArgKind::Fd".to_string(),
                        other => panic!("unknown arg type {other} on {}.{}", i.name, m.name),
                    };
                    write!(o, "{k}, ").unwrap();
                }
                writeln!(o, "] }},").unwrap();
            }
            writeln!(o, "    ];").unwrap();
        }
        writeln!(
            o,
            "    pub static INTERFACE: Interface = Interface {{ name: NAME, version: VERSION, requests: REQUESTS, events: EVENTS }};"
        )
        .unwrap();
        writeln!(o, "}}").unwrap();
    }
    writeln!(
        o,
        "/// Every interface the pinned protocols declare, in file order."
    )
    .unwrap();
    writeln!(o, "pub static INTERFACES: &[&Interface] = &[").unwrap();
    for i in ifaces {
        writeln!(o, "    &{}::INTERFACE,", ident(&i.name)).unwrap();
    }
    writeln!(o, "];").unwrap();
    o
}

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();
    let dir = Path::new(&manifest).join("../docs/research/sources");
    let mut all = Vec::new();
    for f in FILES {
        let path = dir.join(f);
        println!("cargo:rerun-if-changed={}", path.display());
        let xml = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let parsed = parse_file(&xml, f);
        assert!(!parsed.is_empty(), "{f}: no interface");
        all.extend(parsed);
    }
    let out = Path::new(&env::var("OUT_DIR").unwrap()).join("protocol.rs");
    fs::write(&out, emit(&all)).unwrap();
    println!("cargo:rerun-if-changed=build.rs");
}
