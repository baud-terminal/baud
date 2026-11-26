//! A sample module for the code map's extractor. Its second sentence is
//! not mapped.

use std::fmt;

/// A plain function. Its second sentence is not mapped.
pub fn plain() {}

/// A signature over several lines, with a where clause.
pub fn spread<T, U>(
    the_first_argument_with_a_long_name: T,
    the_second_argument_with_a_long_name: U,
) -> Option<T>
where
    T: Clone + fmt::Debug,
    U: Into<String>,
{
    let _ = the_second_argument_with_a_long_name.into();
    Some(the_first_argument_with_a_long_name)
}

/// Built only on Linux.
#[cfg(target_os = "linux")]
pub fn on_linux() -> &'static str {
    "{ not a brace"
}

/// A type with an impl of a trait.
#[derive(Clone, Debug)]
pub struct Sample {
    pub name: String,
}

impl fmt::Display for Sample {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl Sample {
    /// Makes one.
    pub fn new(name: String) -> Self {
        // pub fn in_a_line_comment() {}
        Self { name }
    }

    fn private(&self) -> char {
        '{'
    }
}

/// A limit the crate shares.
pub(crate) const LIMIT: char = '}';

/// The example's item is in a code block.
///
/// ```
/// pub fn in_a_doc_example() {}
/// ```
pub enum Shape {
    Round,
    Square,
}

/* pub fn in_a_block_comment() {}
/* nested */ pub struct StillInTheComment;
 */

pub static GREETING: &str = "pub fn in_a_string() {}";
pub const RAW: &str = r#"pub struct InARawString; "{"#;

pub(super) type Alias<'a> = &'a Sample;

pub trait Named {
    fn named(&self) -> String;
}

pub mod inner {
    //! Not the file's module doc.

    /// Inside a module.
    pub fn deep() {}
}

#[macro_export]
macro_rules! sample_macro {
    () => {
        pub fn made_by_the_macro() {}
    };
}

fn private_fn() {
    pub fn local_item() {}
    local_item();
}

unsafe extern "C" {
    /// Declared, not defined.
    pub fn declared(code: core::ffi::c_int) -> core::ffi::c_int;
}

impl<A, B> Named for (A, B) {
    fn named(&self) -> String {
        String::from("pair")
    }
}

/// Its value is past what a signature shows.
pub const LONG: [&str; 4] = [
    "a value long enough",
    "that the signature",
    "stops at the equals sign",
    "and elides the rest",
];
