//! C Bindings for this crate.

#[no_mangle]
pub extern "C" fn foo_bar(yo: u8) -> u8 {
    yo * 2
}

#[no_mangle]
pub extern "C" fn yay(yo: u8) -> u8 {
    yo * 2
}
