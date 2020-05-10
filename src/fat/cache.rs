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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
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

    pub fn get(&self, s: SectorIdx) -> Option<&CacheEntry> {
        let entry = CacheEntry::new_for_lookup(s);
        self.cache_entry_table
            .as_slice()
            .binary_search(&entry)
            .ok()
            .map(|idx| &self.cache_entry_table.as_slice()[idx])
    }

    pub fn get_mut(&mut self, s: SectorIdx) -> Option<&mut CacheEntry> {
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
    pub fn insert(&mut self,
        s: Sector,
        idx: usize,
        counter: &u64,
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
    pub fn remove(
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
    pub fn for_each_dirty_entry<E, F: FnMut((usize, &mut CacheEntry)) -> Result<(), E>>(
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

