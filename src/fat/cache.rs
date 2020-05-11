//! Home of the `SectorCache` type; that which all writes and reads to `Storage`
//! flow through.

use super::types::SectorIdx;
use crate::util::{BitMap, BitMapLen};

use storage_traits::Storage;
use generic_array::{ArrayLength, GenericArray};

use core::cell::{RefCell, RefMut, Ref};
use core::cmp::Ordering;
use core::marker::PhantomData;
use core::ops::{Index, IndexMut, DerefMut};

/// Counter type with interior mutability that implements `Copy`
/// (unlike `Cell<u64>`).
///
/// Extremely illegal.
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct CopyCounter(u64);

impl CopyCounter {
    fn new(v: u64) -> Self { Self(v) }

    fn set(&self, v: u64) -> u64 {
        #[allow(mutable_transmutes)] // TODO: this is UB!!! Switch to a Cell and use clone for the slice manipulation!
        let c = unsafe { core::mem::transmute::<&CopyCounter, &mut u64>(self) };

        let old = *c;
        *c = v;
        old
    }

    fn get(&self) -> u64 { self.0 }
}

#[derive(Debug, Clone, Copy)]
pub enum CacheEntry {
    /// Present but unmodified; can be freely evicted.
    Resident { s: SectorIdx, arr_idx: usize, age: u64, last_accessed: CopyCounter },
    /// Present and contains modifications.
    Dirty { s: SectorIdx, arr_idx: usize, age: u64, last_accessed: CopyCounter },
    /// Does not contain a sector.
    Free,
}

impl CacheEntry {
    /*pub */fn new(sector: SectorIdx, idx: usize, counter: &mut u64) -> Self {
        let age = *counter;
        *counter = counter.wrapping_add(1);

        if *counter < age { log::warn!("Internal cache counter overflowed!"); }

        Self::Resident { s: sector, arr_idx: idx, age, last_accessed: CopyCounter::new(0) }
    }

    fn new_for_lookup(s: SectorIdx) -> Self {
        Self::Resident { s, arr_idx: 0, age: 0, last_accessed: CopyCounter::new(0) }
    }

    /// Errors if the `CacheEntry` is `Free`, otherwise succeeds.
    /*pub */fn mark_as_dirty(&mut self) -> Result<(), ()> {
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
    /*pub */fn mark_as_clean(&mut self) -> Result<(), ()> {
        use CacheEntry::*;
        *self = match *self {
            Dirty { s, arr_idx, age, last_accessed } =>
                Resident { s, arr_idx, age, last_accessed },

            Resident { .. } | Free => return Err(()),
        };

        Ok(())
    }

    /*pub */fn is_dirty(&self) -> bool {
        matches!(self, CacheEntry::Dirty { .. })
    }

    /// `None` if the `CacheEntry` is `Free`; succeeds otherwise.
    /*pub */fn get_sector_idx(&self) -> Option<SectorIdx> {
        use CacheEntry::*;
        match self {
            Resident { s, .. } | Dirty { s, .. } => Some(*s),
            Free => None,
        }
    }

    /// `None` if the `CacheEntry` is `Free`; succeeds otherwise.
    /*pub */fn get_arr_idx(&self) -> Option<usize> {
        use CacheEntry::*;
        match self {
            Resident { arr_idx, .. } | Dirty { arr_idx, .. } => Some(*arr_idx),
            Free => None,
        }
    }

    /// Returns the previous accessed time on success and errors when the
    /// `CacheEntry` is `Free`.
    /*pub */fn accessed(&self, counter: &mut u64) -> Result<u64, ()> {
        use CacheEntry::*;

        let new_last_accessed = *counter;
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[allow(non_camel_case_types)]
pub struct CacheTable<SIZE: ArrayLength<CacheEntry>> {
    // To help make cache lookups faster, we keep this in sorted order.
    cache_entry_table: GenericArray<CacheEntry, SIZE>,

    length: usize,
}

impl<S: ArrayLength<CacheEntry>> CacheTable<S> {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn capacity() -> usize {
        S::to_usize()
    }

    pub fn len(&self) -> usize {
        self.length
    }

    pub fn free_entries(&self) -> usize {
        Self::capacity() - self.len()
    }

    /*pub */fn get(&self, s: SectorIdx) -> Option<&CacheEntry> {
        let entry = CacheEntry::new_for_lookup(s);
        self.cache_entry_table
            .as_slice()
            .binary_search(&entry)
            .ok()
            .map(|idx| &self.cache_entry_table.as_slice()[idx])
    }

    /*pub */fn get_mut(&mut self, s: SectorIdx) -> Option<&mut CacheEntry> {
        // Basically the same as the above save for the as_mut_slice calls.
        // Blame the borrow checker for the asymmetry.

        let entry = CacheEntry::new_for_lookup(s);
        match self.cache_entry_table
            .as_mut_slice()
            .binary_search(&entry)
            .ok() {
            Some(idx) => Some(&mut self.cache_entry_table[idx]),
            None => None,
        }
    }

    /// All newly inserted entries are marked as resident.
    ///
    /// Returns an `Err(Some(_))` if the table already contains an entry with
    /// the sector in question.
    ///
    /// Returns an `Err(None)` if we're out of space!
    /*pub */fn insert(
        &mut self,
        s: SectorIdx,
        idx: usize,
        counter: &mut u64,
    ) -> Result<&mut CacheEntry, Option<&mut CacheEntry>> {
        let entry = CacheEntry::new(s, idx, counter);
        match self.cache_entry_table.binary_search(&entry) {
            // If the sector is already in the table, return it's entry:
            Ok(idx) => {
                Err(Some(&mut self.cache_entry_table.as_mut_slice()[idx]))
            },

            Err(idx) => {
                // If it's not present, we were just told where to place this
                // entry.

                // First let's make sure we have room for it:
                if self.free_entries() == 0 {
                    return Err(None);
                }

                // Just to be extra sure, double check that the last element
                // really is free (since we're only adding one thing we only
                // need to check the last element):
                match self.cache_entry_table.as_slice().last() {
                    Some(last) => {
                        assert!(last == &CacheEntry::Free)
                    },
                    None => {
                        // Zero does satisfy the `Unsigned` trait so it's
                        // possible to construct an instance of this type with
                        // SIZE = 0, but the above check (free_entries >= 1)
                        // should catch this.
                        unreachable!()
                    },
                }

                // Now, shift everything at and after the index we were told to
                // insert into one place to the right. Note that we stop at
                // self.length() because there's no reason we need to bother
                // copying empty elements.
                self.cache_entry_table.copy_within(idx..(self.length), idx + 1);

                // Increment our length:
                self.length += 1;

                // And finally, put our new element into place and return it.
                let slot = &mut self.cache_entry_table[idx];
                *slot = entry;
                Ok(slot)
            }
        }
    }

    /// Tries to remove an entry with the given sector.
    ///
    /// Returns `Ok(arr_idx)` if the entry was successfully removed.
    ///
    /// Returns `Err(Some(_))` if the entry exists but is marked as dirty.
    ///
    /// Returns `Err(None)` if an entry for the sector does not exist.
    /*pub */fn remove(
        &mut self,
        s: SectorIdx
    ) -> Result<usize, Option<&mut CacheEntry>> {
        use CacheEntry::*;

        let entry = CacheEntry::new_for_lookup(s);
        match self.cache_entry_table.binary_search(&entry) {
            Ok(idx) => {
                match self.cache_entry_table[idx] {
                    Resident { arr_idx, .. } => {
                        // Move the remaining entries left one.
                        //
                        // | a | b | c | E | e | f | _ | _ | _ | _ |
                        //                  \     /
                        //                  copy to:
                        //                     |
                        //                 /---/
                        //                 V
                        //              /     \
                        // | a | b | c | E | e | f | _ | _ | _ | _ |
                        // | a | b | c | e | f | f | _ | _ | _ | _ |
                        //
                        // And then zero the last element:
                        // | a | b | c | e | f | f | _ | _ | _ | _ |
                        //
                        //                   |
                        //                   V
                        //
                        // | a | b | c | e | f | _ | _ | _ | _ | _ |
                        //
                        // This works even when there are no following entries.

                        self.cache_entry_table
                            .copy_within((idx + 1)..(self.length), idx);

                        self.length -= 1;
                        self.cache_entry_table[self.length] = CacheEntry::Free;

                        Ok(arr_idx)
                    },

                    // If it's dirty, error:
                    Dirty { .. } => Err(Some(&mut self.cache_entry_table[idx])),

                    // This can't happen; lookup _can't_ return a Free sector.
                    Free => unreachable!(),
                }
            },

            Err(_) => {
                // If a corresponding Entry is not present, error:
                Err(None)
            }
        }
    }

    /// Calls a function on every dirty `CacheEntry`.
    /*pub */fn for_each_dirty_entry<E, F: FnMut((usize, &mut CacheEntry)) -> Result<(), E>>(
        &mut self,
        func: F,
    ) -> Result<(), E> {
        self.cache_entry_table.iter_mut()
            .enumerate()
            .filter(|(_, e)| e.is_dirty())
            .map(func)
            .collect()
    }
}

// #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
// pub enum EvictionPolicy {
//     Oldest,
//     Youngest,
//     MostRecentlyAccessed,
//     LeastRecentlyAccessed,
//     UnmodifiedThenOldestModified,
//     UnmodifiedThenYoungestModified,
// }

pub trait EvictionPolicy {
    /// Pick which cache entry, between the two given, you'd rather evict.
    ///
    /// Implementors are encouraged to use an ordering that is _asymmetric_,
    /// _transitive_, and _total_.
    ///
    /// This is basically [`Ord`].
    ///
    /// This only takes &self to be object safe.
    fn compare(&self, a: &CacheEntry, b: &CacheEntry) -> Ordering;

    /// Returns `None` if there are no elements in the array.
    fn pick_entry_to_evict<'arr>(&self, arr: &'arr mut [CacheEntry]) -> Option<&'arr mut CacheEntry> {
        arr.iter_mut()
            .max_by(|a, b| self.compare(a, b))
    }
}

pub type DynEvictionPolicy = &'static (dyn EvictionPolicy + Send + Sync + 'static);

impl EvictionPolicy for DynEvictionPolicy {
    #[inline]
    fn compare(&self, a: &CacheEntry, b: &CacheEntry) -> Ordering {
        (*self).compare(a, b)
    }
}

pub mod eviction_policies {
    use super::{CacheEntry::{self, *}, Ordering, EvictionPolicy, DynEvictionPolicy};

    macro_rules! policy {
        ($name:ident ($instance:ident): $($arms:tt)*) => {
            #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord,
                Hash, Default
            )]
            pub struct $name;
            pub static $instance: DynEvictionPolicy = &$name;

            impl EvictionPolicy for $name {
                fn compare(&self, a: &CacheEntry, b: &CacheEntry) -> Ordering {
                    match (a, b) {
                        $($arms)*

                        (Free, Resident { .. }) |
                        (Free, Dirty { .. }) => Ordering::Greater,

                        (Resident { .. }, Free) |
                        (Dirty { .. }, Free) => Ordering::Less,

                        (Free, Free) => Ordering::Equal,
                    }
                }
            }
        };

        (<$inner:ident as $instance:ident> $name:ident with ($a:ident, $b:ident): $($arms:tt)*) => {
            #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord,
                Hash, Default
            )]
            pub struct $name<$inner: EvictionPolicy>($inner);

            impl<$inner: EvictionPolicy> EvictionPolicy for $name<$inner> {
                fn compare(&self, $a: &CacheEntry, $b: &CacheEntry) -> Ordering {
                    let $instance: &$inner = &self.0;

                    match ($a, $b) {
                        $($arms)*

                        (Free, Resident { .. }) |
                        (Free, Dirty { .. }) => Ordering::Greater,

                        (Resident { .. }, Free) |
                        (Dirty { .. }, Free) => Ordering::Less,

                        (Free, Free) => Ordering::Equal,
                    }
                }
            }
        };
    }

    // pub struct Youngest;
    // pub static YOUNGEST: dyn EvectionPolicy = Youngest;
    // impl EvictionPolicy for Youngest {
    //     fn compare(&self, a: &CacheEntry, b: &CacheEntry) -> Ordering {
    //         match (a, b) {
    //             (Resident { age: a, .. }, Resident { age: b, .. }) |
    //             (Resident { age: a, .. }, Dirty { age: b, .. }) |
    //             (Dirty { age: a, .. }, Resident { age: b, .. }) |
    //             (Dirty { age: a, .. }, Dirty { age: b, .. }) => a.cmp(b),

    //             (Free, Resident { .. }) |
    //             (Free, Dirty { .. }) => Ordering::Greater,

    //             (Resident { .. }, Free) |
    //             (Dirty { .. }, Free) => Ordering::Less,
    //         }
    //     }
    // }

    policy! { Youngest (YOUNGEST):
        (Resident { age: a, .. }, Resident { age: b, .. }) |
        (Resident { age: a, .. }, Dirty { age: b, .. }) |
        (Dirty { age: a, .. }, Resident { age: b, .. }) |
        (Dirty { age: a, .. }, Dirty { age: b, .. }) => a.cmp(b),
    }

    // pub struct Oldest;
    // Unfortunately, because of the (Free, _) and (_, Free) cases this can't
    // just be `Youngest::cmp(_, _).reverse()`.
    policy! { Oldest (OLDEST):
        (Resident { age: a, .. }, Resident { age: b, .. }) |
        (Resident { age: a, .. }, Dirty { age: b, .. }) |
        (Dirty { age: a, .. }, Resident { age: b, .. }) |
        (Dirty { age: a, .. }, Dirty { age: b, .. }) => a.cmp(b).reverse(),
    }

    //     MostRecentlyAccessed,
    //     LeastRecentlyAccessed,
    //     UnmodifiedThenOldestModified,
    //     UnmodifiedThenYoungestModified,

    policy! { MostRecentlyAccessed (MOST_RECENTLY_ACCESSED):
        (Resident { last_accessed: a, .. }, Resident { last_accessed: b, .. }) |
        (Resident { last_accessed: a, .. }, Dirty { last_accessed: b, .. }) |
        (Dirty { last_accessed: a, .. }, Resident { last_accessed: b, .. }) |
        (Dirty { last_accessed: a, .. }, Dirty { last_accessed: b, .. }) => a.get().cmp(&b.get()),
    }

    policy! { LeastRecentlyAccessed (LEAST_RECENTLY_ACCESSED):
        (Resident { last_accessed: a, .. }, Resident { last_accessed: b, .. }) |
        (Resident { last_accessed: a, .. }, Dirty { last_accessed: b, .. }) |
        (Dirty { last_accessed: a, .. }, Resident { last_accessed: b, .. }) |
        (Dirty { last_accessed: a, .. }, Dirty { last_accessed: b, .. }) => a.get().cmp(&b.get()).reverse(),
    }

    policy! { <Inner as inner> UnmodifiedFirst with (a, b):
        (Resident { .. }, Dirty { .. }) => Ordering::Greater,
        (Dirty { .. }, Resident { .. }) => Ordering::Less,

        (Resident { .. }, Resident { .. }) |
        (Dirty { .. }, Dirty { .. }) => inner.cmp(a, b),
    }

    /// Prefers unmodified entries over modified entries, with entry age being
    /// used as a tiebreaker (younger entries are picked over older ones).
    pub static UNMODIFIED_THEN_YOUNGEST: dyn EvectionPolicy =
        UnmodifiedFirst<YOUNGEST>(YOUNGEST);

    /// Prefers unmodified entries over modified entries, with entry age being
    /// used as a tiebreaker (older entries are picked over younger ones).
    pub static UNMODIFIED_THEN_OLDEST: dyn EvectionPolicy =
        UnmodifiedFirst<Oldest>(Oldest);

    /// Prefers unmodified entries over modified entries, with last entry access
    /// being used as a tiebreaker (more recently accessed entries are
    /// preferred).
    pub static UNMODIFIED_THEN_MOST_RECENTLY_ACCESSED: dyn EvictionPolicy =
        UnmodifiedFirst<MostRecentlyAccessed>(MostRecentlyAccessed);

    /// Prefers unmodified entries over modified entries, with last entry access
    /// being used as a tiebreaker (less recently accessed entries are
    /// preferred).
    pub static UNMODIFIED_THEN_LEAST_RECENTLY_ACCESSED: dyn EvictionPolicy =
        UnmodifiedFirst<LeastRecentlyAccessed>(LeastRecentlyAccessed);

    policy! { <Inner as inner> ModifiedFirst with (a, b):
        (Dirty { .. }, Resident { .. }) => Ordering::Greater,
        (Resident { .. }, Dirty { .. }) => Ordering::Less,

        (Resident { .. }, Resident { .. }) |
        (Dirty { .. }, Dirty { .. }) => inner.cmp(a, b),
    }

    /// Prefers modified entries over unmodified entries, with entry age being
    /// used as a tiebreaker (younger entries are picked over older ones).
    pub static MODIFIED_THEN_YOUNGEST: dyn EvectionPolicy =
        ModifiedFirst<YOUNGEST>(YOUNGEST);

    /// Prefers modified entries over unmodified entries, with entry age being
    /// used as a tiebreaker (older entries are picked over younger ones).
    pub static MODIFIED_THEN_OLDEST: dyn EvectionPolicy =
        ModifiedFirst<Oldest>(Oldest);

    /// Prefers modified entries over unmodified entries, with last entry access
    /// being used as a tiebreaker (more recently accessed entries are
    /// preferred).
    pub static MODIFIED_THEN_MOST_RECENTLY_ACCESSED: dyn EvictionPolicy =
        ModifiedFirst<MostRecentlyAccessed>(MostRecentlyAccessed);

    /// Prefers modified entries over unmodified entries, with last entry access
    /// being used as a tiebreaker (less recently accessed entries are
    /// preferred).
    pub static MODIFIED_THEN_LEAST_RECENTLY_ACCESSED: dyn EvictionPolicy =
        ModifiedFirst<LeastRecentlyAccessed>(LeastRecentlyAccessed);

}

