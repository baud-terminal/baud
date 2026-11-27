//! The TOML 1.0 scan refuses what TOML 1.1 added and nothing TOML 1.0
//! allows. Every case below was read by Python's `tomllib`, the byte
//! order mark's by 3.13 after the Python here was deleted and the rest
//! before: it parsed each accepted document and refused each refused
//! one, while the `toml` crate parses all of them.

use xtask::toml10::first_refusal;

const ACCEPTED: &[&str] = &[
    "a = { x = [1,\n 2] }\n",
    "a = { x = \"\"\"two\nlines\"\"\" }\n",
    "t = 1979-05-27T07:32:00-07:00\n",
    "t = 07:32:00.5\n",
    "a = \"\\\\e and \\\\x\"\n",
    "a = '\\e \\x'\n",
    "a = '''\\e'''\n",
    "a = \"x # not a comment {\"\n",
    "[t]\n[[arr]]\nb = [\n [1, 2],\n [3],\n]\n",
    "a = \"\"\" \"\" \\\\e \"\"\"\n",
    "a = { x = 1 } # c\n",
    "a = \"\"\"x\"\"\"\"\n",
    "k = \"12:34\"\n",
];

const REFUSED: &[(&str, &str)] = &[
    ("a = { x = 1,\n y = 2 }\n", "an inline table spans lines"),
    ("a = { # c\n x = 1 }\n", "a comment inside an inline table"),
    (
        "a = { x = 1, }\n",
        "a comma after an inline table's last pair",
    ),
    ("a = \"\\e\"\n", "the escape \\e"),
    ("a = \"\"\"\\x41\"\"\"\n", "the escape \\x"),
    ("t = 07:32\n", "a time with no seconds"),
    ("t = 1979-05-27 07:32\n", "a time with no seconds"),
    ("t = 1979-05-27T07:32Z\n", "a time with no seconds"),
    ("\u{feff}a = 1\n", "a byte order mark"),
];

#[test]
fn what_toml_1_0_allows_passes() {
    for doc in ACCEPTED {
        assert_eq!(first_refusal(doc), None, "{doc:?}");
        assert!(doc.parse::<toml::Table>().is_ok(), "{doc:?}");
    }
}

#[test]
fn what_toml_1_1_added_is_refused() {
    for (doc, what) in REFUSED {
        let r = first_refusal(doc).unwrap_or_else(|| panic!("{doc:?} passed"));
        assert_eq!(r.what, *what, "{doc:?}");
        assert!(
            doc.parse::<toml::Table>().is_ok(),
            "{doc:?}: the crate refuses it too"
        );
    }
}

#[test]
fn a_refusal_says_where() {
    let r = first_refusal("a = 1\nb = { x = 1, }\n").expect("a refusal");
    assert_eq!(
        r.to_string(),
        "a comma after an inline table's last pair, which TOML 1.0 does not allow (at line 2, column 12)"
    );
}
