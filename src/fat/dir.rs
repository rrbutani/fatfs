//! Directory entries. Files or Folders.

use crate::Storage;
use super::FatFs;
use super::types::{ClusterIdx, SectorIdx};
use super::cache::EvictionPolicy;
use super::table::FatEntry;
use super::file::File;

use generic_array::{ArrayLength, GenericArray};
use typenum::consts::U512;

use core::cell::RefCell;
use core::convert::TryInto;
use core::fmt::{self, Debug};
use core::iter::Iterator;

#[derive(Debug)]
pub enum Attribute {
    ReadOnly = 0x01,
    Hidden = 0x02,
    System = 0x04,
    VolumeId = 0x08,
    Directory = 0x10,
    Archive = 0x20,
}

impl From<Attribute> for u8 {
    fn from(a: Attribute) -> u8 {
        use Attribute::*;
        match a {
            ReadOnly => 0x01,
            Hidden => 0x02,
            System => 0x04,
            VolumeId => 0x08,
            Directory => 0x10,
            Archive => 0x20,
        }
    }
}

// impl From<u8> for Option<Attribute> {
impl Attribute {
    fn from(u: u8) -> Option<Attribute> {
        use Attribute::*;
        match u {
            0x01 => Some(ReadOnly),
            0x02 => Some(Hidden),
            0x04 => Some(System),
            0x08 => Some(VolumeId),
            0x10 => Some(Directory),
            0x20 => Some(Archive),
            _ => None,
        }
    }
}

impl Attribute {
    fn fmt_attributes(attrs: &AttributeSet, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Attribute::*;

        write!(fmt, "{{ ")?;

        let mut first = true;
        for i in 0..8 {
            let mask = 1 << i;

            if (attrs.inner & mask) != 0 {
                match Attribute::from(mask) {
                    a @ Some(ReadOnly) |
                    a @ Some(Hidden) |
                    a @ Some(System) |
                    a @ Some(VolumeId) |
                    a @ Some(Directory) |
                    a @ Some(Archive) => {
                        if !first { write!(fmt, ", ")? }
                        write!(fmt, "{:?}", a)?;
                    },
                    None => { },
                }

                if first {
                    first = false;
                }
            }
        }

        write!(fmt, " }}")
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct AttributeSet {
    inner: u8
}

impl AttributeSet {
    pub const LFN: AttributeSet = AttributeSet::new()
        .apply(Attribute::ReadOnly)
        .apply(Attribute::Hidden)
        .apply(Attribute::System)
        .apply(Attribute::VolumeId);
}

impl Debug for AttributeSet {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "{} ", core::any::type_name::<Self>())?;
        Attribute::fmt_attributes(self, fmt)
    }
}

impl AttributeSet {
    pub const fn new() -> Self {
        Self { inner: 0 }
    }

    pub const fn apply(mut self, a: Attribute) -> Self {
        self.inner |= a as u8;
        self
    }

    pub fn is_dir(&self) -> bool {
        (self.inner & (Attribute::Directory as u8)) != 0
    }

    pub fn is_file(&self) -> bool {
        (self.inner & (Attribute::Archive as u8)) != 0
    }
}

#[repr(transparent)]
#[derive(Clone, PartialEq, Eq, Default)]
pub struct FileName([u8; 8]);

impl Debug for FileName {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        for i in self.0.iter() {
            if *i == 0x20 || *i == 0x00 {
                return Ok(())
            } else {
                write!(fmt, "{}", *i as char)?;
            }
        }

        Ok(())
    }
}

impl FileName {
    // Just discards extra/non-ascii characters.
    pub fn new(s: &str) -> Self {
        Self(if s.chars().any(|c| !c.is_ascii()) {
            [0; 8]
        } else {
            s.as_bytes()[0..8].try_into().unwrap()
        })
    }
}

#[repr(transparent)]
#[derive(Clone, PartialEq, Eq, Default)]
pub struct FileExt([u8; 3]);

impl Debug for FileExt {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        for i in self.0.iter() {
            if *i == 0x20 || *i == 0x00 {
                return Ok(())
            } else {
                write!(fmt, "{}", *i as char)?;
            }
        }

        Ok(())
    }
}

impl FileExt {
    // Just discards extra/non-ascii characters.
    pub fn new(s: &str) -> Self {
        Self(if s.chars().any(|c| !c.is_ascii()) {
            [0; 3]
        } else {
            s.as_bytes()[0..3].try_into().unwrap()
        })
    }
}


#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DirEntry {
    // Offset: 00
    pub file_name: FileName,
    // Offset: 08
    pub file_ext: FileExt,
    // Offset: 11
    pub attributes: AttributeSet,
    // Offset: 12
    _win_nt: u8,
    // Offset: 13
    pub creation_time_tenth_secs: u8,
    // Offset: 14
    pub creation_time_double_secs: u16,
    // Offset: 16
    pub creation_date: u16,
    // Offset: 18
    pub last_access_date: u16,
    // Offset: 20
    pub cluster_num_upper: u16,
    // Offset: 22
    pub last_modif_time: u16,
    // Offset: 24
    pub last_modif_date: u16,
    // Offset: 26
    pub cluster_num_lower: u16,
    // Offset: 28
    pub file_size: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Exists,
    Deleted,
    End,
}

impl DirEntry {
    pub fn state(&self) -> State {
        match self.file_name.0[0] {
            0x00 => State::End,
            0xE5 => State::Deleted,
            _ => State::Exists,
        }
    }

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn new_file(name: FileName, ext: FileExt, cluster_idx: ClusterIdx) -> Self {
        let mut d = Self::default();

        d.file_name = name;
        d.file_ext = ext;
        d.set_cluster_idx(cluster_idx);
        d.attributes.inner |= Attribute::Archive as u8;

        d
    }

    pub fn new_dir(name: FileName, cluster_idx: ClusterIdx) -> Self {
        let mut d = Self::default();

        d.file_name = name;
        d.set_cluster_idx(cluster_idx);
        d.attributes.inner |= Attribute::Directory as u8;

        d
    }

    pub fn from_arr(arr: [u8; 32]) -> Self {
        macro_rules! e {
            ($ty:tt, $offset:literal :+ $num:literal) => {
                $ty::from_le_bytes(arr[$offset..($offset + $num)].try_into().unwrap())
            };

            ($ty:tt, $offset:literal) => {
                $ty::from_le_bytes(arr[$offset..($offset + core::mem::size_of::<$ty>())].try_into().unwrap())
            };
        }

        Self {
            file_name: FileName(arr[0..8].try_into().unwrap()),
            file_ext: FileExt(arr[8..11].try_into().unwrap()),
            attributes: AttributeSet { inner: arr[11] },
            _win_nt: arr[12],
            creation_time_tenth_secs: arr[13],
            creation_time_double_secs: e!(u16, 14),
            creation_date: e!(u16, 16),
            last_access_date: e!(u16, 18),
            cluster_num_upper: e!(u16, 20),
            last_modif_time: e!(u16, 22),
            last_modif_date: e!(u16, 24),
            cluster_num_lower: e!(u16, 26),
            file_size: e!(u32, 28),
        }
    }

    pub fn from_slice(slice: &[u8]) -> Result<Self, ()> {
        Ok(Self::from_arr(slice.try_into().map_err(|_| ())?))
    }

    pub fn into_arr(&self, arr: &mut [u8; 32]) {
        arr[0..8].copy_from_slice(&self.file_name.0);
        arr[8..11].copy_from_slice(&self.file_ext.0);
        arr[11] = self.attributes.inner;
        arr[12] = self._win_nt;
        arr[13] = self.creation_time_tenth_secs;
        arr[14..16].copy_from_slice(&self.creation_time_double_secs.to_le_bytes());
        arr[16..18].copy_from_slice(&self.creation_date.to_le_bytes());
        arr[18..20].copy_from_slice(&self.last_access_date.to_le_bytes());
        arr[20..22].copy_from_slice(&self.cluster_num_upper.to_le_bytes());
        arr[22..24].copy_from_slice(&self.last_modif_time.to_le_bytes());
        arr[24..26].copy_from_slice(&self.last_modif_date.to_le_bytes());
        arr[26..28].copy_from_slice(&self.cluster_num_lower.to_le_bytes());
        arr[28..32].copy_from_slice(&self.file_size.to_le_bytes());
    }

    pub fn cluster_idx(&self) -> ClusterIdx {
        ClusterIdx::new((self.cluster_num_upper as u32) << 16 | (self.cluster_num_lower as u32))
    }

    pub fn set_cluster_idx(&mut self, c: ClusterIdx) {
        let c = *c.inner();

        let upper = (c >> 16) as u16;
        let lower = c as u16;

        self.cluster_num_upper = upper;
        self.cluster_num_lower = lower;
    }

    // `None` if this is not a directory.
    pub fn into_dir_iter<'f, 's, S, CS, Ev>(
        &self,
        fs: &'f mut FatFs<S, CS, Ev>,
        s: &'s mut S,
    ) -> Option<DirIter<'f, 's, S, CS, Ev>>
    where
        S: Storage<Word = u8, SECTOR_SIZE = U512>,
        CS: ArrayLength<RefCell<GenericArray<u8, U512>>>,
        CS: ArrayLength<super::cache::CacheEntry>,
        CS: crate::util::BitMapLen,
        Ev: EvictionPolicy,
    {
        if self.attributes.is_dir() {
            Some(DirIter::from_cluster(self.cluster_idx(), fs, s))
        } else {
            None
        }
    }

    // `Err` if this is not a file.
    pub fn into_file(self) -> Result<File, Self> {
        if self.attributes.is_file() {
            Ok(File::new(self))
        } else {
            Err(self)
        }
    }
}

pub struct DirIter<'f, 's, S, CS, Ev>
where
    S: Storage<Word = u8, SECTOR_SIZE = U512>,
    CS: ArrayLength<RefCell<GenericArray<u8, U512>>>,
    CS: ArrayLength<super::cache::CacheEntry>,
    CS: crate::util::BitMapLen,
    Ev: EvictionPolicy,
{
    pub file_sys: &'f mut FatFs<S, CS, Ev>,
    pub storage: &'s mut S,

    pub current_cluster: ClusterIdx,
    pub current_offset: Option<u32>,

    hit_end_offset: Option<u32>,
}

impl<'f, 's, S, CS, Ev> DirIter<'f, 's, S, CS, Ev>
where
    S: Storage<Word = u8, SECTOR_SIZE = U512>,
    CS: ArrayLength<RefCell<GenericArray<u8, U512>>>,
    CS: ArrayLength<super::cache::CacheEntry>,
    CS: crate::util::BitMapLen,
    Ev: EvictionPolicy,
{
    pub fn from_cluster(
        cluster: ClusterIdx,
        fs: &'f mut FatFs<S, CS, Ev>,
        storage: &'s mut S
    ) -> Self {
        Self {
            file_sys: fs,
            storage,

            current_cluster: cluster,
            current_offset: Some(0),

            hit_end_offset: None,
        }
    }

    // TODO: support growing directories to more clusters!
    //
    // This only works if the iterator hit the end of a directory structure.
    pub fn add_entry(&mut self, entry: DirEntry) -> Result<(), ()> {
        let bytes_in_a_cluster = self.file_sys.bytes_in_a_cluster();

        if let Some(end) = self.hit_end_offset.take() {
            if end + 64 >= bytes_in_a_cluster {
                unimplemented!()
                // We'd need to go call grow_file...
            } else {
                let f = FatEntry::from(self.current_cluster);
                let mut t = f.upgrade(self.file_sys, self.storage);

                // Write the new entry in the current end location:
                let mut buf = [0u8; 32];
                entry.into_arr(&mut buf);

                t.write(end, buf.iter().cloned()).unwrap();

                // TODO: in the past we actually just called `into_arr` straight
                // on the cached array; I wonder if there's performance gains to
                // be had from exposing that as the API. This is still very
                // doable right here by calling `self.fs.cache.upgrade` but it
                // opens up some edge cases (i.e. access across sectors).

                // Next, write a new terminator entry after the added entry:
                let terminator = DirEntry::empty();
                terminator.into_arr(&mut buf);

                t.write(end + 32, buf.iter().cloned()).unwrap();

                // Finally, restore `current_offset` so the iterator can resume.
                self.current_offset = Some(end);
                Ok(())
            }
        } else {
            Err(())
        }
    }
}

impl<'f, 's, S, CS, Ev> Iterator for DirIter<'f, 's, S, CS, Ev>
where
    S: Storage<Word = u8, SECTOR_SIZE = U512>,
    CS: ArrayLength<RefCell<GenericArray<u8, U512>>>,
    CS: ArrayLength<super::cache::CacheEntry>,
    CS: crate::util::BitMapLen,
    Ev: EvictionPolicy,
{
    type Item = DirEntry;

    fn next(&mut self) -> Option<DirEntry> {
        let entry = if let Some(offset) = self.current_offset {
            let f = FatEntry::from(self.current_cluster);
            let mut t = f.upgrade(self.file_sys, self.storage);

            let mut buf = [0u8; 32];
            t.read(offset, &mut buf).unwrap();
            let entry = DirEntry::from_arr(buf);

            if let State::End = entry.state() {
                self.hit_end_offset = Some(offset);
                self.current_offset = None;
            } else {
                let bytes_in_a_cluster = self.file_sys.bytes_in_a_cluster();
                self.current_offset = Some(if offset + 32 >= bytes_in_a_cluster {
                    let mut tracer = f.trace(self.file_sys, self.storage);
                    self.current_cluster = tracer.next().unwrap().next;

                    (offset + 32) % bytes_in_a_cluster
                } else {
                    offset + 32
                });
            }

            Some(entry)
        } else {
            None
        };

        if let Some(entry) = entry {
            if entry.attributes == AttributeSet::LFN {
                // if so, skip this!
                self.next()
            } else {
                Some(entry)
            }
        } else {
            None
        }
    }
}
