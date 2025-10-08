// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

//! Core type definitions for the Move VM.
//!
//! This module defines fundamental types used throughout the VM including
//! identifiers for packages, modules, and types. These types provide
//! the foundation for the VM's type system and package management.
//!
//! Key types:
//! - **OriginalId**: Runtime package identifier
//! - **DefiningTypeId**: Unique type definition identifier
//! - **VersionId**: Package version tracking
//! - Various identifier types for modules, functions, and fields

use move_core_types::account_address::AccountAddress;

// -------------------------------------------------------------------------------------------------
// Types
// -------------------------------------------------------------------------------------------------

/// The package version ID that a type was defined at, i.e.., the first version the type defintion
/// appears as.
pub type DefiningTypeId = AccountAddress;

/// Version ID: the ID of a given version of the package.
/// For v0 this matches the original ID; for all others it is the on-chain publication ID of that
/// package version. This is use for linkage contexts, etc.
pub type VersionId = AccountAddress;

/// Original ID: An original package ID for v0 of the package.
/// This is the original publication ID, and all versions use it at runtime.
/// This is consistent between versions (e.g., v0 and v1 will use the same Runtime Package ID).
pub type OriginalId = AccountAddress;
