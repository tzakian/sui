// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::programmable_transactions::linkage_view::LinkageView;
use move_binary_format::errors::{Location, PartialVMError, PartialVMResult, VMResult};
use move_core_types::{
    account_address::AccountAddress, identifier::IdentStr, language_storage::ModuleId,
    resolver::ModuleResolver, vm_status::StatusCode,
};
use move_vm_types::data_store::DataStore;
use sui_types::{
    base_types::ObjectID, error::SuiResult, move_package::MovePackage, storage::BackingPackageStore,
};

// Implementation of the `DataStore` trait for the Move VM.
// When used during execution it may have a list of new packages that have
// just been published in the current context. Those are used for module/type
// resolution when executing module init.
// It may be created with an empty slice of packages either when no publish/upgrade
// are performed or when a type is requested not during execution.
pub(crate) struct SuiDataStore<'state, 'a> {
    linkage_view: &'a LinkageView<'state>,
    new_packages: &'a [MovePackage],
}

impl<'state, 'a> SuiDataStore<'state, 'a> {
    pub(crate) fn new(
        linkage_view: &'a LinkageView<'state>,
        new_packages: &'a [MovePackage],
    ) -> Self {
        Self {
            linkage_view,
            new_packages,
        }
    }

    fn get_module(&self, module_id: &ModuleId) -> Option<&Vec<u8>> {
        for package in self.new_packages {
            let module = package.get_module(module_id);
            if module.is_some() {
                return module;
            }
        }
        None
    }
}

// TODO: `DataStore` will be reworked and this is likely to disappear.
//       Leaving this comment around until then as testament to better days to come...
impl DataStore for SuiDataStore<'_, '_> {
    fn link_context(&self) -> AccountAddress {
        self.linkage_view.link_context()
    }

    fn relocate(&self, module_id: &ModuleId) -> PartialVMResult<ModuleId> {
        self.linkage_view.relocate(module_id).map_err(|err| {
            PartialVMError::new(StatusCode::LINKER_ERROR)
                .with_message(format!("Error relocating {module_id}: {err:?}"))
        })
    }

    fn defining_module(
        &self,
        runtime_id: &ModuleId,
        struct_: &IdentStr,
    ) -> PartialVMResult<ModuleId> {
        self.linkage_view
            .defining_module(runtime_id, struct_)
            .map_err(|err| {
                PartialVMError::new(StatusCode::LINKER_ERROR).with_message(format!(
                    "Error finding defining module for {runtime_id}::{struct_}: {err:?}"
                ))
            })
    }

    fn load_module(&self, module_id: &ModuleId) -> VMResult<Vec<u8>> {
        if let Some(bytes) = self.get_module(module_id) {
            return Ok(bytes.clone());
        }
        match self.linkage_view.get_module(module_id) {
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
        // we cannot panic here because during execution and publishing this is
        // currently called from the publish flow in the Move runtime
        Ok(())
    }
}

pub(crate) mod new {
    use move_binary_format::errors::VMError;
    use move_core_types::{
        language_storage::StructTag,
        resolver::{LinkageResolver, MoveResolver, ResourceResolver},
    };
    use sui_types::{
        base_types::MoveObjectType,
        error::{ExecutionError, ExecutionErrorKind, SuiError},
        object::Object,
    };

    use crate::programmable_transactions::linkage_resolution::{
        LinkageAnalysis, PTBLinkageResolver, ResolvedLinkage,
    };

    use super::*;

    // Implementation of the `DataStore` trait for the Move VM. This is meant to be "stupid" and
    // simply be a shim over the `BackingPackageStore` that allows us to layer on new packages that
    // have been published in the current transaction. Importantly, this does not think about, or
    // know about linkage or anything like that.
    //
    // When used during execution it may have a list of new packages that have
    // just been published in the current context. Those are used for module/type
    // resolution when executing module init.
    //
    // It may be created with an empty slice of packages either when no publish/upgrade
    // are performed or when a type is requested not during execution.
    //
    // It may have an `ephemeral_package` that is a package that is in the process of being
    // published and or upgraded. This package is not yet in the list of new packages, and we may
    // be in the process of either executing code in this package (e.g., for `init` functions) or
    // resolving errors that are only resolvable in this package (e.g., for `init` functions that
    // return errors).
    pub(crate) struct SuiDataStore<'state, 'a> {
        /// Interface to resolve packages, modules and resources directly from the store.
        resolver: &'state dyn BackingPackageStore,
        /// New packages -- these are new package that have been published, and initialized
        /// successfully.
        new_packages: &'a [MovePackage],
        /// A possibly new package that we may be in the process of publishing and/or initializing.
        /// This package will only be added to `new_packages` iff publication and initialization
        /// (if any) is pefromed successfully.
        ephemeral_package: Option<&'a MovePackage>,
    }

    impl<'state, 'a> SuiDataStore<'state, 'a> {
        pub(crate) fn new(
            resolver: &'state dyn BackingPackageStore,
            new_packages: &'a [MovePackage],
        ) -> Self {
            Self {
                new_packages,
                resolver,
                ephemeral_package: None,
            }
        }

        pub(crate) fn new_with_ephemeral(
            resolver: &'state dyn BackingPackageStore,
            new_packages: &'a [MovePackage],
            ephemeral_package: Option<&'a MovePackage>,
        ) -> Self {
            Self {
                new_packages,
                resolver,
                ephemeral_package,
            }
        }

        pub(crate) fn get_package(
            &self,
            package_storage_id: ObjectID,
        ) -> SuiResult<Option<MovePackage>> {
            if let Some(pkg) = self
                .new_packages
                .iter()
                .chain(self.ephemeral_package.iter().cloned())
                .find(|package| package.id() == package_storage_id)
            {
                return Ok(Some(pkg.clone()));
            }

            Ok(self
                .resolver
                .get_package_object(&package_storage_id)?
                .map(|pkg| pkg.move_package().clone()))
        }
    }

    ///////////////////////////////////////////////////////////////////////////
    // LinkedDataStore
    ///////////////////////////////////////////////////////////////////////////

    // LinkedDataStore(SuiDataStore) is a wrapper around the `SuiDataStore` that holds linkage
    // information. We generally should not, but we may need to, fetch through both the package
    // cache inside of the `resolver` and inside of the underlying `package_store` (i.e., "call out
    // to disk") in the case where the `package_cache` was dropped due to getting too large.

    pub struct LinkedDataStore<'a> {
        link_context: AccountAddress,
        resolved_linkage: &'a ResolvedLinkage,
        resolver: &'a PTBLinkageResolver,
        package_store: &'a dyn PackageStore,
    }

    impl<'a> LinkedDataStore<'a> {
        pub fn new(
            link_context: AccountAddress,
            resolved_linkage: &'a ResolvedLinkage,
            resolver: &'a PTBLinkageResolver,
            package_store: &'a dyn PackageStore,
        ) -> Self {
            Self {
                link_context,
                resolved_linkage,
                resolver,
                package_store,
            }
        }

        pub fn get_package(&self, package_storage_id: ObjectID) -> SuiResult<Option<MovePackage>> {
            if let Some(pkg) = self
                .resolver
                .package_cache
                .borrow()
                .get(&package_storage_id)
                .cloned()
            {
                return Ok(Some(pkg));
            }
            self.package_store.get_package(&package_storage_id)
        }
    }

    impl DataStore for LinkedDataStore<'_> {
        fn link_context(&self) -> AccountAddress {
            self.link_context
        }

        fn relocate(&self, module_id: &ModuleId) -> PartialVMResult<ModuleId> {
            self.resolved_linkage
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
            self.resolved_linkage
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
            self.load_module(id).map(|bytes| Some(bytes)).map_err(|_| {
                SuiError::from(ExecutionErrorKind::VMVerificationOrDeserializationError)
            })
        }
    }

    // TODO: remove
    impl ResourceResolver for LinkedDataStore<'_> {
        type Error = SuiError;

        fn get_resource(
            &self,
            address: &AccountAddress,
            tag: &move_core_types::language_storage::StructTag,
        ) -> Result<Option<Vec<u8>>, Self::Error> {
            unreachable!("ResourceResolver is not implemented for LinkedDataStore");
        }
    }

    impl LinkageResolver for LinkedDataStore<'_> {
        type Error = SuiError;
    }

    ///////////////////////////////////////////////////////////////////////////
    // LinkableStore
    ///////////////////////////////////////////////////////////////////////////

    pub struct LinkableStore<'a> {
        linkage_analyzer: &'a mut dyn LinkageAnalysis,
        store: &'a dyn PackageStore,
    }

    impl<'a> LinkableStore<'a> {
        pub fn new(
            linkage_analyzer: &'a mut dyn LinkageAnalysis,
            store: &'a dyn PackageStore,
        ) -> Self {
            Self {
                linkage_analyzer,
                store,
            }
        }

        pub fn linked_data_store_for_object_type(
            &mut self,
            object_type: &MoveObjectType,
        ) -> Result<LinkedDataStore<'a>, ExecutionError> {
            let link_context = object_type.address();
            let ids: Vec<_> = StructTag::from(object_type.clone())
                .all_addresses()
                .into_iter()
                .map(ObjectID::from)
                .collect();
            let resolver = self.linkage_analyzer.resolver();
            let resolved_linkage = resolver.type_linkage(ids.as_slice(), self.store)?;
            Ok(LinkedDataStore::new(
                link_context,
                &resolved_linkage,
                &resolver,
                self.store,
            ))
        }
    }
}

// A unifying trait that allows us to load move packages that may not be objects just yet (e.g., if
// they were published in the current transaction). Note that this needs to load `MovePackage`s and
// not `MovePackageObject`s.
pub trait PackageStore {
    fn get_package(&self, id: &ObjectID) -> SuiResult<Option<MovePackage>>;
}

impl<T: BackingPackageStore> PackageStore for T {
    fn get_package(&self, id: &ObjectID) -> SuiResult<Option<MovePackage>> {
        Ok(self
            .get_package_object(id)?
            .map(|x| x.move_package().clone()))
    }
}

impl PackageStore for new::SuiDataStore<'_, '_> {
    fn get_package(&self, id: &ObjectID) -> SuiResult<Option<MovePackage>> {
        self.get_package(*id)
    }
}

impl PackageStore for new::LinkedDataStore<'_> {
    fn get_package(&self, id: &ObjectID) -> SuiResult<Option<MovePackage>> {
        self.get_package(*id)
    }
}
