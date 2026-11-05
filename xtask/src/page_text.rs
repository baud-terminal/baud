//! The pinned learn.microsoft.com pages, checked by the article text they
//! render: `cargo xtask page-text`, and the offline half that
//! `cargo xtask sources` runs.
//!
//! A learn.microsoft.com page is rebuilt with the site around it. Its bytes
//! carry the site's asset version string, and the markup around the
//! article moves, so the page's sha256 stops matching within days while the
//! article does not change. Fetched on 2026-11-01, none of the 57 HTML
//! pages pinned in docs/research/sources/MANIFEST.toml hashed as pinned,
//! one pinned the day before among them, though two fetches that day were
//! byte-identical. What does reproduce is the article's text. Wayback
//! copies of eight of the pages, served in older templates, hash as that
//! day's fetch wherever the article's words had not changed (fvar, gvar,
//! head, avar, and recom, whose revision had moved) and differ where they
//! had (os2's "type face", chapter2's "glyphs IDS" and seven script tags
//! added to scripttags).
//!
//! So each such entry records three more facts about a fetch:
//!
//! - `revision`: the page's `git_commit_id` meta, the commit of the
//!   documentation repository it renders, where the page has one. A label:
//!   the repositories most pages' `gitcommit` meta names answer 404
//!   (GitHub) or ask for a sign-in (Azure DevOps). The three win32-pr pages
//!   (ms-dxgi-flip-model, ms-high-dpi, ms-dxgi-vrr) are the exception:
//!   their revisions are also commits of the public MicrosoftDocs/win32,
//!   the repository each page names for feedback. On 2026-11-01 the GitHub
//!   commits API answered 200 for 3fcc17ad, 55e845e9 and 129846d6 in
//!   MicrosoftDocs/win32, and raw.githubusercontent.com served each page's
//!   .md at its revision. The others resolve in no public repository the
//!   pages name: the sdk-api-build revisions answer 422 in
//!   MicrosoftDocs/sdk-api, the typography ones 422 in
//!   MicrosoftDocs/typography-issues, and the tn-archive page names only
//!   its sign-in repository.
//! - `text_sha256`: the sha256 of the article text, below. This is the
//!   check.
//! - `text_bytes`: that text's length in UTF-8 bytes.
//!
//! The entry's sha256 and bytes stay the record of the page as fetched the
//! day it was read.
//!
//! **The article text** is the character data inside every
//! `<div class="content">` -- the site's template puts the heading in one
//! and the article in another -- in document order, with two kinds of
//! subtree left out:
//!
//! - the page-metadata container (the date, contributor count and feedback
//!   button) and nav elements: an older template put both inside the
//!   article's div, and its article text then carried "Article05/29/2024 3
//!   contributors Feedback In this article", which is the site's and not
//!   the article's;
//! - button, script, style and template elements, which render no article
//!   text.
//!
//! The page is tokenised as Python's `html.parser` tokenised it when the
//! pins were taken (`crate::html`), character references decoded, every run
//! of HTML's ASCII whitespace (tab, line feed, form feed, carriage return,
//! space) becomes one space, so markup reindented around the same words
//! reads the same, and the ends are trimmed. A no-break space is kept, as
//! rendering keeps it: the script tags page writes Yi's tag with two of
//! them.
//!
//! **Refused, never compared:** a page with no such div, or whose text is
//! under `FLOOR` bytes. An extractor that matches nothing hashes the empty
//! string, and a pin recorded from it would pass on any page at all; the
//! first extractor tried on these pages returned empty text on the older
//! template. Each key in `CONTROL` names a sentence the manifest quotes
//! from that page, and a text without it is refused too: a positive
//! control, so a broken extraction is never read as a changed page.
//!
//! Usage:
//!
//! ```text
//! cargo xtask page-text              # fetch every entry, compare
//! cargo xtask page-text --key KEY    # only these entries
//! cargo xtask page-text --from DIR   # read DIR/KEY.html, no fetch
//! cargo xtask page-text --save DIR   # keep each fetch as DIR/KEY.html
//! cargo xtask page-text --print      # the new fields, as TOML
//! cargo xtask page-text --self-test  # synthetic pages, no network
//! ```
//!
//! It needs the network and is run on demand; it fetches through `curl`.
//! `cargo xtask sources`, which must stay offline and deterministic, calls
//! `offline_problems` instead: every page entry carries a well-formed
//! text_sha256 and text_bytes, a revision is well formed where one is
//! recorded, and the self-test passes.
//!
//! Exit: 0 every text matches its pin; 1 a text differs, or an entry lacks
//! its fields; 2 a page was refused; 3 a fetch failed.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use toml::{Table, Value};

use crate::html::{self, Attr, Count, Handler};
use crate::py;
use crate::sha256;

pub const HOST: &str = "https://learn.microsoft.com/";
/// A figure on the site is served as itself and its sha256 reproduces.
pub const NOT_PAGES: [&str; 6] = [".png", ".jpg", ".jpeg", ".gif", ".svg", ".pdf"];
/// The shortest article pinned is loca's, 1,539 bytes on 2026-11-01; a
/// heading alone is under a hundred.
pub const FLOOR: Count = Count(1024);
/// The page entries on 2026-11-01. A scope that matches fewer has stopped
/// matching; lower it only when an entry is removed.
pub const AT_LEAST: Count = Count(59);

/// The sha256 of the empty string.
pub const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

pub const CONTROL: [(&str, &str); 8] = [
    (
        "ms-opentype-head",
        "Left sidebearing point at x=0 (relevant only for TrueType rasterizers)",
    ),
    (
        "ms-opentype-vmtx",
        "The x coordinate of a glyph\u{2019}s vertical origin is not specified in the 'vmtx' table",
    ),
    (
        "ms-opentype-sbix",
        "then the glyph must be drawn using a bitmap from some strike",
    ),
    (
        "ms-opentype-chapter2",
        "Applications may process script-neutral characters together with",
    ),
    (
        "ms-opentype-gpos-2026-10-23",
        "If valueFormat2 is set to 0, then the second glyph of the pair",
    ),
    (
        "ms-itaskbarlist3-setprogressstate",
        "the window with the least progress is used",
    ),
    (
        "ms-devanagari-script-development",
        "Shaping engine implementations may vary somewhat with regard to what sequences are or are not considered valid",
    ),
    (
        "ms-opentype-features-ae",
        "This feature needs to be implemented prior to any other feature",
    ),
];

fn control_for(key: &str) -> Option<&'static str> {
    CONTROL.iter().find(|(k, _)| *k == key).map(|(_, s)| *s)
}

const SKIP: [&str; 5] = ["nav", "button", "script", "style", "template"];

/// What a check found, worst last, as the command's exit code.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Verdict {
    /// The text is as pinned.
    Pinned = 0,
    /// The text differs, or the entry lacks its fields.
    Differs = 1,
    /// The page was refused.
    Refused = 2,
    /// A fetch failed.
    Unfetched = 3,
}

impl From<Verdict> for std::process::ExitCode {
    fn from(v: Verdict) -> Self {
        Self::from(match v {
            Verdict::Pinned => 0,
            Verdict::Differs => 1,
            Verdict::Refused => 2,
            Verdict::Unfetched => 3,
        })
    }
}

fn hexish(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn classes(attrs: &[Attr]) -> Vec<String> {
    let class = attrs
        .iter()
        .rev()
        .find(|(n, _)| n == "class")
        .and_then(|(_, v)| v.clone())
        .unwrap_or_default();
    py::split(&class).into_iter().map(String::from).collect()
}

/// The article text of one page, and the meta tags that label it.
#[derive(Default)]
struct Article {
    /// Open divs inside the current content div, itself included.
    divs: Count,
    /// The tag of the subtree being left out.
    skip: Option<String>,
    /// Open elements of that tag inside it.
    nest: Count,
    found: Count,
    parts: Vec<String>,
    meta: HashMap<String, String>,
}

impl Handler for Article {
    fn start(&mut self, tag: &str, attrs: &[Attr]) {
        if tag == "meta" {
            let get = |name: &str| {
                attrs
                    .iter()
                    .rev()
                    .find(|(n, _)| n == name)
                    .map(|(_, v)| v.clone())
            };
            if let Some(Some(name)) = get("name") {
                if matches!(name.as_str(), "git_commit_id" | "gitcommit" | "updated_at") {
                    let content = get("content").flatten().unwrap_or_default();
                    self.meta.insert(name, content);
                }
            }
        }
        if let Some(skip) = &self.skip {
            if tag == skip {
                self.nest.0 += 1;
            }
            return;
        }
        if self.divs.0 == 0 {
            if tag == "div" && classes(attrs) == ["content"] {
                self.divs = Count(1);
                self.found.0 += 1;
            }
            return;
        }
        if SKIP.contains(&tag)
            || (tag == "div"
                && classes(attrs)
                    .iter()
                    .any(|c| c == "page-metadata-container"))
        {
            // What it held stood between the words on either side of it.
            self.skip = Some(tag.to_string());
            self.nest = Count(1);
            self.parts.push(" ".to_string());
            return;
        }
        if tag == "div" {
            self.divs.0 += 1;
        }
    }

    fn end(&mut self, tag: &str) {
        if let Some(skip) = &self.skip {
            if tag == skip {
                self.nest.0 -= 1;
                if self.nest.0 == 0 {
                    self.skip = None;
                }
            }
            return;
        }
        if self.divs.0 > 0 && tag == "div" {
            self.divs.0 -= 1;
            if self.divs.0 == 0 {
                self.parts.push("\n".to_string());
            }
        }
    }

    fn data(&mut self, data: &str) {
        if self.divs.0 > 0 && self.skip.is_none() {
            self.parts.push(data.to_string());
        }
    }
}

/// The facts the manifest records about a page, and the labels beside
/// them.
#[derive(Clone, Debug)]
pub struct Page {
    pub revision: Option<String>,
    pub repository: String,
    pub updated: Option<String>,
    pub text: String,
    pub text_sha256: String,
    pub text_bytes: Count,
    pub sha256: String,
    pub bytes: Count,
}

/// Why a page was refused, as the refusal line says it.
#[derive(Clone, Debug)]
pub struct Refused(pub String);

/// Python's message for bytes that are not UTF-8.
fn utf8_error(page: &[u8], e: std::str::Utf8Error) -> String {
    let at = e.valid_up_to();
    let lead = page[at];
    let (span, why) = match e.error_len() {
        Some(1) if matches!(lead, 0xc2..=0xf4) => (Count(1), "invalid continuation byte"),
        Some(1) => (Count(1), "invalid start byte"),
        Some(n) => (Count(n), "invalid continuation byte"),
        None => (Count(page.len() - at), "unexpected end of data"),
    };
    if span.0 == 1 {
        format!("'utf-8' codec can't decode byte 0x{lead:02x} in position {at}: {why}")
    } else {
        format!(
            "'utf-8' codec can't decode bytes in position {at}-{}: {why}",
            at + span.0 - 1
        )
    }
}

/// HTML's ASCII whitespace runs as one space, the ends' spaces trimmed.
fn squeeze(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut run = false;
    for c in s.chars() {
        if matches!(c, '\t' | '\n' | '\u{0c}' | '\r' | ' ') {
            if !run {
                out.push(' ');
            }
            run = true;
        } else {
            out.push(c);
            run = false;
        }
    }
    out.trim_matches(' ').to_string()
}

/// `re.sub(r"/(blob|commit)/[0-9a-f]{40}.*", "", s)`: the first such
/// path and the rest of its line removed, on every line.
fn repository(s: &str) -> String {
    let mut out = String::new();
    for (n, line) in s.split('\n').enumerate() {
        if n > 0 {
            out.push('\n');
        }
        let cut = ["/blob/", "/commit/"]
            .iter()
            .flat_map(|p| line.match_indices(p).map(move |(at, _)| (at, p.len())))
            .filter(|&(at, len)| {
                let rest = &line.as_bytes()[at + len..];
                rest.len() >= 40
                    && rest[..40]
                        .iter()
                        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            })
            .map(|(at, _)| at)
            .min();
        out.push_str(cut.map_or(line, |at| &line[..at]));
    }
    out
}

/// The three facts the manifest records, and the labels beside them.
pub fn read(page: &[u8]) -> Result<Page, Refused> {
    let text = std::str::from_utf8(page).map_err(|e| Refused(utf8_error(page, e)))?;
    let mut a = Article::default();
    html::tokenise(text, &mut a);
    if a.found.0 == 0 {
        return Err(Refused(
            "no <div class=\"content\"> on the page".to_string(),
        ));
    }
    let body = squeeze(&a.parts.concat());
    if body.len() < FLOOR.0 {
        return Err(Refused(format!(
            "its article text is {} bytes, under the floor of {}",
            body.len(),
            FLOOR.0
        )));
    }
    let rev = a.meta.get("git_commit_id").cloned().unwrap_or_default();
    let revision = (rev.chars().count() == 40 && hexish(&rev)).then_some(rev);
    Ok(Page {
        revision,
        repository: repository(a.meta.get("gitcommit").map_or("", String::as_str)),
        updated: a.meta.get("updated_at").cloned(),
        text_sha256: sha256::hex(body.as_bytes()),
        text_bytes: Count(body.len()),
        text: body,
        sha256: sha256::hex(page),
        bytes: Count(page.len()),
    })
}

/// Whether a manifest value equals what was read, as Python's `==`
/// between a `str` and what `tomllib` returned.
fn same_str(v: Option<&Value>, s: Option<&str>) -> bool {
    match (v, s) {
        (None, None) => true,
        (Some(Value::String(a)), Some(b)) => a == b,
        _ => false,
    }
}

/// Python's `==` between an `int` and what `tomllib` returned.
fn same_int(v: Option<&Value>, n: Count) -> bool {
    let Ok(n) = i64::try_from(n.0) else {
        return false;
    };
    match v {
        Some(Value::Integer(i)) => *i == n,
        Some(Value::Float(f)) => *f == n as f64,
        Some(Value::Boolean(b)) => i64::from(*b) == n,
        _ => false,
    }
}

/// `Verdict::Pinned` and a note when the text matches the pin, else why
/// not.
pub fn compare(key: &str, entry: &Table, page: &[u8], control: Option<&str>) -> (Verdict, String) {
    let got = match read(page) {
        Ok(got) => got,
        Err(Refused(why)) => return (Verdict::Refused, format!("refused: {why}")),
    };
    let control = control
        .filter(|c| !c.is_empty())
        .or_else(|| control_for(key));
    if let Some(c) = control.filter(|c| !c.is_empty()) {
        if !got.text.contains(c) {
            return (
                Verdict::Refused,
                format!(
                    "refused: the text does not hold the sentence its control quotes, {}",
                    py::repr_str(c)
                ),
            );
        }
    }
    let Some(pin) = entry.get("text_sha256") else {
        return (
            Verdict::Differs,
            "the entry records no text_sha256".to_string(),
        );
    };
    let mut notes = Vec::new();
    if !same_str(entry.get("sha256"), Some(&got.sha256)) {
        notes.push(format!(
            "page {} bytes, pinned {}, another sha256",
            got.bytes.0,
            py::str_of(entry.get("bytes"))
        ));
    }
    if !same_str(entry.get("revision"), got.revision.as_deref()) {
        let was = py::str_of(entry.get("revision"));
        let now = got.revision.clone().unwrap_or_else(|| "None".to_string());
        notes.push(format!(
            "revision {} -> {}",
            py::head(&was, Count(8)),
            py::head(&now, Count(8))
        ));
    }
    if same_str(Some(pin), Some(&got.text_sha256))
        && same_int(entry.get("text_bytes"), got.text_bytes)
    {
        let mut all = vec!["text as pinned".to_string()];
        all.extend(notes);
        return (Verdict::Pinned, all.join("; "));
    }
    let mut all = vec![format!(
        "TEXT CHANGED: {} bytes {}, pinned {} {}",
        got.text_bytes.0,
        py::head(&got.text_sha256, Count(8)),
        py::str_of(entry.get("text_bytes")),
        py::head(&py::str_of(Some(pin)), Count(8))
    )];
    all.extend(notes);
    (Verdict::Differs, all.join("; "))
}

/// Whether an entry is a learn.microsoft.com page this check covers.
pub fn in_scope(entry: &Table) -> bool {
    let url = entry.get("url").and_then(Value::as_str).unwrap_or("");
    let lower = url.to_lowercase();
    url.starts_with(HOST)
        && !same_str(entry.get("status"), Some("gated"))
        && !NOT_PAGES.iter().any(|s| lower.ends_with(s))
}

fn field_problems(key: &str, entry: &Table) -> Vec<String> {
    let mut out = Vec::new();
    let t = entry.get("text_sha256");
    let n = entry.get("text_bytes");
    let r = entry.get("revision");
    match t {
        Some(Value::String(s)) if s.chars().count() == 64 && hexish(s) => {
            if s == EMPTY {
                out.push(format!("{key}: text_sha256 is the empty string's"));
            }
        }
        _ => out.push(format!(
            "{key}: text_sha256 is not 64 lowercase hex: {}",
            py::repr(t)
        )),
    }
    let whole = match n {
        Some(Value::Integer(i)) => usize::try_from(*i).is_ok_and(|i| i >= FLOOR.0),
        _ => false,
    };
    if !whole {
        out.push(format!(
            "{key}: text_bytes is {}, want an integer of at least {}",
            py::repr(n),
            FLOOR.0
        ));
    }
    let good_rev = match r {
        None => true,
        Some(Value::String(s)) => s.chars().count() == 40 && hexish(s),
        Some(_) => false,
    };
    if !good_rev {
        out.push(format!(
            "{key}: revision is not 40 lowercase hex: {}",
            py::repr(r)
        ));
    }
    out
}

/// What `cargo xtask sources` fails on: every page entry carries a
/// well-formed text_sha256 and text_bytes, a revision is well formed
/// wherever one is recorded, an entry outside the scope that carries any
/// of the three is held to the same, the scope still matches, and the
/// self-test passes.
pub fn offline_problems(sources: &Table) -> Vec<String> {
    let mut out = Vec::new();
    let empty = Table::new();
    let entry = |v: &Value| v.as_table().cloned().unwrap_or_else(|| empty.clone());
    let scope: Vec<&str> = sources
        .iter()
        .filter(|(_, e)| in_scope(&entry(e)))
        .map(|(k, _)| k.as_str())
        .collect();
    if scope.len() < AT_LEAST.0 {
        out.push(format!(
            "only {} learn.microsoft.com page entries, want at least {}: the scope stopped matching",
            scope.len(),
            AT_LEAST.0
        ));
    }
    for (k, e) in sources {
        let e = entry(e);
        let carries = ["revision", "text_sha256", "text_bytes"]
            .iter()
            .any(|f| e.contains_key(*f));
        if scope.contains(&k.as_str()) || carries {
            out.extend(field_problems(k, &e));
        }
    }
    out.extend(self_test().into_iter().map(|m| format!("self-test: {m}")));
    out
}

/// How many entries are in scope.
pub fn pages(sources: &Table) -> Count {
    Count(
        sources
            .values()
            .filter(|e| e.as_table().is_some_and(in_scope))
            .count(),
    )
}

// -- the self-test: synthetic pages, so no vendor page has to be committed --

const SENTENCE: &str =
    "Each glyph has a top side bearing (the 'yi\u{a0}\u{a0}' tag) &amp; an advance. ";

fn body() -> String {
    format!(
        "<p>{}</p>\n<pre><code>uint16 numOfLongVerMetrics;</code></pre>",
        SENTENCE.repeat(20)
    )
}

/// The page's template.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Template {
    Current,
    Older,
}

fn page(body: &str, asset: &str, template: Template, indent: &str) -> Vec<u8> {
    let head = format!(
        "<html><head><meta name=\"git_commit_id\" content=\"{}\" />\
         <link rel=\"stylesheet\" href=\"/static/assets/{asset}/styles/site.css\">\
         <meta name=\"updated_at\" content=\"2024-05-31T17:41:00Z\" /></head><body>",
        "a".repeat(40)
    );
    let meta = format!(
        "<div class=\"page-metadata-container\">{indent}<ul class=\"metadata page-metadata\"><li>Article</li>\
         <li><time datetime=\"2024-05-29T08:00:00Z\">05/29/2024</time></li>\
         <li><button>3 contributors</button></li></ul>\
         <div id=\"user-feedback\"><button><span>Feedback</span></button></div></div>\
         <nav class=\"doc-outline\"><h2>In this article</h2></nav>"
    );
    let art = match template {
        Template::Older => format!(
            "<div class=\"content \"><h1 id=\"t\">vmtx table</h1>{meta}<!-- <content> -->{body}</div>"
        ),
        Template::Current => format!(
            "<div class=\"content\"><h1 id=\"t\">vmtx table</h1></div>{indent}{meta}{indent}<div class=\"content\">{body}</div>"
        ),
    };
    format!("{head}<main>{indent}{art}{indent}</main><footer>{asset}</footer></body></html>")
        .into_bytes()
}

const ASSET: &str = "0.4.03571.8296-bb082b73";
const ASSET2: &str = "0.4.03568.8292-0a6c2196";

fn std_page(body: &str) -> Vec<u8> {
    page(body, ASSET, Template::Current, "\n\t\t")
}

/// The extractor on synthetic pages: what each must give, and nothing
/// when all do.
pub fn self_test() -> Vec<String> {
    let mut bad = Vec::new();
    let body = body();
    let base = std_page(&body);
    let pin = match read(&base) {
        Ok(p) => p,
        Err(Refused(why)) => return vec![format!("the pinned page was refused: {why}")],
    };
    let mut entry = Table::new();
    if let Some(r) = &pin.revision {
        entry.insert("revision".into(), Value::String(r.clone()));
    }
    entry.insert("text_sha256".into(), Value::String(pin.text_sha256.clone()));
    entry.insert(
        "text_bytes".into(),
        Value::Integer(i64::try_from(pin.text_bytes.0).unwrap_or(0)),
    );
    entry.insert("sha256".into(), Value::String(pin.sha256.clone()));
    entry.insert(
        "bytes".into(),
        Value::Integer(i64::try_from(pin.bytes.0).unwrap_or(0)),
    );

    fn expect(bad: &mut Vec<String>, name: &str, want: Verdict, got: (Verdict, String)) {
        if got.0 != want {
            bad.push(format!(
                "{name}: exit {}, want {} ({})",
                got.0 as i32, want as i32, got.1
            ));
        }
    }
    let bad_ = &mut bad;
    expect(
        bad_,
        "the pinned page",
        Verdict::Pinned,
        compare("k", &entry, &base, None),
    );
    expect(
        bad_,
        "new chrome around the same text",
        Verdict::Pinned,
        compare(
            "k",
            &entry,
            &page(&body, ASSET2, Template::Current, "\n    "),
            None,
        ),
    );
    expect(
        bad_,
        "the older template, metadata inside the article",
        Verdict::Pinned,
        compare(
            "k",
            &entry,
            &page(&body, ASSET, Template::Older, "\n\t\t"),
            None,
        ),
    );
    expect(
        bad_,
        "a script inside the article",
        Verdict::Pinned,
        compare(
            "k",
            &entry,
            &std_page(&format!("{body}<script>var v = 1;</script>")),
            None,
        ),
    );
    expect(
        bad_,
        "one word of the article changed",
        Verdict::Differs,
        compare(
            "k",
            &entry,
            &std_page(&body.replacen("advance.", "advances.", 1)),
            None,
        ),
    );
    expect(
        bad_,
        "a no-break space turned into a space",
        Verdict::Differs,
        compare(
            "k",
            &entry,
            &std_page(&body.replacen('\u{a0}', " ", 1)),
            None,
        ),
    );
    expect(
        bad_,
        "an article cut to its heading",
        Verdict::Refused,
        compare("k", &entry, &std_page(""), None),
    );
    let renamed = String::from_utf8_lossy(&base).replace("class=\"content\"", "class=\"article\"");
    expect(
        bad_,
        "no content div at all",
        Verdict::Refused,
        compare("k", &entry, renamed.as_bytes(), None),
    );
    if read(&page(&body, ASSET2, Template::Current, "\n\t\t")).is_ok_and(|p| p.sha256 == pin.sha256)
    {
        bad_.push(
            "the chrome change did not change the page bytes, so it tests nothing".to_string(),
        );
    }
    if pin.text.contains("&amp;") || !pin.text.contains('\u{a0}') {
        bad_.push("character references are not decoded as the page renders them".to_string());
    }
    expect(
        bad_,
        "a control sentence the text holds",
        Verdict::Pinned,
        compare("k", &entry, &base, Some("the 'yi\u{a0}\u{a0}' tag) & an")),
    );
    expect(
        bad_,
        "a control sentence the text does not hold",
        Verdict::Refused,
        compare("k", &entry, &base, Some("a sentence never written")),
    );
    bad
}

// -- on demand: the network ---------------------------------------------------

/// Python's message for a file that could not be read.
pub fn os_error(e: &io::Error, path: &Path) -> String {
    let desc = e.to_string();
    let desc = desc.split(" (os error").next().unwrap_or(&desc).to_string();
    match e.raw_os_error() {
        Some(code) => format!(
            "[Errno {code}] {desc}: {}",
            py::repr_str(&path.to_string_lossy())
        ),
        None => desc,
    }
}

/// A page through `curl`, failing on an HTTP error as `urlopen` does.
fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let out = Command::new("curl")
        .args(["-sSfL", "--max-time", "60", url])
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(out.stdout)
}

/// The command's options.
#[derive(Default)]
pub struct Options {
    pub keys: Vec<String>,
    pub from: Option<PathBuf>,
    pub save: Option<PathBuf>,
    pub print: Print,
    pub self_test: Print,
}

/// Whether an option was given.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Print {
    #[default]
    No,
    Yes,
}

impl Options {
    pub fn parse(args: &[&str]) -> Result<Self, String> {
        let mut o = Self::default();
        let mut it = args.iter();
        while let Some(&a) = it.next() {
            let mut value = |name: &str| {
                it.next()
                    .map(|v| v.to_string())
                    .ok_or(format!("argument {name}: expected one argument"))
            };
            match a {
                "--key" => o.keys.push(value("--key")?),
                "--from" => o.from = Some(PathBuf::from(value("--from")?)),
                "--save" => o.save = Some(PathBuf::from(value("--save")?)),
                "--print" => o.print = Print::Yes,
                "--self-test" => o.self_test = Print::Yes,
                other => return Err(format!("unrecognized arguments: {other}")),
            }
        }
        Ok(o)
    }
}

/// `cargo xtask page-text`: what it prints, and its exit code.
pub fn run(root: &Path, o: &Options, out: &mut impl Write) -> Result<Verdict, String> {
    let w = |out: &mut dyn Write, s: String| writeln!(out, "{s}").map_err(|e| e.to_string());
    if o.self_test == Print::Yes {
        let bad = self_test();
        for b in &bad {
            w(out, format!("  FAIL {b}"))?;
        }
        let tail = if bad.is_empty() {
            "ok".to_string()
        } else {
            format!("{} failed", bad.len())
        };
        w(out, format!("self-test: {tail}"))?;
        return Ok(if bad.is_empty() {
            Verdict::Pinned
        } else {
            Verdict::Differs
        });
    }
    let manifest = root.join("docs/research/sources/MANIFEST.toml");
    let text = fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let table: Table = text
        .parse()
        .map_err(|e| format!("{}: {e}", manifest.display()))?;
    let sources = table
        .get("sources")
        .and_then(Value::as_table)
        .cloned()
        .unwrap_or_default();
    let keys: Vec<String> = if o.keys.is_empty() {
        sources
            .iter()
            .filter(|(_, e)| e.as_table().is_some_and(in_scope))
            .map(|(k, _)| k.clone())
            .collect()
    } else {
        o.keys.clone()
    };
    let mut worst = Verdict::Pinned;
    let mut hits = Count(0);
    for key in &keys {
        let Some(entry) = sources.get(key).and_then(Value::as_table) else {
            w(out, format!("  FAIL {key}: not in the manifest"))?;
            worst = worst.max(Verdict::Differs);
            continue;
        };
        let raw = match &o.from {
            Some(dir) => {
                let p = dir.join(format!("{key}.html"));
                fs::read(&p).map_err(|e| os_error(&e, &p))
            }
            None => fetch(entry.get("url").and_then(Value::as_str).unwrap_or("")),
        };
        let raw = match raw {
            Ok(r) => r,
            Err(e) => {
                w(out, format!("  FAIL {key}: {e}"))?;
                worst = worst.max(Verdict::Unfetched);
                continue;
            }
        };
        if let Some(dir) = &o.save {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let p = dir.join(format!("{key}.html"));
            fs::write(&p, &raw).map_err(|e| format!("{}: {e}", p.display()))?;
        }
        if o.print == Print::Yes {
            let got = match read(&raw) {
                Ok(g) => g,
                Err(Refused(why)) => {
                    w(out, format!("# {key}: refused: {why}"))?;
                    worst = worst.max(Verdict::Refused);
                    continue;
                }
            };
            if let Some(c) = control_for(key) {
                if !got.text.contains(c) {
                    w(
                        out,
                        format!(
                            "# {key}: refused: the text does not hold {}",
                            py::repr_str(c)
                        ),
                    )?;
                    worst = worst.max(Verdict::Refused);
                    continue;
                }
            }
            w(out, format!("[sources.{key}]"))?;
            if let Some(r) = &got.revision {
                w(out, format!("revision    = \"{r}\""))?;
            }
            w(out, format!("text_sha256 = \"{}\"", got.text_sha256))?;
            w(out, format!("text_bytes  = {}", got.text_bytes.0))?;
            let repo = if got.repository.is_empty() {
                "no repository named".to_string()
            } else {
                got.repository.clone()
            };
            let updated = got.updated.clone().unwrap_or_else(|| "None".to_string());
            w(out, format!("# {repo}; updated_at {updated}"))?;
            continue;
        }
        let (code, why) = compare(key, entry, &raw, None);
        if code == Verdict::Pinned && control_for(key).is_some() {
            hits.0 += 1;
        }
        let tag = if code == Verdict::Pinned {
            "ok  "
        } else {
            "FAIL"
        };
        w(out, format!("  {tag} {key}: {why}"))?;
        worst = worst.max(code);
    }
    if o.print == Print::No {
        w(
            out,
            format!(
                "{} checked, worst exit {}; {} control sentence(s) found",
                keys.len(),
                worst as i32,
                hits.0
            ),
        )?;
    }
    Ok(worst)
}
