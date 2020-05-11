// Requires the `no_std` feature to be disabled so that `File`s implement the
// `Storage` trait.
//
// Run with --no-default-features.

use fs::gpt::{Gpt, PartitionEntry};
use fs::fat::{FatFs,
    types::SectorIdx,
    cache::eviction_policies::{
        UNMODIFIED_THEN_LEAST_RECENTLY_ACCESSED,
        LeastRecentlyAccessed,
        UnmodifiedFirst,
    },
    dir::{DirIter, State},
};

use storage_traits::{FileBackedStorage, Storage};
use generic_array::GenericArray;
use typenum::consts::{U512, U32};

const FILE_PATH: &'static str = "assets/gpt.img";

const SD_CARD_PATH: &'static str = "/dev/mmcblk0";
const SD_CARD_SIZE: usize = 31_449_088;

fn main() {
    // let mut f = FileBackedStorage::from_file(FILE_PATH).unwrap();
    let mut s = FileBackedStorage::from_file_with_explicit_size(SD_CARD_PATH, SD_CARD_SIZE).unwrap();

    let g = Gpt::read_gpt(&mut s).unwrap();
    let p = g.get_partition_entry(&mut s, 0).unwrap();

    let mut f = FatFs::<_, U32, _>::mount(&mut s, &p,
        UnmodifiedFirst::<LeastRecentlyAccessed>::default(),
    ).unwrap();

    println!("{:#?}", g);
    println!("{:#?}", p);

    println!("{:#?}", f);
    println!("{:#?}", f.get_boot_sect(&mut s));
    println!("{:?}", f.root_dir_cluster_num);

    for dir in DirIter::from_cluster(f.root_dir_cluster_num, &mut f, &mut s) {
        if let State::Exists = dir.state() {
            // println!("{:#?}", dir);
            println!("{:#?}.{:#?}", dir.file_name, dir.file_ext);
        }
    }

    // println!("{:?}", f);
}

// fn recur(d: DirEntry, prefix: String) {
//     for dir in i {
//         if let State::Exists = dir.state() {
//             println!("{}{:#?}.{:#?}", prefix, dir.name, dir.ext);

//             if let Some(i) = dir.into_dir_iter()
//         }
//     }
// }
