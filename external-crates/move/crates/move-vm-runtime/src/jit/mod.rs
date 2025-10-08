// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

//! Just-In-Time compilation and optimization framework for Move bytecode.
//!
//! This module implements bytecode transformation passes that optimize and prepare
//! Move bytecode for efficient execution. The JIT system operates in two phases:
//! compilation (bytecode to AST) and optimization (AST transformations).
//!
//! Key components:
//! - **Execution AST**: Optimized representation for runtime execution
//! - **Optimization passes**: Dead code elimination and other transformations
//! - **Translation layer**: Converts between bytecode and AST representations
//!
//! The JIT integrates with:
//! - The validation layer to receive verified bytecode
//! - The cache system to store compiled packages
//! - The interpreter which executes the optimized AST

//! Just-In-Time compilation and optimization for the Move VM.
//!
//! This module provides bytecode optimization and AST translation capabilities
//! for the Move VM. It transforms verified Move bytecode into an optimized
//! runtime representation that can be executed more efficiently.
//!
//! Key components:
//! - **execution**: Runtime AST representation and translation from bytecode
//! - **optimization**: Bytecode optimization passes for performance improvements
//!
//! The JIT system can optionally optimize bytecode based on VM configuration,
//! performing transformations like dead code elimination while preserving
//! program semantics.

pub mod execution;
pub mod optimization;

use crate::{
    jit::execution::ast::Package,
    jit::optimization::{optimize, to_optimized_form},
    natives::functions::NativeFunctions,
    validation::verification,
};
use move_binary_format::errors::PartialVMResult;
use move_vm_config::runtime::VMConfig;

/// Translates a verified Move package into the runtime execution format.
/// Optionally applies bytecode optimizations based on VM configuration before
/// generating the final executable representation.
pub fn translate_package(
    vm_config: &VMConfig,
    natives: &NativeFunctions,
    loaded_package: verification::ast::Package,
) -> PartialVMResult<Package> {
    let opt_package = if vm_config.optimize_bytecode {
        optimize(loaded_package)
    } else {
        to_optimized_form(loaded_package)
    };
    execution::translate::package(natives, opt_package)
}

#[cfg(test)]
mod tests {
    use super::translate_package;
    use crate::{
        jit::execution::ast::Package as RuntimePackage, validation::verification::ast as verif_ast,
    };
    use indexmap::IndexMap;
    use move_binary_format::file_format::empty_module;
    use move_core_types::{account_address::AccountAddress, language_storage::ModuleId};
    use move_vm_config::runtime::VMConfig;
    use std::collections::BTreeMap;

    fn make_verified_empty_package(
        original_id: AccountAddress,
        version_id: AccountAddress,
    ) -> verif_ast::Package {
        // Minimal valid module
        let module = empty_module();
        let module_id: ModuleId = module.self_id();

        // Assemble verification package with a single module and minimal tables
        verif_ast::Package {
            original_id,
            version_id,
            modules: BTreeMap::from([(module_id, verif_ast::Module { value: module })]),
            type_origin_table: IndexMap::new(),
            linkage_table: BTreeMap::from([(original_id, version_id)]),
            version: 0,
        }
    }

    fn assert_basic_runtime_pkg(
        pkg: &RuntimePackage,
        original_id: AccountAddress,
        version_id: AccountAddress,
    ) {
        assert_eq!(pkg.original_id, original_id);
        assert_eq!(pkg.version_id, version_id);
        // One module translated from the single compiled module
        assert_eq!(pkg.loaded_modules.len(), 1);
    }

    #[test]
    fn translate_without_optimization() {
        let original_id = AccountAddress::from([1u8; 32]);
        let version_id = AccountAddress::from([2u8; 32]);
        let verified = make_verified_empty_package(original_id, version_id);

        let vm_config = VMConfig {
            optimize_bytecode: false,
            ..VMConfig::default()
        };
        let natives = crate::natives::functions::NativeFunctions::empty_for_testing().unwrap();

        let result = translate_package(&vm_config, &natives, verified);
        let runtime_pkg = result.expect("translate_package should succeed for minimal package");
        assert_basic_runtime_pkg(&runtime_pkg, original_id, version_id);
    }

    #[test]
    fn translate_with_optimization() {
        let original_id = AccountAddress::from([3u8; 32]);
        let version_id = AccountAddress::from([4u8; 32]);
        let verified = make_verified_empty_package(original_id, version_id);

        let vm_config = VMConfig {
            optimize_bytecode: true,
            ..VMConfig::default()
        };
        let natives = crate::natives::functions::NativeFunctions::empty_for_testing().unwrap();

        let result = translate_package(&vm_config, &natives, verified);
        let runtime_pkg = result.expect("translate_package should succeed for minimal package");
        assert_basic_runtime_pkg(&runtime_pkg, original_id, version_id);
    }
}
