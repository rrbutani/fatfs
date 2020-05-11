//! Home of the `Bits` trait.

// TODO: Grab the full version of this from lc3_isa and make it it's own crate.
// (add in the set functionality here, unify the Sized + Copy stuff by putting
// those bounds only on the functions that need them)
pub trait Bits {
    fn bit(&self, b: u32) -> bool;
    fn b(&self, b: u32) -> bool { self.bit(b) }

    fn set_bit(&mut self, b: u32, v: bool);
}

impl Bits for u8 {
    fn bit(&self, b: u32) -> bool {
        ((*self >> b) & 1) == 1
    }

    fn set_bit(&mut self, b: u32, v: bool) {
        *self = (*self & !(1 << b)) | (((v as Self) << b) as Self);
    }
}

#[cfg(test)]
mod bits {
    use super::*;
    use assert_eq as eq;

    #[test]
    fn get() {
        let a: u8 =  0b1010_0101;
        const T: bool = true;
        const F: bool = false;

        eq!(T, a.b(7));
        eq!(F, a.b(6));
        eq!(T, a.b(5));
        eq!(F, a.b(4));
        eq!(F, a.b(3));
        eq!(T, a.b(2));
        eq!(F, a.b(1));
        eq!(T, a.b(0));
    }

    #[test]
    #[should_panic]
    fn out_of_range() {
        let _ = 78u8.b(8);
    }

    #[test]
    fn set() {
        let mut a: u8 = 0b0000000;

        a.set_bit(0, true);
        eq!(a, 1);

        a.set_bit(0, false);
        eq!(a, 0);
    }
}
