// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

pub mod cached_package_store;
pub mod legacy;
pub mod linkable_data_store;
pub mod linked_data_store;

use std::rc::Rc;
use sui_types::{
    base_types::ObjectID, error::SuiResult, move_package::MovePackage, storage::BackingPackageStore,
};

// A unifying trait that allows us to load move packages that may not be objects just yet (e.g., if
// they were published in the current transaction). Note that this needs to load `MovePackage`s and
// not `MovePackageObject`s.
pub trait PackageStore {
    fn get_package(&self, id: &ObjectID) -> SuiResult<Option<Rc<MovePackage>>>;
}

/// A trait that allows us to resolve a type to its defining ID as well as load packages.
/// TODO: Examine rolling this into `PackageStore` in the near future as we incorporate this into
/// the new PTB runtime.
pub trait ResolvablePackageStore: PackageStore {
    // TODO: Remove this once we start using Rust 1.86
    fn as_package_store(&self) -> &dyn PackageStore;

    fn resolve_type_to_defining_id(
        &self,
        module_address: ObjectID,
        module_name: String,
        type_name: String,
    ) -> SuiResult<Option<ObjectID>>;
}

impl<T: BackingPackageStore> PackageStore for T {
    fn get_package(&self, id: &ObjectID) -> SuiResult<Option<Rc<MovePackage>>> {
        Ok(self
            .get_package_object(id)?
            .map(|x| Rc::new(x.move_package().clone())))
    }
}
