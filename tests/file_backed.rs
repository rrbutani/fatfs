// Requires the `no_std` feature to be disabled so that `File`s implement the
// `Storage` trait.
//
// Run with --no-default-features.

use storage_traits::{FileBackedStorage, Storage};
use generic_array::GenericArray;
use typenum::consts::U512;

const FILE_PATH: &'static str = "assets/gpt.img";

const SD_CARD_PATH: &'static str = "/dev/mmcblk0";
// const SD_CARD_SIZE: usize = (16 * 1024 * 1024 * 1024) / 512;
const SD_CARD_SIZE: usize = 31_449_088;

fn read_sector_one(mut storage: FileBackedStorage) {
    eprintln!("size in:\n  - sectors: {:#X}\n  - words: {:#X}\n  - bytes: {:#X} ({}) (2 ** {})",
        storage.capacity(),
        storage.capacity_in_words(),
        storage.capacity_in_bytes(),
        storage.capacity_in_bytes(),
        storage.capacity_in_bytes().trailing_zeros(),
    );

    let mut sector = GenericArray::default();
    storage.read_sector(0, &mut sector).unwrap();

    let mut checksum: u64 = 0;

    // TODO: use an actual checksum here?
    // or, don't bother; just for testing.
    for byte in sector.as_slice() {
        checksum = checksum.wrapping_add(*byte as u64);
    }

    // Should fail!
    assert_eq!(0, checksum);
}

#[test]
fn file() {
    read_sector_one(FileBackedStorage::from_file(FILE_PATH).unwrap())
}

#[test]
fn card() {
    read_sector_one(FileBackedStorage::from_file_with_explicit_size(SD_CARD_PATH, SD_CARD_SIZE).unwrap())
}
