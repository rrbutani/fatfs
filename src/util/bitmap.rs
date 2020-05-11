//! Home of `BitMap`.

use super::Bits;

use generic_array::{ArrayLength, GenericArray};
use typenum::consts::{U8, U1};
use typenum::marker_traits::Unsigned;

use core::ops::{Add, Div};
use core::marker::PhantomData;

pub trait BitMapLen
where
    Self: Div<U8>,
    Self: Unsigned,
    // Unfortunately if we put these here, users of this trait have to prove
    // that their type satisfies these constraints instead of the type being
    // implicitly required to meet these constraints. This defeats the purpose
    // of having this trait in the first place so instead we have a blanket impl
    // with these requirements and we make this a sealed trait.
    // <Self as Div<U8>>::Output: Add<U1>,
    // <<Self as Div<U8>>::Output as Add<U1>>::Output: ArrayLength<u8>,
    Self: bitmap_len_private::Sealed,
{
    type ArrLen: ArrayLength<u8>;
}

mod bitmap_len_private {
    use super::*;
    pub trait Sealed { }

    impl<T> Sealed for T
    where
        T: Div<U8>,
        // In the case where Len is a multiple of 8 this will waste a byte,
        // which is okay, I think.
        <T as Div<U8>>::Output: Add<U1>,
        <<T as Div<U8>>::Output as Add<U1>>::Output: ArrayLength<U8>,
    { }
}

impl<T: bitmap_len_private::Sealed> BitMapLen for T
where
    T: Div<U8>,
    T: Unsigned,
    // In the case where Len is a multiple of 8 this will waste a byte, which is
    // okay, I think.
    <T as Div<U8>>::Output: Add<U1>,
    <<T as Div<U8>>::Output as Add<U1>>::Output: ArrayLength<u8>,
{
    type ArrLen = <<T as Div<U8>>::Output as Add<U1>>::Output;
}

// A bad version of BitVec, I guess.
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitMap<LEN: BitMapLen> {
    arr: GenericArray<u8, LEN::ArrLen>,

    // Helper variables to speed up some queries:
    length: usize,
    num_free_bits: usize,
    next_free: usize,

    _l: PhantomData<LEN>,
}

#[allow(non_camel_case_types)]
impl<LEN: BitMapLen> BitMap<LEN> {
    pub fn new() -> Self {
        Self {
            arr: Default::default(),

            length: LEN::to_usize(),
            num_free_bits: LEN::to_usize(),
            next_free: 0,

            _l: PhantomData,
        }
    }

    pub fn length(&self) -> usize {
        self.length
    }

    pub fn empty_bits(&self) -> usize {
        self.num_free_bits
    }

    pub fn clear_all(&mut self) {
        // Optimizer, save us.
        for b in 0..self.length() {
            let _ = self.set(b, false).unwrap();
        }
    }

    // Returns `Ok` if in bounds and `Err` otherwise.
    fn in_bounds(&self, bit: usize) -> Result<(), ()> {
        if (0..self.length()).contains(&bit) {
            Ok(())
        } else {
            Err(())
        }
    }

    // Returns `Ok(idx, offset)` if in bounds and `Err` if not in bounds.
    fn bit_to_idx(&self, bit: usize) -> Result<(usize, usize), ()> {
        self.in_bounds(bit).map(|()| {
            ((bit / 8), (bit % 8))
        })
    }

    // Returns `Ok(bool)` if in bounds and `Err` otherwise.
    pub fn get(&self, bit: usize) -> Result<bool, ()> {
        self.bit_to_idx(bit).map(|(idx, offset)| {
            self.arr[idx].b(offset as u32)
        })
    }

    // Returns the previous value of the bit.
    //
    // Returns an `Err` if out of bounds.
    pub fn set(&mut self, bit: usize, val: bool) -> Result<bool, ()> {
        self.bit_to_idx(bit).map(|(idx, offset)| {
            let prev: bool = self.arr[idx].b(offset as u32);
            self.arr[idx].set_bit(offset as u32, val);

            match (prev, val) {
                (false, true) => self.num_free_bits -= 1,
                (true, false) => {
                    self.num_free_bits += 1;
                    self.next_free = bit;
                },

                (true, true) | (false, false) => { },
            }

            prev
        })
    }

    // Returns `Err` if there are no empty bits available.
    pub fn next_empty_bit(&mut self) -> Result<usize, ()> {
        // The only way this get can fail is if the length is 0. If this happens
        // we should return Err since we really do not have any empty bits (or
        // _any_ bits) available. So, the `?` is appropriate here.
        if self.get(self.next_free)? == false {
            return Ok(self.next_free);
        } else {
            // If that didn't work we need to do a sweep.
            if self.num_free_bits == 0 {
                return Err(());
            }

            for b in (self.next_free..self.length()).chain(0..self.next_free) {
                if self.get(b).unwrap() == false {
                    self.next_free = b;
                    return Ok(b);
                }
            }

            Err(())
        }
    }
}

#[cfg(test)]
mod bitmap {
    use super::*;
    use typenum::consts::U31;

    use assert_eq as eq;

    #[test]
    fn basic() {
        let mut b = BitMap::<U31>::new();

        eq!(b.length(), 31);
        eq!(b.empty_bits(), 31);

        // Get:
        for idx in 0..31 {
            eq!(b.get(idx), Ok(false));
        }

        // Get out of range:
        eq!(b.get(32), Err(()));

        // Set all using get next free:
        for _ in 0..31 {
            let idx = b.next_empty_bit();
            assert!(idx.is_ok());

            b.set(idx.unwrap(), true).unwrap();
        }

        // There should be no empty bits now:
        eq!(b.empty_bits(), 0);
        eq!(b.next_empty_bit(), Err(()));

        // Finally, clear them all:
        b.clear_all();
        eq!(b.empty_bits(), 31);
        eq!(b.length(), 31);
    }
}
