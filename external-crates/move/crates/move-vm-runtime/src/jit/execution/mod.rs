// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

//! Execution-optimized AST representation for Move bytecode.
//!
//! This module defines the runtime AST that represents compiled Move code
//! in a form optimized for interpretation. The AST is more efficient than
//! raw bytecode as it pre-resolves references and optimizes common patterns.
//!
//! The execution AST is created during package loading and cached for reuse
//! across multiple executions, amortizing compilation costs.

//! Runtime execution representation and translation.
//!
//! This module contains the runtime AST representation used during Move VM execution
//! and the translation logic that converts optimized bytecode into this executable form.
//!
//! - **ast**: Runtime AST types and data structures
//! - **translate**: Translation from optimized bytecode to runtime representation

pub mod ast;
pub mod translate;
