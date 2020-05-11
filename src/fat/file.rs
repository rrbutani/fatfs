//! Files. Just files.

use super::FatFs;
use super::dir::DirEntry;
use super::cache::EvictionPolicy;
use crate::util::BitMapLen;

use storage_traits::Storage;
use generic_array::{ArrayLength, GenericArray};
use typenum::consts::U512;

use core::cell::RefCell;


#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    inner: DirEntry,
}

impl File {
    pub(in super) const fn new(inner: DirEntry) -> Self {
        Self { inner }
    }

    pub fn upgrade<'file, 'f, 's, S, CS, Ev>(
        &'file self,
        fs: &'f mut FatFs<S, CS, Ev>,
        storage: &'s mut S,
    ) -> FileWrapper<'file, 'f, 's, S, CS, Ev>
    where
        S: Storage<Word = u8, SECTOR_SIZE = U512>,
        CS: ArrayLength<RefCell<GenericArray<u8, U512>>>,
        CS: ArrayLength<super::cache::CacheEntry>,
        CS: BitMapLen,
        Ev: EvictionPolicy,
    {
        FileWrapper::from(self, fs, storage)
    }
}

pub struct FileWrapper<'file, 'f, 's, S, CS, Ev>
where
    S: Storage<Word = u8, SECTOR_SIZE = U512>,
    CS: ArrayLength<RefCell<GenericArray<u8, U512>>>,
    CS: ArrayLength<super::cache::CacheEntry>,
    CS: BitMapLen,
    Ev: EvictionPolicy,
{
    pub fs: &'f mut FatFs<S, CS, Ev>,
    pub storage: &'s mut S,

    pub inner: &'file File,
}

impl<'file, 'f, 's, S, CS, Ev> FileWrapper<'file, 'f, 's, S, CS, Ev>
where
    S: Storage<Word = u8, SECTOR_SIZE = U512>,
    CS: ArrayLength<RefCell<GenericArray<u8, U512>>>,
    CS: ArrayLength<super::cache::CacheEntry>,
    CS: BitMapLen,
    Ev: EvictionPolicy,
{
    pub fn from(
        inner: &'file File,
        fs: &'f mut FatFs<S, CS, Ev>,
        storage: &'s mut S,
    ) -> Self {
        Self { inner, fs, storage }
    }


}
