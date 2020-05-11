//! FAT filesystem things!

use super::Storage;
use super::gpt::{PartitionEntry, Guid};
use super::util::BitMapLen;

use boot_sector::BootSector;

use generic_array::{ArrayLength, GenericArray};
use typenum::consts::U512;

use core::cell::RefCell;
use core::convert::TryInto;
use core::marker::PhantomData;
use core::ops::Range;

pub mod cache;
use cache::{SectorCache, EvictionPolicy, DynEvictionPolicy};

pub mod types;
use types::{SectorIdx, ClusterIdx};

pub mod boot_sector;
pub mod table;
pub mod dir;
pub mod file;

const FAT_ENTRY_SIZE_IN_BYTES: u16 = 4;

// Another TODO: relax the 512B sector size restriction in this file.

// TODO: this should hold a mutable reference to the storage that it is backed
// by; we currently don't do this to make the FFI a little easier.

#[allow(non_camel_case_types)]
#[derive(Debug)]
pub struct FatFs<S, CACHE_SIZE, Ev = DynEvictionPolicy>
where
    S: Storage<Word = u8, SECTOR_SIZE = U512>,
    CACHE_SIZE: ArrayLength<RefCell<GenericArray<u8, U512>>>,
    CACHE_SIZE: ArrayLength<cache::CacheEntry>,
    CACHE_SIZE: BitMapLen,
    Ev: EvictionPolicy,
{
    pub starting_lba: SectorIdx,
    pub ending_lba: SectorIdx,
    pub num_sectors: u64,

    pub sector_size_in_bytes: u16, // Currently we _assume_ this is 512 (todo!)..
    pub fat_table_size_in_sectors: u32,
    pub num_fat_tables: u8, // TODO! we currently ignore all but the first (i.e. we don't update the other ones..)
    pub cluster_size_in_sectors: u8,

    pub fat_starting_sector: SectorIdx,
    pub root_dir_cluster_num: ClusterIdx,
    pub next_known_free_cluster: ClusterIdx,

    pub cache: SectorCache<S, U512, CACHE_SIZE, Ev>,

    // storage: &'s mut S,
    _s: PhantomData</*&'s */S>,
}

impl<S, CS, Ev> FatFs<S, CS, Ev>
where
    S: Storage<Word = u8, SECTOR_SIZE = U512>,
    CS: ArrayLength<RefCell<GenericArray<u8, U512>>>,
    CS: ArrayLength<cache::CacheEntry>,
    CS: BitMapLen,
    Ev: EvictionPolicy,
{
    pub fn mount(s: &/*'s*/ mut S, partition: &PartitionEntry, ev: Ev) -> Result<Self, ()> {
        if partition.partition_type != Guid::microsoft_basic_data() {
            return Err(());
        }

        let mut cache = SectorCache::new(s, SectorIdx::new(partition.last_lba), ev);

        let boot_sect = BootSector::read(
            &cache.upgrade(s).get(SectorIdx::new(partition.first_lba))
        );
        assert_eq!(512, boot_sect.bpb.bytes_per_logical_sector);

        let starting_lba = SectorIdx::new(partition.first_lba);
        let ending_lba = SectorIdx::new(partition.last_lba);

        let cluster_size_in_sectors = boot_sect.bpb.logical_sectors_per_cluster;

        let num_sectors = partition.last_lba - partition.first_lba;

        Ok(Self {
            starting_lba,
            ending_lba,
            num_sectors,

            sector_size_in_bytes: boot_sect.bpb.bytes_per_logical_sector,
            fat_table_size_in_sectors: boot_sect.bpb.logical_sectors_per_fat_extended,
            num_fat_tables: boot_sect.bpb.num_file_alloc_tables,
            cluster_size_in_sectors,

            fat_starting_sector: boot_sect.starting_fat_sector(),
            root_dir_cluster_num: ClusterIdx::new(boot_sect.bpb.root_dir_cluster_num),
            next_known_free_cluster: ClusterIdx::new(boot_sect.bpb.root_dir_cluster_num),

            cache,

            _s: PhantomData,
        })
    }

    pub fn bytes_in_a_cluster(&self) -> u32 {
        (self.cluster_size_in_sectors as u32) * (self.sector_size_in_bytes as u32)
    }

    /// Cluster Index to the corresponding FAT Table entry's sector and byte
    /// offset.
    pub fn cluster_to_table_pos(&self, idx: ClusterIdx) -> (SectorIdx, u16) {
        Self::cluster_to_table_pos_inner(
            self.sector_size_in_bytes,
            self.fat_starting_sector,
            idx,
        )
    }

    pub fn cluster_to_table_pos_inner(
        sector_size_in_bytes: u16,
        fat_starting_sector: SectorIdx,
        idx: ClusterIdx,
    ) -> (SectorIdx, u16) {
        let cluster_entries_per_fat_sector: u64 =
            (sector_size_in_bytes as u64) / 4;

        let sector_idx =
            fat_starting_sector.inner() + ((*idx as u64) / cluster_entries_per_fat_sector);

        let byte_offset = idx.inner() % (cluster_entries_per_fat_sector as u32);
        let byte_offset = byte_offset * (FAT_ENTRY_SIZE_IN_BYTES as u32);

        (SectorIdx::new(sector_idx), byte_offset as u16)
    }

    pub fn cluster_to_sector(&self, idx: ClusterIdx, offset: u32) -> (SectorIdx, u16) {
        // Convert the cluster idx + offset to sector idx.
        let sector_idx = (*idx.inner() as u64) * (self.cluster_size_in_sectors as u64);
        let sector_idx = sector_idx + ((offset as u64) / (self.sector_size_in_bytes as u64));
        // let sector_idx = sector_idx - 2;

        // Add in the number of sectors used for the FAT/boot sector/whatever.
        let sector_idx = sector_idx + *self.fat_starting_sector.inner() +
            (self.fat_table_size_in_sectors as u64) * (self.num_fat_tables as u64);
        let sector_idx = SectorIdx::new(sector_idx);

        let offset = offset % (self.sector_size_in_bytes as u32);

        (sector_idx, offset as u16)
    }

    pub fn cluster_to_sector_range(&self, idx: ClusterIdx) -> Range<SectorIdx> {
        let (start, _) = self.cluster_to_sector(idx, 0);

        start..SectorIdx::new(*start.inner() + (self.cluster_size_in_sectors as u64))
    }

    pub fn get_boot_sect(&mut self, s: & mut S) -> Result<BootSector, ()> {
        Ok(BootSector::read(&*self.cache.upgrade(s).get(self.starting_lba)))
    }

    pub fn next_free_cluster(&mut self, s: &mut S) -> Result<ClusterIdx, ()> {
        let num_clusters = self.fat_table_size_in_sectors *
            ((self.sector_size_in_bytes as u32) / (FAT_ENTRY_SIZE_IN_BYTES as u32));

        let ssib = self.sector_size_in_bytes;
        let fss = self.fat_starting_sector;
        let to_table_pos = move |idx| Self::cluster_to_table_pos_inner(ssib, fss, idx);

        let mut cache = self.cache.upgrade(s);

        // Rather than attempt to free up space or detect when we're at full
        // capacity or do _anything_ intelligent, this will simply spin if we're
        // full.
        loop {
            let (sector, offset) = to_table_pos(self.next_known_free_cluster);

            let next = ClusterIdx::new(u32::from_le_bytes(
                cache.get(sector)[offset as usize .. (offset + 4) as usize].try_into().unwrap(),
            ));

            if table::FatEntry::from(next) == table::FatEntry::FREE {
                // Mark this cluster as the end of a chain:
                let bytes = table::FatEntry::END_OF_CHAIN.next.to_le_bytes();

                cache.get_mut(sector)[(offset as usize)..(offset as usize + (FAT_ENTRY_SIZE_IN_BYTES as usize))]
                    .copy_from_slice(&bytes);

                let current_cluster = self.next_known_free_cluster;
                self.next_known_free_cluster =
                    ClusterIdx::new((self.next_known_free_cluster.inner() + 1) % num_clusters);

                break Ok(current_cluster);
            }

            // If that didn't work, onto the next!
            self.next_known_free_cluster = ClusterIdx::new((self.next_known_free_cluster.inner() + 1) % num_clusters);
        }
    }

    fn range_chk(&self, sector: SectorIdx, offset: u16, len: usize) -> Result<(), ()> {
        let valid_sector_range = self.starting_lba..=self.ending_lba;

        // Check for a valid offset.
        if !(0..self.sector_size_in_bytes).contains(&offset) {
            return Err(())
        }

        // Check that the entire range is in bounds.
        let ending_offset = offset as u64 + len as u64;
        let ending_sector = SectorIdx::new(sector.inner() +
            (ending_offset / (self.sector_size_in_bytes as u64)) +
            if ending_offset % (self.sector_size_in_bytes as u64) == 0 { 0 } else { 1 }
        );
        if !(
            valid_sector_range.contains(&sector) &&
            valid_sector_range.contains(&ending_sector)
        ) {
            return Err(())
        }

        Ok(())
    }

    pub fn read(&mut self, s: &mut S, mut sector: SectorIdx, mut offset: u16, buffer: &mut [u8]) -> Result<(), ()> {
        self.range_chk(sector, offset, buffer.len())?;

        let cache = self.cache.upgrade(s);

        // TODO: write a less clunky version of this that auto-vectorizers can
        // actually do something with.
        //
        // as in, use copy_from_slice and split into the appropriate chunks
        //
        // or maybe this is good enough
        // who knows
        for b in buffer.iter_mut() {
            *b = cache.get(sector)[offset as usize];

            offset += 1;

            if offset == self.sector_size_in_bytes {
                offset = 0;
                sector = SectorIdx::new(sector.inner() + 1);
            }
        }

        Ok(())
    }

    pub fn write_iter(&mut self, s: &mut S, mut sector: SectorIdx, mut offset: u16, data: impl Iterator<Item = u8>) -> Result<(), ()> {
        // Since we don't know how many elements this iterator will produce
        // up-front, we can't do a perfect job here.
        //
        // Note that this is potentially hazardous; bad iterator impls may
        // incorrectly overestimate their lower bound in which case this
        // function will fail even if we could actually handle the number of
        // elements that would have been produced.
        self.range_chk(sector, offset, data.size_hint().0)?;

        let mut cache = self.cache.upgrade(s);

        for b in data {
            cache.get_mut(sector)[offset as usize] = b;

            offset += 1;

            if offset == self.sector_size_in_bytes{
                offset = 0;
                sector = SectorIdx::new(sector.inner() + 1);
            }

            // Unfortunately we can't do this check up-front since we're dealing
            // with an iterator.
            if sector > self.ending_lba { return Err(()) }
        }

        Ok(())
    }

    pub fn write(&mut self, s: &mut S, sector: SectorIdx, offset: u16, buffer: &[u8]) -> Result<(), ()> {
        // self.range_chk(sector, offset, buffer.len())?; // Unnecessary since we pass along a ExactSizeIterator.
        self.write_iter(s, sector, offset, buffer.iter().cloned())
    }

    pub fn format(_storage: &/*'s*/ mut S, partition: &PartitionEntry) -> Result<Self, ()> {
        if partition.partition_type != Guid::microsoft_basic_data() {
            return Err(());
        }

        todo!();

        // Self::mount(storage, partition)
    }
}
