
use crate::Storage;
use super::FatFs;
use super::types::{ClusterIdx, SectorIdx};
use super::cache::EvictionPolicy;

use generic_array::{ArrayLength, GenericArray};
use typenum::consts::U512;

use core::cell::RefCell;
use core::convert::TryInto;
use core::iter::Iterator;
use core::ops::Range;

// Another TODO: relax the 512B sector size restriction in this file.

#[repr(transparent)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FatEntry {
    pub next: ClusterIdx,
}

impl FatEntry {
    pub const fn from(next: ClusterIdx) -> Self {
        Self { next }
    }

    pub fn trace<'f, 's, S, CS, Ev>(
        &self,
        fs: &'f mut FatFs<S, CS, Ev>,
        storage: &'s mut S,
    ) -> FatEntryTracer<'f, 's, S, CS, Ev>
    where
        S: Storage<Word = u8, SECTOR_SIZE = U512>,
        CS: ArrayLength<RefCell<GenericArray<u8, U512>>>,
        CS: ArrayLength<super::cache::CacheEntry>,
        CS: crate::util::BitMapLen,
        Ev: EvictionPolicy,
    {
        FatEntryTracer::starting_at(fs, storage, self.next)
    }

    pub fn upgrade_from_tracer<'fet, 'f, S, CS, Ev>(
        &'fet self,
        fet: &'f mut FatEntryTracer<'f, 'f, S, CS, Ev>,
    ) -> FatEntryWrapper<'fet, 'f, 'f, S, CS, Ev>
    where
        S: Storage<Word = u8, SECTOR_SIZE = U512>,
        CS: ArrayLength<RefCell<GenericArray<u8, U512>>>,
        CS: ArrayLength<super::cache::CacheEntry>,
        CS: crate::util::BitMapLen,
        Ev: EvictionPolicy,
    {
        self.upgrade(fet.file_sys, fet.storage)
    }

    pub fn upgrade<'fet, 'f, 's, S, CS, Ev>(
        &'fet self,
        fs: &'f mut FatFs<S, CS, Ev>,
        storage: &'s mut S,
    ) -> FatEntryWrapper<'fet, 'f, 's, S, CS, Ev>
    where
        S: Storage<Word = u8, SECTOR_SIZE = U512>,
        CS: ArrayLength<RefCell<GenericArray<u8, U512>>>,
        CS: ArrayLength<super::cache::CacheEntry>,
        CS: crate::util::BitMapLen,
        Ev: EvictionPolicy,
    {
        FatEntryWrapper::from(self, fs, storage)
    }
}

impl FatEntry {
    pub const FREE: FatEntry = FatEntry::from(ClusterIdx::new(0x0000_0000));
    pub const END_OF_CHAIN: FatEntry = FatEntry::from(ClusterIdx::new(0xFFFF_FFF8));
}

pub struct FatEntryWrapper<'fet, 'f, 's, S, CS, Ev>
where
    S: Storage<Word = u8, SECTOR_SIZE = U512>,
    CS: ArrayLength<RefCell<GenericArray<u8, U512>>>,
    CS: ArrayLength<super::cache::CacheEntry>,
    CS: crate::util::BitMapLen,
    Ev: EvictionPolicy,
{
    inner: &'fet FatEntry,
    fs: &'f mut FatFs<S, CS, Ev>,
    storage: &'s mut S,
}

impl<'fet, 'f, 's, S, CS, Ev> FatEntryWrapper<'fet, 'f, 's, S, CS, Ev>
where
    S: Storage<Word = u8, SECTOR_SIZE = U512>,
    CS: ArrayLength<RefCell<GenericArray<u8, U512>>>,
    CS: ArrayLength<super::cache::CacheEntry>,
    CS: crate::util::BitMapLen,
    Ev: EvictionPolicy,
{
    pub fn from(
        inner: &'fet FatEntry,
        fs: &'f mut FatFs<S, CS, Ev>,
        storage: &'s mut S,
    ) -> Self {
        Self { inner, fs, storage }
    }

    pub fn range(&self) -> Range<SectorIdx> {
        self.fs.cluster_to_sector_range(self.inner.next)
    }

    fn cluster_size_in_bytes(&self) -> u32 {
        (self.fs.cluster_size_in_sectors as u32) *
        (self.fs.sector_size_in_bytes as u32)
    }

    fn range_chk(&self, offset: u32, len: usize) -> Result<(), ()> {
        let max_offset = offset.checked_add(len.try_into().unwrap()).unwrap();

        if max_offset >= self.cluster_size_in_bytes() {
            Err(())
        } else {
            Ok(())
        }
    }

    // offset into this cluster
    //
    // users of this should constrain buf to the file's end?
    pub fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), ()> {
        self.range_chk(offset, buf.len())?;

        let (sector_idx, offset) = self.fs.cluster_to_sector(self.inner.next, offset);

        // Since this is within a cluster, the sectors are back to back and
        // we can just call fs.read once.
        self.fs.read(self.storage, sector_idx, offset, buf)
    }

    // offset into this cluster
    //
    // users of this should constrain buf to the file's end? or grow the file?
    pub fn write(&mut self, offset: u32, data: impl Iterator<Item = u8>) -> Result<(), ()> {
        self.range_chk(offset, data.size_hint().0)?;

        let (sector_idx, offset) = self.fs.cluster_to_sector(self.inner.next, offset);

        // Since this is within a cluster, the sectors are back to back and
        // we can just call fs.write once.
        self.fs.write_iter(self.storage, sector_idx, offset, data)
    }
}

#[derive(Debug)]
pub struct FatEntryTracer<'f, 's, S, CS, Ev>
where
    S: Storage<Word = u8, SECTOR_SIZE = U512>,
    CS: ArrayLength<RefCell<GenericArray<u8, U512>>>,
    CS: ArrayLength<super::cache::CacheEntry>,
    CS: crate::util::BitMapLen,
    Ev: EvictionPolicy,
{
    pub file_sys: &'f mut FatFs<S, CS, Ev>,
    pub storage: &'s mut S,

    pub current_cluster_idx: Option<ClusterIdx>,
    hit_end: Option<ClusterIdx>,
}

impl<'f, 's, S, CS, Ev> FatEntryTracer<'f, 's, S, CS, Ev>
where
    S: Storage<Word = u8, SECTOR_SIZE = U512>,
    CS: ArrayLength<RefCell<GenericArray<u8, U512>>>,
    CS: ArrayLength<super::cache::CacheEntry>,
    CS: crate::util::BitMapLen,
    Ev: EvictionPolicy,
{
    pub fn root(
        fs: &'f mut FatFs<S, CS, Ev>,
        storage: &'s mut S
    ) -> Self {
        Self::starting_at(fs, storage, fs.root_dir_cluster_num)
    }

    pub fn starting_at(
        fs: &'f mut FatFs<S, CS, Ev>,
        storage: &'s mut S,
        cluster_idx: ClusterIdx
    ) -> Self {
        let (sector, _offset) = fs.cluster_to_table_pos(cluster_idx);

        Self {
            file_sys: fs,
            storage,

            current_cluster_idx: Some(cluster_idx),
            hit_end: None,
        }
    }

    pub fn capacity(mut self) -> usize {
        let cluster_size_in_bytes =
            (self.file_sys.cluster_size_in_sectors as usize) *
            (self.file_sys.sector_size_in_bytes as usize);

        self.count() * cluster_size_in_bytes
    }

    /// Only works when the iterator has run out; returns `Err` otherwise.
    pub fn grow_file(&mut self) -> Result<(), ()> {
        if let Some(last_cluster) = self.hit_end.take() {
            let given = self.file_sys.next_free_cluster(self.storage).unwrap();

            let (sector, offset) = self.file_sys.cluster_to_table_pos(
                last_cluster,
            );

            // Make the last cluster point to the new cluster:
            let bytes = given.to_le_bytes();

            self.file_sys.write(self.storage, sector, offset, &bytes).unwrap();

            // Make it so the iterator can be resumed:
            self.current_cluster_idx = Some(given);

            Ok(())
        } else {
            Err(())
        }
    }
}

impl<'f, 's, S, CS, Ev> Iterator for /*&mut */FatEntryTracer<'f, 's, S, CS, Ev>
where
    S: Storage<Word = u8, SECTOR_SIZE = U512>,
    CS: ArrayLength<RefCell<GenericArray<u8, U512>>>,
    CS: ArrayLength<super::cache::CacheEntry>,
    CS: crate::util::BitMapLen,
    Ev: EvictionPolicy,
{
    type Item = FatEntry;

    fn next(&mut self) -> Option<FatEntry> {
        if let Some(idx) = self.current_cluster_idx {
            let (sector, offset) = self.file_sys.cluster_to_table_pos(
                idx
            );

            // Get the next cluster index:
            let mut buf = [0u8; 4];
            self.file_sys.read(self.storage, sector, offset, &mut buf).unwrap();

            let next: ClusterIdx = ClusterIdx::new(u32::from_le_bytes(buf));
            let fat_entry = FatEntry::from(next);

            if fat_entry == FatEntry::END_OF_CHAIN {
                self.current_cluster_idx = None;
                self.hit_end = Some(idx);
            } else {
                self.current_cluster_idx = Some(next);
            }

            Some(FatEntry::from(idx))
        } else {
            None
        }
    }
}


// impl<'fet, 'f, 's, 'a, S: Storage<Word = u8, SECTOR_SIZE = U512>> Iterator for &'fet mut FatEntryTracer<'f, 's, 'a, S> {
//     type Item = (Cluster, FatEntryWrapper<'fet, 'f, 's, 'a, S>);

//     fn next(&mut self) -> Option<(Cluster, FatEntryWrapper<'fet, 'f, 's, 'a, S>)> {
//         if let Some(idx) = self.current_cluster_idx {
//             let (sector, offset) = cluster_idx_to_fat_sector_and_offset(
//                 self.file_sys.fat_starting_sector,
//                 self.file_sys.sector_size_in_bytes,
//                 idx
//             );

//             if sector != self.current_cached_sector_idx {
//                 self.storage.read_sector(sector as usize, self.sector).unwrap();

//                 self.current_cached_sector_idx = sector;
//             }

//             let next: Cluster = Cluster::from_le_bytes(self.sector[(offset as usize)..(offset as usize)+4].try_into().unwrap());
//             let fat_entry = FatEntry::from(next);

//             if fat_entry == FatEntry::END_OF_CHAIN {
//                 self.current_cluster_idx = None;
//             } else {
//                 self.current_cluster_idx = Some(next);
//             }

//             let fat_entry_wrapper = FatEntryWrapper::<'fet, 'f, 's, 'a, S>::from(next, self);

//             Some((idx, fat_entry_wrapper))
//         } else {
//             None
//         }
//     }
// }
