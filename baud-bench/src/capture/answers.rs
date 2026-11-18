//! `--answer QUERY=REPLY`, as Python escape strings: each `QUERY` a captured
//! program writes is answered with `REPLY`, as a terminal answers it. fish
//! waits on its `DA1` before it draws a prompt, so a capture of fish answers
//! it, in `capture-tui` and `capture-resize` alike.
//!
//! A query is answered once for each time it has appeared in everything read
//! so far, so one split across two reads is answered when its second half
//! arrives.

use std::ffi::OsString;

use super::py;

/// A query, and the reply owed to it.
pub type Owed = (Vec<u8>, Vec<u8>);

/// The answers given, and how many times each has been sent.
pub struct Answers {
    // A word with no `=` is a tuple of one, which dies at the first read.
    pairs: Vec<Vec<Vec<u8>>>,
    answered: Vec<usize>,
    seen: Vec<u8>,
}

impl Answers {
    /// The `--answer` words, read as the script read them.
    pub fn parse(words: &[&OsString]) -> Result<Answers, String> {
        let mut pairs = Vec::new();
        for w in words {
            let text = w
                .to_str()
                .ok_or_else(|| "'utf-8' codec can't encode: surrogates not allowed".to_string())?;
            let mut parts = Vec::new();
            match text.split_once('=') {
                Some((q, r)) => {
                    parts.push(py::unescape(q)?);
                    parts.push(py::unescape(r)?);
                }
                None => parts.push(py::unescape(text)?),
            }
            pairs.push(parts);
        }
        let answered = vec![0; pairs.len()];
        Ok(Answers {
            pairs,
            answered,
            seen: Vec::new(),
        })
    }

    /// After `chunk` is read: each reply now owed, in the order the answers
    /// were given, with the query it answers, each to be sent once.
    pub fn owed(&mut self, chunk: &[u8]) -> Result<Vec<Owed>, String> {
        self.seen.extend_from_slice(chunk);
        let mut out = Vec::new();
        for (i, a) in self.pairs.iter().enumerate() {
            let [q, r] = a.as_slice() else {
                return Err("not enough values to unpack (expected 2, got 1)".to_string());
            };
            while py::count(&self.seen, q) > self.answered[i] {
                self.answered[i] += 1;
                out.push((q.clone(), r.clone()));
            }
        }
        Ok(out)
    }
}
