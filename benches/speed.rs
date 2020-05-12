//! A read benchmark that tries to measure read speed.

extern crate criterion;

use criterion::{
    BatchSize, BenchmarkId, Criterion, Throughput, PlotConfiguration, AxisScale,
    criterion_group, criterion_main,
};

use fs::{
    gpt::{
        Gpt, PartitionEntry,
    },
    fat::{
        FatFs,
        cache::eviction_policies::{
            LeastRecentlyAccessed,
            UnmodifiedFirst,
        },
        dir::{
            DirIter, State,
        },
        table::FatEntry,
    }
};

use storage_traits::{FileBackedStorage, Storage};

use typenum::consts::U16384;

const FILES: &[(&'static str, u64)] = &[
    ("/1k", 1682929735),
    ("/100k", 1128310450),
    ("/5M", 3452563717),
    // ("/50M", 2602218254),
];

// const IMG_FILE_PATH: &'static str = "assets/fat.img";
const IMG_FILE_PATH: &'static str = "assets/disk.img";

const SD_CARD_PATH: &'static str = "/dev/mmcblk0";
const SD_CARD_SIZE: usize = 31_449_088;

fn bench_read_speed(c: &mut Criterion) {
    let mut group = c.benchmark_group("read speed");

    let plot_config = PlotConfiguration::default()
        .summary_scale(AxisScale::Logarithmic);
    group.plot_config(plot_config);

    let mut s = FileBackedStorage::from_file(IMG_FILE_PATH).unwrap();
    // let mut s = FileBackedStorage::from_file_with_explicit_size(SD_CARD_PATH, SD_CARD_SIZE).unwrap();
    let g = Gpt::read_gpt(&mut s).unwrap();
    let p = g.get_partition_entry(&mut s, 0).unwrap();

    let mut f = FatFs::<_, U16384, _>::mount(&mut s, &p,
        UnmodifiedFirst::<LeastRecentlyAccessed>::default(),
    ).unwrap();

    let bytes_in_a_cluster = f.bytes_in_a_cluster();

    for (path, _) in FILES.iter() {
        let (_, entry) = f.lookup_path(&mut s, path.as_bytes()).unwrap();
        let file_size = entry.file_size;

        group.throughput(Throughput::Elements(file_size as u64));

        group.bench_with_input(
            BenchmarkId::new("file read speed", file_size),
            &entry,
            |b, p| b.iter(|| {
                let mut c = p.cluster_idx();
                let mut offset = 0;
                let mut checksum: u64 = 0;

                for _ in 0..p.file_size {
                    while offset >= bytes_in_a_cluster {
                        offset -= bytes_in_a_cluster;

                        let fe = FatEntry::from(c);
                        c = fe.trace(&mut f, &mut s).next().unwrap().next;
                    }

                    // Assumes contiguous clusters for the moment..
                    // TODO: when offset is a multiple of the cluster size, update
                    // the cluster...
                    let (_, offs) = f.cluster_to_sector(c, offset);

                    let mut buf = [0];
                    let fe = FatEntry::from(c);
                    fe.upgrade(&mut f, &mut s).read(offs as u32, &mut buf).unwrap();

                    checksum = checksum.wrapping_add(buf[0] as u64);

                    offset += 1;
                }

                if checksum % 56789 == 6 {
                    println!("{}", checksum & 7);
                }
            })
        );
    }
}

criterion_group!(benches, bench_read_speed);
// criterion_main!(benches);

fn main() {
    std::thread::Builder::new()
        .stack_size(1024 * 1024 * 1024)
        .spawn(|| {
            // let mut crit = Default::default();
            // bench_read_speed(&mut crit);

            benches();

            Criterion::default()
                .configure_from_args()
                .final_summary();
        })
        .unwrap()
        .join()
        .unwrap();
}
