// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

//! Bytecode optimization framework for the Move VM.
//!
//! This module provides infrastructure for bytecode optimization passes that
//! improve execution performance. Optimizations operate on the intermediate
//! AST representation before final compilation to execution format.
//!
//! Current optimizations:
//! - Dead code elimination: Removes unreachable code
//! - Future: Constant propagation, inlining, etc.
//!
//! The optimization pipeline is extensible, allowing new passes to be added
//! as needed for performance improvements.

//! Bytecode optimization framework for the Move VM.
//!
//! This module implements optimization passes that improve the performance
//! of Move bytecode while preserving program semantics. It provides both
//! a simple pass-through mode and an optimizing mode that applies various
//! transformation passes.
//!
//! Current optimizations include:
//! - Dead code elimination to remove unreachable instructions
//! - Future optimizations can be added in the optimizations module

use crate::{dbg_println, validation::verification::ast as input};

pub mod ast;
pub mod optimizations;
pub mod translate;

/// Converts verified bytecode to the optimization AST without applying optimizations.
/// This provides a direct translation that maintains the original program structure.
pub fn to_optimized_form(input: input::Package) -> ast::Package {
    translate::package(input)
}

/// Applies optimization passes to the verified bytecode.
/// Currently performs dead code elimination with debug logging of transformations.
pub fn optimize(input: input::Package) -> ast::Package {
    let mut opt = translate::package(input);
    dbg_println!(flag: optimizer, "Blocks: {:#?}", opt);
    optimizations::dead_code_elim::package(&mut opt);
    dbg_println!(flag: optimizer, "Dead Code elim: {:#?}", opt);
    opt
}
