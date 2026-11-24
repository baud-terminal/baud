//! The replies a connection waits for, matched by REPLY_SERIAL in one slot
//! read (`[[source: dbus-spec]]`, "Message Types": a METHOD_RETURN or
//! ERROR names the call it answers by REPLY_SERIAL).

use core::fmt;

use super::header::Serial;

/// How many calls an [`Awaiting`] has refused because their slot was held.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Refusals(u32);

impl Refusals {
    /// The count.
    #[inline]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A call [`Awaiting::insert`] refused: its value, handed back, and the
/// serial of the call that holds the slot.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Refused<T> {
    value: T,
    held: Serial,
}

impl<T> Refused<T> {
    /// The serial of the call whose reply holds the slot.
    #[inline]
    pub const fn held(&self) -> Serial {
        self.held
    }

    /// The value the refused call would have left.
    #[inline]
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T> fmt::Debug for Refused<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Refused(slot held by serial {})", self.held.get())
    }
}

/// The replies a connection waits for, at most `N` (a power of two), each
/// call's value in slot `serial & (N - 1)` with its serial, so a reply's
/// REPLY_SERIAL finds its call in one read and the stored serial confirms
/// it. A call whose slot is held is refused and counted rather than
/// evicting the call that holds it, whose reply would then find nothing.
/// Serials are drawn in order, so `N` calls in flight at once each have
/// their own slot.
///
/// A reply is looked up by a [`Serial`], never a number:
///
/// ```compile_fail,E0308
/// use baud_domain::dbus::Awaiting;
/// let mut awaiting: Awaiting<&str, 32> = Awaiting::new();
/// let _ = awaiting.take(5u32);
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::{Awaiting, Serials};
/// let mut awaiting: Awaiting<&str, 32> = Awaiting::new();
/// let sent = Serials::new().next();
/// awaiting.insert(sent, "notify").expect("a free slot");
/// assert_eq!(awaiting.take(sent), Some("notify"));
/// ```
///
/// `N` that is not a power of two does not compile:
///
/// ```compile_fail,E0080
/// use baud_domain::dbus::Awaiting;
/// let _: Awaiting<u8, 24> = Awaiting::new();
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::Awaiting;
/// let _: Awaiting<u8, 32> = Awaiting::new();
/// ```
#[derive(Clone, Debug)]
pub struct Awaiting<T, const N: usize> {
    slots: [Option<(Serial, T)>; N],
    refused: Refusals,
}

impl<T, const N: usize> Awaiting<T, N> {
    /// Nothing waited for.
    pub const fn new() -> Awaiting<T, N> {
        const { assert!(N.is_power_of_two(), "Awaiting's N is a power of two") };
        Awaiting {
            slots: [const { None }; N],
            refused: Refusals(0),
        }
    }

    /// The slot `serial` names: its low bits.
    const fn slot(serial: Serial) -> usize {
        serial.get() as usize & (N - 1)
    }

    /// Waits for the reply to `serial`, keeping `value` for it; refused,
    /// with `value` handed back and the refusal counted, when another call
    /// holds the slot.
    #[must_use = "a refused call's reply is not waited for"]
    pub fn insert(&mut self, serial: Serial, value: T) -> Result<(), Refused<T>> {
        let slot = &mut self.slots[Self::slot(serial)];
        if let Some((held, _)) = slot {
            self.refused.0 = self.refused.0.saturating_add(1);
            return Err(Refused { value, held: *held });
        }
        *slot = Some((serial, value));
        Ok(())
    }

    /// The value kept for the call `reply_to` answers, taken: `None` when
    /// no call of that serial is waited for, the slot left as it was.
    pub fn take(&mut self, reply_to: Serial) -> Option<T> {
        let slot = &mut self.slots[Self::slot(reply_to)];
        match slot {
            Some((held, _)) if *held == reply_to => slot.take().map(|(_, value)| value),
            _ => None,
        }
    }

    /// How many calls were refused because their slot was held.
    #[inline]
    pub const fn refusals(&self) -> Refusals {
        self.refused
    }
}

impl<T, const N: usize> Default for Awaiting<T, N> {
    fn default() -> Awaiting<T, N> {
        Awaiting::new()
    }
}
