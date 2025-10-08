// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

//! Helper functions for the Move interpreter.
//!
//! This module provides utility functions that support the main interpreter execution,
//! including type instantiation, struct operations, and value conversions.
//! These helpers abstract common patterns used across different bytecode instructions.
//!
//! Key functionality:
//! - Type parameter substitution for generics
//! - Struct field access and manipulation
//! - Value packing and unpacking
//! - Runtime type information helpers

//! Helper functions for type instantiation and generic resolution during bytecode execution.
//!
//! This module provides utilities for instantiating generic functions, structs, and enums
//! with concrete type arguments. It performs type substitution and ensures that type
//! instantiations don't exceed complexity limits to prevent resource exhaustion attacks.
//!
//! These functions support the interpreter by resolving generic bytecode instructions
//! to their concrete types at runtime.

use crate::{
    execution::dispatch_tables::VirtualTableKey,
    jit::execution::ast::{
        ArenaType, FunctionInstantiation, StructInstantiation, Type, TypeNodeCount, TypeSubst,
        VariantInstantiation,
    },
    shared::constants::MAX_TYPE_INSTANTIATION_NODES,
};

use move_binary_format::errors::{PartialVMError, PartialVMResult};
use move_core_types::vm_status::StatusCode;

/// Instantiates a generic function with concrete type arguments.
/// Performs type substitution on the function's type parameters and validates
/// that the resulting instantiation doesn't exceed the maximum type node limit.
pub fn instantiate_generic_function(
    fun_inst: &FunctionInstantiation,
    type_params: &[Type],
) -> PartialVMResult<Vec<Type>> {
    let instantiation: Vec<_> = fun_inst
        .instantiation
        .to_ref()
        .iter()
        .map(|ty| ty.subst(type_params))
        .collect::<PartialVMResult<_>>()?;

    // Check if the function instantiation over all generics is larger
    // than MAX_TYPE_INSTANTIATION_NODES.
    let mut sum_nodes = 1u64;
    for ty in type_params.iter().chain(instantiation.iter()) {
        sum_nodes = sum_nodes.saturating_add(ty.count_type_nodes());
        if sum_nodes > MAX_TYPE_INSTANTIATION_NODES {
            return Err(PartialVMError::new(StatusCode::TOO_MANY_TYPE_NODES));
        }
    }
    Ok(instantiation)
}

/// Instantiates a single type with the provided type arguments.
/// If no type arguments are provided, returns the type as-is.
pub fn instantiate_single_type(ty: &ArenaType, ty_args: &[Type]) -> PartialVMResult<Type> {
    if !ty_args.is_empty() {
        ty.subst(ty_args)
    } else {
        Ok(ty.to_type())
    }
}

/// Instantiates a generic struct type with concrete type arguments.
/// Creates a DatatypeInstantiation with the struct's vtable key and instantiated type parameters.
pub fn instantiate_struct_type(
    struct_inst: &StructInstantiation,
    ty_args: &[Type],
) -> PartialVMResult<Type> {
    let type_params = struct_inst.type_params.to_ref();
    instantiate_datatype_common(&struct_inst.def_vtable_key, type_params, ty_args)
}

/// Instantiates a generic enum type from a variant instantiation.
/// Extracts the enum definition and instantiates it with concrete type arguments.
pub fn instantiate_enum_type(
    variant_inst: &VariantInstantiation,
    ty_args: &[Type],
) -> PartialVMResult<Type> {
    let enum_inst = variant_inst.enum_inst.to_ref();
    let type_params = enum_inst.type_params.to_ref();
    instantiate_datatype_common(&enum_inst.def_vtable_key, type_params, ty_args)
}

/// Common implementation for instantiating both struct and enum datatypes.
/// Validates type node limits and performs type parameter substitution.
fn instantiate_datatype_common(
    datatype_key: &VirtualTableKey,
    type_params: &[ArenaType],
    ty_args: &[Type],
) -> PartialVMResult<Type> {
    // Before instantiating the type, count the # of nodes of all type arguments plus
    // existing type instantiation.
    // If that number is larger than MAX_TYPE_INSTANTIATION_NODES, refuse to construct this type.
    // This prevents constructing larger and larger types via datatype instantiation.
    let mut sum_nodes = 1u64;
    for ty in type_params.iter() {
        sum_nodes = sum_nodes.saturating_add(ty.count_type_nodes());
        if sum_nodes > MAX_TYPE_INSTANTIATION_NODES {
            return Err(PartialVMError::new(StatusCode::TOO_MANY_TYPE_NODES));
        }
    }
    for ty in ty_args.iter() {
        sum_nodes = sum_nodes.saturating_add(ty.count_type_nodes());
        if sum_nodes > MAX_TYPE_INSTANTIATION_NODES {
            return Err(PartialVMError::new(StatusCode::TOO_MANY_TYPE_NODES));
        }
    }

    Ok(Type::DatatypeInstantiation(Box::new((
        datatype_key.clone(),
        type_params
            .iter()
            .map(|ty| ty.subst(ty_args))
            .collect::<PartialVMResult<_>>()?,
    ))))
}
