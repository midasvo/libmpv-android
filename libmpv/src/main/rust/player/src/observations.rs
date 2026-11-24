//! The `reply_userdata` ids of `MPVLib.observeProperty`'s observations, by
//! property name, so that `unobserveProperty(name)` can undo them all
//! (`mpv_unobserve_property` takes an id, not a name).
//!
//! The ids are negative as `i64`, which tells the event thread to deliver
//! their changes to the `EventObserver` callbacks; the property flows' ids,
//! chosen by Kotlin, are positive. Ids are never reused.

use std::collections::HashMap;
use std::ffi::{CStr, CString};

#[derive(Default)]
pub struct Observations {
    last: u64,
    by_name: HashMap<CString, Vec<u64>>,
}

/// The top bit: negative as an `i64`.
const LEGACY: u64 = 1 << 63;

/// Whether a `reply_userdata` belongs to `MPVLib.observeProperty` (or is the 0
/// that the wrapper used before it had ids).
pub fn is_legacy(userdata: u64) -> bool {
    userdata as i64 <= 0
}

impl Observations {
    /// A new id for an observation of `name`, recorded under it.
    pub fn add(&mut self, name: &CStr) -> u64 {
        // 2^63 observations would take millennia; wrap rather than overflow
        self.last = (self.last + 1) & !LEGACY;
        let id = LEGACY | self.last;
        self.by_name.entry(name.to_owned()).or_default().push(id);
        id
    }

    /// Forgets `id` again (mpv refused the observation).
    pub fn remove(&mut self, name: &CStr, id: u64) {
        if let Some(ids) = self.by_name.get_mut(name) {
            ids.retain(|&i| i != id);
            if ids.is_empty() {
                self.by_name.remove(name);
            }
        }
    }

    /// Takes every id recorded for `name`.
    pub fn take(&mut self, name: &CStr) -> Vec<u64> {
        self.by_name.remove(name).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_negative_distinct_and_per_name() {
        let mut o = Observations::default();
        let a = o.add(c"volume");
        let b = o.add(c"volume");
        let c = o.add(c"pause");
        assert!(is_legacy(a) && is_legacy(b) && is_legacy(c));
        assert!((a as i64) < 0);
        assert_ne!(a, b);
        assert_eq!(o.take(c"volume"), vec![a, b]);
        assert_eq!(o.take(c"volume"), Vec::<u64>::new());
        assert_eq!(o.take(c"pause"), vec![c]);
        assert_eq!(o.take(c"never"), Vec::<u64>::new());
    }

    #[test]
    fn a_refused_observation_is_forgotten() {
        let mut o = Observations::default();
        let a = o.add(c"x");
        let b = o.add(c"x");
        o.remove(c"x", a);
        assert_eq!(o.take(c"x"), vec![b]);
        let c = o.add(c"y");
        o.remove(c"y", c);
        assert!(o.by_name.is_empty());
        o.remove(c"z", 1); // unknown: nothing happens
    }

    #[test]
    fn flow_ids_and_zero_are_told_apart() {
        assert!(is_legacy(0), "the id every observation had before");
        assert!(!is_legacy(1));
        assert!(!is_legacy(i64::MAX as u64));
        assert!(is_legacy(u64::MAX));
    }

    #[test]
    fn ids_wrap_without_reaching_the_positive_range() {
        let mut o = Observations {
            last: !LEGACY - 1,
            ..Default::default()
        };
        assert_eq!(o.add(c"x"), u64::MAX);
        let wrapped = o.add(c"x");
        assert_eq!(wrapped, LEGACY);
        assert!(is_legacy(wrapped));
    }
}
