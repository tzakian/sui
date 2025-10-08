// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

//! Global string interning for Move identifiers.
//!
//! This module implements a global string interner for Move identifiers (function names,
//! type names, etc.). String interning is an important optimization that:
//! - Reduces memory usage by storing each unique string only once
//! - Enables fast string comparisons via integer key comparison
//! - Improves cache locality by replacing scattered string data with compact keys
//!
//! The interner is global and thread-safe, shared across all Move VM instances. This design
//! choice enables:
//! - Reuse of interned strings across multiple VM executions
//! - Amortized interning cost when loading similar packages repeatedly
//! - Consistent memory bounds via configurable maximum identifier slots
//!
//! Integration with the VM architecture:
//! - Used by the package cache to intern all identifiers during package loading
//! - Referenced by the execution engine for fast name lookups
//! - Shared across validation, JIT compilation, and runtime execution phases

use lasso::{Spur, ThreadedRodeo};
use move_binary_format::errors::{PartialVMError, PartialVMResult};
use move_core_types::{
    identifier::{IdentStr, Identifier},
    vm_status::StatusCode,
};
use once_cell::sync::Lazy;
use std::sync::Arc;

// -------------------------------------------------------------------------------------------------
// Global Interner
// -------------------------------------------------------------------------------------------------

/// IDENTIFIER INTERNER
/// The Identifier Interner is global across Move Runtimes and defined here. This is for two reasons:
/// 1. The interner is _always_ a win compared to non-interned identifiers, which hold their
///    strings in boxes. This is always a strict memory win, in all cases. The overall size of the
///    interner plus its definitions is always going to be smaller than holding those individual
///    identifiers.
/// 2. Different runs will benefit from intern reuse: even if the runtime is discarded, interning
///    is a near-constant cost when spinning up a new runtime. Moreover, the interner can be set to
///    have a maximum memory it will refuse to exceed.
///    TODO: Set up this; `lasso` supports it but we need to expose that interface.
/// 3. If absolutely necessary, the execution layer _can_ dump the interner.
static STRING_INTERNER: Lazy<Arc<IdentifierInterner>> =
    Lazy::new(|| Arc::new(IdentifierInterner::default()));

// For msim -- it needs to manually initialize the interner in the setup phase for determinism
// testing.
#[cfg(msim)]
pub fn init_interner() {
    let _ = &*STRING_INTERNER;
}

/// Returns a reference to the global identifier interner.
/// This interner is shared across all VM instances.
fn global_interner() -> Arc<IdentifierInterner> {
    Arc::clone(&STRING_INTERNER)
}

/// Interns an identifier and returns its unique key.
///
/// Takes a Move identifier and returns a compact key for fast comparison.
/// The identifier string is stored only once, regardless of how many times it's interned.
///
/// # Errors
/// Returns `INTERNER_LIMIT_REACHED` if the interner has reached its maximum capacity.
pub(crate) fn intern_identifier(ident: &Identifier) -> PartialVMResult<IdentifierKey> {
    let interner = global_interner();
    interner.get_or_intern_str_internal(ident.borrow_str())
}

/// Interns an identifier string and returns its unique key.
///
/// Similar to `intern_identifier` but takes an `IdentStr` directly.
///
/// # Errors
/// Returns `INTERNER_LIMIT_REACHED` if the interner has reached its maximum capacity.
pub(crate) fn intern_ident_str(ident_str: &IdentStr) -> PartialVMResult<IdentifierKey> {
    let interner = global_interner();
    interner.get_or_intern_str_internal(ident_str.borrow_str())
}

/// Interns an identifier with a custom error message on failure.
///
/// The `key_type` parameter provides context for better error reporting
/// (e.g., "function name", "type name").
///
/// # Errors
/// Returns `INTERNER_LIMIT_REACHED` with a descriptive message including the key type.
pub(crate) fn intern_identifier_with_msg(
    ident: &Identifier,
    key_type: &str,
) -> PartialVMResult<IdentifierKey> {
    let interner = global_interner();
    interner
        .get_or_intern_str_internal(ident.borrow_str())
        .map_err(|err| err.with_message(format!("While attempting to intern {key_type}")))
}

/// Resolves an interned key back to its original identifier.
///
/// Used when the actual string is needed (e.g., for error messages or serialization).
/// The key must exist in the interner; this is an invariant violation if not found.
///
/// # Errors
/// Returns `UNKNOWN_INVARIANT_VIOLATION_ERROR` if the key doesn't exist in the interner.
pub(crate) fn resolve_interned(key: &IdentifierKey, key_type: &str) -> PartialVMResult<Identifier> {
    let interner = global_interner();
    interner.resolve_ident(key, key_type)
}

// -------------------------------------------------------------------------------------------------
// Types
// -------------------------------------------------------------------------------------------------

/// A wrapper around a lasso [`ThreadedRodeo`] with some niceties to make it easier to use in the VM.
#[derive(Debug)]
pub struct IdentifierInterner(ThreadedRodeo);

/// Maximum number of identifiers we can ever intern.
/// FIXME: Set to 1 billion, but should be experimentally determined based on actual run data.
const IDENTIFIER_SLOTS: usize = 1_000_000_000;

// Note: these are not orderable -- their ordering is unstable, so they should not be
// used as keys in any data structure that requires orderability so as to ensure determinism.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
// Testing.
pub(crate) struct IdentifierKey(Spur);

// -------------------------------------------------------------------------------------------------
// -------------------------------------------------------------------------------------------------
// Impls
// -------------------------------------------------------------------------------------------------

impl Default for IdentifierInterner {
    /// Creates a new identifier interner with capacity for up to 1 billion unique identifiers.
    fn default() -> Self {
        let rodeo = ThreadedRodeo::with_capacity(lasso::Capacity::for_strings(IDENTIFIER_SLOTS));
        Self(rodeo)
    }
}

impl IdentifierInterner {
    /// Resolves an interned key to its identifier string.
    /// Private helper that performs the actual resolution with safety checks.
    ///
    /// # Safety
    /// The unsafe code is creating an identifier without checking its validity, but it
    /// was added as a valid identifier to the interner in the first place so does not need to be
    /// checked.
    #[allow(unsafe_code)]
    fn resolve_ident(&self, key: &IdentifierKey, key_type: &str) -> PartialVMResult<Identifier> {
        if let Some(result) = self.0.try_resolve(&key.0) {
            unsafe { Ok(Identifier::new_unchecked(result)) }
        } else {
            Err(
                PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR)
                    .with_message(format!("Failed to find {key_type} key in ident interner.")),
            )
        }
    }

    /// Interns an identifier using this specific interner instance.
    #[allow(dead_code)]
    pub(crate) fn get_or_intern_identifier(
        &self,
        ident: &Identifier,
    ) -> PartialVMResult<IdentifierKey> {
        self.get_or_intern_str_internal(ident.borrow_str())
    }

    /// Interns an identifier string using this specific interner instance.
    #[allow(dead_code)]
    pub(crate) fn get_or_intern_ident_str(
        &self,
        ident_str: &IdentStr,
    ) -> PartialVMResult<IdentifierKey> {
        self.get_or_intern_str_internal(ident_str.borrow_str())
    }

    /// Internal helper that performs the actual string interning.
    /// Handles the low-level interaction with the lasso interner.
    ///
    /// # Errors
    /// Returns `INTERNER_LIMIT_REACHED` if the interner cannot allocate more strings.
    fn get_or_intern_str_internal(&self, string: &str) -> PartialVMResult<IdentifierKey> {
        match self.0.try_get_or_intern(string) {
            Ok(result) => Ok(IdentifierKey(result)),
            Err(err) => Err(PartialVMError::new(StatusCode::INTERNER_LIMIT_REACHED)
                .with_message(format!("Failed to intern {string} ident; error: {err:?}."))),
        }
    }
}
