//! The plasma example's report, read as Python's `json.load` reads it,
//! each number kept as its text: `json.load` makes `45` an `int` and
//! `50.362` the `float` nearest it, and the report prints either back as
//! the text it was, so the text is what the summary line shows and
//! [`Value::num`] is what the checks compare.

/// A JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Num(String),
    Str(String),
    List(Vec<Value>),
    Map(Vec<(String, Value)>),
    Bool(bool),
    Null,
}

impl Value {
    /// The member `key` of an object.
    ///
    /// # Errors
    /// When this is not an object or has no such member.
    pub fn get(&self, key: &str) -> Result<&Value, String> {
        match self {
            Value::Map(m) => m
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v)
                .ok_or_else(|| format!("no {key:?} in the report")),
            _ => Err(format!("not an object where {key:?} was looked for")),
        }
    }

    /// A number, as Python's `float()` of its text.
    ///
    /// # Errors
    /// When it is not a number.
    pub fn num(&self) -> Result<f64, String> {
        match self {
            Value::Num(t) => t.parse().map_err(|_| format!("{t} is not a number")),
            other => Err(format!("{other:?} is not a number")),
        }
    }

    /// A string's text.
    ///
    /// # Errors
    /// When it is not a string.
    pub fn text(&self) -> Result<&str, String> {
        match self {
            Value::Str(s) => Ok(s),
            other => Err(format!("{other:?} is not a string")),
        }
    }

    /// A list's numbers.
    ///
    /// # Errors
    /// When it is not a list of numbers.
    pub fn nums(&self) -> Result<Vec<f64>, String> {
        match self {
            Value::List(l) => l.iter().map(Value::num).collect(),
            other => Err(format!("{other:?} is not a list")),
        }
    }

    /// `str()` of what `json.load` makes of it.
    #[must_use]
    pub fn py(&self) -> String {
        match self {
            Value::Num(t) => t.clone(),
            Value::Str(s) => s.clone(),
            Value::List(l) => format!(
                "[{}]",
                l.iter().map(Value::repr).collect::<Vec<_>>().join(", ")
            ),
            Value::Map(_) => "{...}".to_string(),
            Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
            Value::Null => "None".to_string(),
        }
    }

    /// `repr()` of it, as a list prints its items.
    #[must_use]
    pub fn repr(&self) -> String {
        match self {
            Value::Str(s) => crate::py::repr_str(s),
            other => other.py(),
        }
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.b.get(self.i).is_some_and(|c| c.is_ascii_whitespace()) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> Result<(), String> {
        self.ws();
        if self.b.get(self.i) == Some(&c) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected {:?} at byte {}", char::from(c), self.i))
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let start = self.i;
        while let Some(&c) = self.b.get(self.i) {
            match c {
                b'"' => {
                    let s =
                        std::str::from_utf8(&self.b[start..self.i]).map_err(|e| e.to_string())?;
                    self.i += 1;
                    return Ok(s.to_string());
                }
                b'\\' => return Err("an escape, which the report never writes".into()),
                _ => self.i += 1,
            }
        }
        Err("an unterminated string".into())
    }

    fn value(&mut self) -> Result<Value, String> {
        self.ws();
        match self.b.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                let mut m = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Value::Map(m));
                }
                loop {
                    let k = self.string()?;
                    self.eat(b':')?;
                    m.push((k, self.value()?));
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Value::Map(m));
                        }
                        _ => return Err(format!("a bad object at byte {}", self.i)),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut l = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Value::List(l));
                }
                loop {
                    l.push(self.value()?);
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Value::List(l));
                        }
                        _ => return Err(format!("a bad list at byte {}", self.i)),
                    }
                }
            }
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b't') if self.b[self.i..].starts_with(b"true") => {
                self.i += 4;
                Ok(Value::Bool(true))
            }
            Some(b'f') if self.b[self.i..].starts_with(b"false") => {
                self.i += 5;
                Ok(Value::Bool(false))
            }
            Some(b'n') if self.b[self.i..].starts_with(b"null") => {
                self.i += 4;
                Ok(Value::Null)
            }
            Some(_) => {
                let start = self.i;
                while self
                    .b
                    .get(self.i)
                    .is_some_and(|c| c.is_ascii_digit() || b"+-.eE".contains(c))
                {
                    self.i += 1;
                }
                let t = std::str::from_utf8(&self.b[start..self.i]).map_err(|e| e.to_string())?;
                if t.is_empty() || t.parse::<f64>().is_err() {
                    return Err(format!("a bad value at byte {start}"));
                }
                Ok(Value::Num(t.to_string()))
            }
            None => Err("the report ends early".into()),
        }
    }
}

/// Parse a whole document.
///
/// # Errors
/// When it is not one JSON value.
pub fn parse(text: &str) -> Result<Value, String> {
    let mut p = Parser {
        b: text.as_bytes(),
        i: 0,
    };
    let v = p.value()?;
    p.ws();
    if p.i != p.b.len() {
        return Err(format!("text after the value at byte {}", p.i));
    }
    Ok(v)
}
