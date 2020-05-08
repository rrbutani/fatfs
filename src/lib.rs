
// Mark the crate as no_std if the feature is enabled (and only)
#![cfg_attr(all(feature = "no_std", not(test)), no_std)]

