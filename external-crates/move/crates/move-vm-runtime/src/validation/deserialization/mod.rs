//! Binary package deserialization.
//!
//! This module converts serialized Move bytecode into the VM's internal AST representation.
//! Deserialization is the first step in package loading, transforming the compact
//! binary format into structures suitable for verification and execution.
//!
//! The deserialization process:
//! - Validates binary format structure
//! - Resolves constant pool references
//! - Builds the initial AST representation
//! - Prepares packages for verification

pub mod ast;
pub mod translate;
