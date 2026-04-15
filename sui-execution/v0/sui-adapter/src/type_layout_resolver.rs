// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use async_trait::async_trait;
use move_core_types::annotated_value as A;
use move_core_types::language_storage::StructTag;
use move_vm_runtime::move_vm::MoveVM;
use sui_types::execution::TypeLayoutStore;
use sui_types::{error::SuiError, layout_resolver::LayoutResolver};

/// Retrieve a `MoveStructLayout` from a `Type`.
pub struct TypeLayoutResolver<'state, 'vm> {
    _vm: &'vm MoveVM,
    _state_view: Box<dyn TypeLayoutStore + 'state>,
}

impl<'state, 'vm> TypeLayoutResolver<'state, 'vm> {
    pub fn new(vm: &'vm MoveVM, state_view: Box<dyn TypeLayoutStore + 'state>) -> Self {
        Self {
            _vm: vm,
            _state_view: state_view,
        }
    }
}

#[async_trait(?Send)]
impl LayoutResolver for TypeLayoutResolver<'_, '_> {
    async fn get_annotated_layout(
        &mut self,
        _struct_tag: &StructTag,
    ) -> Result<A::MoveDatatypeLayout, SuiError> {
        todo!()
    }
}
