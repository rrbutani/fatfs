
use core::fmt::Debug;

use generic_array::{ArrayLength, GenericArray};
use typenum::marker_traits::Unsigned;

// TODO: update this to be an extension to the Storage trait.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WriteError<T> {
    /// For calls to `write_bytes` or `write_sector` that fall outside of the
    /// partition's space. The requested_offset (/ the sector size) must be
    /// greater than `Storage::SECTOR_SIZE` (i.e. out of range).
    OutOfRange { requested_offset: usize },
    Other(T),
}

impl<T> From<T> for WriteError<T> {
    fn from(other: T) -> Self {
        WriteError::Other(other)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ReadError<T> {
    /// For when requested data has not been written to before.
    ///
    /// Implementations can choose to simply return 0s instead of returning this
    /// error in such cases.
    Uninitialized { offset: usize },
    /// For when calls to `read_bytes` or `read_sector` fall outside of the
    /// medium's space. The requested_offset (/ the sector size) must be
    /// greater than `Storage::SECTOR_SIZE` (i.e. out of range).
    OutOfRange { requested_offset: usize },
    Other(T),
}

impl<T> From<T> for ReadError<T> {
    fn from(other: T) -> Self {
        ReadError::Other(other)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EraseError<W, T> {
    ErrorInIndividualErase(WriteError<W>),
    Other(T),
}

impl<W, T> From<T> for EraseError<W, T> {
    fn from(other: T) -> Self {
        EraseError::Other(other)
    }
}

// Specialization, where art thou!??
// impl<W, T> From<WriteError<W>> for EraseError<W, T> {
//     fn from(write_err: WriteError<W>) -> Self {
//         EraseError::ErrorInIndividualErase(write_err)
//     }
// }

/// Implementors of this trait provide access to a partition on some sector
/// based storage medium.
pub trait Storage {
    #[allow(non_camel_case_types)]
    type SECTOR_SIZE: ArrayLength<u8>;

    type ReadErr: Debug;
    type WriteErr: Debug;
    type EraseErr: Debug;

    /// Reads in some chunk of data. There is no guarantee that the requested
    /// chunk is aligned to a sector or smaller than a sector.
    ///
    /// This function should never panic but can return errors for the
    /// appropriate cases (i.e. out of range).
    fn read_bytes(
        &mut self,
        offset: usize,
        buffer: &mut [u8],
    ) -> Result<(), ReadError<Self::ReadErr>>;

    /// Reads in an entire sector.
    ///
    /// This has a default implementation that just calls `read_bytes`;
    /// implementations that are able to do better for their specific medium
    /// should provide their own (better) implementation.
    ///
    /// Alternatively, if an implementation detects when calls to `read_byte`
    /// can take advantage of entire sector reads, there is no need to override
    /// this function; the default implementation will also benefit from this.
    #[inline]
    fn read_sector(
        &mut self,
        sector_idx: usize,
        buffer: &mut GenericArray<u8, Self::SECTOR_SIZE>,
    ) -> Result<(), ReadError<Self::ReadErr>> {
        self.read_bytes(sector_idx * Self::SECTOR_SIZE::to_usize(), buffer.as_mut_slice())
    }

    // /// Writes some chunk of data. There is no guarantee that the given chunk
    // /// is aligned to a sector or smaller than a sector.
    // ///
    // /// This function should never panic but can return errors for the
    // /// appropriate cases (i.e. out of range).
    // fn write_bytes(
    //     &mut self,
    //     offset: usize,
    //     buffer: &[u8],
    // ) -> Result<(), WriteError<Self::WriteErr>>;

    // /// Writes out an entire sector.
    // ///
    // /// This has a default implementation that just calls `write_bytes`;
    // /// implementations that are able to do better for their specific medium
    // /// should provide their own (better) implementation.
    // ///
    // /// Alternatively, as with `Storage::read_sector` if an implementation
    // /// detects when calls to `write_byte` can take advantage of entire sector
    // /// writes, there is no need to override this function; the default
    // /// implementation will also benefit from this.
    // #[inline]
    // fn write_sector(
    //     &mut self,
    //     sector_idx: usize,
    //     buffer: &GenericArray<u8, Self::SECTOR_SIZE>,
    // ) -> Result<(), WriteError<Self::WriteErr>> {
    //     self.write_bytes(sector_idx * Self::SECTOR_SIZE::to_usize(), buffer.as_slice())
    // }

    /// Writes out an entire sector. Note that this function takes a sector
    /// index rather than an offset.
    ///
    /// This function should never panic but can return errors for the
    /// appropriate cases (i.e. `sector_idx` >= `self.sector_count()`).
    fn write_sector(
        &mut self,
        sector_idx: usize,
        buffer: &GenericArray<u8, Self::SECTOR_SIZE>,
    ) -> Result<(), WriteError<Self::WriteErr>>;

    /// Returns the number of sectors in the partition
    fn sector_count(&self) -> usize;

    /// Returns one greater than the largest valid offset for the partition.
    fn byte_count(&self) -> usize {
        self.sector_count() * Self::SECTOR_SIZE::to_usize()
    }

    /// Erases the entire partition.
    ///
    /// This has a default impl that just calls `write` with 0s across all the
    /// sectors in the partition. Mediums that have a more efficient way to
    /// erase themselves can provide their own implementations of this.
    #[inline]
    fn erase(&mut self) -> Result<(), EraseError<Self::WriteErr, Self::EraseErr>> {


        for idx in 0..self.sector_count() {
            self.write_sector(idx, )
        }
    }
}

using_std! {
    use std::fmt;

    macro_rules! display_using_debug {
        ($ty:ty) => { impl<T: fmt::Debug> Display for $ty<T> {
            fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
                Debug::fmt(fmt)
            }
        }};
    }

    macro_rules! err {
        ($ty:ty) => {
            display_using_debug!($ty);

            impl<T: Debug> std::error::Error for $ty<T> { }
        };
    }

    err!(WriteError);
    err!(ReadError);
    err!(EraseError);
}
