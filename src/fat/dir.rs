//! Directory entries and files.

use super::table::Cluster;
use super::FatFs;
use crate::Storage;

use generic_array::GenericArray;
use typenum::consts::U512;


use core::fmt::{self, Debug};
use core::convert::TryInto;
use core::iter::Iterator;

pub enum Attribute {
    ReadOnly = 0x01,
    Hidden = 0x02,
    System = 0x04,
    VolumeId = 0x08,
    Directory = 0x10,
    Archive = 0x20,
}

#[repr(transparent)]
#[derive(Debug, Default)]
pub struct AttributeSet {
    inner: u8
}

impl AttributeSet {
    pub fn is_dir(&self) -> bool {
        (self.inner & (Attribute::Directory as u8)) != 0
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


#[derive(Debug, Default)]
pub struct DirEntry {
    // Offset: 00
    file_name: FileName,
    // Offset: 08
    file_ext: FileExt,
    // Offset: 11
    attributes: AttributeSet,
    // Offset: 12
    _win_nt: u8,
    // Offset: 13
    creation_time_tenth_secs: u8,
    // Offset: 14
    creation_time_double_secs: u16,
    // Offset: 16
    creation_date: u16,
    // Offset: 18
    last_access_date: u16,
    // Offset: 20
    cluster_num_upper: u16,
    // Offset: 22
    last_modif_time: u16,
    // Offset: 24
    last_modif_date: u16,
    // Offset: 26
    cluster_num_lower: u16,
    // Offset: 28
    file_size: u32,
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

    pub fn new_file(name: FileName, ext: FileExt, cluster_num: Cluster) -> Self {
        let mut d = Self::default();

        d.file_name = name;
        d.file_ext = ext;
        d.set_cluster_num(cluster_num);

        d
    }

    pub fn new_dir(name: FileName, cluster_num: Cluster) -> Self {
        let mut d = Self::default();

        d.file_name = name;
        d.set_cluster_num(cluster_num);
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

    pub fn into_arr(&self, arr: &mut [u8; 32]) -> Self {
        // TODO!
        todo!()
    }

    pub fn cluster_num(&self) -> Cluster {
        (self.cluster_num_upper as u32) << 16 | (self.cluster_num_lower as u32)
    }

    pub fn set_cluster_num(&mut self, c: Cluster) {
        let upper = (c >> 16) as u16;
        let lower = c as u16;

        self.cluster_num_upper = upper;
        self.cluster_num_lower = lower;
    }

    // `None` if this is not a directory.
    pub fn into_dir_iter<'f, 's, 'a, S: Storage<Word = u8, SECTOR_SIZE = U512>>(&self, fs: &'f mut FatFs<S>, s: &'s mut S, a: &'a mut GenericArray<u8, U512>) -> Option<DirIter<'f, 's, 'a, S>> {
        if self.attributes.is_dir() {
            Some(DirIter::from_cluster(self.cluster_num(), fs, s, a))
        } else {
            None
        }
    }
}

pub struct DirIter<'f, 's, 'a, S: Storage<Word = u8, SECTOR_SIZE = U512>> {
    pub file_sys: &'f mut FatFs<S>,
    pub storage: &'s mut S,

    pub current_cluster: Cluster,
    pub current_offset: Option<u32>,

    hit_end_offset: Option<u32>,

    pub current_cached_sector_idx: Option<u64>,
    pub sector: &'a mut GenericArray<u8, U512>,
}

impl<'f, 's, 'a, S: Storage<Word = u8, SECTOR_SIZE = U512>> DirIter<'f, 's, 'a, S> {
    pub fn from_cluster(cluster: Cluster, fs: &'f mut FatFs<S>, storage: &'s mut S, array: &'a mut GenericArray<u8, U512>) -> Self {
        Self {
            file_sys: fs,
            storage,

            current_cluster: cluster,
            current_offset: Some(0),

            hit_end_offset: None,

            current_cached_sector_idx: None,
            sector: array,
        }
    }

    // TODO: support directories that are larger than a cluster!
    pub fn add_entry(&mut self, entry: DirEntry) -> Result<(), ()> {
        let bytes_in_a_cluster = (self.file_sys.cluster_size_in_sectors as u32) * (self.file_sys.sector_size_in_bytes as u32);

        if let Some(end) = self.hit_end_offset.take() {
            if end + 64 >= bytes_in_a_cluster {
                unimplemented!()
                // We'd need to go call grow_file...
            } else {
                entry.into_arr(&mut self.sector[end as usize..(end + 32) as usize].try_into().unwrap());

                let terminator = DirEntry::empty();
                terminator.into_arr(&mut self.sector[(end + 32) as usize..(end + 64) as usize].try_into().unwrap());

                self.current_offset = Some(end);
                Ok(())
            }
        } else {
            Err(())
        }
    }
}

impl<'f, 's, 'a, S: Storage<Word = u8, SECTOR_SIZE = U512>> Iterator for DirIter<'f, 's, 'a, S> {
    type Item = DirEntry;

    fn next(&mut self) -> Option<DirEntry> {
        if let Some(offset) = self.current_offset {
            let mut fet = super::table::FatEntryTracer::starting_at(&mut self.file_sys, self.storage, self.sector, dbg!(self.current_cluster)).unwrap();

            let (c, _) = Iterator::next(&mut &mut fet).unwrap();

            let mut buf = [0u8; 32];
            super::table::FatEntryWrapper::from(c, &mut fet).read(offset as u16, &mut buf).unwrap();

            let entry = DirEntry::from_arr(buf);

            if let State::End = entry.state() {
                self.hit_end_offset = Some(offset);
                self.current_offset = None;
            } else {
                self.current_offset = Some(offset + 32);
            }

            Some(entry)
        } else {
            None
        }
    }
}
