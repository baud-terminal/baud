//! A value and the generation every change to it moves: a reader that kept
//! the generation it last took knows, by one compare, whether there is
//! anything new to read.

/// **Where a [`Tracked`] value stands**: moved by every `&mut` of it, so
/// two equal generations of one `Tracked` mean the value is the one it was
/// when the first was read. Ordering one generation before another says
/// nothing a reader needs, so there is no `Ord`.
///
/// Made only by a `Tracked`: the field is private, so a reader cannot hold
/// a generation its value never reached and skip a change.
///
/// ```compile_fail,E0423
/// let _ = baud_domain::PendingGen(0);
/// ```
///
/// The legal path:
///
/// ```
/// let t = baud_domain::Tracked::new(0u8);
/// let _: baud_domain::PendingGen = t.generation();
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PendingGen(u64);

impl PendingGen {
    /// The generation after this one.
    const fn next(self) -> PendingGen {
        PendingGen(self.0.wrapping_add(1))
    }
}

/// **A value whose every `&mut` moves its generation** ([`PendingGen`]).
///
/// The only `&mut T` is [`Tracked::edit`], which moves the generation
/// before it lends the value, so no change goes unseen by a reader
/// comparing generations; [`Tracked::set`] moves it only when the new
/// value differs. There is no `DerefMut`, no `get_mut`, and no `Clone` or
/// `Default`, either of which would make a second value with the first's
/// generation.
///
/// ```compile_fail,E0616
/// fn f(t: &mut baud_domain::Tracked<u8>) { t.value = 1; }
/// ```
///
/// ```compile_fail,E0599
/// fn f(t: &mut baud_domain::Tracked<u8>) { let _ = t.get_mut(); }
/// ```
///
/// The legal path:
///
/// ```
/// fn f(t: &mut baud_domain::Tracked<u8>) { *t.edit() = 1; }
/// let mut t = baud_domain::Tracked::new(0u8);
/// let was = t.generation();
/// f(&mut t);
/// assert_ne!(t.generation(), was);
/// ```
#[derive(Debug)]
pub struct Tracked<T> {
    value: T,
    at: PendingGen,
}

impl<T> Tracked<T> {
    /// `value`, at its first generation.
    pub const fn new(value: T) -> Tracked<T> {
        Tracked {
            value,
            at: PendingGen(0),
        }
    }

    /// The value.
    pub const fn get(&self) -> &T {
        &self.value
    }

    /// Where the value stands.
    pub const fn generation(&self) -> PendingGen {
        self.at
    }

    /// **The value to change**: the generation moves first, whether or not
    /// the caller then changes anything.
    pub fn edit(&mut self) -> &mut T {
        self.at = self.at.next();
        &mut self.value
    }

    /// The value replaced by `value`; the generation moves only if the two
    /// differ.
    pub fn set(&mut self, value: T)
    where
        T: PartialEq,
    {
        if self.value != value {
            *self.edit() = value;
        }
    }
}
