//! Home of the `SectorCache` type; that which all writes and reads to `Storage`
//! flow through.

use super::types::SectorIdx;
use crate::util::{BitMap, BitMapLen};

use storage_traits::Storage;
use generic_array::{ArrayLength, GenericArray};

use core::cell::{Cell, RefCell, RefMut, Ref};
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
        (Dirty { .. }, Dirty { .. }) => inner.compare(a, b),
    }

    /// Prefers unmodified entries over modified entries, with entry age being
    /// used as a tiebreaker (younger entries are picked over older ones).
    pub static UNMODIFIED_THEN_YOUNGEST: DynEvictionPolicy =
        &UnmodifiedFirst::<Youngest>(Youngest);

    /// Prefers unmodified entries over modified entries, with entry age being
    /// used as a tiebreaker (older entries are picked over younger ones).
    pub static UNMODIFIED_THEN_OLDEST: DynEvictionPolicy =
        &UnmodifiedFirst::<Oldest>(Oldest);

    /// Prefers unmodified entries over modified entries, with last entry access
    /// being used as a tiebreaker (more recently accessed entries are
    /// preferred).
    pub static UNMODIFIED_THEN_MOST_RECENTLY_ACCESSED: DynEvictionPolicy =
        &UnmodifiedFirst::<MostRecentlyAccessed>(MostRecentlyAccessed);

    /// Prefers unmodified entries over modified entries, with last entry access
    /// being used as a tiebreaker (less recently accessed entries are
    /// preferred).
    pub static UNMODIFIED_THEN_LEAST_RECENTLY_ACCESSED: DynEvictionPolicy =
        &UnmodifiedFirst::<LeastRecentlyAccessed>(LeastRecentlyAccessed);

    policy! { <Inner as inner> ModifiedFirst with (a, b):
        (Dirty { .. }, Resident { .. }) => Ordering::Greater,
        (Resident { .. }, Dirty { .. }) => Ordering::Less,

        (Resident { .. }, Resident { .. }) |
        (Dirty { .. }, Dirty { .. }) => inner.compare(a, b),
    }

    /// Prefers modified entries over unmodified entries, with entry age being
    /// used as a tiebreaker (younger entries are picked over older ones).
    pub static MODIFIED_THEN_YOUNGEST: DynEvictionPolicy =
        &ModifiedFirst::<Youngest>(Youngest);

    /// Prefers modified entries over unmodified entries, with entry age being
    /// used as a tiebreaker (older entries are picked over younger ones).
    pub static MODIFIED_THEN_OLDEST: DynEvictionPolicy =
        &ModifiedFirst::<Oldest>(Oldest);

    /// Prefers modified entries over unmodified entries, with last entry access
    /// being used as a tiebreaker (more recently accessed entries are
    /// preferred).
    pub static MODIFIED_THEN_MOST_RECENTLY_ACCESSED: DynEvictionPolicy =
        &ModifiedFirst::<MostRecentlyAccessed>(MostRecentlyAccessed);

    /// Prefers modified entries over unmodified entries, with last entry access
    /// being used as a tiebreaker (less recently accessed entries are
    /// preferred).
    pub static MODIFIED_THEN_LEAST_RECENTLY_ACCESSED: DynEvictionPolicy =
        &ModifiedFirst::<LeastRecentlyAccessed>(LeastRecentlyAccessed);

}

#[allow(non_camel_case_types)]
pub struct SectorCache<StorageImpl, SECTOR_SIZE, CACHE_SIZE_IN_SECTORS, Eviction = DynEvictionPolicy>
where
    StorageImpl: Storage<Word = u8, SECTOR_SIZE = SECTOR_SIZE>,
    SECTOR_SIZE: ArrayLength<u8>,
    CACHE_SIZE_IN_SECTORS: ArrayLength<RefCell<GenericArray<u8, SECTOR_SIZE>>>,
    CACHE_SIZE_IN_SECTORS: ArrayLength<CacheEntry>,
    CACHE_SIZE_IN_SECTORS: BitMapLen,
    Eviction: EvictionPolicy,
{
    cached_sectors: GenericArray<RefCell<GenericArray<u8, SECTOR_SIZE>>, CACHE_SIZE_IN_SECTORS>,
    cache_table: CacheTable<CACHE_SIZE_IN_SECTORS>,
    cache_bitmap: BitMap<CACHE_SIZE_IN_SECTORS>,

    max_sector_idx: SectorIdx,

    eviction_policy: Eviction,
    counter: RefCell<u64>,

    _s: PhantomData<StorageImpl>,
}

#[allow(non_camel_case_types)]
impl<S, SECT_SIZE, CACHE_SIZE, Ev> SectorCache<S, SECT_SIZE, CACHE_SIZE, Ev>
where
    S: Storage<Word = u8, SECTOR_SIZE = SECT_SIZE>,
    SECT_SIZE: ArrayLength<u8>,
    CACHE_SIZE: ArrayLength<RefCell<GenericArray<u8, SECT_SIZE>>>,
    CACHE_SIZE: ArrayLength<CacheEntry>,
    CACHE_SIZE: BitMapLen,
    Ev: EvictionPolicy,
{
    pub /*const*/ fn cache_size_in_bytes() -> usize {
        SECT_SIZE::to_usize() * CACHE_SIZE::to_usize()
    }

    pub fn new(_witness: &S, max_sector_idx: SectorIdx, ev: Ev) -> Self {
        Self {
            cached_sectors: Default::default(),
            cache_table: CacheTable::new(),
            cache_bitmap: BitMap::new(),

            max_sector_idx,

            eviction_policy: ev,
            counter: RefCell::new(0),

            _s: PhantomData,
        }
    }

    /// Returns `Err` if there are no entries there to evict.
    /*pub */fn evict_entry(&mut self, storage: &mut S) -> Result<(), ()> {
        if self.cache_table.len() == 0 { return Err(()); }

        let entry = self.eviction_policy.pick_entry_to_evict(
                &mut self.cache_table.cache_entry_table)
            .expect("must give an entry to evict when the cache table is not \
                empty");

        let sector_idx = entry.get_sector_idx().expect("dirty entries have a sector index");
        let arr_idx = entry.get_arr_idx().expect("dirty entries have an arr index");

        // Check if the entry we're to remove is dirty:
        if entry.is_dirty() {
            // If it is, write it out:
            storage.write_sector(
                sector_idx.idx(),
                // We do a mutable borrow here even though we don't _need_ to
                // because we want to make sure that no one else has a reference
                // to this sector that's being evicted. While we don't remove
                // the sector or overwrite it here (which is why we don't need
                // a mutable reference) we're presumably about to.
                &self.cached_sectors[arr_idx].try_borrow_mut().expect("no references to a sector we're about to evict"),
            ).unwrap();

            // And mark it as clean:
            entry.mark_as_clean().unwrap();
        }

        // And finally, remove it from the table and the bitmap:
        self.cache_table.remove(sector_idx).expect("to be able to remove clean entries");
        self.cache_bitmap.set(sector_idx.idx(), false).unwrap();

        Ok(())
    }

    // Since storage has to be passed into us, unfortunately we can't do this
    // on Drop...
    pub fn flush(&mut self, storage: &mut S) -> Result<(), ()> {
        let ref cached_sectors = self.cached_sectors;

        self.cache_table.for_each_dirty_entry(|(idx, e)| {
            storage.write_sector(
                e.get_sector_idx().expect("dirty entries have a sector index").idx(),
                // We don't actually need a mutable borrow here but, as the
                // message below explains, we should always get it and it's a
                // good sanity test.
                &cached_sectors[idx].try_borrow_mut().expect("no references to any sectors when we have a mutable reference to the sector cache"),
            ).unwrap();

            e.mark_as_clean()
        })
    }

    pub fn upgrade<'s>(
        &'s mut self,
        storage: &'s mut S
    ) -> SectorCacheWithStorage<'s, S, SECT_SIZE, CACHE_SIZE, Ev, UnIndexable> {
        // TODO: should we enable flush on Drop here?

        SectorCacheWithStorage::new(self, storage)
    }

    pub fn get_sector_entry(
        &mut self,
        storage: &mut S,
        index: SectorIdx,
    ) -> &CacheEntry {
        // See if we've already got this sector in the cache:
        if let Some(c) = self.cache_table.get(index) {
            c
        } else {
            // If we don't, try to load it into the cache.

            // First, let's get the index where we can place the sector:
            // let idx = match self.cache_bitmap.next_empty_bit() {
            //     Ok(idx) => idx,
            //     Err(()) => {
                    // If the cache is full, we need to evict a sector.
                    self.evict_entry(storage)/*.expect("eviction to succeed")*/;

                    // Now, we can try to get an index again; this time it
                    // _must_ succeed:
            //         self.cache_bitmap.next_empty_bit().expect("an empty sector after eviction")
            //     },
            // };

            unreachable!()
            // // Load the sector in:
            // // (it's a little silly that we go lookup the index to this sector
            // // again but it's worth it for maintaining the symmetry)
            // storage.read_sector(
            //     index.idx(),
            //     &mut self.cached_sectors[idx].try_borrow_mut().expect("clean entries to have no references")
            // ).unwrap();

            // // Add to the cache table and the bitmap:
            // self.cache_bitmap.set(idx, true).unwrap();
            // match self.cache_table
            //         .insert(index, idx, &mut self.counter.borrow_mut()) {
            //     Ok(entry) => entry,

            //     // It's not possible that we're out of space; the cache bitmap
            //     // gave us an index.
            //     Err(None) => unreachable!(),

            //     // It's not possible that this sector is already cached; we
            //     // started by looking it up.
            //     Err(Some(_)) => unreachable!(),
            // }
        }

        // // See if we've already got this sector in the cache:
        // if let Some(_) = self.cache_table.get(index) {
        //     // return c; // Unfortunately the borrow checker is not smart enough
        //                  // to see that this arm is mutually exclusive from the
        //                  // other arm because of the return.
        // } else {
        //     // If we don't, try to load it into the cache.

        //     // First, let's get the index where we can place the sector:
        //     let idx = match self.cache_bitmap.next_empty_bit() {
        //         Ok(idx) => idx,
        //         Err(()) => {
        //             // If the cache is full, we need to evict a sector.
        //             self.evict_entry(storage).expect("eviction to succeed");

        //             // Now, we can try to get an index again; this time it
        //             // _must_ succeed:
        //             self.cache_bitmap.next_empty_bit().expect("an empty sector after eviction")
        //         },
        //     };

        //     // Load the sector in:
        //     // (it's a little silly that we go lookup the index to this sector
        //     // again but it's worth it for maintaining the symmetry)
        //     storage.read_sector(
        //         index.idx(),
        //         &mut self.cached_sectors[idx].try_borrow_mut().expect("clean entries to have no references")
        //     ).unwrap();

        //     // Add to the cache table and the bitmap:
        //     self.cache_bitmap.set(idx, true).unwrap();
        //     match self.cache_table
        //             .insert(index, idx, &mut self.counter.borrow_mut()) {
        //         Ok(entry) => /*entry*/ {},

        //         // It's not possible that we're out of space; the cache bitmap
        //         // gave us an index.
        //         Err(None) => unreachable!(),

        //         // It's not possible that this sector is already cached; we
        //         // started by looking it up.
        //         Err(Some(_)) => unreachable!(),
        //     }
        // }

        // self.cache_table.get(index).unwrap()
    }
}

#[allow(non_camel_case_types)]
impl<S, SECT_SIZE, CACHE_SIZE> SectorCache<S, SECT_SIZE, CACHE_SIZE, DynEvictionPolicy>
where
    S: Storage<Word = u8, SECTOR_SIZE = SECT_SIZE>,
    SECT_SIZE: ArrayLength<u8>,
    CACHE_SIZE: ArrayLength<RefCell<GenericArray<u8, SECT_SIZE>>>,
    CACHE_SIZE: ArrayLength<CacheEntry>,
    CACHE_SIZE: BitMapLen,
{
    pub fn change_eviction_policy(&mut self, ev: DynEvictionPolicy) {
        self.eviction_policy = ev
    }
}

#[allow(non_camel_case_types)]
impl<S, SECT_SIZE, CACHE_SIZE, Ev> Drop for SectorCache<S, SECT_SIZE, CACHE_SIZE, Ev>
where
    S: Storage<Word = u8, SECTOR_SIZE = SECT_SIZE>,
    SECT_SIZE: ArrayLength<u8>,
    CACHE_SIZE: ArrayLength<RefCell<GenericArray<u8, SECT_SIZE>>>,
    CACHE_SIZE: ArrayLength<CacheEntry>,
    CACHE_SIZE: BitMapLen,
    Ev: EvictionPolicy,
{
    fn drop(&mut self) {
        let mut i = 0;

        self.cache_table.for_each_dirty_entry::<(), _>(|_| {
            i += 1;

            Ok(())
        }).unwrap();

        if i != 0 {
            panic!("A SectorCache was dropped with dirty entries ({} of them)!", i);
        }
    }
}

pub struct UnIndexable;
pub struct Indexable;

#[allow(non_camel_case_types)]
pub struct SectorCacheWithStorage<'s, StorageImpl, SECTOR_SIZE, CACHE_SIZE_IN_SECTORS, Eviction, Ty = UnIndexable>
where
    StorageImpl: Storage<Word = u8, SECTOR_SIZE = SECTOR_SIZE>,
    SECTOR_SIZE: ArrayLength<u8>,
    CACHE_SIZE_IN_SECTORS: ArrayLength<RefCell<GenericArray<u8, SECTOR_SIZE>>>,
    CACHE_SIZE_IN_SECTORS: ArrayLength<CacheEntry>,
    CACHE_SIZE_IN_SECTORS: BitMapLen,
    Eviction: EvictionPolicy,
{
    sector_cache: Cell<Option<&'s mut SectorCache<StorageImpl, SECTOR_SIZE, CACHE_SIZE_IN_SECTORS, Eviction>>>,
    storage: Cell<Option<&'s mut StorageImpl>>,

    flush_on_drop: bool,

    _ty: PhantomData<Ty>,
}

#[allow(non_camel_case_types)]
impl<'s, S, SS, CS, Ev, Ty> SectorCacheWithStorage<'s, S, SS, CS, Ev, Ty>
where
    S: Storage<Word = u8, SECTOR_SIZE = SS>,
    SS: ArrayLength<u8>,
    CS: ArrayLength<RefCell<GenericArray<u8, SS>>>,
    CS: ArrayLength<CacheEntry>,
    CS: BitMapLen,
    Ev: EvictionPolicy,
{
    pub fn flush_on_drop(&mut self, enable: bool) {
        self.flush_on_drop = enable
    }

    fn refs<R, F: FnOnce(&'s mut SectorCache<S, SS, CS, Ev>, &'s mut S) -> R>(&self, func: F) -> R {
        let (mut sector_cache_ref, mut storage_ref) = (
            self.sector_cache.take().unwrap(),
            self.storage.take().unwrap(),
        );

        let res = func(sector_cache_ref, storage_ref);

        self.sector_cache.set(Some(sector_cache_ref));
        self.storage.set(Some(storage_ref));

        res
    }

    /// Note: this will panic if, in order to load the requested sector, we end
    /// up needing to evict a sector that has a borrow currently out.
    pub fn get<'r>(&'r self, index: SectorIdx) -> Ref<'r, GenericArray<u8, SS>> {
        let arr_idx = self.get_inner(index);

        self.refs(|sector_cache, _| {
            sector_cache.cached_sectors[arr_idx]
                .try_borrow()
                .expect("immutable sector borrows always succeed")
        })
    }

    // Note: this will panic if, in order to load the requested sector, we end
    // up needing to evict a sector that has a borrow currently out.
    fn get_inner<'r>(&'r self, index: SectorIdx) -> usize {
        self.refs(|mut sector_cache, mut storage| {
            let mut counter = sector_cache.counter.borrow();
            let cache_entry = sector_cache.get_sector_entry(&mut storage, index);

            // Mark the entry as accessed.
            cache_entry
                .accessed(&mut counter)
                .expect("entry isn't `Free`");

            // Finally, get the entry's corresponding sector cache array:
            cache_entry
                .get_arr_idx()
                .expect("entry has an arr index")
        })
    }

    pub fn get_mut(&mut self, index: SectorIdx) -> &mut GenericArray<u8, SS> {
        todo!()
    }
}

#[allow(non_camel_case_types)]
impl<'s, S, SS, CS, Ev> SectorCacheWithStorage<'s, S, SS, CS, Ev, UnIndexable>
where
    S: Storage<Word = u8, SECTOR_SIZE = SS>,
    SS: ArrayLength<u8>,
    CS: ArrayLength<RefCell<GenericArray<u8, SS>>>,
    CS: ArrayLength<CacheEntry>,
    CS: BitMapLen,
    Ev: EvictionPolicy,
{
    pub fn new(sc: &'s mut SectorCache<S, SS, CS, Ev>, stor: &'s mut S) -> Self {
        Self {
            sector_cache: Cell::new(Some(sc)),
            storage: Cell::new(Some(stor)),

            flush_on_drop: false,

            _ty: PhantomData,
        }
    }

    /// It turns out we cannot safely implement Index (and therefore IndexMut)
    /// for this type,
    ///
    /// The issue is that in Index we really mutate the underlying sector cache
    /// (when evicting sectors) but we also give out references to specific
    /// sectors _within_ the cache. This is a problem because it means it's
    /// possible for us to evict a sector that someone already has a reference
    /// to!
    ///
    /// If we want to support having multiple borrows of sectors out at once
    /// from this type (this isn't supported for mutable borrows of sectors
    /// which is why IndexMut doesn't have this problem) we need to turn to some
    /// kind of reference counting (I'm reasonably sure we can't reason about
    /// this at compile time since cache eviction is dynamic and all).
    ///
    /// We can have each borrow be given a type like `Ref` that maintains a
    /// reference count variable for the sector that is being borrowed. On Drop,
    /// it decrements the reference count (which means that it needs a reference
    /// to the instance of this type). If this sounds a lot like RefCell, that's
    /// because it is basically RefCell.
    ///
    /// Rather than reinvent the wheel we can just wrap our sectors in RefCells
    /// and give out leases to those for `get` and `get_mut`. This then becomes
    /// safe (and possible to write in safe Rust!).
    ///
    /// For Index, because we _must_ return references to things we cannot do
    /// this (i.e. we can't return a concrete type with a Drop impl and we
    /// can't impl Drop on a reference — even for a type we own). Even so, we
    /// offer an Index and IndexMut impl for a variant of this type. You have
    /// to use this function to make said variant of the type and this function
    /// is unsafe because you have to promise that you'll make sure you don't
    /// hold onto sectors that get evicted.
    pub unsafe fn make_indexable(self) -> SectorCacheWithStorage<'s, S, SS, CS, Ev, Indexable> {
        // let flush_on_drop = self.flush_on_drop;
        // self.flush_on_drop = false;

        // SectorCacheWithStorage {
        //     sector_cache: self.sector_cache.clone(),
        //     storage: self.storage.clone(),
        //     flush_on_drop,
        //     _ty: PhantomData
        // }

        // I think this is safe..
        #[allow(unused_unsafe)]
        unsafe { core::mem::transmute(self) }
    }
}

#[allow(non_camel_case_types)]
impl<'s, S, SS, CS, Ev, Ty> Drop for SectorCacheWithStorage<'s, S, SS, CS, Ev, Ty>
where
    S: Storage<Word = u8, SECTOR_SIZE = SS>,
    SS: ArrayLength<u8>,
    CS: ArrayLength<RefCell<GenericArray<u8, SS>>>,
    CS: ArrayLength<CacheEntry>,
    CS: BitMapLen,
    Ev: EvictionPolicy,
{
    fn drop(&mut self) {
        if self.flush_on_drop {
            self.refs(|sector_cache, storage| {
                sector_cache.flush(storage).unwrap()
            })
        }
    }
}

#[allow(non_camel_case_types)]
impl<'s, S, SECT_SIZE, CACHE_SIZE, Ev> Index<SectorIdx> for SectorCacheWithStorage<'s, S, SECT_SIZE, CACHE_SIZE, Ev, Indexable>
where
    S: Storage<Word = u8, SECTOR_SIZE = SECT_SIZE>,
    SECT_SIZE: ArrayLength<u8>,
    CACHE_SIZE: ArrayLength<RefCell<GenericArray<u8, SECT_SIZE>>>,
    CACHE_SIZE: ArrayLength<CacheEntry>,
    CACHE_SIZE: BitMapLen,
    Ev: EvictionPolicy,
{
    type Output = GenericArray<u8, SECT_SIZE>;

    fn index(&self, index: SectorIdx) -> &GenericArray<u8, SECT_SIZE> {
        // Ideally we'd just call `get` here and use `Ref::leak` but that
        // requires nightly so we end up having to copy over the contents of
        // that function here (get_inner exists so we don't have to copy
        // _everything_).

        let arr_idx = self.get_inner(index);

        self.refs(|sector_cache, _| {
            unsafe {
                sector_cache
                    .cached_sectors[arr_idx]
                    .try_borrow_unguarded() // This is potentially dangerous but the users opted in.
                    .unwrap()
            }
        })
    }
}

#[allow(non_camel_case_types)]
impl<'s, S, SECT_SIZE, CACHE_SIZE, Ev> IndexMut<SectorIdx> for SectorCacheWithStorage<'s, S, SECT_SIZE, CACHE_SIZE, Ev, Indexable>
where
    S: Storage<Word = u8, SECTOR_SIZE = SECT_SIZE>,
    SECT_SIZE: ArrayLength<u8>,
    CACHE_SIZE: ArrayLength<RefCell<GenericArray<u8, SECT_SIZE>>>,
    CACHE_SIZE: ArrayLength<CacheEntry>,
    CACHE_SIZE: BitMapLen,
    Ev: EvictionPolicy,
{
    fn index_mut(&mut self, index: SectorIdx) -> &mut GenericArray<u8, SECT_SIZE> {
        // let (cache_table, storage) = self.refs();

        // See if we've already got this sector in the cache:

        todo!()
    }
}

// TODO: i wonder if it'd be practical to implement Index<Range<Sector>> for
// SectorCache.. the problems (that i can think of immediately) are:
//   - what happens when you ask for more sectors than we can accommodate
//   - what happens when some (or all) of the sectors you ask for are cached
//     but aren't back to back?
//
// Rearranging sectors to make sure we have a contiguous slice also sounds
// pretty painful.. we also don't actually have a contiguous block of memory
// underneath us (we've got a GenericArray of GenericArrays) so I think that
// makes this pretty much impossible without changing the internal to actually
// just use one huge u8 array (length = Mul<SECTOR_SIZE, CACHE_SIZE>).
//
// In any case, the use case for having an actually contiguous array of memory
// that represents a file seems extremely small/niche.
