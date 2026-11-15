//! Memory allocation APIs based on the Storage Performance Development Kit's memory allocators.
mod dma;

use std::{alloc::AllocError, ptr::NonNull};

pub use dma::DmaAllocator;

/// A type alias for the standard library's result type for the [`Allocator`] trait.
///
/// [`Allocator`]: https://doc.rust-lang.org/stable/std/alloc/trait.Allocator.html
pub type AllocResult = std::result::Result<NonNull<[u8]>, AllocError>;
