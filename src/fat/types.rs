//! Some type definitions for the driver.

use core::convert::TryInto;
use core::iter::Iterator;
use core::ops::Range;

macro_rules! newtype {
    ([$m:ident] $name:tt: $inner:ty $(where constructor = $c:ident)?) => {
        #[doc(hidden)]
        #[allow(non_snake_case)]
        pub mod $m {
            use core::ops::{Deref, DerefMut};

            // TODO: make the debug impl print out the name of the wrapper and the
            // inner type.

            // Doing this gives us bounded impls for this traits for free (i.e.
            // `Newtype<Inner>` will be `Copy` only if `Inner` is `Copy`.)
            //
            // We need one of these per newtype so that the type alias actually
            // does point to a unique type; otherwise (for example) two `u64`
            // newtypes would both be aliased to `Newtype<u64>`.
            #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
            #[repr(transparent)]
            #[doc(hidden)]
            pub struct Newtype<Inner>(pub(in super) Inner);

            impl<Inner> Deref for Newtype<Inner> {
                type Target = Inner;

                #[inline]
                fn deref(&self) -> &Inner { &self.0 }
            }

            impl<Inner> DerefMut for Newtype<Inner> {
                #[inline]
                fn deref_mut(&mut self) -> &mut Inner { &mut self.0 }
            }
        }

        pub type $name = $m::Newtype<$inner>;

        impl $name {
            pub fn inner(&self) -> &$inner { &**self }
        }

        $(
            impl $name {
                pub const fn $c(inner: $inner) -> Self {
                    Self(inner)
                }
            }
        )?
    };
}

newtype!{ [_s] SectorIdx: u64 where constructor = new }
newtype!{ [_c] ClusterIdx: u32 where constructor = new }

impl SectorIdx {
    pub fn idx(&self) -> usize {
        self.0.try_into().unwrap()
    }
}

impl ClusterIdx {
    pub fn sector_offset(&self, sectors_in_a_cluster: u8) -> SectorIdx {
        SectorIdx::new((self.0 as u64) * (sectors_in_a_cluster as u64))
    }
}

newtype! { [_sr] SectorRange: Range<SectorIdx> where constructor = new }

impl Iterator for SectorRange {
    type Item = SectorIdx;

    #[inline]
    fn next(&mut self) -> Option<SectorIdx> {
        if self.start < self.end {
            if let Some(n) = self.start.inner().checked_add(1) {
                let old = self.start;
                self.start = SectorIdx::new(n);
                Some(old)
            } else {
                None
            }
        } else {
            None
        }
    }
}
