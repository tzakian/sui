// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

//! Package cache and loader for the Move VM.
//!
//! This module serves as the central orchestrator for package management in the Move VM. It
//! handles the responsibility of loading, publishing, verifying, and caching Move packages to the
//! VM.
//!
//! Key responsibilities:
//! - **Package caching**: Maintains verified and compiled packages in memory for fast access
//! - **Concurrent loading**: Handles safe concurrent package loading with proper locking
//! - **Version management**: Tracks packages by their unique version IDs
//! - **Memory efficiency**: Shares compiled/loaded package data across VM instances via [`Arc`]s
//!
//! Integration with VM architecture:
//! - Used by the runtime to resolve package dependencies before execution
//! - Coordinates with the validation layer to verify packages before caching
//! - Coordinates with the JIT compiler to compile and cache executable code
//! - Works with the runtime to link and execute cached packages
//! - Enables package sharing across multiple concurrent executions

use crate::{jit, shared::types::VersionId, validation::verification};
use move_vm_config::runtime::VMConfig;
use parking_lot::RwLock;
use std::{collections::HashMap, sync::Arc};

// -------------------------------------------------------------------------------------------------
// Types
// -------------------------------------------------------------------------------------------------

/// Compiled and verified Move package ready for execution.
/// Contains both the verified AST and runtime AST (for execution).
#[derive(Debug)]
pub struct Package {
    pub verified: Arc<verification::ast::Package>,
    pub runtime: Arc<jit::execution::ast::Package>,
}

type PackageCache = HashMap<VersionId, Arc<Package>>;

/// Central cache for Move packages in the VM.
/// Manages package loading, caching, and resolution.
#[derive(Debug)]
pub struct MoveCache {
    pub(crate) vm_config: Arc<VMConfig>,
    pub(crate) package_cache: Arc<RwLock<PackageCache>>,
}

/// Result of a package resolution attempt.
#[derive(Debug)]
pub enum ResolvedPackageResult {
    /// The package was found, loaded, and cached.
    Found(Arc<Package>),
    /// The package was not found.
    NotFound,
}

// -------------------------------------------------------------------------------------------------
// Impls
// -------------------------------------------------------------------------------------------------

impl MoveCache {
    /// Creates a new package cache with the given VM configuration.
    pub fn new(vm_config: Arc<VMConfig>) -> Self {
        Self {
            vm_config,
            package_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    // -------------------------------------------
    // Caching Operations
    // -------------------------------------------

    /// Adds a verified package to the cache.
    ///
    /// This operation is idempotent - if the package already exists in the cache it's a no-op.
    /// Thread-safe for concurrent package loading scenarios where multiple threads
    /// might verify and attempt to cache the same package.
    ///
    /// Takes ownership of the verified and runtime ASTs and wraps them in Arc for sharing.
    pub fn add_to_cache(
        &self,
        package_key: VersionId,
        verified: verification::ast::Package,
        runtime: jit::execution::ast::Package,
    ) {
        // NB: We grab a write lock here to ensure that we don't double-insert a package.
        let mut package_cache = self.package_cache.write();

        if package_cache.contains_key(&package_key) {
            return;
        }
        let verified = Arc::new(verified);
        let runtime = Arc::new(runtime);
        let package = Package { verified, runtime };
        package_cache.insert(package_key, Arc::new(package));
    }

    /// Retrieves a cached package by its version ID.
    ///
    /// Returns `None` if the package hasn't been loaded and cached yet.
    /// The returned `Arc` allows efficient sharing without copying.
    pub fn cached_package_at(&self, package_key: VersionId) -> Option<Arc<Package>> {
        self.package_cache.read().get(&package_key).map(Arc::clone)
    }

    // -------------------------------------------
    // Getters
    // -------------------------------------------

    /// Returns a reference to the underlying package cache.
    #[cfg(test)]
    pub(crate) fn package_cache(&self) -> &RwLock<PackageCache> {
        &self.package_cache
    }

    // -------------------------------------------
    // Cache Eviction For Testing
    // -------------------------------------------

    /// Removes a package from the cache for testing purposes.
    /// Returns true if the package was present and removed.
    #[cfg(test)]
    pub(crate) fn remove_package(&self, version_id: &VersionId) -> bool {
        self.package_cache.write().remove(version_id).is_some()
    }
}

impl Package {
    /// Creates a new package from pre-Arc'ed verified and runtime ASTs.
    /// Used internally when packages are already Arc-wrapped.
    pub(crate) fn new(
        verified: Arc<verification::ast::Package>,
        runtime: Arc<jit::execution::ast::Package>,
    ) -> Self {
        Self { verified, runtime }
    }

    /// Returns the number of types loaded in this package.
    /// Used for testing and verification of package loading.
    #[cfg(test)]
    pub(crate) fn loaded_types_len(&self) -> usize {
        self.runtime.vtable.types.len()
    }
}

// -------------------------------------------------------------------------------------------------
// Other Impls
// -------------------------------------------------------------------------------------------------

impl Clone for MoveCache {
    /// Makes a shallow copy of the VM Cache by cloning all the internal `Arc`s.
    fn clone(&self) -> Self {
        let MoveCache {
            vm_config,
            package_cache,
        } = self;
        Self {
            vm_config: vm_config.clone(),
            package_cache: package_cache.clone(),
        }
    }
}
