//! The tokeniser against what Python 3.13's `html.parser` reported for
//! the same input, with `convert_charrefs=True`: each case's events were
//! taken from it before the Python was deleted. The cases are the rules a
//! regular-expression port gets wrong -- an attribute quote that never
//! closes (the whitespace before it is given back and the value is empty),
//! the longest-prefix named reference, the numeric references the HTML
//! Standard replaces or drops, `</>` and a bogus end tag, an end tag that
//! only starts like the raw-text element's, the escapable and the plain
//! raw-text elements, comments in their three closings, a reference cut
//! at the end of the input, and an unterminated tag.

use xtask::html::{Attr, Count, Handler, entity_count, tokenise};
use xtask::page_text;
use xtask::py::repr_str;

/// Events as the cases spell them: `<tag a='v' b=None>`, `</tag>` and
/// `|data`, adjacent data joined.
#[derive(Default)]
struct Events(Vec<String>);

impl Handler for Events {
    fn start(&mut self, tag: &str, attrs: &[Attr]) {
        let a: String = attrs
            .iter()
            .map(|(k, v)| match v {
                Some(v) => format!(" {k}={}", repr_str(v)),
                None => format!(" {k}=None"),
            })
            .collect();
        self.0.push(format!("<{tag}{a}>"));
    }
    fn end(&mut self, tag: &str) {
        self.0.push(format!("</{tag}>"));
    }
    fn data(&mut self, data: &str) {
        match self.0.last_mut() {
            Some(last) if last.starts_with('|') => last.push_str(data),
            _ => self.0.push(format!("|{data}")),
        }
    }
}

const CASES: &[(&str, &[&str])] = &[
    ("<a href= 'x>b", &["<a href='' 'x=None>", "|b"]),
    (
        "<a href= 'never closed>text",
        &["<a href='' 'never=None closed=None>", "|text"],
    ),
    ("x &notit; y &notin; z", &["|x \u{ac}it; y \u{2209} z"]),
    (
        "&#x110000;&#xD800;&#0;&#128;&#1;&#65",
        &["|\u{fffd}\u{fffd}\u{fffd}\u{20ac}A"],
    ),
    ("a </> b </ x> c", &["|a  b  c"]),
    (
        "<script>if (a</scriptx) {}</script>after",
        &["<script>", "|if (a</scriptx) {}", "</script>", "|after"],
    ),
    (
        "<title>&amp; t</title><textarea>&lt;</textarea>",
        &[
            "<title>",
            "|& t",
            "</title>",
            "<textarea>",
            "|<",
            "</textarea>",
        ],
    ),
    ("<style>&amp;</style>", &["<style>", "|&amp;", "</style>"]),
    (
        "<div class=\"content\" data-x=&ampx class='z'>t</div>",
        &[
            "<div class='content' data-x='&ampx' class='z'>",
            "|t",
            "</div>",
        ],
    ),
    (
        "<p a=\"&amp;\" b='&amp=' c=&notit;>",
        &["<p a='&' b='&amp=' c='&notit;'>"],
    ),
    (
        "<!-- c --!> after <!---> x <![CDATA[ <b> ]]> y",
        &["| after  x  y"],
    ),
    (
        "<br/><img src=x /><a/ b>",
        &["<br>", "</br>", "<img src='x'>", "</img>", "<a b=None>"],
    ),
    ("tail &amp", &["|tail &"]),
    ("<a b", &[]),
    ("x < y <1 </", &["|x < y <1 </"]),
    ("<plaintext><b>&amp;", &["<plaintext>", "|<b>&amp;"]),
];

#[test]
fn every_case_tokenises_as_html_parser_did() {
    for (src, want) in CASES {
        let mut got = Events::default();
        tokenise(src, &mut got);
        assert_eq!(got.0, *want, "{src:?}");
    }
}

/// The pinned list holds the names Python's `html.entities.html5` held.
#[test]
fn the_pinned_entity_list_has_every_name() {
    assert_eq!(entity_count(), Count(2231));
}

#[test]
fn the_extractors_self_test_passes() {
    assert_eq!(page_text::self_test(), Vec::<String>::new());
}
