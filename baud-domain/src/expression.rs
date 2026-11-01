//! **iTerm2's expressions**, what an interpolated string holds in each
//! `\(…)`, read and evaluated (`[[source:
//! iterm2-doc-scripting-fundamentals]]`, "Expressions").
//!
//! **The grammar is the documentation's.** "The following types of
//! expressions are supported": interpolated strings, "decimal integer
//! literals" such as `2`, floating point such as `2.3` or `1.2e5`, array
//! literals "of the form `[ expression, expression, ... ]`", array
//! dereferences "of the form `path[index]`" where "`index` is a number",
//! function calls, paths to variables such as `jobName` and
//! `tab.currentSession.path`, and the operators, "listed below from lowest
//! precedence (binds least tightly) to highest": the ternary `condition ?
//! whenTrue : whenFalse`, `||`, `&&`, `==` and `!=`, `<`, `>`, `<=` and
//! `>=`, `+` and `-`, `*` and `/`, the unary `!` and `-`, and parentheses
//! "to group a subexpression". A negative literal, `-3`, is the unary
//! minus. [`read`] climbs those levels, each binary operator taking its
//! left operand first and the ternary nesting to its right, and hands
//! each form to a [`Fold`], so one reading both lists the names an
//! expression uses ([`names`]) and evaluates it ([`evaluate`]).
//!
//! **Where the page is silent, iTerm2's behaviour is followed, as its
//! source at 964a75d shows it** (`[[source:
//! iterm2-964a75d-expression-parser]]`, `[[source:
//! iterm2-964a75d-subexpression]]`, `[[source:
//! iterm2-964a75d-expression-evaluator]]`), restated in this module's own
//! structure; none of that code is carried.
//!
//! * A name is ASCII letters, digits, `_` and `-`, beginning with a letter
//!   or `_` (`[[source: iterm2-964a75d-coreparse-identifier]]`), so
//!   `user.a-b` is one name and `a - b` a subtraction; paths join names
//!   with `.` (**departure**: with no white space around it, where
//!   iTerm2's tokens also join `a . b`). A path written with `?` straight after it, `user.x?`
//!   (`[[source: iterm2-964a75d-optional-path]]`), or an expression
//!   followed by a `?` with nothing after it, reads an undefined variable
//!   as null. A path may begin `session.`, iTerm2's alias for the
//!   session's own variables (`[[source: iterm2-964a75d-ptysession]]`;
//!   the badge page's example is `\(session.name)`). `true` and `false`
//!   are 1 and 0, and `null` is null. A number begins with a digit, its
//!   fraction and exponent read as `strtod` reads them (`[[source:
//!   iterm2-964a75d-coreparse-number]]`).
//! * A value is a number, a string, an array or null; a comparison and a
//!   `!` give 1 or 0.
//! * `+`, `-`, `*`, `/`, `&&` and `||` take numbers, a string that reads
//!   as a number counting as one, and evaluate both sides; anything else is
//!   an error, so `+` joins no strings.
//! * `==` and `!=` compare numbers by value, strings and arrays by their
//!   contents and null with null; values of two kinds are unequal.
//! * `<`, `>`, `<=` and `>=` order numbers by value, strings and arrays
//!   item by item and then by length; null is neither less nor greater
//!   than anything, `<=` and `>=` holding between two nulls; any other
//!   pair is an error. **Departure**: strings are ordered by code point,
//!   where Foundation's `compare:` holds canonically equivalent strings
//!   equal.
//! * The unary operators take a number, a string not counting as one; the
//!   ternary's condition takes a number or null, null being false, and
//!   only the branch it picks is evaluated.
//! * `path[index]` takes an array and a number, the index truncated; one
//!   past either end is an error.
//! * A value is written as text: a string as it is, a number as
//!   Foundation's `NSNumber` writes a double, `%0.16g` (`[[source:
//!   apple-nsnumber-stringvalue]]`, `[[source:
//!   apple-nsnumber-description-withlocale]]`), an array as `[a, b]`, null
//!   as nothing.
//!
//! **A reference to an undefined variable is empty, and an undefined
//! variable an operator is given is a fault**, as iTerm2's lax policy
//! reads (`[[source: iterm2-964a75d-advanced-settings]]`): "an expression
//! in an interpolated string that references an undefined variable will
//! be treated as an empty string. For example, “\(bogus)”" -- an
//! expression that is the reference, `bogus` or `bogus[1]`, parenthesised
//! or not ([`Evaluated::Undefined`]). The page's example is that bare
//! reference, and iTerm2's code, its comments ("The user must write
//! \"var?\" explicitly to treat undefined as null", `[[source:
//! iterm2-964a75d-expression-parser]]`) and its tests ("undefinedVar ==
//! null (without ?) should be an error", `[[source:
//! iterm2-964a75d-expression-parser-tests]]`) read the text so: inside an operator, a ternary, an array or an index, an undefined
//! variable is a fault, [`Fault::Undefined`], unless written with `?`.
//! The mechanism is two files'. An undefined variable is a Nil carrying a
//! fallback error, which `deoptionalized` turns into that error unless it
//! was written `var?`, and for which `asSubexpression` hands an operator
//! nothing (`[[source: iterm2-964a75d-parsed-expression]]`); each
//! operator given nothing fails, "Nil operand in binary operation (+)"
//! and its like (`[[source: iterm2-964a75d-subexpression]]`). A
//! string inside an expression is an interpolated string of its own, a
//! reference standing alone in it emptied on its own.
//!
//! The page sets no limit on how deep an expression nests; [`DEPTH`]
//! levels -- parentheses, operators, arrays, strings -- are read, more
//! than a format of `DEPTH` bytes can open, and one deeper is unreadable.

use crate::interpolate::{Interpolated, string_end, string_parts};

/// **How many levels deep an expression is read**, its strings' own
/// expressions counted: a parenthesis, a unary operator, an operand of a
/// binary one, a ternary's branch, an array, an index and a string each
/// take one, and each takes at least one byte, so no expression of fewer
/// bytes nests past it.
pub const DEPTH: usize = 512;

/// A unary operator.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unary {
    /// `!`, logical NOT.
    Not,
    /// `-`, negation.
    Negate,
}

/// A binary operator.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Binary {
    Or,
    And,
    Equal,
    NotEqual,
    Less,
    Greater,
    LessOrEqual,
    GreaterOrEqual,
    Plus,
    Minus,
    Times,
    DividedBy,
}

/// A binary operator's place in the documentation's list, the lowest
/// first: `||`, `&&`, equality, relational, additive, multiplicative.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Level {
    Or,
    And,
    Equality,
    Relational,
    Additive,
    Multiplicative,
}

impl Level {
    /// The level above, `None` past the multiplicative.
    fn above(self) -> Option<Level> {
        Some(match self {
            Level::Or => Level::And,
            Level::And => Level::Equality,
            Level::Equality => Level::Relational,
            Level::Relational => Level::Additive,
            Level::Additive => Level::Multiplicative,
            Level::Multiplicative => return None,
        })
    }
}

impl Binary {
    fn level(self) -> Level {
        match self {
            Binary::Or => Level::Or,
            Binary::And => Level::And,
            Binary::Equal | Binary::NotEqual => Level::Equality,
            Binary::Less | Binary::Greater | Binary::LessOrEqual | Binary::GreaterOrEqual => {
                Level::Relational
            }
            Binary::Plus | Binary::Minus => Level::Additive,
            Binary::Times | Binary::DividedBy => Level::Multiplicative,
        }
    }
}

/// Whether a path reads an undefined variable as null.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Optional {
    No,
    Yes,
}

/// A byte offset into an expression.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ByteAt(usize);

impl ByteAt {
    pub const fn of(index: usize) -> ByteAt {
        ByteAt(index)
    }

    pub fn index(self) -> usize {
        self.0
    }
}

/// **Why an expression cannot be read.**
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unreadable {
    /// Off the grammar, from this byte.
    Syntax(ByteAt),
    /// A function call, at its name: a format a program sends may not
    /// make one ([`crate::calls_a_function`]).
    Call(ByteAt),
    /// Nested past [`DEPTH`].
    TooDeep,
}

/// **Where an expression sits**: how many strings deep, for the split
/// ([`crate::interpolated_parts`] reads sixteen), and how many levels deep
/// it is read from, for [`DEPTH`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Nest {
    strings: usize,
    depth: usize,
}

impl Nest {
    /// A format's own expression.
    pub const TOP: Nest = Nest {
        strings: 0,
        depth: 0,
    };

    /// An expression in a string of an expression at `self`, at `depth`.
    fn inner(self, depth: usize) -> Nest {
        Nest {
            strings: self.strings + 1,
            depth: depth + 1,
        }
    }
}

/// **What each form of an expression becomes**: [`read`] calls one of
/// these for every form it reads, its operands first.
pub trait Fold<'s> {
    type Value;
    /// An array literal's items as they are gathered.
    type Items;
    fn number(&mut self, n: f64) -> Self::Value;
    fn null(&mut self) -> Self::Value;
    /// A string: its text between the quotes, an interpolated string,
    /// in an expression at `nest`.
    fn string(&mut self, text: &'s str, nest: Nest) -> Self::Value;
    fn path(&mut self, path: &'s str, optional: Optional) -> Self::Value;
    /// `path[index]`.
    fn element(&mut self, path: &'s str, index: Self::Value) -> Self::Value;
    fn unary(&mut self, op: Unary, operand: Self::Value) -> Self::Value;
    fn binary(&mut self, op: Binary, lhs: Self::Value, rhs: Self::Value) -> Self::Value;
    fn ternary(
        &mut self,
        condition: Self::Value,
        then: Self::Value,
        otherwise: Self::Value,
    ) -> Self::Value;
    fn items(&mut self) -> Self::Items;
    fn item(&mut self, items: &mut Self::Items, item: Self::Value);
    fn array(&mut self, items: Self::Items) -> Self::Value;
}

/// A punctuation or operator token.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sym {
    Question,
    Colon,
    Open,
    Close,
    OpenBracket,
    CloseBracket,
    Comma,
    Not,
    Op(Binary),
}

/// A token, and where it starts.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Token<'s> {
    Number(f64),
    Str(&'s str),
    Path(&'s str, Optional),
    True,
    False,
    Null,
    Sym(Sym),
    End,
}

/// What a conditional expression was, for a `?` after it: a path alone,
/// which the `?` makes read an undefined variable as null, or anything
/// else, which it leaves as it is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape<'s> {
    Path(&'s str),
    Other,
}

/// **`e`, an expression at `nest`, read into `fold`**: its value, or why
/// it cannot be read. A function call is refused at its name, a form off
/// the grammar at its first byte, and one nested past [`DEPTH`] whole.
pub fn read<'s, F: Fold<'s>>(e: &'s str, nest: Nest, fold: &mut F) -> Result<F::Value, Unreadable> {
    let mut r = Reader {
        s: e,
        at: 0,
        nest,
        depth: nest.depth,
        fold,
    };
    let v = r.expression()?;
    match r.next()? {
        (Token::End, _) => Ok(v),
        (_, at) => Err(Unreadable::Syntax(ByteAt(at))),
    }
}

struct Reader<'s, 'f, F> {
    s: &'s str,
    at: usize,
    nest: Nest,
    depth: usize,
    fold: &'f mut F,
}

/// A byte that may begin a name.
fn name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

impl<'s, F: Fold<'s>> Reader<'s, '_, F> {
    /// One level deeper for `read`, or unreadable past [`DEPTH`].
    fn deeper<T>(
        &mut self,
        read: impl FnOnce(&mut Self) -> Result<T, Unreadable>,
    ) -> Result<T, Unreadable> {
        if self.depth >= DEPTH {
            return Err(Unreadable::TooDeep);
        }
        self.depth += 1;
        let v = read(self);
        self.depth -= 1;
        v
    }

    /// The start of the next token, white space passed over.
    fn skip(&mut self) {
        while let Some(c) = self.s[self.at..].chars().next() {
            if !c.is_whitespace() {
                break;
            }
            self.at += c.len_utf8();
        }
    }

    /// The next token without taking it.
    fn peek(&mut self) -> Result<(Token<'s>, usize), Unreadable> {
        let at = self.at;
        let t = self.next();
        self.at = at;
        t
    }

    /// The next token and where it starts.
    fn next(&mut self) -> Result<(Token<'s>, usize), Unreadable> {
        self.skip();
        let start = self.at;
        let b = self.s.as_bytes();
        let Some(&c) = b.get(start) else {
            return Ok((Token::End, start));
        };
        let at2 = |n: usize| b.get(start + n).copied();
        let (sym, len) = match (c, at2(1)) {
            (b'=', Some(b'=')) => (Sym::Op(Binary::Equal), 2),
            (b'!', Some(b'=')) => (Sym::Op(Binary::NotEqual), 2),
            (b'<', Some(b'=')) => (Sym::Op(Binary::LessOrEqual), 2),
            (b'>', Some(b'=')) => (Sym::Op(Binary::GreaterOrEqual), 2),
            (b'&', Some(b'&')) => (Sym::Op(Binary::And), 2),
            (b'|', Some(b'|')) => (Sym::Op(Binary::Or), 2),
            (b'<', _) => (Sym::Op(Binary::Less), 1),
            (b'>', _) => (Sym::Op(Binary::Greater), 1),
            (b'+', _) => (Sym::Op(Binary::Plus), 1),
            (b'-', _) => (Sym::Op(Binary::Minus), 1),
            (b'*', _) => (Sym::Op(Binary::Times), 1),
            (b'/', _) => (Sym::Op(Binary::DividedBy), 1),
            (b'!', _) => (Sym::Not, 1),
            (b'?', _) => (Sym::Question, 1),
            (b':', _) => (Sym::Colon, 1),
            (b'(', _) => (Sym::Open, 1),
            (b')', _) => (Sym::Close, 1),
            (b'[', _) => (Sym::OpenBracket, 1),
            (b']', _) => (Sym::CloseBracket, 1),
            (b',', _) => (Sym::Comma, 1),
            (b'"', _) => {
                let end = string_end(b, start + 1, self.nest.strings)
                    .ok_or(Unreadable::Syntax(ByteAt(start)))?;
                self.at = end + 1;
                return Ok((Token::Str(&self.s[start + 1..end]), start));
            }
            (d, _) if d.is_ascii_digit() => return Ok((self.number(start), start)),
            (n, _) if name_start(n) => return Ok((self.path(start), start)),
            _ => return Err(Unreadable::Syntax(ByteAt(start))),
        };
        self.at = start + len;
        Ok((Token::Sym(sym), start))
    }

    /// A number from `start`: digits, a fraction and an exponent.
    fn number(&mut self, start: usize) -> Token<'s> {
        let b = self.s.as_bytes();
        let digits = |mut i: usize| {
            while b.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
            i
        };
        let mut end = digits(start);
        if b.get(end) == Some(&b'.') {
            end = digits(end + 1);
        }
        if matches!(b.get(end), Some(b'e' | b'E')) {
            let sign = usize::from(matches!(b.get(end + 1), Some(b'+' | b'-')));
            if b.get(end + 1 + sign).is_some_and(u8::is_ascii_digit) {
                end = digits(end + 1 + sign);
            }
        }
        self.at = end;
        Token::Number(self.s[start..end].parse().unwrap_or(f64::NAN))
    }

    /// A path from `start`: names joined by `.`; with a `?` straight after
    /// it, optional; `true`, `false` and `null` alone.
    fn path(&mut self, start: usize) -> Token<'s> {
        let b = self.s.as_bytes();
        let tight = start
            + b[start..]
                .iter()
                .take_while(|&&c| c.is_ascii_alphanumeric() || c == b'_' || c == b'.')
                .count();
        let word = &self.s[start..tight];
        if b.get(tight) == Some(&b'?')
            && !word.ends_with('.')
            && !matches!(word, "true" | "false" | "null")
        {
            self.at = tight + 1;
            return Token::Path(word, Optional::Yes);
        }
        let name = |i: usize| {
            i + b[i..]
                .iter()
                .take_while(|&&c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
                .count()
        };
        // A `.` that no name follows ends the path before it, and the
        // reading fails on that `.`.
        let mut end = name(start);
        while b.get(end) == Some(&b'.') && b.get(end + 1).is_some_and(|&c| name_start(c)) {
            end = name(end + 1);
        }
        self.at = end;
        let p = &self.s[start..end];
        match p {
            "true" => Token::True,
            "false" => Token::False,
            "null" => Token::Null,
            _ => Token::Path(p, Optional::No),
        }
    }

    /// `expression`: an array, or a conditional expression.
    fn expression(&mut self) -> Result<F::Value, Unreadable> {
        let (t, _) = self.peek()?;
        if t != Token::Sym(Sym::OpenBracket) {
            return Ok(self.conditional()?.0);
        }
        self.next()?;
        self.deeper(|r| {
            let mut items = r.fold.items();
            if r.peek()?.0 == Token::Sym(Sym::CloseBracket) {
                r.next()?;
                return Ok(r.fold.array(items));
            }
            loop {
                let item = r.expression()?;
                r.fold.item(&mut items, item);
                match r.next()? {
                    (Token::Sym(Sym::Comma), _) => continue,
                    (Token::Sym(Sym::CloseBracket), _) => return Ok(r.fold.array(items)),
                    (_, at) => return Err(Unreadable::Syntax(ByteAt(at))),
                }
            }
        })
    }

    /// The ternary and a trailing `?`, over the `||` level.
    fn conditional(&mut self) -> Result<(F::Value, Shape<'s>), Unreadable> {
        let (c, shape) = self.binary(Level::Or)?;
        if self.peek()?.0 != Token::Sym(Sym::Question) {
            return Ok((c, shape));
        }
        self.next()?;
        let ends = matches!(
            self.peek()?.0,
            Token::End | Token::Sym(Sym::Close | Sym::CloseBracket | Sym::Comma | Sym::Colon)
        );
        if ends {
            let v = match shape {
                Shape::Path(p) => self.fold.path(p, Optional::Yes),
                Shape::Other => c,
            };
            return Ok((v, Shape::Other));
        }
        self.deeper(|r| {
            let (then, _) = r.conditional()?;
            match r.next()? {
                (Token::Sym(Sym::Colon), _) => {}
                (_, at) => return Err(Unreadable::Syntax(ByteAt(at))),
            }
            let (otherwise, _) = r.conditional()?;
            Ok((r.fold.ternary(c, then, otherwise), Shape::Other))
        })
    }

    /// The binary operators of `min` and above, climbed: each operator's
    /// right operand holds only those binding tighter, so each takes its
    /// left operand first.
    fn binary(&mut self, min: Level) -> Result<(F::Value, Shape<'s>), Unreadable> {
        let (mut lhs, mut shape) = self.unary()?;
        loop {
            let (op, level) = match self.peek()?.0 {
                Token::Sym(Sym::Op(op)) if op.level() >= min => (op, op.level()),
                _ => return Ok((lhs, shape)),
            };
            self.next()?;
            let rhs = self.deeper(|r| match level.above() {
                Some(up) => r.binary(up),
                None => r.unary(),
            })?;
            lhs = self.fold.binary(op, lhs, rhs.0);
            shape = Shape::Other;
        }
    }

    /// `!` and `-` before an operand, or the operand.
    fn unary(&mut self) -> Result<(F::Value, Shape<'s>), Unreadable> {
        let op = match self.peek()?.0 {
            Token::Sym(Sym::Not) => Unary::Not,
            Token::Sym(Sym::Op(Binary::Minus)) => Unary::Negate,
            _ => return self.primary(),
        };
        self.next()?;
        let (v, _) = self.deeper(Self::unary)?;
        Ok((self.fold.unary(op, v), Shape::Other))
    }

    /// A literal, a string, a path, an element, or a parenthesised
    /// subexpression.
    fn primary(&mut self) -> Result<(F::Value, Shape<'s>), Unreadable> {
        let (t, at) = self.next()?;
        let v = match t {
            Token::Number(n) => self.fold.number(n),
            Token::True => self.fold.number(1.0),
            Token::False => self.fold.number(0.0),
            Token::Null => self.fold.null(),
            Token::Str(text) => {
                let nest = Nest {
                    strings: self.nest.strings,
                    depth: self.depth,
                };
                self.deeper(|r| Ok(r.fold.string(text, nest)))?
            }
            Token::Path(p, optional) => {
                match self.peek()?.0 {
                    Token::Sym(Sym::Open) if optional == Optional::No => {
                        return Err(Unreadable::Call(ByteAt(at)));
                    }
                    Token::Sym(Sym::OpenBracket) if optional == Optional::No => {
                        self.next()?;
                        let index = self.deeper(|r| r.conditional())?.0;
                        match self.next()? {
                            (Token::Sym(Sym::CloseBracket), _) => {}
                            (_, at) => return Err(Unreadable::Syntax(ByteAt(at))),
                        }
                        return Ok((self.fold.element(p, index), Shape::Other));
                    }
                    _ => {}
                }
                let v = self.fold.path(p, optional);
                return Ok((
                    v,
                    match optional {
                        Optional::No => Shape::Path(p),
                        Optional::Yes => Shape::Other,
                    },
                ));
            }
            Token::Sym(Sym::Open) => {
                let v = self.deeper(|r| r.conditional())?.0;
                match self.next()? {
                    (Token::Sym(Sym::Close), _) => v,
                    (_, at) => return Err(Unreadable::Syntax(ByteAt(at))),
                }
            }
            _ => return Err(Unreadable::Syntax(ByteAt(at))),
        };
        Ok((v, Shape::Other))
    }
}

/// The [`Fold`] that only visits: every path it reads, its strings' too.
struct Visit<P> {
    named: P,
    found: Found,
}

/// Whether a visit found a name it looked for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Found {
    No,
    Yes,
}

impl<P: FnMut(&str) -> bool> Visit<P> {
    fn saw(&mut self, path: &str) {
        if (self.named)(path) {
            self.found = Found::Yes;
        }
    }
}

impl<'s, P: FnMut(&str) -> bool> Fold<'s> for Visit<P> {
    type Value = ();
    type Items = ();
    fn number(&mut self, _: f64) {}
    fn null(&mut self) {}
    fn string(&mut self, text: &'s str, nest: Nest) {
        string_parts(text, nest.strings, |p| {
            if let Interpolated::Expression(e) = p {
                let _ = read(e, nest.inner(nest.depth), self);
            }
        });
    }
    fn path(&mut self, path: &'s str, _: Optional) {
        self.saw(path);
    }
    fn element(&mut self, path: &'s str, _: ()) {
        self.saw(path);
    }
    fn unary(&mut self, _: Unary, _: ()) {}
    fn binary(&mut self, _: Binary, _: (), _: ()) {}
    fn ternary(&mut self, _: (), _: (), _: ()) {}
    fn items(&mut self) {}
    fn item(&mut self, _: &mut (), _: ()) {}
    fn array(&mut self, _: ()) {}
}

/// **Whether the expression `e` names a path `named` holds for**, in its
/// strings' expressions at any depth too, as far as it can be read.
pub fn names(e: &str, named: impl FnMut(&str) -> bool) -> bool {
    let mut v = Visit {
        named,
        found: Found::No,
    };
    let _ = read(e, Nest::TOP, &mut v);
    v.found == Found::Yes
}

#[cfg(feature = "alloc")]
pub use eval::*;

#[cfg(feature = "alloc")]
mod eval {
    use super::*;
    use crate::interpolate::{expand_escapes, interpolated_parts};
    use alloc::string::String;
    use alloc::vec::Vec;
    use core::cmp::Ordering;

    /// **A value**, as JSON has them, iTerm2's booleans being numbers.
    #[derive(Clone, PartialEq, Debug)]
    pub enum Value {
        Null,
        Number(f64),
        Text(String),
        Array(Vec<Value>),
    }

    /// **Why an expression has no value.**
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum Fault {
        Unreadable(Unreadable),
        /// An arithmetic or logical operator given something not a number.
        NotNumbers(Binary),
        /// A relational operator given two values it does not order.
        Unordered(Binary),
        /// A unary operator given something not a number.
        NotANumber(Unary),
        /// A ternary whose condition is neither a number nor null.
        Condition,
        /// An index into a variable that is not an array.
        NotAnArray,
        /// An index that is not a number.
        NotAnIndex,
        /// An index past the array's ends.
        OutOfRange,
        /// An undefined variable written without `?`, given to an
        /// operator, a ternary, an array or an index.
        Undefined,
    }

    impl core::fmt::Display for Fault {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.write_str(match self {
                Fault::Unreadable(Unreadable::Syntax(_)) => "Syntax error",
                Fault::Unreadable(Unreadable::Call(_)) => "Function calls are not allowed",
                Fault::Unreadable(Unreadable::TooDeep) => "Nested too deeply",
                Fault::NotNumbers(Binary::And | Binary::Or) => "Logical operators need numbers",
                Fault::NotNumbers(_) => "Arithmetic needs numbers",
                Fault::Unordered(_) => "These values cannot be ordered",
                Fault::NotANumber(Unary::Not) => "Logical not needs a number",
                Fault::NotANumber(Unary::Negate) => "Negation needs a number",
                Fault::Condition => "The condition is not a number",
                Fault::NotAnArray => "Not an array",
                Fault::NotAnIndex => "The index is not a number",
                Fault::OutOfRange => "The index is out of range",
                Fault::Undefined => "Undefined variable",
            })
        }
    }

    /// **What an expression evaluates to**: a value, empty for a reference
    /// to an undefined variable standing alone, or a fault.
    #[derive(Clone, PartialEq, Debug)]
    pub enum Evaluated {
        Value(Value),
        Undefined,
        Fault(Fault),
    }

    impl From<Result<Value, Fault>> for Evaluated {
        fn from(r: Result<Value, Fault>) -> Evaluated {
            match r {
                Ok(v) => Evaluated::Value(v),
                Err(f) => Evaluated::Fault(f),
            }
        }
    }

    /// What an operator is given: an undefined variable is a fault there.
    fn operand(e: Evaluated) -> Evaluated {
        match e {
            Evaluated::Undefined => Evaluated::Fault(Fault::Undefined),
            e => e,
        }
    }

    /// `a` and `b` joined by `f`: the first fault, an undefined variable
    /// one, else what `f` makes of the two values.
    fn both(
        a: Evaluated,
        b: Evaluated,
        f: impl FnOnce(Value, Value) -> Result<Value, Fault>,
    ) -> Evaluated {
        match (operand(a), operand(b)) {
            (Evaluated::Value(x), Evaluated::Value(y)) => f(x, y).into(),
            (Evaluated::Fault(e), _) | (_, Evaluated::Fault(e)) => Evaluated::Fault(e),
            _ => Evaluated::Fault(Fault::Undefined),
        }
    }

    /// The number a value counts as for arithmetic: a number, or a string
    /// that reads as one.
    fn number_of(v: &Value) -> Option<f64> {
        match v {
            Value::Number(n) => Some(*n),
            Value::Text(s) => s.parse::<f64>().ok(),
            _ => None,
        }
    }

    /// Both values as numbers, or neither.
    fn numbers(a: &Value, b: &Value) -> Option<(f64, f64)> {
        Some((number_of(a)?, number_of(b)?))
    }

    fn truth(t: bool) -> Value {
        Value::Number(match t {
            true => 1.0,
            false => 0.0,
        })
    }

    /// Two values the same: numbers by value, strings and arrays by their
    /// contents, null with null.
    fn same(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => x == y,
            (Value::Text(x), Value::Text(y)) => x == y,
            (Value::Array(x), Value::Array(y)) => {
                x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q))
            }
            (Value::Null, Value::Null) => true,
            _ => false,
        }
    }

    /// Two arrays in order: item by item, an equal or unordered pair
    /// passed over, then by length.
    fn order_items(a: &[Value], b: &[Value]) -> Ordering {
        for (x, y) in a.iter().zip(b) {
            let o = match (x, y) {
                (Value::Number(p), Value::Number(q)) => p.partial_cmp(q).unwrap_or(Ordering::Equal),
                (Value::Text(p), Value::Text(q)) => p.cmp(q),
                (Value::Array(p), Value::Array(q)) => order_items(p, q),
                _ => Ordering::Equal,
            };
            if o != Ordering::Equal {
                return o;
            }
        }
        a.len().cmp(&b.len())
    }

    fn binary(op: Binary, a: Value, b: Value) -> Result<Value, Fault> {
        let nums = numbers(&a, &b);
        match op {
            Binary::Plus | Binary::Minus | Binary::Times | Binary::DividedBy => {
                let (x, y) = nums.ok_or(Fault::NotNumbers(op))?;
                Ok(Value::Number(match op {
                    Binary::Plus => x + y,
                    Binary::Minus => x - y,
                    Binary::Times => x * y,
                    _ => x / y,
                }))
            }
            Binary::And | Binary::Or => {
                let (x, y) = nums.ok_or(Fault::NotNumbers(op))?;
                Ok(truth(match op {
                    Binary::And => x != 0.0 && y != 0.0,
                    _ => x != 0.0 || y != 0.0,
                }))
            }
            Binary::Equal | Binary::NotEqual => {
                let eq = match nums {
                    Some((x, y)) => x == y,
                    None => same(&a, &b),
                };
                Ok(truth(eq == (op == Binary::Equal)))
            }
            Binary::Less | Binary::Greater | Binary::LessOrEqual | Binary::GreaterOrEqual => {
                let o = match (nums, &a, &b) {
                    (Some((x, y)), _, _) => x.partial_cmp(&y),
                    (None, Value::Text(x), Value::Text(y)) => Some(x.cmp(y)),
                    (None, Value::Array(x), Value::Array(y)) => Some(order_items(x, y)),
                    (None, Value::Null, _) | (None, _, Value::Null) => {
                        let nulls = a == Value::Null && b == Value::Null;
                        let holds = matches!(op, Binary::LessOrEqual | Binary::GreaterOrEqual);
                        return Ok(truth(nulls && holds));
                    }
                    _ => return Err(Fault::Unordered(op)),
                };
                Ok(truth(match (op, o) {
                    (_, None) => false,
                    (Binary::Less, Some(o)) => o == Ordering::Less,
                    (Binary::Greater, Some(o)) => o == Ordering::Greater,
                    (Binary::LessOrEqual, Some(o)) => o != Ordering::Greater,
                    (_, Some(o)) => o != Ordering::Less,
                }))
            }
        }
    }

    /// The [`Fold`] that evaluates, its variables read through `vars`.
    struct Eval<'v> {
        vars: &'v dyn Fn(&str) -> Option<Value>,
    }

    impl<'s> Fold<'s> for Eval<'_> {
        type Value = Evaluated;
        type Items = Result<Vec<Value>, Evaluated>;

        fn number(&mut self, n: f64) -> Evaluated {
            Evaluated::Value(Value::Number(n))
        }
        fn null(&mut self) -> Evaluated {
            Evaluated::Value(Value::Null)
        }
        fn string(&mut self, text: &'s str, nest: Nest) -> Evaluated {
            let mut out = String::new();
            let mut fault = None;
            string_parts(text, nest.strings, |p| match p {
                Interpolated::Literal(l) => expand_escapes(l, |c| out.push(c)),
                Interpolated::Expression(e) => match read(e, nest.inner(nest.depth), self) {
                    Ok(Evaluated::Value(v)) => write(&v, &mut out),
                    Ok(Evaluated::Undefined) => {}
                    Ok(Evaluated::Fault(f)) => {
                        fault.get_or_insert(f);
                    }
                    Err(u) => {
                        fault.get_or_insert(Fault::Unreadable(u));
                    }
                },
            });
            match fault {
                Some(f) => Evaluated::Fault(f),
                None => Evaluated::Value(Value::Text(out)),
            }
        }
        fn path(&mut self, path: &'s str, optional: Optional) -> Evaluated {
            match ((self.vars)(path), optional) {
                (Some(v), _) => Evaluated::Value(v),
                (None, Optional::Yes) => Evaluated::Value(Value::Null),
                (None, Optional::No) => Evaluated::Undefined,
            }
        }
        fn element(&mut self, path: &'s str, index: Evaluated) -> Evaluated {
            let array = self.path(path, Optional::No);
            // An undefined array indexed is the reference standing alone.
            let index = operand(index);
            if array == Evaluated::Undefined && matches!(index, Evaluated::Value(_)) {
                return Evaluated::Undefined;
            }
            both(array, index, |a, i| {
                let Value::Array(items) = a else {
                    return Err(Fault::NotAnArray);
                };
                let Value::Number(n) = i else {
                    return Err(Fault::NotAnIndex);
                };
                let n = core::f64::math::trunc(n);
                if !(0.0..items.len() as f64).contains(&n) {
                    return Err(Fault::OutOfRange);
                }
                items.into_iter().nth(n as usize).ok_or(Fault::OutOfRange)
            })
        }
        fn unary(&mut self, op: Unary, given: Evaluated) -> Evaluated {
            match operand(given) {
                Evaluated::Value(Value::Number(n)) => Evaluated::Value(match op {
                    Unary::Negate => Value::Number(-n),
                    Unary::Not => truth(n == 0.0),
                }),
                Evaluated::Value(_) => Evaluated::Fault(Fault::NotANumber(op)),
                other => other,
            }
        }
        fn binary(&mut self, op: Binary, lhs: Evaluated, rhs: Evaluated) -> Evaluated {
            both(lhs, rhs, |a, b| binary(op, a, b))
        }
        fn ternary(&mut self, c: Evaluated, then: Evaluated, otherwise: Evaluated) -> Evaluated {
            // An undefined variable in either branch is a fault whichever
            // the condition picks, as iTerm2 refuses it when it reads it.
            let c = operand(c);
            if matches!(c, Evaluated::Value(_))
                && [&then, &otherwise].contains(&&Evaluated::Undefined)
            {
                return Evaluated::Fault(Fault::Undefined);
            }
            let holds = match c {
                Evaluated::Value(Value::Null) => false,
                Evaluated::Value(Value::Number(n)) => n != 0.0 && !n.is_nan(),
                Evaluated::Value(_) => return Evaluated::Fault(Fault::Condition),
                other => return other,
            };
            match holds {
                true => then,
                false => otherwise,
            }
        }
        fn items(&mut self) -> Self::Items {
            Ok(Vec::new())
        }
        fn item(&mut self, items: &mut Self::Items, item: Evaluated) {
            *items = match (core::mem::replace(items, Ok(Vec::new())), operand(item)) {
                (Err(e), _) => Err(e),
                (Ok(_), Evaluated::Fault(f)) => Err(Evaluated::Fault(f)),
                (Ok(mut v), Evaluated::Value(x)) => {
                    v.push(x);
                    Ok(v)
                }
                (Ok(_), Evaluated::Undefined) => Err(Evaluated::Fault(Fault::Undefined)),
            };
        }
        fn array(&mut self, items: Self::Items) -> Evaluated {
            match items {
                Ok(v) => Evaluated::Value(Value::Array(v)),
                Err(e) => e,
            }
        }
    }

    /// **The expression `e`, at `nest`, evaluated** with its variables
    /// read through `vars`, `None` for an undefined one.
    pub fn evaluate_expression(
        e: &str,
        nest: Nest,
        vars: &dyn Fn(&str) -> Option<Value>,
    ) -> Evaluated {
        let mut fold = Eval { vars };
        match read(e, nest, &mut fold) {
            Ok(v) => v,
            Err(u) => Evaluated::Fault(Fault::Unreadable(u)),
        }
    }

    /// **An interpolated string evaluated**, as iTerm2 evaluates a badge's
    /// format: each literal part's escapes expanded and each expression its
    /// value written as text, an expression naming an undefined variable
    /// empty; with no expression the whole string's escapes expanded, as
    /// [`crate::interpolate`] does. The first fault fails the whole string,
    /// as iTerm2's evaluator ends a string at its first error.
    pub fn evaluate(s: &str, vars: &dyn Fn(&str) -> Option<Value>) -> Result<String, Fault> {
        let mut out = String::new();
        let mut any = false;
        interpolated_parts(s, |p| any |= matches!(p, Interpolated::Expression(_)));
        if !any {
            expand_escapes(s, |c| out.push(c));
            return Ok(out);
        }
        let mut fault = None;
        interpolated_parts(s, |p| match p {
            Interpolated::Literal(l) => expand_escapes(l, |c| out.push(c)),
            Interpolated::Expression(e) => match evaluate_expression(e, Nest::TOP, vars) {
                Evaluated::Value(v) => write(&v, &mut out),
                Evaluated::Undefined => {}
                Evaluated::Fault(f) => {
                    fault.get_or_insert(f);
                }
            },
        });
        match fault {
            Some(f) => Err(f),
            None => Ok(out),
        }
    }

    /// **A value as text**, as iTerm2 writes one into a string: a string
    /// as it is, a number as [`write_number`], an array's items each so,
    /// `[a, b]`, and null as nothing.
    pub fn write(v: &Value, out: &mut String) {
        match v {
            Value::Null => {}
            Value::Number(n) => write_number(*n, out),
            Value::Text(s) => out.push_str(s),
            Value::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    write(item, out);
                }
                out.push(']');
            }
        }
    }

    /// **A number as `NSNumber` writes a double**, C's `%0.16g`
    /// (`[[source: apple-nsnumber-description-withlocale]]`): sixteen
    /// significant digits, in fixed notation while the exponent is from -4
    /// to 15 and as `1e+16` past that, trailing zeros and a bare point
    /// dropped; `nan`, `inf` and `-inf` as C writes them.
    pub fn write_number(n: f64, out: &mut String) {
        use core::fmt::Write;
        if n.is_nan() {
            return out.push_str("nan");
        }
        if n.is_infinite() {
            return out.push_str(if n < 0.0 { "-inf" } else { "inf" });
        }
        let mut sci = String::new();
        let _ = write!(sci, "{n:.15e}");
        let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
        let exp: i32 = exp.parse().unwrap_or(0);
        let (sign, mantissa) = match mantissa.strip_prefix('-') {
            Some(m) => ("-", m),
            None => ("", mantissa),
        };
        let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
        out.push_str(sign);
        let trim = |s: &str| String::from(s.trim_end_matches('0'));
        if (-4..16).contains(&exp) {
            let (int, frac) = match usize::try_from(exp) {
                Ok(e) => (String::from(&digits[..=e]), trim(&digits[e + 1..])),
                Err(_) => {
                    let zeros = usize::try_from(-exp - 1).unwrap_or(0);
                    let mut f: String = core::iter::repeat_n('0', zeros).collect();
                    f.push_str(&digits);
                    (String::from("0"), trim(&f))
                }
            };
            out.push_str(&int);
            if !frac.is_empty() {
                out.push('.');
                out.push_str(&frac);
            }
        } else {
            out.push_str(&digits[..1]);
            let frac = trim(&digits[1..]);
            if !frac.is_empty() {
                out.push('.');
                out.push_str(&frac);
            }
            let _ = write!(
                out,
                "e{}{:02}",
                if exp < 0 { '-' } else { '+' },
                exp.unsigned_abs()
            );
        }
    }
}
