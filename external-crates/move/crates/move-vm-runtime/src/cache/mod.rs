// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

//! Caching infrastructure for the Move VM.
//!
//! This module provides the core caching mechanisms that enable efficient package loading
//! and execution in the Move VM. The cache system is designed to minimize memory usage
//! while maximizing performance through:
//!
//! - **Arena allocation**: Bulk memory management for package data
//! - **String interning**: Deduplication of identifiers across packages
//! - **Package caching**: Retention of verified and compiled packages
//!
//! The cache integrates with the VM's execution pipeline by storing validated and compiled
//! packages after they pass verification, making them immediately available for execution without
//! recompilation. This allows the same verified and compiled package to be relinked and reused
//! repeatedly and across multiple transactions.

pub mod arena;
pub mod identifier_interner;
pub mod move_cache;
