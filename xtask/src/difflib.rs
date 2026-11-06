//! `difflib.unified_diff` over lines, as `loop-head` prints it: Python's
//! `SequenceMatcher` with its defaults, ported rule for rule, so a diff of
//! two functions' bodies reads as it did.
//!
//! **Two of its rules decide what the diff looks like and neither is the
//! obvious one.** The longest match is found first and the two sides around
//! it recursively, not a minimal edit script. And `autojunk`: once `b` has
//! 200 lines, a line in more than one percent of them, and more than one
//! line besides, is "popular" and cannot start a match, only extend one. A
//! function's `ret`-like instructions are exactly that, so a port without
//! it aligns the same two bodies differently.

use std::collections::HashMap;
use std::ops::Range;

type At = (usize, usize, usize);

/// `unified_diff`'s `n`, the lines of context `loop-head` asked for.
const CONTEXT: usize = 2;

/// `SequenceMatcher(None, a, b)`.
struct Matcher<'a> {
    a: &'a [String],
    b: &'a [String],
    /// `b2j`, the popular lines already out of it.
    positions: HashMap<&'a str, Vec<usize>>,
}

impl<'a> Matcher<'a> {
    fn new(a: &'a [String], b: &'a [String]) -> Self {
        let mut positions: HashMap<&str, Vec<usize>> = HashMap::new();
        for (j, line) in b.iter().enumerate() {
            positions.entry(line.as_str()).or_default().push(j);
        }
        if b.len() >= 200 {
            let ntest = b.len() / 100 + 1;
            positions.retain(|_, at| at.len() <= ntest);
        }
        Self { a, b, positions }
    }

    /// `find_longest_match`: the leftmost of the longest in `a`, and of
    /// those the leftmost in `b`, then extended through popular lines.
    fn longest(&self, alo: Range<usize>, blo: Range<usize>) -> At {
        let (mut besti, mut bestj, mut best) = (alo.start, blo.start, 0);
        let mut lengths: HashMap<usize, usize> = HashMap::new();
        let nothing = Vec::new();
        for i in alo.clone() {
            let mut next = HashMap::new();
            let at = self.positions.get(self.a[i].as_str()).unwrap_or(&nothing);
            for &j in at {
                if j < blo.start {
                    continue;
                }
                if j >= blo.end {
                    break;
                }
                let before = j.checked_sub(1).and_then(|p| lengths.get(&p));
                let k = before.copied().unwrap_or(0) + 1;
                next.insert(j, k);
                if k > best {
                    (besti, bestj, best) = (i + 1 - k, j + 1 - k, k);
                }
            }
            lengths = next;
        }
        while besti > alo.start && bestj > blo.start && self.a[besti - 1] == self.b[bestj - 1] {
            besti -= 1;
            bestj -= 1;
            best += 1;
        }
        while besti + best < alo.end
            && bestj + best < blo.end
            && self.a[besti + best] == self.b[bestj + best]
        {
            best += 1;
        }
        (besti, bestj, best)
    }

    /// `get_matching_blocks`.
    fn blocks(&self) -> Vec<At> {
        let (la, lb) = (self.a.len(), self.b.len());
        let mut queue = vec![(0..la, 0..lb)];
        let mut found = Vec::new();
        while let Some((alo, blo)) = queue.pop() {
            let (i, j, k) = self.longest(alo.clone(), blo.clone());
            if k == 0 {
                continue;
            }
            found.push((i, j, k));
            if alo.start < i && blo.start < j {
                queue.push((alo.start..i, blo.start..j));
            }
            if i + k < alo.end && j + k < blo.end {
                queue.push((i + k..alo.end, j + k..blo.end));
            }
        }
        found.sort_unstable();
        let (mut i1, mut j1, mut k1) = (0, 0, 0);
        let mut joined = Vec::new();
        for (i2, j2, k2) in found {
            if i1 + k1 == i2 && j1 + k1 == j2 {
                k1 += k2;
            } else {
                if k1 != 0 {
                    joined.push((i1, j1, k1));
                }
                (i1, j1, k1) = (i2, j2, k2);
            }
        }
        if k1 != 0 {
            joined.push((i1, j1, k1));
        }
        joined.push((la, lb, 0));
        joined
    }
}

/// What a range of an opcode does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tag {
    Replace,
    Delete,
    Insert,
    Equal,
}

#[derive(Clone, Debug)]
struct Op {
    tag: Tag,
    a: Range<usize>,
    b: Range<usize>,
}

fn opcodes(blocks: &[At]) -> Vec<Op> {
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    for &(ai, bj, size) in blocks {
        let tag = match (i < ai, j < bj) {
            (true, true) => Some(Tag::Replace),
            (true, false) => Some(Tag::Delete),
            (false, true) => Some(Tag::Insert),
            (false, false) => None,
        };
        if let Some(tag) = tag {
            out.push(Op {
                tag,
                a: i..ai,
                b: j..bj,
            });
        }
        (i, j) = (ai + size, bj + size);
        if size != 0 {
            out.push(Op {
                tag: Tag::Equal,
                a: ai..i,
                b: bj..j,
            });
        }
    }
    out
}

/// `get_grouped_opcodes`: runs of change with `n` lines of context, and
/// nothing when the two are equal.
fn grouped(mut codes: Vec<Op>) -> Vec<Vec<Op>> {
    let n = CONTEXT;
    if codes.is_empty() {
        codes.push(Op {
            tag: Tag::Equal,
            a: 0..1,
            b: 0..1,
        });
    }
    if let Some(first) = codes.first_mut().filter(|c| c.tag == Tag::Equal) {
        first.a.start = first.a.start.max(first.a.end.saturating_sub(n));
        first.b.start = first.b.start.max(first.b.end.saturating_sub(n));
    }
    if let Some(last) = codes.last_mut().filter(|c| c.tag == Tag::Equal) {
        last.a.end = last.a.end.min(last.a.start + n);
        last.b.end = last.b.end.min(last.b.start + n);
    }
    let mut groups = Vec::new();
    let mut group: Vec<Op> = Vec::new();
    for mut c in codes {
        if c.tag == Tag::Equal && c.a.len() > 2 * n {
            group.push(Op {
                tag: c.tag,
                a: c.a.start..c.a.end.min(c.a.start + n),
                b: c.b.start..c.b.end.min(c.b.start + n),
            });
            groups.push(std::mem::take(&mut group));
            c.a.start = c.a.start.max(c.a.end.saturating_sub(n));
            c.b.start = c.b.start.max(c.b.end.saturating_sub(n));
        }
        group.push(c);
    }
    let nothing = group.is_empty() || matches!(group.as_slice(), [only] if only.tag == Tag::Equal);
    if !nothing {
        groups.push(group);
    }
    groups
}

/// A hunk header's range: one line has no length, and no lines name the
/// line before them.
fn range(r: &Range<usize>) -> String {
    match r.len() {
        1 => (r.start + 1).to_string(),
        0 => format!("{},0", r.start),
        len => format!("{},{len}", r.start + 1),
    }
}

/// `difflib.unified_diff(a, b, from, to, lineterm="", n=2)`.
pub fn unified_diff(a: &[String], b: &[String], from: &str, to: &str) -> Vec<String> {
    let matcher = Matcher::new(a, b);
    let groups = grouped(opcodes(&matcher.blocks()));
    let mut out = Vec::new();
    if !groups.is_empty() {
        out.push(format!("--- {from}"));
        out.push(format!("+++ {to}"));
    }
    for group in groups {
        let (first, last) = (&group[0], &group[group.len() - 1]);
        out.push(format!(
            "@@ -{} +{} @@",
            range(&(first.a.start..last.a.end)),
            range(&(first.b.start..last.b.end))
        ));
        for c in &group {
            if c.tag == Tag::Equal {
                out.extend(a[c.a.clone()].iter().map(|l| format!(" {l}")));
                continue;
            }
            if matches!(c.tag, Tag::Replace | Tag::Delete) {
                out.extend(a[c.a.clone()].iter().map(|l| format!("-{l}")));
            }
            if matches!(c.tag, Tag::Replace | Tag::Insert) {
                out.extend(b[c.b.clone()].iter().map(|l| format!("+{l}")));
            }
        }
    }
    out
}
