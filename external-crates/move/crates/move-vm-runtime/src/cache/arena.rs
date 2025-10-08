// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

//! Memory arena allocator for efficient package data management.
//!
//! This module provides arena-based memory allocation for the Move VM's package cache.
//! Arena allocation is crucial for performance as it allows bulk allocation and deallocation
//! of package data without individual object overhead. When a package is loaded, all its
//! data structures (modules, functions, types) are allocated in a single arena, enabling
//! efficient memory usage and cache-friendly data layout.
//!
//! The arena integrates with the broader VM architecture by:
//! - Supporting the package cache with fast, contiguous allocations
//! - Providing memory limits to prevent runaway package sizes
//! - Enabling zero-cost abstractions over allocated data via [`ArenaVec`] and [`ArenaBox`]

#![allow(unsafe_code)]

use crate::shared::vm_pointer::VMPointer;
use bumpalo::Bump;
use move_binary_format::errors::{PartialVMError, PartialVMResult};
use move_core_types::vm_status::StatusCode;

// -------------------------------------------------------------------------------------------------
// Types - Arenas for Cache Allocations
// -------------------------------------------------------------------------------------------------

/// Memory arena for bulk allocations with configurable size limits.
/// Uses bump allocation for fast, cache-friendly memory management.
pub struct Arena(Bump);

/// An arena-allocated vector. Notably, `Drop` does not drop the elements it holds, as that is the
/// perview of the arena it was allocated in.
#[derive(Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ArenaVec<T>(std::mem::ManuallyDrop<Vec<T>>);

/// An arena-allocated vector. Notably, `Drop` does not drop the value it holds, as that is the
/// perview of the arena it was allocated in.
#[derive(Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ArenaBox<T>(std::mem::ManuallyDrop<Box<T>>);

/// Size of a package arena.
/// This is 10 megabytes, which should be more than enough room for any pacakge on chain.
/// FIXME: Test this limit and validate. See how large packages are in backtesting and double that
/// limit, setting it here.
const ARENA_SIZE: usize = 10_000_000;

// -------------------------------------------------------------------------------------------------
// Impls
// -------------------------------------------------------------------------------------------------

impl Default for Arena {
    /// Creates a new arena with the default size limit (10MB).
    /// This limit prevents individual packages from consuming excessive memory.
    fn default() -> Self {
        let bump = Bump::new();
        bump.set_allocation_limit(Some(ARENA_SIZE));
        Arena(bump)
    }
}

impl Arena {
    /// Allocates a vector in the arena from an iterator of items.
    ///
    /// Takes an exact-size iterator and allocates a contiguous vector in the arena.
    /// Returns an ArenaVec that references the arena-allocated memory.
    ///
    /// # Errors
    /// Returns `PACKAGE_ARENA_LIMIT_REACHED` if allocation would exceed arena limits.
    ///
    /// # Safety
    /// 1. It is the caller's responsibility to ensure that `self` is not shared across threads
    ///    during this call.
    /// 2. This vector is allocated in the arena, and thus even if it is dropped its memory will not
    ///    be reclaimed until the arena is discarded.
    pub fn alloc_vec<T>(
        &self,
        items: impl ExactSizeIterator<Item = T>,
    ) -> PartialVMResult<ArenaVec<T>> {
        let size = items.len();
        if let Ok(slice) = self.0.try_alloc_slice_fill_iter(items) {
            Ok(ArenaVec(unsafe {
                std::mem::ManuallyDrop::new(Vec::from_raw_parts(slice.as_mut_ptr(), size, size))
            }))
        } else {
            Err(PartialVMError::new(StatusCode::PACKAGE_ARENA_LIMIT_REACHED))
        }
    }

    /// Allocates a single item in the arena as a boxed value.
    ///
    /// Takes an item and allocates it in the arena, returning an [`ArenaBox`] handle.
    /// Should only be used for fixed-size types without internal heap allocations.
    ///
    /// # Errors
    /// Returns `PACKAGE_ARENA_LIMIT_REACHED` if allocation would exceed arena limits.
    ///
    /// # Safety
    /// 0. This must never be called on strings (or any other type that contains internal
    ///    allocations or pointers), as they have heap-allocated byte arrays that will be leaked
    ///    when the arena is released. This box is allocated in the arena, and thus even if it is
    ///    dropped, any internal memory will not be reclaimed.
    /// 1. It is the caller's responsibility to ensure that `self` is not shared across threads
    ///    during this call.
    pub fn alloc_box<T>(&self, item: T) -> PartialVMResult<ArenaBox<T>> {
        if let Ok(slice) = self.0.try_alloc(item) {
            Ok(ArenaBox(unsafe {
                std::mem::ManuallyDrop::new(Box::from_raw(slice as *mut T))
            }))
        } else {
            Err(PartialVMError::new(StatusCode::PACKAGE_ARENA_LIMIT_REACHED))
        }
    }
}

impl<T> ArenaVec<T> {
    /// Returns an iterator over references to the vector's elements.
    pub fn iter(&self) -> std::slice::Iter<T> {
        self.0.iter()
    }

    /// Returns an iterator over mutable references to the vector's elements.
    pub(crate) fn iter_mut(&mut self) -> std::slice::IterMut<T> {
        self.0.iter_mut()
    }

    /// Creates an empty ArenaVec without arena allocation.
    /// Useful for representing empty collections without arena overhead.
    pub fn empty() -> Self {
        ArenaVec(std::mem::ManuallyDrop::new(vec![]))
    }

    /// Converts the vector elements to stable VM pointers.
    /// These pointers remain valid for the lifetime of the arena.
    pub fn to_ptrs(&self) -> Vec<VMPointer<T>> {
        self.0
            .iter()
            .map(|val_ref| VMPointer::from_ref(val_ref))
            .collect()
    }
}

impl<T> ArenaBox<T> {
    /// Returns a reference to the boxed value.
    pub fn inner_ref(&self) -> &T {
        &self.0
    }
}

// -------------------------------------------------------------------------------------------------
// Trait Implementations
// -------------------------------------------------------------------------------------------------

// SAFETY: these are okay, if callers follow the documented safety requirements for `Arena`'s
// unsafe methods.

unsafe impl Send for Arena {}
unsafe impl Sync for Arena {}

// -----------------------------------------------
// ArenaVec Trait Implementations
// -----------------------------------------------

impl<T> std::ops::Deref for ArenaVec<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// -----------------------------------------------
// ArenaVec Trait Implementations
// -----------------------------------------------

impl<T> std::ops::Deref for ArenaBox<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
