// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

//! Core execution engine for the Move VM.
//!
//! This module implements the runtime execution of Move bytecode, providing the
//! interpreter, value management, and dispatch mechanisms needed to run Move programs.
//! The execution engine is the heart of the VM, transforming verified bytecode into
//! actual computation.
//!
//! Key components:
//! - **Interpreter**: Stack-based bytecode interpreter with frame management
//! - **Values**: Runtime representation of Move values including references
//! - **Dispatch tables**: Virtual method tables for polymorphic function calls
//! - **VM context**: Execution environment with gas metering and native functions
//! - **Tracing**: Execution tracing for debugging and profiling
//!
//! The execution engine integrates with:
//! - The validation layer to ensure only verified code runs
//! - The cache system to retrieve compiled packages
//! - The native function interface for host-provided functionality
//! - The gas metering system for resource accounting
//!
//! Execution flow:
//! 1. Functions are resolved through dispatch tables
//! 2. The interpreter creates stack frames for function calls
//! 3. Bytecode instructions manipulate the operand stack and locals
//! 4. Values are managed with Move's ownership and reference semantics
//! 5. Gas is metered throughout execution

pub mod dispatch_tables;
pub mod interpreter;
pub mod tracing;
pub mod values;
pub mod vm;
pub use crate::jit::execution::ast::Type;
pub use crate::jit::execution::ast::TypeSubst;
