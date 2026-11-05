//! A value worked out once for a key, and kept while the key is the same: for
//! what is derived from something that is itself kept, such as a setup's
//! part, and asked for again by every run that uses it.

use std::sync::{Arc, Mutex, PoisonError};

/// The value last worked out, and the key it was worked out for.
pub struct Memo<K, V> {
    slot: Mutex<Option<(K, Arc<V>)>>,
}

impl<K: PartialEq, V> Memo<K, V> {
    /// Nothing worked out yet.
    pub fn new() -> Self {
        Self {
            slot: Mutex::new(None),
        }
    }

    /// The value for `key`: the one kept, if it was worked out for `key`, or
    /// else `make`'s, kept in its place. Asked for at once from two threads,
    /// it is worked out once, and the second waits for it.
    pub fn get_or_try<E>(&self, key: K, make: impl FnOnce() -> Result<V, E>) -> Result<Arc<V>, E> {
        let mut slot = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((k, v)) = slot.as_ref()
            && *k == key
        {
            return Ok(Arc::clone(v));
        }
        let v = Arc::new(make()?);
        *slot = Some((key, Arc::clone(&v)));
        Ok(v)
    }

    /// The value for `key`, as [`Memo::get_or_try`], from a `make` that
    /// cannot fail.
    pub fn get_or(&self, key: K, make: impl FnOnce() -> V) -> Arc<V> {
        let made: Result<Arc<V>, std::convert::Infallible> = self.get_or_try(key, || Ok(make()));
        match made {
            Ok(v) => v,
            Err(never) => match never {},
        }
    }
}

impl<K: PartialEq, V> Default for Memo<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Clone, V> Clone for Memo<K, V> {
    fn clone(&self) -> Self {
        let slot = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        Self {
            slot: Mutex::new(slot.as_ref().map(|(k, v)| (k.clone(), Arc::clone(v)))),
        }
    }
}

impl<K: std::fmt::Debug, V> std::fmt::Debug for Memo<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let slot = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        f.debug_struct("Memo")
            .field("key", &slot.as_ref().map(|(k, _)| k))
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// Asked again for the same key, the value kept is given; for another, it
    /// is worked out again and kept instead; a failure keeps nothing.
    #[test]
    fn a_value_is_worked_out_once_a_key() {
        let memo: Memo<u32, u32> = Memo::new();
        let made = Cell::new(0);
        let make = |v: u32| {
            made.set(made.get() + 1);
            v
        };
        assert_eq!(*memo.get_or(1, || make(10)), 10);
        assert_eq!(*memo.get_or(1, || make(11)), 10);
        assert_eq!(made.get(), 1);
        assert_eq!(*memo.get_or(2, || make(20)), 20);
        assert_eq!(made.get(), 2);
        let failed: Result<Arc<u32>, ()> = memo.get_or_try(3, || Err(()));
        assert!(failed.is_err());
        assert_eq!(*memo.get_or(2, || make(21)), 20, "the failure kept nothing");
    }
}
