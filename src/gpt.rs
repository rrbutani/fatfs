//! Utilities for reading and creating GUID Partition Tables (GPT).
//!
//! Currently this file (intentionally) doesn't really do a good job exposing
//! all the fields/functionality in GPT. Right now we pretty much just have
//! exactly what we need for single partition disks.

use super::Storage;

use storage_traits::errors::{ReadError, WriteError};
use generic_array::GenericArray;
use typenum::consts::U512;

use core::fmt::{self, Debug};
use core::convert::TryInto;

pub const GPT_SIGNATURE: [u8; 8] = *b"EFI PART";

/// Represents a "middle-endian" 128 bit GUID (as used in GPT).
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Guid {
    first: u32,
    second: u16,
    third: u16,
    fourth: u16,
    fifth_p1: u16, // Since we don't have 48 bit types...
    fifth_p2: u32,
}

impl Guid {
    pub fn from_mixed_u128(u: u128) -> Self {
        Self::from_mixed(u.to_le_bytes())
    }

    pub fn microsoft_basic_data() -> Self {
        Guid::from_mixed_u128(0xEBD0A0A2_B9E5_4433_87C0_68B6B72699C7u128)
    }

    pub fn from_mixed([
        p, o, n, m,
        l, k,
        j, i,
        g, h,
        e, f,
        a, b, c, d,
    ]: [u8; 16]) -> Self {
        Self {
            first: u32::from_le_bytes([a, b, c, d]),
            second: u16::from_le_bytes([e, f]),
            third: u16::from_le_bytes([g, h]),
            fourth: u16::from_be_bytes([i, j]),
            fifth_p1: u16::from_be_bytes([k, l]),
            fifth_p2: u32::from_be_bytes([m, n, o, p]),
        }
    }

    pub fn to_bytes(&self) -> [u8; 16] {
        let [a, b, c, d] = self.first.to_le_bytes();
        let [e, f] = self.second.to_le_bytes();
        let [g, h] = self.third.to_le_bytes();
        let [i, j] = self.fourth.to_be_bytes();
        let [k, l] = self.fifth_p1.to_be_bytes();
        let [m, n, o, p] = self.fifth_p2.to_be_bytes();

        [a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p]
    }

    pub fn from_bytes([
        a, b, c, d,
        e, f,
        g, h,
        i, j,
        k, l,
        m, n, o, p
    ]: [u8; 16]) -> Self {
        Self {
            first: u32::from_le_bytes([a, b, c, d]),
            second: u16::from_le_bytes([e, f]),
            third: u16::from_le_bytes([g, h]),
            fourth: u16::from_be_bytes([i, j]),
            fifth_p1: u16::from_be_bytes([k, l]),
            fifth_p2: u32::from_be_bytes([m, n, o, p]),
        }
    }
}

impl Debug for Guid {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "{:08X}-{:04X}-{:04X}-{:04X}-{:04X}{:08X}",
            self.first,
            self.second,
            self.third,
            self.fourth,
            self.fifth_p1,
            self.fifth_p2,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Descriptions sourced from [here](https://en.wikipedia.org/wiki/GUID_Partition_Table#Partition_entries_(LBA_2%E2%80%9333)).
pub struct Gpt {
    revision: u32,
    /// Header size in little endian (usually 92 bytes).
    header_size: u32,
    /// CRC32 of the start of the header up to [`header_size`].
    header_crc32: u32,
    current_lba: u64,
    backup_lba: u64,
    /// First usable LBA for partitions (primary partition table last LBA + 1)
    first_usable_lba: u64,
    /// Last usable LBA (secondary partition table first LBA − 1)
    last_usable_lba: u64,
    /// Disk GUID in mixed endian.
    disk_guid: Guid,
    /// Starting LBA of array of partition entries (always 2 in primary copy).
    partition_entries_starting_lba: u64,
    /// Number of partition entries in array.
    num_partition_entries: u32,
    /// Size of a single partition entry (usually 128 bytes).
    partition_entry_size: u32,
    /// CRC32 of partition entries array in little endian.
    partition_entries_crc32: u32,
}

#[derive(Clone)]
pub struct PartitionEntry {
    partition_type: Guid,
    unique_guid: Guid,
    // Little endian
    first_lba: u64,
    // Little endian, inclusive (usually odd)
    last_lba: u64,
    // bit 60 denotes read only
    attribute_flags: u64,
    // UTF-16 LE.
    name: [u16; 36],
}

impl Debug for PartitionEntry {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct(core::any::type_name::<Self>())
            .field("partition_type", &self.partition_type)
            .field("unique_guid", &self.unique_guid)
            .field("first_lba", &self.first_lba)
            .field("last_lba", &self.last_lba)
            .field("attribute_flags", &self.attribute_flags)
            .field("name", &"Name") // TODO: parse name into a String on std
            .finish()
    }
}

impl PartitionEntry {
    pub fn fat(beginning: u64, end: u64) -> Self {
        Self {
            partition_type: Guid::microsoft_basic_data(),
            unique_guid: Guid::from_mixed_u128(0x1234567890ABCDEF1234567890ABCDEFu128),
            first_lba: beginning,
            last_lba: end,
            attribute_flags: 0,
            name: {
                let name = "RTOS"; // TODO: not this.
                let mut iter = name.encode_utf16();
                let mut buf = [0u16; 36];

                buf[0] = iter.next().unwrap();
                buf[1] = iter.next().unwrap();
                buf[2] = iter.next().unwrap();
                buf[3] = iter.next().unwrap();

                buf
            }
        }
    }
}

// TODO: an iterator over partition entries...

impl Gpt {
    pub fn read_gpt<S: Storage<Word = u8, SECTOR_SIZE = U512>>(storage: &mut S) -> Result<Gpt, ()> {
        let mut sector = GenericArray::default();
        storage.read_sector(1, &mut sector).unwrap(); // TODO: don't unwrap.

        let sector = sector.as_slice();

        if sector[0..8] != GPT_SIGNATURE {
            return Err(());
        }

        Ok(Self {
            revision: u32::from_le_bytes(sector[8..12].try_into().unwrap()),
            header_size: u32::from_le_bytes(sector[12..16].try_into().unwrap()),
            header_crc32: u32::from_le_bytes(sector[16..20].try_into().unwrap()),
            current_lba: u64::from_le_bytes(sector[24..32].try_into().unwrap()),
            backup_lba: u64::from_le_bytes(sector[32..40].try_into().unwrap()),
            first_usable_lba: u64::from_le_bytes(sector[40..48].try_into().unwrap()),
            last_usable_lba: u64::from_le_bytes(sector[48..56].try_into().unwrap()),
            disk_guid: Guid::from_bytes(sector[56..72].try_into().unwrap()),
            partition_entries_starting_lba: u64::from_le_bytes(sector[72..80].try_into().unwrap()),
            num_partition_entries: u32::from_le_bytes(sector[80..84].try_into().unwrap()),
            partition_entry_size: u32::from_le_bytes(sector[84..88].try_into().unwrap()),
            partition_entries_crc32: u32::from_le_bytes(sector[88..92].try_into().unwrap()),
        })
    }

    pub fn get_partition_entry<S: Storage<Word = u8, SECTOR_SIZE = U512>>(&self, storage: &mut S, idx: u32) -> Result<PartitionEntry, ()> {
        if idx != 0 { unimplemented!() /* TODO!! Err on out of range, etc. */ }

        let mut sector = GenericArray::default();
        storage.read_sector(self.partition_entries_starting_lba as usize, &mut sector).unwrap(); // TODO: don't unwrap.

        let entry = &sector.as_slice()[0..(self.partition_entry_size as usize)];

        Ok(PartitionEntry {
            partition_type: Guid::from_bytes(entry[0..16].try_into().unwrap()),
            unique_guid: Guid::from_bytes(entry[16..32].try_into().unwrap()),
            first_lba: u64::from_le_bytes(entry[32..40].try_into().unwrap()),
            last_lba: u64::from_le_bytes(entry[40..48].try_into().unwrap()),
            attribute_flags: u64::from_le_bytes(entry[48..56].try_into().unwrap()),
            name: {
                let mut buf = [0u16; 36];

                for i in 0..36 {
                    buf[i] = ((entry[48 + 2 * i + 1] as u16) << 8) | (entry[48 + 2 * i] as u16);
                }

                buf
            }
        })
    }

    // pub fn write_fat_gpt<S: Storage<Word = u8, SECTOR_SIZE = U512>>(storage: &mut S) -> Result<(), WriteError<S::WriteErr>> {
    //     let mut sector = GenericArray::default();

    //     sector[0..7] = GPT_SIGNATURE;


    //     storage.write_sector(1, &sector)
    // }
}


#[cfg(test)]
mod gpt_tests {
    use super::*;

    // Test case comes from here: https://developer.apple.com/library/archive/technotes/tn2166/_index.html#//apple_ref/doc/uid/DTS10003927-CH1-SUBSECTION11
    #[test]
    fn guid_mixed_to_disk() {
        assert_eq!(
            Guid::from_mixed(0xC12A7328_F81F_11D2_BA4B_00A0C93EC93Bu128.to_le_bytes()).to_bytes(),
            [0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b],
        )
    }

    #[test]
    fn roundtrip() {
        fn trip(a: u128) {
            let g = Guid::from_mixed_u128(a);

            assert_eq!(g, Guid::from_bytes(g.to_bytes()));
        }

        trip(0xC12A7328_F81F_11D2_BA4B_00A0C93EC93Bu128);
    }
}
