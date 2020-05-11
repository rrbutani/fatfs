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
        *self = *self | (((v as Self) << b) as Self);
    }
}

#[cfg(test)]
mod bits {
    use super::*;

    #[test]
    fn get() {
        let a: u8 =  0b1010_0101;
        const T: bool = true;
        const F: bool = false;

        assert_eq!(T, a.b(7));
        assert_eq!(F, a.b(6));
        assert_eq!(T, a.b(5));
        assert_eq!(F, a.b(4));
        assert_eq!(F, a.b(3));
        assert_eq!(T, a.b(2));
        assert_eq!(F, a.b(1));
        assert_eq!(T, a.b(0));
    }

    #[test]
    #[should_panic]
    fn out_of_range() {
        let _ = 78u8.b(8);
    }
}
