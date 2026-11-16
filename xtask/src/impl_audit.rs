//! The raw-integer audit of one file: nine patterns, two sums.
//!
//! Each pattern is counted over the whole file in multi-line mode, as
//! non-overlapping matches from the left, which is what `re.findall` did
//! and what `Regex::find_iter` does. `[^)]*?` crosses newlines in both, and
//! `\b`, `\w` and `\s` are Unicode classes in both. Their sets are not
//! identical -- this crate's `\w` also takes combining marks, connector
//! punctuation and the joiners, and Python's took the other numerics -- so
//! the port was held to Python's count on every tracked `.rs` file of the
//! tree before the Python was deleted, and agreed on all of them.
//!
//! The patterns match text, not syntax: a cast spelled in a comment or a
//! string counts. Files that must stay at zero say so where they need to.

use std::sync::LazyLock;

use regex::Regex;

/// Which of the two sums a category is reported in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sum {
    /// The original categories, gated as `total`.
    Total,
    /// The signature categories, gated separately as `signature`.
    Signature,
}

/// One pattern and the line it is reported on.
pub struct Category {
    /// The name the table prints, padded to 44 columns.
    pub name: &'static str,
    /// The pattern, without the multi-line flag.
    pub pattern: &'static str,
    /// The sum it is counted in.
    pub sum: Sum,
}

/// **The signature categories are reported and gated SEPARATELY from the
/// original ones.** Folding them into one number would either fail a limit
/// calibrated to a narrower detector, or -- worse -- invite raising that
/// limit. Two numbers, each pinned at what it actually is.
///
/// The `usize`-params rule only saw `usize`, and only on `&mut self`
/// methods. It read 0 for a file with eight raw-primitive sites --
/// `final_byte: u8`, `param: u8`, `-> Option<u8>` and an `app: bool` that
/// the bool rule could not see either, because that one is anchored to a
/// struct field. A gate that agrees with whoever wrote the code is the
/// failure this repo keeps recording; the three signature categories are
/// what that read found.
///
/// **A generic fn is read like any other.** Each pattern that starts at
/// `fn` takes an optional generic list between the name and the parameters
/// -- lifetimes, type parameters with their bounds, const generics, over one
/// line or several -- ending at the first `>` that a `(` follows, with no
/// `{` or `;` inside it, so the parameters and the return after it are
/// counted as they are on a fn with no list. A const generic's own type is
/// inside the list and is not a parameter: the language requires it to be a
/// primitive. A parameter list that holds a parenthesised type, such as a
/// closure bound or a tuple, still ends the scan at its first `)`, generic
/// or not.
pub const CATEGORIES: [Category; 9] = [
    Category {
        name: "raw index arithmetic  (y * MAX_COLS + x)",
        pattern: r"\*\s*MAX_COLS",
        sum: Sum::Total,
    },
    Category {
        name: "usize params on operations",
        pattern: r"fn \w+(?:<[^{;]*?>)?\(&mut self[^)]*: usize",
        sum: Sum::Total,
    },
    Category {
        name: "raw primitive fn params",
        pattern: r"fn \w+(?:<[^{;]*?>)?\([^)]*?\b\w+: (u8|u16|u32|u64|usize|i8|i16|i32|i64|isize)\b",
        sum: Sum::Signature,
    },
    Category {
        name: "raw primitive fn returns",
        pattern: r"fn \w+(?:<[^{;]*?>)?\([^)]*\)\s*->\s*(?:Option<)?(u8|u16|u32|u64|usize)\b",
        sum: Sum::Signature,
    },
    Category {
        name: "bool as a fn param",
        pattern: r"fn \w+(?:<[^{;]*?>)?\([^)]*?\b\w+: bool\b",
        sum: Sum::Signature,
    },
    Category {
        name: "usize/u-typed struct fields",
        pattern: r"^\s+\w+: (usize|u8|u16|u32|u64),",
        sum: Sum::Total,
    },
    Category {
        name: "raw casts (as usize / as u16 / as u32)",
        pattern: r"\bas (usize|u16|u32|u8)\b",
        sum: Sum::Total,
    },
    Category {
        name: "raw byte range tests",
        pattern: r"0x[0-9a-f]+\.\.0x[0-9a-f]+\)\.contains",
        sum: Sum::Total,
    },
    Category {
        name: "bool as a flag field",
        pattern: r"^\s+\w+: bool,",
        sum: Sum::Total,
    },
];

/// The nine patterns, compiled once for every file an audit reads.
static COMPILED: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    CATEGORIES
        .iter()
        .map(|c| Regex::new(&format!("(?m){}", c.pattern)).expect("a category's pattern"))
        .collect()
});

/// One file's counts, category by category, in `CATEGORIES` order.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Counts(pub [Sites; 9]);

/// A number of matches.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Sites(pub usize);

impl std::ops::Add for Sites {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl std::fmt::Display for Sites {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Counts {
    /// Counts every category over `src`.
    pub fn of(src: &str) -> Self {
        let mut n = [Sites(0); 9];
        for (slot, re) in n.iter_mut().zip(COMPILED.iter()) {
            *slot = Sites(re.find_iter(src).count());
        }
        Self(n)
    }

    /// The sum of the categories reported in `sum`.
    pub fn sum(&self, sum: Sum) -> Sites {
        CATEGORIES
            .iter()
            .zip(self.0)
            .filter(|(c, _)| c.sum == sum)
            .fold(Sites(0), |acc, (_, n)| acc + n)
    }

    /// The table, as the shell budgets once parsed it: the `signature` line
    /// second to last and the `total` line last, so `tail -1` and
    /// `grep '^  total'` read the same number.
    pub fn table(&self) -> String {
        let mut out = format!("  {:<44} occurrences\n", "category");
        for (c, n) in CATEGORIES.iter().zip(self.0) {
            out += &format!("  {:<44} {n}\n", c.name);
        }
        out += &format!("  {:<44} ----\n", "");
        out += &format!("  {:<44} {}\n", "signature", self.sum(Sum::Signature));
        out += &format!("  {:<44} {}\n", "total", self.sum(Sum::Total));
        out
    }
}
