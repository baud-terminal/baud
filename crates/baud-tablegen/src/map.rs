//! A value for every code point, with the line that set it.

use crate::{CODE_POINTS, CpRange, Error, Kind, Row, Source};

/// A value at each code point, in two layers: defaults (`@missing` lines,
/// each overriding the ones before it, UAX #44 section 4.2.10,
/// `[[source: uax44]]`), and data rows, which may not cover a code point
/// another row of the file covered. Any other precedence a table has is
/// written in its build script, not left to the order of the rows.
#[derive(Debug)]
pub struct CodepointMap<V> {
    values: Vec<Option<V>>,
    /// The line of the data row that set each code point; 0 for none.
    set_by: Vec<usize>,
}

impl<V: Copy + PartialEq> Default for CodepointMap<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: Copy + PartialEq> CodepointMap<V> {
    pub fn new() -> Self {
        CodepointMap {
            values: vec![None; CODE_POINTS],
            set_by: vec![0; CODE_POINTS],
        }
    }

    /// `v` as the default over `r`, wherever no data row has set a value.
    pub fn default_over(&mut self, r: CpRange, v: V) {
        for cp in r.iter() {
            let i = at(cp);
            if self.set_by[i] == 0 {
                self.values[i] = Some(v);
            }
        }
    }

    /// `v` over `r`, from `row` of `src`. A code point an earlier row set
    /// is a duplicate when `r` is that one code point, and an overlap when
    /// `r` is a range; the error names the earlier row.
    pub fn assign(&mut self, src: &Source, row: &Row<'_>, r: CpRange, v: V) -> Result<(), Error> {
        if let Some(cp) = r.iter().find(|&cp| self.set_by[at(cp)] != 0) {
            let earlier = self.set_by[at(cp)];
            let kind = if r.is_one() {
                Kind::Duplicate
            } else {
                Kind::Overlap
            };
            return Err(row.error(
                kind,
                0,
                format!(
                    "U+{cp:04X} is already set by line {earlier} ({:?})",
                    src.line(earlier)
                ),
            ));
        }
        for cp in r.iter() {
            self.values[at(cp)] = Some(v);
            self.set_by[at(cp)] = row.line();
        }
        Ok(())
    }

    pub fn get(&self, cp: u32) -> Option<V> {
        self.values.get(at(cp)).copied().flatten()
    }

    /// Whether a data row set `cp`.
    pub fn is_assigned(&self, cp: u32) -> bool {
        self.set_by.get(at(cp)).is_some_and(|&l| l != 0)
    }

    /// The code points data rows set, in order, with their values.
    pub fn assigned(&self) -> impl Iterator<Item = (u32, V)> + '_ {
        (0..=crate::LAST).filter_map(move |cp| {
            let i = at(cp);
            match (self.set_by[i], self.values[i]) {
                (0, _) | (_, None) => None,
                (_, Some(v)) => Some((cp, v)),
            }
        })
    }

    /// Every code point's value, refused if any has none.
    pub fn complete(&self, path: &str) -> Result<Vec<V>, Error> {
        match self.values.iter().position(Option::is_none) {
            Some(i) => Err(Error::new(
                Kind::Invariant,
                path,
                format!("U+{i:04X} has no value, and no default covers it"),
            )),
            None => Ok(self.values.iter().map(|v| v.expect("checked")).collect()),
        }
    }
}

/// `cp`'s index, which a `u32` code point always has on a host with a
/// `usize` of 32 bits or more.
fn at(cp: u32) -> usize {
    usize::try_from(cp).unwrap_or(usize::MAX)
}
