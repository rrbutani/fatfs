
use crate::Storage;
use super::FatFs;

use generic_array::GenericArray;
use typenum::consts::U512;

use core::iter::Iterator;
use core::ops::Range;
use core::convert::TryInto;

// Another TODO: relax the 512B sector size restriction in this file.

pub type Cluster = u32;
pub type Sector = u64;

#[repr(transparent)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FatEntry {
    pub next: Cluster,
}

impl FatEntry {
    pub const fn from(next: Cluster) -> Self {
        Self { next }
    }

    pub fn trace<'f, 's, 'a, S: Storage<Word = u8, SECTOR_SIZE = U512>>(&self, fs: &'f mut FatFs<S>, storage: &'s mut S, array: &'a mut GenericArray<u8, U512>) -> Result<FatEntryTracer<'f, 's, 'a, S>, ()> {
        FatEntryTracer::starting_at(fs, storage, array, self.next)
    }
}

pub struct FatEntryWrapper<'fet, 'f, 's, 'a, S: Storage<Word = u8, SECTOR_SIZE = U512>> {
    inner: FatEntry,
    handle: &'fet mut FatEntryTracer<'f, 's, 'a, S>
}

impl<'fet, 'f, 's, 'a, S: Storage<Word = u8, SECTOR_SIZE = U512>> FatEntryWrapper<'fet, 'f, 's, 'a, S> {
    pub fn from(next: Cluster, handle: &'fet mut FatEntryTracer<'f, 's, 'a, S>) -> Self {
        Self {
            inner: FatEntry::from(next),
            handle
        }
    }

    pub fn range(&self) -> Range<Sector> {
        cluster_to_sector_range(
            self.handle.file_sys.fat_starting_sector,
            self.handle.file_sys.fat_table_size_in_sectors,
            self.handle.file_sys.cluster_size_in_sectors,
            self.inner.next,
        )
    }

    // offset into this cluster
    //
    // users of this should constrain buf to the file's end?
    pub fn read(&mut self, offset: u16, buf: &mut [u8]) -> Result<usize, ()> {
        let bytes_in_sector = self.handle.file_sys.sector_size_in_bytes as u32;
        let range = dbg!(self.range());
        let starting_sector = ((offset as u32) / bytes_in_sector) as u64 + range.start;

        // Only necessary to give Err(()) instead of Ok(0)...
        if starting_sector >= range.end {
            return Err(());
        }

        // Really leaning on the optimizer here..
        for (idx, b) in buf.iter_mut().enumerate() {
            let (sector, offset) = {
                let full_offset: u32 = (offset as u32) + (idx as u32);

                (
                    range.start + (full_offset / bytes_in_sector) as u64,
                    full_offset % bytes_in_sector,
                )
            };

            if dbg!(sector) >= range.end {
                return Ok(idx);
            }

            if sector != self.handle.current_cached_sector_idx {
                self.handle.storage.read_sector(sector as usize, self.handle.sector).unwrap();
                self.handle.current_cached_sector_idx = sector;
            }

            *b = self.handle.sector[offset as usize];
        }

        Ok(buf.len())
    }

    // offset into this cluster
    //
    // users of this should constrain buf to the file's end? or grow the file?
    pub fn write(&mut self, offset: u16, buf: &[u8]) -> Result<usize, ()> {
        let bytes_in_sector = self.handle.file_sys.sector_size_in_bytes as u32;
        let range = self.range();
        let starting_sector = ((offset as u32) / bytes_in_sector) as u64 + range.start;

        // Only necessary to return Err(()) instead of Ok(())...
        if starting_sector >= range.end {
            return Err(())
        }

        let mut stale = true;

        // Save us, rustc!
        for (idx, b) in buf.iter().enumerate() {
            let (sector, offset) = {
                let full_offset: u32 = (offset as u32) + (idx as u32);

                (
                    range.start + (full_offset / bytes_in_sector) as u64,
                    full_offset & bytes_in_sector,
                )
            };

            if sector >= range.end {
                if !stale {
                    self.handle.storage.write_sector(sector as usize, self.handle.sector).unwrap();
                }

                return Ok(idx);
            }

            if sector != self.handle.current_cached_sector_idx {
                if !stale {
                    self.handle.storage.write_sector(self.handle.current_cached_sector_idx as usize, self.handle.sector).unwrap();
                }

                self.handle.current_cached_sector_idx = sector;
                self.handle.storage.read_sector(sector as usize, self.handle.sector).unwrap();
                stale = false;
            }

            self.handle.sector[offset as usize] = *b;
        }

        if !stale {
            self.handle.storage.write_sector(self.handle.current_cached_sector_idx as usize, self.handle.sector).unwrap();
        }

        Ok(buf.len())
    }
}

impl FatEntry {
    pub const FREE: FatEntry = FatEntry::from(0x0000_0000);
    pub const END_OF_CHAIN: FatEntry = FatEntry::from(0xFFFF_FFF8);
}

#[derive(Debug)]
pub struct FatEntryTracer<'f, 's, 'a, S: Storage<Word = u8, SECTOR_SIZE = U512>> {
    pub file_sys: &'f mut FatFs<S>,
    pub storage: &'s mut S,

    pub current_cluster_idx: Option<Cluster>,
    hit_end: Option<Cluster>,

    pub current_cached_sector_idx: u64, // TODO: consistency in LBA repr

    pub sector: &'a mut GenericArray<u8, U512>,
}

pub fn cluster_idx_to_fat_sector_and_offset(
    fat_starting_sector: u32,
    sector_size_in_bytes: u16,
    cluster_idx: Cluster
) -> (Sector, u16) {
    let cluster_entries_per_fat_sector: u32 = (sector_size_in_bytes as u32) / 4;

    dbg!(((fat_starting_sector + (cluster_idx / cluster_entries_per_fat_sector)) as u64,
        ((cluster_idx % (cluster_entries_per_fat_sector as u32)) as u16) * 4))
}

pub fn cluster_to_sector_range(
    fat_starting_sector: u32,
    fat_table_size_in_sectors: u32,
    cluster_size_in_sectors: u8,
    cluster_idx: Cluster
) -> Range<Sector> {
    let start = fat_starting_sector + fat_table_size_in_sectors;
    let start = start as u64 + (cluster_size_in_sectors as u64 * cluster_idx as u64);

    start..(start + (cluster_size_in_sectors as u64))
}

impl<'f, 's, 'a, S: Storage<Word = u8, SECTOR_SIZE = U512>> FatEntryTracer<'f, 's, 'a, S> {
    pub fn root(fs: &'f mut FatFs<S>, storage: &'s mut S, array: &'a mut GenericArray<u8, U512>) -> Result<Self, ()> {
        Self::starting_at(fs, storage, array, fs.root_dir_cluster_num)
    }

    pub fn starting_at(fs: &'f mut FatFs<S>, storage: &'s mut S, array: &'a mut GenericArray<u8, U512>, cluster_idx: u32) -> Result<Self, ()> {
        let (sector, _offset) = cluster_idx_to_fat_sector_and_offset(
            fs.fat_starting_sector,
            fs.sector_size_in_bytes,
            cluster_idx,
        );

        storage.read_sector(dbg!(sector as usize), array).map_err(|_| ())?;

        Ok(Self {
            file_sys: fs,
            storage,

            current_cluster_idx: Some(cluster_idx),
            hit_end: None,

            current_cached_sector_idx: sector,

            sector: array,
        })
    }

    pub fn capacity(mut self) -> usize {
        let cluster_size_in_bytes = (self.file_sys.cluster_size_in_sectors as usize) * (self.file_sys.sector_size_in_bytes as usize);

        self.count() * cluster_size_in_bytes
    }

    pub fn grow_file(&mut self) -> Result<(), ()> {
        if let Some(last_cluster) = self.hit_end.take() {
            let given = self.file_sys.next_free_cluster(self.storage, self.sector).unwrap();

            let (sector, offset) = cluster_idx_to_fat_sector_and_offset(
                self.file_sys.fat_starting_sector,
                self.file_sys.sector_size_in_bytes,
                last_cluster,
            );

            // self.sector got trashed so we need to re-read it:
            self.storage.read_sector(sector as usize, self.sector).unwrap();

            // Make the last cluster point to the new cluster:
            let bytes = given.to_le_bytes();

            self.sector[offset as usize + 0] = bytes[0];
            self.sector[offset as usize + 1] = bytes[1];
            self.sector[offset as usize + 2] = bytes[2];
            self.sector[offset as usize + 3] = bytes[3];

            // And write it out immediately:
            self.storage.write_sector(sector as usize, self.sector).unwrap();

            self.current_cluster_idx = Some(given);

            Ok(())
        } else {
            Err(())
        }
    }
}

impl<'f, 's, 'a, S: Storage<Word = u8, SECTOR_SIZE = U512>> Iterator for &mut FatEntryTracer<'f, 's, 'a, S> {
    type Item = (Cluster, FatEntry);

    fn next(&mut self) -> Option<(Cluster, FatEntry)> {
        if let Some(idx) = self.current_cluster_idx {
            let (sector, offset) = cluster_idx_to_fat_sector_and_offset(
                self.file_sys.fat_starting_sector,
                self.file_sys.sector_size_in_bytes,
                idx
            );

            if sector != self.current_cached_sector_idx {
                self.storage.read_sector(dbg!(sector as usize), self.sector).unwrap();

                self.current_cached_sector_idx = sector;
            }

            let next: Cluster = Cluster::from_le_bytes(dbg!(self.sector[(offset as usize)..(offset as usize)+4].try_into().unwrap()));
            let fat_entry = FatEntry::from(dbg!(next));

            if fat_entry == FatEntry::END_OF_CHAIN {
                self.current_cluster_idx = None;
                self.hit_end = Some(idx);
            } else {
                self.current_cluster_idx = Some(next);
            }

            Some((dbg!(idx), fat_entry))
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
