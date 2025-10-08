// Copyright (c) The Diem Core Contributors
// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

//! Runtime value representation for Move execution.
//!
//! This module defines how Move values are represented during execution,
//! including primitive types, structs, vectors, and references.
//! The value system enforces Move's ownership and borrowing rules at runtime.
//!
//! Key concepts:
//! - Values can be owned, borrowed immutably, or borrowed mutably
//! - References track their origin for safety
//! - Container types (structs, vectors) maintain internal structure
//! - Conversion between runtime and serialized representations

//! Value representation and manipulation for the Move VM.
//!
//! This module provides the runtime value system that represents all Move values during execution.
//! It includes implementations for primitive types, complex types like structs and vectors,
//! and reference types that enable Move's borrowing semantics.
//!
//! The value system supports:
//! - Primitive types (integers, booleans, addresses)
//! - Complex types (structs, enums, vectors)
//! - Reference types (mutable and immutable references)
//! - Memory management and lifetime tracking
//! - Type casting and validation

pub mod values_impl;
pub use values_impl::*;
