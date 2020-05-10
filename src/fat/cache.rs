//! Home of the `SectorCache` type; that which all writes and reads to `Storage`
//! flow through.

use super::types::SectorIdx;

use storage_traits::Storage;
use generic_array::{ArrayLength, GenericArray};

use core::cell::{Cell, RefCell};
use core::cmp::Ordering;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};

#[derive(Debug, Clone, Copy, Hash)]
pub enum CacheEntry {
    /// Present but unmodified; can be freely evicted.
    Resident { s: SectorIdx, arr_idx: usize, age: u64, last_accessed: Cell<u64> },
    /// Present and contains modifications.
    Dirty { s: SectorIdx, arr_idx: usize, age: u64, last_accessed: Cell<u64> },
    /// Does not contain a sector.
    Free,
}

impl CacheEntry {
    pub fn new(sector: SectorIdx, idx: usize, counter: &mut u64) -> Self {
        let age = *counter;
        *counter = counter.wrapping_add(1);

        if *counter < new_last_accessed { log::warn!("Internal cache counter overflowed!"); }

        Self::Resident { s: sector, arr_idx: idx, age, last_accessed: Cell::new(age) }
    }

    fn new_for_lookup(s: SectorIdx) -> Self {
        Self::Resident { s, arr_idx: 0, age: 0, last_accessed: 0 }
    }

    /// Errors if the `CacheEntry` is `Free`, otherwise succeeds.
    pub fn mark_as_dirty(&mut self) -> Result<(), ()> {
        use CacheEntry::*;
        *self = match *self {
            Resident { s, arr_idx, age, last_accessed } |
            Dirty { s, arr_idx, age, last_accessed } =>
                Dirty { s, arr_idx, age, last_accessed },
            Free => return Err(()),
        };

        Ok(())
    }

    /// Errors if the `CacheEntry` is not `Dirty`.
    pub fn mark_as_clean(&mut self) -> Result<(), ()> {
        use CacheEntry::*;
        *self = match *self {
            Dirty { s, arr_idx, age, last_accessed } =>
                Resident { s, arr_idx, age, last_accessed },

            Resident { .. } | Free => return Err(()),
        };

        Ok(())
    }

    pub fn is_dirty(&self) -> bool {
        matches!(self, CacheEntry::Dirty { .., .. })
    }

    /// `None` if the `CacheEntry` is `Free`; succeeds otherwise.
    pub fn get_sector_idx(&self) -> Option<SectorIdx> {
        use CacheEntry::*;
        match self {
            Resident { s, .. } | Dirty { s, .. } => Some(s),
            Free => None,
        }
    }

    pub fn accessed(&self, counter: &mut u64) -> Result<usize, ()> {
        let new_last_accessed = counter;
        *counter = counter.wrapping_add(1);

        if *counter < new_last_accessed { log::warn!("Internal cache counter overflowed!"); }

        match self {
            Resident { last_accessed, .. } | Dirty { last_accessed, .. } => {
                let last = last_accessed.get();
                last_accessed.set(new_last_accessed);
                Ok(last)
            },
            Free => return Err(())
        }
    }
}

impl PartialEq for CacheEntry {
    fn eq(&self, other: &Self) -> bool {
        use CacheEntry::*;
        match (self, other) {
            // Once we get or patterns..
            // (Resident { s: a, .. } | Dirty { s: a, .. },
            //    Resident { s: b, .. } | Dirty { s: b, .. }) => a.eq(b),
            // (Resident { .. } | Dirty { .. }, Free) |
            // (Free, Resident { .. } | Dirty { .. }) => false
            // (Free, Free) => true

            (Resident { s: a, .. }, Resident { s: b, ..}) |
            (Resident { s: a, .. }, Dirty { s: b, ..}) |
            (Dirty { s: a, .. }, Resident { s: b, ..}) |
            (Dirty { s: a, .. }, Dirty { s: b, ..}) => a.eq(b),

            (Free, Free) => true,

            (Resident{ .. }, Free) |
            (Dirty { .. }, Free) |
            (Free, Resident { .. }) |
            (Free, Dirty { .. }) => false,
        }
    }
}

impl Eq for CacheEntry { }

impl PartialOrd for CacheEntry {
    #[must_use]
    #[inline]
    fn partial_cmp(&self, other: &CacheEntry) -> Option<Ordering> {
        use CacheEntry::*;
        Some(match (self, other) {
            // Again, or patterns would be nice here..

            (Resident { s: a, .. }, Resident { s: b, .. }) |
            (Resident { s: a, .. }, Dirty { s: b, .. }) |
            (Dirty { s: a, .. }, Resident { s: b, .. }) |
            (Dirty { s: a, .. }, Dirty { s: b, .. }) => a.cmp(b),

            (Free, Free) => Ordering::Equal,

            (Resident { .. }, Free) |
            (Dirty { .. }, Free) => Ordering::Greater,

            (Free, Resident { .. }) |
            (Free, Dirty { .. }) => Ordering::Less,
        })
    }
}

impl Ord for CacheEntry {
    #[must_use]
    #[inline]
    fn cmp(&self, other: &CacheEntry) -> Ordering {
        // The `PartialOrd` impl always returns `Some` so this is okay.
        self.partial_cmp(other).unwrap()
    }
}

impl Default for CacheEntry { fn default() -> Self { CacheEntry::Free } }

