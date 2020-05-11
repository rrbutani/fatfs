
// Mark the crate as no_std if the feature is enabled (and only)
#![cfg_attr(all(feature = "no_std", not(test)), no_std)]

#[allow(unused_extern_crates)]
extern crate core; // makes rls actually look into the standard library (hack)

// // Gotta do this since we're a staticlib:
// // (it'd be nicer to be able to use `panic_halt` or its ilk, but alas)

#[cfg_attr(target_os = "none", panic_handler)]
#[cfg(target_os = "none")]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

macro_rules! using_std { ($($i:item)*) => ($(#[cfg(not(feature = "no_std"))]$i)*) }


#[cfg(feature = "bindings")]
pub mod bindings;

pub mod mutex;
use mutex::Mutex;

use storage_traits::Storage;

pub mod gpt;
pub mod fat;

pub mod util;
