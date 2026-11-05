//! A property's closed set of values.

use crate::{Error, Errors, Kind, Row, Source};

/// A property's values, numbered in the order the table stores them, each
/// with its short and long name. A data row's value is checked against it,
/// and one outside it is an error that names where the set is defined.
#[derive(Debug)]
pub struct ValueSet {
    property: String,
    /// Where the set comes from: `data/<aliases>:<first>-<last>`, or the
    /// build script for a set it declares.
    defined: String,
    names: Vec<(String, String)>,
}

impl ValueSet {
    /// `property`'s values in PropertyValueAliases.txt
    /// (`[[source: unicode-propertyvaluealiases]]`), the rows
    /// `<property> ; <short> ; <long> [; <alias>...]`, numbered in `order`,
    /// a list of short names that must be that set exactly. (`ccc` rows
    /// carry a number before the short name and are not read here.)
    pub fn aliases(src: &Source, property: &str, order: &[&str]) -> Result<ValueSet, Errors> {
        let mut errs = Errors::default();
        let mut found: Vec<(String, String, usize)> = Vec::new();
        let mut lines = (0, 0);
        for row in src.rows().filter(|r| r.field(0) == property) {
            if row.len() < 3 {
                errs.push(row.error(
                    Kind::FieldCount,
                    row.len(),
                    format!("{} fields, at least 3 expected", row.len()),
                ));
                continue;
            }
            if let Some((_, _, at)) = found.iter().find(|(s, _, _)| s == row.field(1)) {
                errs.push(row.error(
                    Kind::Duplicate,
                    1,
                    format!(
                        "`{property}` value `{}` is already given on line {at}",
                        row.field(1)
                    ),
                ));
                continue;
            }
            if lines.0 == 0 {
                lines.0 = row.line();
            }
            lines.1 = row.line();
            found.push((
                row.field(1).to_string(),
                row.field(2).to_string(),
                row.line(),
            ));
        }
        if found.is_empty() {
            errs.push(src.error(Kind::Count, format!("no `{property}` rows")));
            return Err(errs);
        }
        let defined = format!("{}:{}-{}", src.path(), lines.0, lines.1);
        let missing: Vec<&str> = found
            .iter()
            .map(|(s, _, _)| s.as_str())
            .filter(|s| !order.contains(s))
            .collect();
        let extra: Vec<&&str> = order
            .iter()
            .filter(|o| !found.iter().any(|(s, _, _)| s == *o))
            .collect();
        if !missing.is_empty() || !extra.is_empty() || order.len() != found.len() {
            errs.push(Error::new(
                Kind::Invariant,
                &defined,
                format!(
                    "the table's `{property}` order is not this set: \
                     {missing:?} have no place in it, {extra:?} are not values"
                ),
            ));
            return Err(errs);
        }
        let names = order
            .iter()
            .map(|o| {
                let (s, l, _) = found.iter().find(|(s, _, _)| s == o).expect("checked");
                (s.clone(), l.clone())
            })
            .collect();
        errs.or(ValueSet {
            property: property.to_string(),
            defined,
            names,
        })
    }

    /// A closed set the build script declares, for a property whose file
    /// gives no alias rows: `names` are its values, in order, each its own
    /// long name.
    pub fn closed(property: &str, defined: &str, names: &[&str]) -> ValueSet {
        ValueSet {
            property: property.to_string(),
            defined: defined.to_string(),
            names: names
                .iter()
                .map(|n| (n.to_string(), n.to_string()))
                .collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// The short names, in order.
    pub fn short(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(|(s, _)| s.as_str())
    }

    /// Field `i` of `row` as a short name: its number in the set.
    pub fn check(&self, row: &Row<'_>, i: usize) -> Result<usize, Error> {
        self.find(row, i, |(s, _)| s == row.field(i))
    }

    /// Field `i` of `row` as a long name, the spelling of `@missing` lines.
    pub fn check_long(&self, row: &Row<'_>, i: usize) -> Result<usize, Error> {
        self.find(row, i, |(_, l)| l == row.field(i))
    }

    fn find(
        &self,
        row: &Row<'_>,
        i: usize,
        is: impl Fn(&(String, String)) -> bool,
    ) -> Result<usize, Error> {
        self.names.iter().position(is).ok_or_else(|| {
            row.error(
                Kind::UnknownValue,
                i,
                format!(
                    "{} `{}` is not one of the {} values of {}",
                    self.property,
                    row.field(i),
                    self.names.len(),
                    self.defined
                ),
            )
        })
    }
}
