// Requires the `no_std` feature to be disabled so that `File`s implement the
// `Storage` trait.
//
// Run with --no-default-features.

use fs::gpt::{Gpt, PartitionEntry};
use fs::fat::FatFs;
use fs::fat::dir::DirIter;

use storage_traits::{FileBackedStorage, Storage};
use generic_array::GenericArray;
use typenum::consts::U512;

const FILE_PATH: &'static str = "assets/gpt.img";

const SD_CARD_PATH: &'static str = "/dev/mmcblk0";
const SD_CARD_SIZE: usize = 31_449_088;

fn main() {
    // let mut f = FileBackedStorage::from_file(FILE_PATH).unwrap();
    let mut s = FileBackedStorage::from_file_with_explicit_size(SD_CARD_PATH, SD_CARD_SIZE).unwrap();

    let g = Gpt::read_gpt(&mut s).unwrap();
    let p = g.get_partition_entry(&mut s, 0).unwrap();

    let mut f = FatFs::mount(&mut s, &p).unwrap();

    println!("{:#?}", g);
    println!("{:#?}", p);

    println!("{:#?}", f.get_boot_sect(&mut s));

    let mut sector = GenericArray::default();

    println!("{:?}", f.root_dir_cluster_num);

    for dir in DirIter::from_cluster(f.root_dir_cluster_num, &mut f, &mut s, &mut sector) {
        println!("{:?}", dir);
    }

    // println!("{:?}", f);
}
