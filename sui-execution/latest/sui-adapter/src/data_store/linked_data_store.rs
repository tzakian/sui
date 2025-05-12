// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::rc::Rc;

use crate::{
    data_store::PackageStore,
    linkage::{Linkage, analysis::PTBLinkageResolver},
};
use move_binary_format::errors::{Location, PartialVMError, PartialVMResult, VMResult};
use move_core_types::{
    account_address::AccountAddress,
    identifier::IdentStr,
    language_storage::ModuleId,
    resolver::{LinkageResolver, ModuleResolver},
    vm_status::StatusCode,
};
use move_vm_types::data_store::DataStore;
use sui_types::{
    base_types::ObjectID,
    error::{ExecutionErrorKind, SuiError, SuiResult},
    move_package::MovePackage,
};

// LinkedDataStore(SuiDataStore) is a wrapper around the `SuiDataStore` that holds linkage
// information. We generally should not, but we may need to, fetch through both the package
// cache inside of the `resolver` and inside of the underlying `package_store` (i.e., "call out
// to disk") in the case where the `package_cache` was dropped due to getting too large.
pub struct LinkedDataStore<'a> {
    pub linkage: &'a Linkage,
    pub resolver: &'a PTBLinkageResolver,
    pub package_store: &'a dyn PackageStore,
}

impl<'a> LinkedDataStore<'a> {
    pub fn new(
        linkage: &'a Linkage,
        resolver: &'a PTBLinkageResolver,
        package_store: &'a dyn PackageStore,
    ) -> Self {
        Self {
            linkage,
            resolver,
            package_store,
        }
    }

    pub fn get_package(&self, package_storage_id: ObjectID) -> SuiResult<Option<Rc<MovePackage>>> {
        self.package_store.get_package(&package_storage_id)
    }
}

impl DataStore for LinkedDataStore<'_> {
    fn link_context(&self) -> AccountAddress {
        self.linkage.link_context
    }

    fn relocate(&self, module_id: &ModuleId) -> PartialVMResult<ModuleId> {
        self.linkage
            .resolved_linkage
            .linkage
            .get(&ObjectID::from(*module_id.address()))
            .map(|obj_id| ModuleId::new(**obj_id, module_id.name().to_owned()))
            .ok_or_else(|| {
                PartialVMError::new(StatusCode::LINKER_ERROR).with_message(format!(
                    "Error relocating {module_id} -- could not find linkage"
                ))
            })
    }

    fn defining_module(
        &self,
        module_id: &ModuleId,
        struct_: &IdentStr,
    ) -> PartialVMResult<ModuleId> {
        self.linkage.resolved_linkage
                .resolve_type_to_defining_id(
                    self.resolver,
                    ObjectID::from(*module_id.address()),
                    module_id.name().to_string(),
                    struct_.to_string(),
                )
                .map(|obj_id| ModuleId::new(*obj_id, module_id.name().to_owned()))
                .ok_or_else(|| {
                    PartialVMError::new(StatusCode::LINKER_ERROR).with_message(format!(
                        "Error finding defining module for {module_id}::{struct_} -- could nod find linkage"
                    ))
                })
    }

    // NB: module_id is original ID based
    fn load_module(&self, module_id: &ModuleId) -> VMResult<Vec<u8>> {
        let package_storage_id = ObjectID::from(*module_id.address());
        match self
            .get_package(package_storage_id)
            .map(|pkg| pkg.and_then(|pkg| pkg.get_module(module_id).cloned()))
        {
            Ok(Some(bytes)) => Ok(bytes),
            Ok(None) => Err(PartialVMError::new(StatusCode::LINKER_ERROR)
                .with_message(format!("Cannot find {:?} in data cache", module_id))
                .finish(Location::Undefined)),
            Err(err) => {
                let msg = format!("Unexpected storage error: {:?}", err);
                Err(
                    PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR)
                        .with_message(msg)
                        .finish(Location::Undefined),
                )
            }
        }
    }

    fn publish_module(&mut self, _module_id: &ModuleId, _blob: Vec<u8>) -> VMResult<()> {
        Ok(())
    }
}

impl DataStore for &LinkedDataStore<'_> {
    fn link_context(&self) -> AccountAddress {
        DataStore::link_context(*self)
    }

    fn relocate(&self, module_id: &ModuleId) -> PartialVMResult<ModuleId> {
        DataStore::relocate(*self, module_id)
    }

    fn defining_module(
        &self,
        module_id: &ModuleId,
        struct_: &IdentStr,
    ) -> PartialVMResult<ModuleId> {
        DataStore::defining_module(*self, module_id, struct_)
    }

    fn load_module(&self, module_id: &ModuleId) -> VMResult<Vec<u8>> {
        DataStore::load_module(*self, module_id)
    }

    fn publish_module(&mut self, _module_id: &ModuleId, _blob: Vec<u8>) -> VMResult<()> {
        Ok(())
    }
}

impl ModuleResolver for LinkedDataStore<'_> {
    type Error = SuiError;

    fn get_module(&self, id: &ModuleId) -> Result<Option<Vec<u8>>, Self::Error> {
        self.load_module(id)
            .map(|bytes| Some(bytes))
            .map_err(|_| SuiError::from(ExecutionErrorKind::VMVerificationOrDeserializationError))
    }
}

impl LinkageResolver for LinkedDataStore<'_> {
    type Error = SuiError;
}
