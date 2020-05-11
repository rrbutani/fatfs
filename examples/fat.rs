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
    }
};

use storage_traits::{FileBackedStorage, Storage};
use generic_array::GenericArray;
use typenum::consts::{U512, U32};

const FILE_PATH: &'static str = "assets/fat.img.mut";

const SD_CARD_PATH: &'static str = "/dev/mmcblk0";
const SD_CARD_SIZE: usize = 31_449_088;

fn main() {
    // let mut s = FileBackedStorage::from_file(FILE_PATH).unwrap();
    let mut s = FileBackedStorage::from_file_with_explicit_size(SD_CARD_PATH, SD_CARD_SIZE).unwrap();

    let g = Gpt::read_gpt(&mut s).unwrap();
    let p = g.get_partition_entry(&mut s, 0).unwrap();

    let mut f = FatFs::<_, U32, _>::mount(
        &mut s,
        &p,
        UnmodifiedFirst::<LeastRecentlyAccessed>::default(),
    ).unwrap();

    println!("{:#?}", g);
    println!("{:#?}", p);

    println!("{:#?}", f.get_boot_sect(&mut s));
    println!("{:#?}", f);

    let mut buf = [0u8; 512];

    for i in 2048..2150 {
        f.read(&mut s, SectorIdx::new(i as u64), 0, &mut buf);

        println!("{} → ({:#6X}):", i, i * 512);
        for (o, a) in buf.chunks(16).enumerate() {
            match a {
                a @ [_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _] =>
                    println!("{:#6X}: {:2X?}", i * 512 + 16 * o, a),
                _ => unreachable!(),
            }
        }
    }

    // println!("{:#?}", f.next_free_cluster(&mut s));
}
