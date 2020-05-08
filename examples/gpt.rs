// Requires the `no_std` feature to be disabled so that `File`s implement the
// `Storage` trait.
//
// Run with --no-default-features.

use fs::gpt::{Gpt, PartitionEntry};

use storage_traits::{FileBackedStorage, Storage};
use generic_array::GenericArray;
use typenum::consts::U512;

const FILE_PATH: &'static str = "assets/gpt.img";

const SD_CARD_PATH: &'static str = "/dev/mmcblk0";
const SD_CARD_SIZE: usize = 31_449_088;

fn main() {
    // let mut f = FileBackedStorage::from_file(FILE_PATH).unwrap();
    let mut f = FileBackedStorage::from_file_with_explicit_size(SD_CARD_PATH, SD_CARD_SIZE).unwrap();

    let g = Gpt::read_gpt(&mut f).unwrap();
    let p = g.get_partition_entry(&mut f, 0).unwrap();

    println!("{:?}", g);
    println!("{:?}", p);
}
