//! FAT filesystem things!

use super::Storage;
use super::gpt::{PartitionEntry, Guid};

use boot_sector::BootSector;

use generic_array::GenericArray;
use typenum::consts::U512;

use core::marker::PhantomData;
use core::convert::TryInto;

pub mod boot_sector;

// Another TODO: relax the 512B sector size restriction in this file.

// TODO: this should hold a mutable reference to the storage that it is backed
// by; we currently don't do this to make the FFI a little easier.

#[derive(Debug)]
pub struct FatFs</*'s, */S: Storage<Word = u8, SECTOR_SIZE = U512>> {
    pub starting_lba: u64,
    pub ending_lba: u64,

    pub sector_size_in_bytes: u16, // Currently we _assume_ this is 512 (todo!)..
    pub fat_table_size_in_sectors: u32,

    pub cluster_size_in_sectors: u8,
    pub fat_starting_sector: u32,
    pub root_dir_cluster_num: u32,

    pub next_known_free_cluster: u32,

    // storage: &'s mut S,
    _s: PhantomData</*&'s */S>,
}

impl</*'s, */S: Storage<Word = u8, SECTOR_SIZE = U512>> FatFs</*'s, */S> {
    pub fn mount(s: &/*'s*/ mut S, partition: &PartitionEntry) -> Result<Self, ()> {
        if partition.partition_type != Guid::microsoft_basic_data() {
            return Err(());
        }

        let mut sector = GenericArray::default();
        s.read_sector(partition.first_lba as usize, &mut sector).map_err(|_| ())?;

        let boot_sect = BootSector::read(&sector);
        assert_eq!(512, boot_sect.bpb.bytes_per_logical_sector);

        Ok(Self {
            starting_lba: partition.first_lba,
            ending_lba: partition.last_lba,

            sector_size_in_bytes: boot_sect.bpb.bytes_per_logical_sector,
            fat_table_size_in_sectors: boot_sect.bpb.logical_sectors_per_fat_extended,

            cluster_size_in_sectors: boot_sect.bpb.logical_sectors_per_cluster,
            fat_starting_sector: boot_sect.starting_fat_sector(),
            root_dir_cluster_num: boot_sect.bpb.root_dir_cluster_num,

            next_known_free_cluster: boot_sect.starting_fat_sector(),

            _s: PhantomData,
        })
    }

    pub fn get_boot_sect(&self, s: &/*'s*/ mut S) -> Result<BootSector, ()> {
        let mut sector = GenericArray::default();
        s.read_sector(self.starting_lba as usize, &mut sector).map_err(|_| ())?;

        Ok(BootSector::read(&sector))
    }

}
