//! The table behind the `Long` handles that Kotlin holds.
//!
//! A handle is an opaque number, never a pointer: native code only ever looks
//! it up here, so a stale, unknown or forged handle finds nothing instead of
//! reaching freed memory. Handles count up from 1 and are never reused, so a
//! stale handle cannot find a newer instance either.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

pub struct Registry<T> {
    inner: Mutex<Inner<T>>,
}

struct Inner<T> {
    last: i64,
    entries: BTreeMap<i64, Arc<T>>,
}

impl<T> Registry<T> {
    pub const fn new() -> Self {
        Registry {
            inner: Mutex::new(Inner {
                last: 0,
                entries: BTreeMap::new(),
            }),
        }
    }

    // The lock is held only for a map operation (no callback, no JNI or mpv
    // call happens under it), so a panic cannot leave the map half-updated:
    // poisoning carries no information here.
    fn lock(&self) -> MutexGuard<'_, Inner<T>> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Stores `value` and returns its new handle, which is never 0 (Kotlin's
    /// "no instance") and never negative. Gives `value` back only if the
    /// handles ran out.
    pub fn insert(&self, value: Arc<T>) -> Result<i64, Arc<T>> {
        let mut inner = self.lock();
        let Some(handle) = inner.last.checked_add(1) else {
            return Err(value);
        };
        inner.last = handle;
        inner.entries.insert(handle, value);
        Ok(handle)
    }

    /// The entry for `handle`, if it is still registered. The returned `Arc`
    /// keeps the entry's memory alive however long the caller uses it, even if
    /// the handle is removed meanwhile.
    pub fn get(&self, handle: i64) -> Option<Arc<T>> {
        self.lock().entries.get(&handle).cloned()
    }

    pub fn remove(&self, handle: i64) -> Option<Arc<T>> {
        self.lock().entries.remove(&handle)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn handles_start_at_one_and_are_not_reused() {
        let r = Registry::new();
        let a = r.insert(Arc::new("a")).unwrap();
        let b = r.insert(Arc::new("b")).unwrap();
        assert_eq!(a, 1);
        assert_eq!(b, 2);
        assert_eq!(*r.remove(a).unwrap(), "a");
        let c = r.insert(Arc::new("c")).unwrap();
        assert_eq!(c, 3);
        assert!(r.get(a).is_none());
        assert_eq!(*r.get(c).unwrap(), "c");
    }

    #[test]
    fn unknown_and_stale_handles_find_nothing() {
        let r = Registry::new();
        assert!(r.get(0).is_none());
        assert!(r.get(-1).is_none());
        assert!(r.get(i64::MAX).is_none());
        let h = r.insert(Arc::new(5)).unwrap();
        assert!(r.remove(h).is_some());
        assert!(r.remove(h).is_none(), "second remove");
        assert!(r.get(h).is_none());
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn a_looked_up_entry_outlives_its_removal() {
        let r = Registry::new();
        let value = Arc::new(String::from("still here"));
        let h = r.insert(Arc::clone(&value)).unwrap();
        let held = r.get(h).unwrap();
        r.remove(h);
        drop(value);
        assert_eq!(*held, "still here");
        assert_eq!(Arc::strong_count(&held), 1);
    }

    #[test]
    fn exhausted_handles_fail_cleanly() {
        let r = Registry::new();
        r.lock().last = i64::MAX - 1;
        assert_eq!(r.insert(Arc::new(1)), Ok(i64::MAX));
        assert_eq!(*r.insert(Arc::new(2)).unwrap_err(), 2);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn concurrent_inserts_get_distinct_handles() {
        let r = Arc::new(Registry::new());
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let r = Arc::clone(&r);
                thread::spawn(move || {
                    (0..100)
                        .map(|i| r.insert(Arc::new(i)).unwrap())
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        let mut all: Vec<i64> = threads
            .into_iter()
            .flat_map(|t| t.join().unwrap())
            .collect();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), 800);
        assert_eq!(r.len(), 800);
    }
}
