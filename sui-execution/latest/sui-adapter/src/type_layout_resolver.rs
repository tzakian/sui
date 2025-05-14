// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::data_store::cached_package_store::CachedPackageStore;
use crate::data_store::linkable_data_store::LinkableStore;
use crate::programmable_transactions::context::load_type_from_struct;
use move_core_types::annotated_value as A;
use move_core_types::language_storage::StructTag;
use move_vm_runtime::move_vm::MoveVM;
use sui_types::base_types::ObjectID;
use sui_types::error::SuiResult;
use sui_types::execution::TypeLayoutStore;
use sui_types::storage::{BackingPackageStore, PackageObject};
use sui_types::{error::SuiError, layout_resolver::LayoutResolver};

/// Retrieve a `MoveStructLayout` from a `Type`.
/// Invocation into the `Session` to leverage the `LinkageView` implementation
/// common to the runtime.
pub struct TypeLayoutResolver<'state, 'vm> {
    vm: &'vm MoveVM,
    linkage_view: LinkableStore<'state>,
}

/// Implements SuiResolver traits by providing null implementations for module and resource
/// resolution and delegating backing package resolution to the trait object.
struct NullSuiResolver<'state>(Box<dyn TypeLayoutStore + 'state>);

impl<'state, 'vm> TypeLayoutResolver<'state, 'vm> {
    pub fn new(vm: &'vm MoveVM, state_view: Box<dyn TypeLayoutStore + 'state>) -> Self {
        let linkage_view = LinkableStore::new(Box::new(IndexedPackageStore::new(Box::new(
            CachedPackageStore::new(Box::new(NullSuiResolver(state_view))),
        ))));
        Self { vm, linkage_view }
    }
}

impl LayoutResolver for TypeLayoutResolver<'_, '_> {
    fn get_annotated_layout(
        &mut self,
        struct_tag: &StructTag,
    ) -> Result<A::MoveDatatypeLayout, SuiError> {
        let linkage = self
            .linkage_view
            .linkage_for_struct_tag(struct_tag)
            .map_err(|err| SuiError::FailObjectLayout {
                st: format!("Error resolving struct tag {}: {}", struct_tag, err),
            })?;
        let linked_store = self.linkage_view.linked_data_store_for_linkage(&linkage);
        let Ok(ty) = load_type_from_struct(self.vm, &mut linked_store, &[], struct_tag) else {
            return Err(SuiError::FailObjectLayout {
                st: format!("{}", struct_tag),
            });
        };
        let layout = self.vm.get_runtime().type_to_fully_annotated_layout(&ty);
        match layout {
            Ok(A::MoveTypeLayout::Struct(s)) => Ok(A::MoveDatatypeLayout::Struct(s)),
            Ok(A::MoveTypeLayout::Enum(e)) => Ok(A::MoveDatatypeLayout::Enum(e)),
            _ => Err(SuiError::FailObjectLayout {
                st: format!("{}", struct_tag),
            }),
        }
    }
}

impl BackingPackageStore for NullSuiResolver<'_> {
    fn get_package_object(&self, package_id: &ObjectID) -> SuiResult<Option<PackageObject>> {
        self.0.get_package_object(package_id)
    }
}
