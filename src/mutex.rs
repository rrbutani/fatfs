//! A common Mutex interface.
//!
//! Nothing here implements poisoning! If you panic while having locked one of
//! these mutexes, no guarantees about what happens next!
//!
//! (We are okay with this because, as configured, we can't recover from panics
//! on embedded anyways — our panic handler just spins forever.)

trait MutexInterface<T>: Sync {
    fn new(inner: T) -> Self;

    // Run a function n a critical section:
    fn cs<F: FnOnce(&mut T) -> R, R>(&self, func: F) -> R;

    // Get mutable access to the inner data *using a mutable reference*.
    // Since Rust can statically prove that we have exclusive access in this
    // case, no locking occurs.
    //
    // Other Mutex impls (like `bare_metal::Mutex`) provide this functionality
    // using the `BorrowMut` trait; we choose to have this function exist in
    // this trait as well because we implement this trait for foreign types like
    // `std::sync::Mutex` that do not implement `BorrowMut` in this manner (for
    // good reason since locking isn't _technically_ infallible for std Mutexes;
    // for us though, because we've chosen not to care about poisoning, the
    // distinction can be ignored).
    fn get_mut(&mut self) -> &mut T;
}

#[cfg(not(feature = "no_std"))]
pub mod from_std {
    use super::MutexInterface;

    pub use std::sync::Mutex;

    impl<T: Send> MutexInterface<T> for Mutex<T> {
        fn new(inner: T) -> Self {
            Mutex::new(inner)
        }

        #[inline]
        fn cs<F: FnOnce(&mut T) -> R, R>(&self, func: F) -> R {
            let mut inner = self.lock().unwrap();

            func(&mut *inner)
        }

        #[inline]
        fn get_mut(&mut self) -> &mut T {
            self.get_mut().unwrap()
        }
    }
}

#[cfg(feature = "external_mutex")]
pub mod external_mutex {
    use super::MutexInterface;

    use core::ptr;
    use core::cell::Cell;

    // Represents an opaque type on the C side.
    #[repr(C)] pub struct TcbList { _priv: [u8; 0] }

    #[repr(C)]
    pub struct Semaphore {
        locked: u8,
        blocked: *mut TcbList,
    }

    extern "C" {
        pub fn semaphore_init(s: *mut Semaphore, locked: u8);
        pub fn semaphore_wait(s: *mut Semaphore);
        pub fn semaphore_signal(s: *mut Semaphore);
    }

    pub struct Mutex<T> {
        semaphore: Cell<Semaphore>,
        inner: Cell<T>,
    }

    impl<T: Send> MutexInterface<T> for Mutex<T> {
        fn new(inner: T) -> Self {
            let semaphore = Cell::new(Semaphore {
                locked: 0,
                blocked: ptr::null::<TcbList>() as *mut TcbList,
            });

            unsafe { semaphore_init(semaphore.as_ptr(), 0); }

            Self {
                semaphore,
                inner: Cell::new(inner),
            }
        }

        #[inline]
        fn cs<F: FnOnce(&mut T) -> R, R>(&self, func: F) -> R {
            unsafe { semaphore_wait(self.semaphore.as_ptr()); }

            let res = func(unsafe { &mut *self.inner.as_ptr() });

            unsafe { semaphore_signal(self.semaphore.as_ptr()); }

            res
        }

        #[inline]
        fn get_mut(&mut self) -> &mut T {
            self.inner.get_mut()
        }
    }

    // It's Sync! The people who implemented the Semaphore promised!
    unsafe impl<T> Sync for Mutex<T> where T: Send { }
}

// We exclude this when external is enabled so that non-cortex M ARM users can
// still build this crate: cortex_m should compile for them but it will not
// actually provide the functions that we use below.
//
// Unfortunately, users in this situation will get a cryptic error about the
// cortex_m crate not having certain functions. In order to use this crate, such
// users must enable the "external_mutex" feature and provide their own Mutex
// impl using the FFI functions.
//
// A current consequence of the feature configuration is that even if you intend
// to use your own Mutex implemented in Rust that implements the MutexInterface
// trait, you must satisfy at least one of the built-in Mutexes (std,
// bare-metal cortex-m, or external). If there's a need, in the future this
// shortcoming can be remedied by having a default stub Mutex that just crashes
// (TODO).
#[cfg(all(target_arch = "arm"))]
pub mod bare_metal {
    use super::MutexInterface;

    use core::cell::Cell;

    use bare_metal::CriticalSection;
    use cortex_m::interrupt;

    // // Unfortunately, the `bare_metal::Mutex` does not provide a way for us to
    // // lean on our static guarantees when we have a `&mut Self`, so we basically
    // // go and reconstruct it here:
    // Update: this is not true; it does so via BorrowMut.

    // Unfortunately, the `bare_metal::Mutex` does not provide us with a mutable
    // reference to the type it wraps so we basically go and reconstruct it
    // here:

    pub struct Mutex<T> {
        inner: Cell<T>,
    }

    // Taken from `bare_metal`:
    impl<T> Mutex<T> {
        /// Borrows the data for the duration of the critical section.
        #[inline]
        pub fn borrow<'cs>(&'cs self, _cs: &'cs CriticalSection) -> &'cs mut T {
            unsafe { &mut *self.inner.as_ptr() }
        }
    }

    impl<T: Send> MutexInterface<T> for Mutex<T> {
        fn new(value: T) -> Self {
            Mutex {
                inner: Cell::new(value),
            }
        }

        #[inline]
        fn cs<F: FnOnce(&mut T) -> R, R>(&self, func: F) -> R {
            interrupt::free(|cs| {
                func(self.borrow(cs))
            })
        }

        #[inline]
        fn get_mut(&mut self) -> &mut T {
            self.inner.get_mut()
        }
    }

    // As with the actual `bare_metal::Mutex`:
    unsafe impl<T> Sync for Mutex<T> where T: Send {}
}

//  ARM  | no_std | no bindings | → default mutex = ((cortex-m) bare_metal or error), or external (on feat)
//  ARM  | no_std |    bindings | → default mutex = ((cortex-m) bare_metal or error), or external (on feat)
//  ARM  |    std | no bindings | → default mutex = std, or external (on feat)
//  ARM  |    std |    bindings | → default mutex = std, or external (on feat)
// Other |    std | no bindings | → default mutex = std, or external (on feat)
// Other |    std |    bindings | → default mutex = std, or external (on feat)
// Other | no_std | no bindings | → default mutex = error, external (on feat)
// Other | no_std |    bindings | → default mutex = error, external (on feat)

cfg_if::cfg_if! {
    if #[cfg(feature = "external_mutex")] {
        pub use external_mutex::Mutex;
    } else if #[cfg(all(target_arch = "arm", feature = "no_std"))] {
        pub use bare_metal::Mutex;
    } else if #[cfg(not(feature = "no_std"))] {
        pub use from_std::Mutex;
    } else if #[cfg(feature = "no_std")] {
        compile_error!("Please enable the `external-mutex` feature and provide \
            a Mutex implementation.");
    } else {
        compile_error!("Unreachable!!");
    }
}
