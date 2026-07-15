// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::{get_extension, object_runtime::ObjectRuntime};
use move_binary_format::safe_assert_eq;
use move_core_types::{account_address::AccountAddress, gas_algebra::InternalGas};
use move_vm_runtime::native_charge_gas_early_exit;
use move_vm_runtime::natives::functions::{NativeContext, NativeResult};
use move_vm_runtime::{execution::values::Value, pop_arg};
use smallvec::smallvec;
use std::collections::VecDeque;

const E_INVALID_PACKAGE_VERSION: u64 = 6;

#[derive(Clone)]
pub struct PackageVersioningOriginalPackageIdImplCostParams {
    pub package_original_package_id_impl_cost_base: InternalGas,
}

pub fn original_package_id_impl(
    context: &mut NativeContext,
    ty_args: Vec<move_vm_runtime::execution::Type>,
    mut args: VecDeque<Value>,
) -> move_binary_format::errors::PartialVMResult<NativeResult> {
    safe_assert_eq!(ty_args.len(), 0);
    safe_assert_eq!(args.len(), 2);

    let cost = crate::get_extension!(context, crate::NativesCostTable)?
        .package_original_package_id_impl_cost_params
        .package_original_package_id_impl_cost_base;
    native_charge_gas_early_exit!(context, cost);

    let version = pop_arg!(args, u64).into();
    let package_id = pop_arg!(args, AccountAddress).into();
    let Some(package) =
        get_extension!(context, ObjectRuntime)?.get_package_at_version(package_id, version)
    else {
        return Ok(NativeResult::err(
            context.gas_used(),
            E_INVALID_PACKAGE_VERSION,
        ));
    };
    let original_id = package.original_package_id();

    Ok(NativeResult::ok(
        context.gas_used(),
        smallvec![Value::address(original_id.into())],
    ))
}
