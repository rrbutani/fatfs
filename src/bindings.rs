//! C Bindings for this crate.

#[no_mangle]
pub extern "C" fn foo_bar(yo: u8) -> u8 {
    yo * 2
}

#[no_mangle]
pub extern "C" fn yay(yo: u8) -> u8 {
    yo * 2
}

#[no_mangle]
pub extern "C" fn new_edisk_storage(drive_num: u8, size_in_sectors: u64) -> edisk::EDiskStorage {
    edisk::EDiskStorage { drive_num, size_in_sectors }
}

#[no_mangle]
pub extern "C" fn sector_sum(storage: &mut edisk::EDiskStorage, sector_num: u32) -> u64 {
    use storage_traits::Storage;
    use generic_array::GenericArray;

    if sector_num >= (storage.capacity() as u32) {
        0
    } else {
        let mut sector = GenericArray::default();
        storage.read_sector(sector_num as usize, &mut sector).unwrap();

        let mut checksum: u64 = 0;
        for byte in sector.as_slice() {
            checksum = checksum.wrapping_add(*byte as u64);
        }

        checksum
    }
}

pub mod efile {
    use super::edisk::EDiskStorage;
    use crate::mutex::{Mutex, MutexInterface};
    use crate::gpt::Gpt;
    use crate::fat::FatFs;
    use crate::fat::cache::eviction_policies::{LeastRecentlyAccessed, UnmodifiedFirst};
    use crate::fat::dir::{DirIter, State};

    use typenum::consts::{U512, U32, U16};

    use core::slice::from_raw_parts;

    static STORAGE: Mutex<Option<EDiskStorage>> = Mutex::new(None);
    static FS: Mutex<Option<
        FatFs<EDiskStorage, U16, UnmodifiedFirst<LeastRecentlyAccessed>>
    >> = Mutex::new(None);

    #[no_mangle]
    pub extern "C" fn eFile_Init() { }

    #[no_mangle]
    pub extern "C" fn eFile_Mount(drive_num: u8, size_in_sectors: u64) {
        STORAGE.cs(|s| {
            *s = Some(EDiskStorage { drive_num, size_in_sectors });

            let s = s.as_mut().unwrap();

            let g = Gpt::read_gpt(s).unwrap();
            let p = g.get_partition_entry(s, 0).unwrap();

            FS.cs(|f| {
                *f = Some(FatFs::mount(s, &p,
                    UnmodifiedFirst::<LeastRecentlyAccessed>::default()).unwrap()
                );
            })
        })
    }

    #[no_mangle]
    pub extern "C" fn eFile_NewFile(path: *const u8, len: u16) -> bool {
        let path = unsafe { from_raw_parts(path, len as usize) };

        todo!()
    }

    #[no_mangle]
    pub extern "C" fn eFile_NewDir(path: *const u8, len: u16) -> bool {
        let path = unsafe { from_raw_parts(path, len as usize) };

        todo!()
    }

    #[no_mangle]
    pub extern "C" fn eFile_Read(path: *const u8, len: u16, offset: u32, buf: *mut u8, buf_len: u32) -> bool {
        let path = unsafe { from_raw_parts(path, len as usize) };

        todo!()
    }

    #[no_mangle]
    pub extern "C" fn eFile_ReadAll(path: *const u8, len: u16, func: extern "C" fn(u8)) -> bool {
        let path = unsafe { from_raw_parts(path, len as usize) };

        todo!()
    }

    #[no_mangle]
    pub extern "C" fn eFile_Append(path: *const u8, len: u16, buf: *const u8, buf_len: u32) -> bool {
        let path = unsafe { from_raw_parts(path, len as usize) };

        todo!()
    }

    #[no_mangle]
    pub extern "C" fn eFile_Delete(path: *const u8, len: u16) -> bool {
        let path = unsafe { from_raw_parts(path, len as usize) };

        STORAGE.cs(|s| s.as_mut().map(|s| FS.cs(|f| f.as_mut().map(|f| {
            if let Ok(p) = f.lookup_path(s, path) {
                DirIter::from_cluster(f.root_dir_cluster_num, f, s)
                    .delete(p)
                    .is_ok()
            } else {
                false
            }
        })).unwrap_or(false)).unwrap_or(false))
    }

    #[no_mangle]
    pub extern "C" fn eFile_DirList(path: *const u8, len: u16, func: extern "C" fn(*const u8, *const u8)) -> bool {
        let path = unsafe { from_raw_parts(path, len as usize) };

        STORAGE.cs(|s| s.as_mut().map(|s| FS.cs(|f| f.as_mut().map(|f| {
            if let Ok((_, de)) = f.lookup_path(s, path) {
                for (_, dir) in DirIter::from_cluster(de.cluster_idx(), f, s) {
                    if let State::Exists = dir.state() {
                        func(
                            dir.file_name.0.as_ptr(),
                            dir.file_ext.0.as_ptr(),
                        )
                    }
                }
                true
            } else {
                false
            }
        })).unwrap_or(false)).unwrap_or(false))
    }

    #[no_mangle]
    pub extern "C" fn eFile_Flush() -> bool {
        STORAGE.cs(|s| s.as_mut().map(|s| FS.cs(|f| f.as_mut().map(|f| {
            f.cache.flush(s).is_ok()
        })).unwrap_or(false)).unwrap_or(false))
    }
}

pub mod edisk {
    use storage_traits::{Storage, errors::{ReadError, WriteError}};
    use generic_array::GenericArray;
    use typenum::consts::U512;

    #[repr(C)]
    pub struct EDiskStorage {
        pub drive_num: u8,
        pub size_in_sectors: u64,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
    pub struct UnknownError;

    #[repr(C)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum DResult {
        /// Successful
        ResOk = 0,
        /// R/W Error
        ResError = 1,
        /// Write Protected
        ResWrPrt = 2,
        /// Not Ready
        ResNotRdy = 3,
        /// Invalid Parameter
        ResParErr = 4,
    }

    extern "C" {
        fn eDisk_Read(drv: u8, buff: *mut u8, sector: u32, count: u32) -> DResult;
        fn eDisk_ReadBlock(buff: *mut u8, sector: u32) -> DResult;

        fn eDisk_Write(drv: u8, buff: *const u8, sector: u32, count: u32) -> DResult;
        fn eDisk_WriteBlock(buff: *const u8, sector: u32) -> DResult;
    }

    impl Storage for EDiskStorage {
        type Word = u8;
        type SECTOR_SIZE = U512;

        type ReadErr = DResult;
        type WriteErr = DResult;

        fn capacity(&self) -> usize {
            self.size_in_sectors as usize
        }

        fn read_sector(
            &mut self,
            sector_idx: usize,
            buffer: &mut GenericArray<u8, U512>,
        ) -> Result<(), ReadError<DResult>> {
            if (sector_idx as u64) >= self.size_in_sectors {
                return Err(ReadError::OutOfRange {
                    requested_offset: sector_idx,
                    max_offset: self.size_in_sectors as usize,
                });
            }

            match unsafe { eDisk_Read(
                self.drive_num,
                buffer.as_mut_slice().as_mut_ptr(),
                sector_idx as u32,
                1,
            ) } {
                DResult::ResOk => Ok(()),
                e => Err(ReadError::Other(e)),
            }
        }

        fn write_sector(
            &mut self,
            sector_idx: usize,
            words: &GenericArray<u8, U512>,
        ) -> Result<(), WriteError<DResult>> {
            if (sector_idx as u64) >= self.size_in_sectors {
                return Err(WriteError::OutOfRange {
                    requested_offset: sector_idx,
                    max_offset: self.size_in_sectors as usize,
                });
            }

            match unsafe { eDisk_Write(
                self.drive_num,
                words.as_slice().as_ptr(),
                sector_idx as u32,
                1,
            ) } {
                DResult::ResOk => Ok(()),
                e => Err(WriteError::Other(e))
            }
        }
    }
}
