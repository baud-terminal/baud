//! iTerm2's `OSC 1337` keys, `OUT_DIR/iterm_keys.rs`: every key the four
//! pinned pages that define one name, and every key iTerm2's
//! `executeXtermSetKvp` compares, from the names alone, pinned as
//! `[[source: iterm2-osc1337-doc-keys]]` and
//! `[[source: iterm2-osc1337-source-keys]]`. The pages and the source are
//! GPL-2.0 and `external`, never in the tree; line 1 of each names file
//! gives its parents' sha256s, and the manifest says how the names were
//! read. `every_iterm2_key_has_its_row` (`src/vt/iterm.rs`) holds the
//! table there to these lists.
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`build`] returns.

use baud_tablegen::{Emit, Errors, Input, Kind, Row, Source};
use std::fmt::Write as _;

/// The four pages' names: a key and the first page to name it.
pub const DOC: Input = Input {
    file: "iterm2-osc1337-doc-keys.txt",
    key: "iterm2-osc1337-doc-keys",
    header: "# iTerm2's OSC 1337 keys its pages name, from \
             [[source: iterm2-escape-codes]] at sha256 \
             7ee11f94b5dcd4e41a8df9a035a9612c92be831ccdd50948d4220fd872e8f0f3, \
             [[source: iterm2-images]] at sha256 \
             3b619ce2ce4d7798950f3c826b01c5676ecdee57bc89c5a5130863c1337d0290, \
             [[source: iterm2-badges]] at sha256 \
             fb56da7ab7a7f02e8e3b11d068cf2091a6b6d60a71f4ad005f33f3a86582129d, \
             [[source: iterm2-feature-reporting]] at sha256 \
             e359dbc6dadb49160a1c7d829e4f7ac9b480f9ebc7f73b3a98b3964a2b98f896:",
};

/// The source's names: a key each.
pub const SOURCE: Input = Input {
    file: "iterm2-osc1337-source-keys.txt",
    key: "iterm2-osc1337-source-keys",
    header: "# iTerm2's OSC 1337 keys executeXtermSetKvp compares, from \
             [[source: iterm2-964a75d-vt100terminal]] at sha256 \
             37fd1636c907eb9350fe32297eb7386cccf9836be3d9a90510dd6fa6c1157abe:",
};

/// The files, under `data/`, in the order [`build`] reads them.
pub const INPUTS: [Input; 2] = [DOC, SOURCE];

/// The pages, in the order a key's first page is taken.
const PAGES: [&str; 4] = [
    "iterm2-escape-codes",
    "iterm2-images",
    "iterm2-badges",
    "iterm2-feature-reporting",
];

/// How many keys each list holds.
const DOC_KEYS: usize = 39;
const SOURCE_KEYS: usize = 54;

/// One key each page must give, the positive control that each was read:
/// `SetProfile` is written with no `;` after `1337` in its page.
const DOC_CONTROLS: [(&str, &str); 4] = [
    ("SetProfile", "iterm2-escape-codes"),
    ("FileEnd", "iterm2-images"),
    ("SetBadgeFormat", "iterm2-badges"),
    ("Capabilities", "iterm2-feature-reporting"),
];

/// Two keys the source's compares must give.
const SOURCE_CONTROLS: [&str; 2] = ["CursorShape", "ExecFailed"];

/// The images page's grammar line uses this word for a placeholder rather
/// than a key, and it is the one word left out.
const PLACEHOLDER: &str = "key";

/// A list's keys: field 0 of each row, an identifier of ASCII letters and
/// digits, in strictly ascending byte order, so none is given twice.
fn keys<'a>(src: &'a Source, fields: usize, errs: &mut Errors) -> Vec<(Row<'a>, &'a str)> {
    let mut out: Vec<(Row<'a>, &'a str)> = Vec::new();
    for row in src.rows() {
        if errs.take(row.expect(fields)).is_none() {
            continue;
        }
        let k = row.field(0);
        if k.is_empty() || !k.bytes().all(|b| b.is_ascii_alphanumeric()) {
            errs.push(row.error(Kind::Malformed, 0, format!("{k:?} is not a key")));
            continue;
        }
        if k == PLACEHOLDER {
            errs.push(row.error(Kind::Control, 0, "the grammar's placeholder is not a key"));
            continue;
        }
        if let Some((prev, p)) = out.last() {
            if *p == k {
                errs.push(row.error(
                    Kind::Duplicate,
                    0,
                    format!("{k} is listed already on line {}", prev.line()),
                ));
                continue;
            }
            if p.as_bytes() > k.as_bytes() {
                errs.push(row.error(Kind::Invariant, 0, format!("{k} is not after {p}")));
                continue;
            }
        }
        out.push((row, k));
    }
    out
}

fn block(rs: &mut String, name: &str, doc: &[&str], keys: &[&str]) {
    for l in doc {
        let _ = writeln!(rs, "/// {l}");
    }
    let _ = writeln!(rs, "pub const {name}: [&[u8]; {}] = [", keys.len());
    for k in keys {
        let _ = writeln!(rs, "    b\"{k}\",");
    }
    rs.push_str("];\n");
}

pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let doc = Source::load(&DOC, read);
    let source = Source::load(&SOURCE, read);
    let (doc, source) = match (doc, source) {
        (Ok(d), Ok(s)) => (d, s),
        (d, s) => {
            let mut errs = Errors::default();
            for e in [d.err(), s.err()].into_iter().flatten() {
                errs.push(e);
            }
            return Err(errs);
        }
    };
    let mut errs = Errors::default();
    let doc_rows = keys(&doc, 2, &mut errs);
    for (row, _) in &doc_rows {
        let page = row.field(1);
        if !PAGES.contains(&page) {
            errs.push(row.error(
                Kind::UnknownValue,
                1,
                format!("{page:?} is none of the four pages"),
            ));
        }
    }
    for (key, page) in DOC_CONTROLS {
        let got = doc_rows
            .iter()
            .find(|(_, k)| *k == key)
            .map(|(r, _)| r.field(1));
        if got != Some(page) {
            errs.push(doc.error(
                Kind::Control,
                format!("control: {key} is not found in {page} but {got:?}"),
            ));
        }
    }
    let source_rows = keys(&source, 1, &mut errs);
    for key in SOURCE_CONTROLS {
        if !source_rows.iter().any(|(_, k)| *k == key) {
            errs.push(source.error(
                Kind::Control,
                format!("control: {key} is missing from executeXtermSetKvp's keys"),
            ));
        }
    }
    errs.count(doc.path(), "keys", doc_rows.len(), DOC_KEYS);
    errs.count(source.path(), "keys", source_rows.len(), SOURCE_KEYS);
    if !errs.is_empty() {
        return Err(errs);
    }
    let d: Vec<&str> = doc_rows.iter().map(|(_, k)| *k).collect();
    let s: Vec<&str> = source_rows.iter().map(|(_, k)| *k).collect();
    let mut rs = String::new();
    block(
        &mut rs,
        "DOC_KEYS",
        &[
            "Every key `[[source: iterm2-escape-codes]]`, `[[source: iterm2-images]]`,",
            "`[[source: iterm2-badges]]` and `[[source: iterm2-feature-reporting]]`",
            "name after `OSC 1337`.",
        ],
        &d,
    );
    rs.push('\n');
    block(
        &mut rs,
        "SOURCE_KEYS",
        &[
            "Every key `executeXtermSetKvp` compares",
            "(`[[source: iterm2-964a75d-vt100terminal]]`).",
        ],
        &s,
    );
    let mut out = Emit::new(
        "iterm_keys",
        "baud-domain/build.rs (build/iterm_keys.rs)",
        &[&doc, &source],
    );
    out.code(&rs);
    Ok(out)
}
