// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use async_trait::async_trait;
use move_core_types::account_address::AccountAddress;
use sui_types::base_types::ObjectID;
use sui_types::storage::BackingPackageStore;

use crate::error::Error;
use crate::{Package, PackageStore, Result};

/// Adapts a sync [`BackingPackageStore`] into an async [`PackageStore`] for use with
/// [`crate::Resolver`]. This bridges the validator/authority storage layer (which uses
/// `BackingPackageStore` backed by RocksDB) into the `sui-package-resolver` world.
pub struct BackingPackageStoreAdapter<S> {
    backing: S,
}

impl<S> BackingPackageStoreAdapter<S> {
    pub fn new(backing: S) -> Self {
        Self { backing }
    }
}

#[async_trait]
impl<S> PackageStore for BackingPackageStoreAdapter<S>
where
    S: BackingPackageStore + Send + Sync,
{
    async fn fetch(&self, id: AccountAddress) -> Result<Arc<Package>> {
        let object_id = ObjectID::from(id);
        let package_obj = self
            .backing
            .get_package_object(&object_id)
            .map_err(|e| Error::Store {
                store: "BackingPackageStore",
                error: e.to_string(),
            })?
            .ok_or(Error::PackageNotFound(id))?;
        Ok(Arc::new(Package::read_from_object(package_obj.object())?))
    }
}

/// A [`PackageStore`] that checks a primary store first, falling back to a secondary store if the
/// primary does not contain the requested package. This is the async equivalent of
/// [`sui_types::inner_temporary_store::PackageStoreWithFallback`].
pub struct FallbackPackageStore<P, F> {
    primary: P,
    fallback: F,
}

impl<P, F> FallbackPackageStore<P, F> {
    pub fn new(primary: P, fallback: F) -> Self {
        Self { primary, fallback }
    }
}

#[async_trait]
impl<P, F> PackageStore for FallbackPackageStore<P, F>
where
    P: PackageStore,
    F: PackageStore,
{
    async fn fetch(&self, id: AccountAddress) -> Result<Arc<Package>> {
        match self.primary.fetch(id).await {
            Ok(package) => Ok(package),
            Err(_) => self.fallback.fetch(id).await,
        }
    }
}

