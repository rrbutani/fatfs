//! Types and tools for the FAT Boot Sector and friends.
//!
//! Majority of the docs here are sourced from [this page](https://en.wikipedia.org/wiki/Design_of_the_FAT_file_system).

// We only support the FAT32 variants so expect 25 byte DOS 3.31 BIOS Parameter
// Blocks (BPBs) with the extensions (?).

// Another TODO: relax the 512B sector size restriction in this file.

use super::types::SectorIdx;

use generic_array::GenericArray;
use typenum::consts::U512;

use core::convert::TryInto;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootSector {
    // Offset: 0x003
    pub oem_name: [u8; 8],

    pub bpb: BiosParameterBlock,

    // ignoring the other fields...
}

impl BootSector {
    pub fn new(starting_lba: u32, ending_lba: u32) -> BootSector {
        Self {
            oem_name: *b"r3-fatfs",
            bpb: BiosParameterBlock::new(starting_lba, ending_lba),
        }
    }

    pub fn read(sector: &GenericArray<u8, U512>) -> Self {
        Self {
            oem_name: sector.as_slice()[3..(3 + 8)].try_into().unwrap(),
            bpb: BiosParameterBlock::read(sector),
        }
    }

    pub fn write(&self, sector: &mut GenericArray<u8, U512>) {
        // TODO!
        todo!()
    }
}

// FAT32 Extended BIOS Parameter Block (includes DOS 3.31 BPB which includes the
// DOS 2.0 BPB).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiosParameterBlock {
    // From the DOS 2.0 BPB:

    /// Bytes per logical sector in powers of two; the most common value is 512.
    // Offset: 0x00B
    pub bytes_per_logical_sector: u16,

    /// Logical sectors per cluster.
    /// Allowed values are 1, 2, 4, 8, 16, 32, 64, and 128.
    // Offset: 0x00D; def = 16
    pub logical_sectors_per_cluster: u8,

    // Offset: 0x00E; def = 0x0020
    pub num_reserved_logical_sectors: u16,

    // Offset: 0x010
    pub num_file_alloc_tables: u8,

    // Offset: 0x011; this is 0 for FAT32
    pub max_root_dir_entries: u16,

    // Offset: 0x013; this is 0 for FAT32
    pub total_logical_sectors: u16,

    // Offset: 0x015; def = 0xF8
    pub media_descriptor: u8,

    // Offset: 0x016; this is 0 for FAT32 (offset 0x024 is used instead)
    pub logical_sectors_per_fat: u16,


    // Now, the added fields from the DOS 3.31 BPB:

    /// Physical sectors per track for disks with INT 13h CHS geometry,[10]
    /// e.g., 18 for a “1.44 MB” (1440 KB) floppy. Unused for drives, which
    /// don't support CHS access any more. Identical to an entry available since
    /// DOS 3.0.
    // Offset: 0x018; def = 0x0010
    pub phys_sectors_per_track: u16,

    /// Number of heads for disks with INT 13h CHS geometry,[10] e.g., 2 for a
    /// double sided floppy. Unused for drives, which don't support CHS access
    /// any more. Identical to an entry available since DOS 3.0.
    // Offset: 0x01A; def = 0x0004
    pub num_heads: u16,

    /// Count of hidden sectors preceding the partition that contains this FAT
    /// volume. This field should always be zero on media that are not
    /// partitioned.
    // Offset: 0x01C; def = 0x0800 on this medium (just use the starting LBA)
    pub hidden_preceeding_sectors: u32,

    /// Total logical sectors (if greater than 65535; otherwise, see offset
    /// 0x013).
    // Offset: 0x020; (ending - starting should do it)
    pub total_logical_sectors_extended: u32,

    // /// Physical drive number (0x00 for (first) removable media, 0x80 for
    // /// (first) fixed disk as per INT 13h).
    // // Offset: 0x024; def = 3b?
    // phys_drive_number: u8,

    // // extended_boot_sig: u8,

    // // Offset = 0x027; def = 0
    // volume_id: u32,

    // // Offset = 0x02B; def = blanks (0x20)
    // partition_label: [u8; 11]

    // // Offset = 0x036; def = ["FAT32   "]
    // file_system_type: [u8; 8]


    /// Logical sectors per file allocation table (corresponds with the old
    /// entry at offset 0x0B in the DOS 2.0 BPB).
    ///
    /// The byte at offset 0x026 in this entry should never become 0x28 or 0x29
    /// in order to avoid any misinterpretation with the EBPB format under
    /// non-FAT32 aware operating systems.
    // Offset: 0x024; def = 0x00003bf0?
    pub logical_sectors_per_fat_extended: u32,

    /// Drive description / mirroring flags (bits 3-0: zero-based number of
    /// active FAT, if bit 7 set. If bit 7 is clear, all FATs are mirrored
    /// as usual. Other bits reserved and should be 0.)
    // Offset: 0x028; def = 0x0000
    pub drive_desc_mirroring_flags: u16,

    /// Version (defined as 0.0). The high byte of the version number is stored
    /// at offset 0x02B, and the low byte at offset 0x02A. FAT32
    /// implementations should refuse to mount volumes with version numbers
    /// unknown by them.
    // Offset: 0x02A; def = 0x0000
    pub version: u16,

    /// Cluster number of root directory start, typically 2 (first cluster)
    /// if it contains no bad sector.
    // Offset: 0x02C; def = 2
    pub root_dir_cluster_num: u32,

    /// Logical sector number of FS Information Sector, typically 1, i.e., the
    /// second of the three FAT32 boot sectors.
    // Offset: 0x030; def = 1
    pub fs_info_logical_sector_num: u16,

    /// First logical sector number of a copy of the three FAT32 boot sectors,
    /// typically 6.
    ///
    /// Values of 0x0000 (and/or 0xFFFF) are reserved and indicate that no
    /// backup sector is available.
    // Offset: 0x032; def = 0x0000
    pub boot_sector_backup_logical_sector_start_num: u16,

    // Offset: 0x040; def = 0x80
    pub phys_drive_number: u8,

    // reserved
    // extended boot sig

    // Offset: 0x043
    pub volume_id: u32,

    // Offset = 0x047; def = blanks (0x20)
    pub volume_label: [u8; 11],

    // Offset = 0x052; def = ["FAT32   "]
    pub file_system_type: [u8; 8],
}

impl BiosParameterBlock {
    pub fn new(starting_lba: u32, ending_lba: u32) -> Self {
        // TODO: this assumes a sector size of 512 and 16 clusters per block.

        let sectors_per_cluster = 16;
        let sector_size = 512;

        Self {
            bytes_per_logical_sector: sector_size,
            logical_sectors_per_cluster: sectors_per_cluster,
            num_reserved_logical_sectors: 0x0020,
            num_file_alloc_tables: 1,
            max_root_dir_entries: 0,
            total_logical_sectors: 0,
            media_descriptor: 0xF8,
            logical_sectors_per_fat: 0,

            phys_sectors_per_track: 0x0010,
            num_heads: 0x0004,
            hidden_preceeding_sectors: starting_lba,
            total_logical_sectors_extended: (ending_lba - starting_lba),
            logical_sectors_per_fat_extended: {
                let sectors = ending_lba - starting_lba;
                let clusters = sectors / (sectors_per_cluster as u32);

                let fat_entries_per_sector = sector_size / (32 / 8);
                let num_sectors_for_fat = clusters / (fat_entries_per_sector as u32);

                num_sectors_for_fat
            },
            drive_desc_mirroring_flags: 0,
            version: 0x0000,
            root_dir_cluster_num: 2,
            fs_info_logical_sector_num: 1, // TODO!
            boot_sector_backup_logical_sector_start_num: 0, // TODO: no backup for now!

            phys_drive_number: 0x80,
            volume_id: 0x00,
            volume_label: *b"RTOS_FSYS  ",
            file_system_type: *b"FAT32   ",
        }
    }

    pub fn read(sector: &GenericArray<u8, U512>) -> Self {
        let sector = sector.as_slice();

        macro_rules! e {
            ($ty:tt, $offset:literal :+ $num:literal) => {
                $ty::from_le_bytes(sector[$offset..($offset + $num)].try_into().unwrap())
            };

            ($ty:tt, $offset:literal) => {
                $ty::from_le_bytes(sector[$offset..($offset + core::mem::size_of::<$ty>())].try_into().unwrap())
            };
        }

        Self {
            // bytes_per_logical_sector: u16::from_le_bytes(sector[0x00B..(0x00B + 2)].try_into().unwrap()),
            // logical_sectors_per_cluster: u8::from_le_bytes(sector[0x00D..(0x00D + 1)].)
            bytes_per_logical_sector: e!(u16, 0x00B),
            logical_sectors_per_cluster: e!(u8, 0x00D),
            num_reserved_logical_sectors: e!(u16, 0x00E),
            num_file_alloc_tables: e!(u8, 0x010),
            max_root_dir_entries: e!(u16, 0x011),
            total_logical_sectors: e!(u16, 0x013),
            media_descriptor: e!(u8, 0x015),
            logical_sectors_per_fat: e!(u16, 0x016),

            phys_sectors_per_track: e!(u16, 0x018),
            num_heads: e!(u16, 0x01A),
            hidden_preceeding_sectors: e!(u32, 0x01C),
            total_logical_sectors_extended: e!(u32, 0x020),
            logical_sectors_per_fat_extended: e!(u32, 0x024),
            drive_desc_mirroring_flags: e!(u16, 0x028),
            version: e!(u16, 0x02A),
            root_dir_cluster_num: e!(u32, 0x02C),
            fs_info_logical_sector_num: e!(u16, 0x030),
            boot_sector_backup_logical_sector_start_num: e!(u16, 0x032),
            phys_drive_number: e!(u8, 0x40),
            volume_id: e!(u32, 0x043),
            volume_label: {
                sector[0x047..(0x047 + 11)].try_into().unwrap()
            },
            file_system_type: {
                sector[0x052..(0x052 + 8)].try_into().unwrap()
            }
        }
    }

    pub fn write(&self, sector: &mut GenericArray<u8, U512>) {
        // TODO!
        todo!()
    }
}

// TODO: FS Information Sector

impl BootSector {
    pub fn starting_fat_sector(&self) -> u32 {
        (self.bpb.num_reserved_logical_sectors as u32)
            + self.bpb.hidden_preceeding_sectors
    }
}
