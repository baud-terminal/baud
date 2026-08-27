use core::marker::PhantomData;

/// An invariant lifetime. Two `Brand`s from different scopes are never equal,
/// so values carrying one cannot be passed where the other is expected.
///
/// Invariance is the point: `fn(&'id ()) -> &'id ()` makes `'id` neither
/// covariant nor contravariant, so the compiler will not silently coerce one
/// brand into another.
pub struct Brand<'id>(pub(crate) PhantomData<fn(&'id ()) -> &'id ()>);

impl<'id> Clone for Brand<'id> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'id> Copy for Brand<'id> {}

impl<'id> PartialEq for Brand<'id> {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl<'id> Eq for Brand<'id> {}

impl<'id> PartialOrd for Brand<'id> {
    fn partial_cmp(&self, o: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

impl<'id> Ord for Brand<'id> {
    fn cmp(&self, _: &Self) -> core::cmp::Ordering {
        core::cmp::Ordering::Equal
    }
}
