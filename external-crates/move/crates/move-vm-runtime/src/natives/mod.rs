// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

//! Native function implementations for the Move VM.
//!
//! This module provides the infrastructure for implementing native functions
//! that can be called from Move bytecode. Native functions allow the VM to
//! interface with the host environment and provide functionality that would
//! be impossible or inefficient to implement in pure Move code.
//!
//! Key components:
//! - **functions**: Core native function framework and registry
//! - **extensions**: Extension mechanisms for custom native function sets
//! - **move_stdlib**: Standard library native function implementations
//!
//! Native functions are registered by module and function name, and can
//! be invoked during bytecode execution when encountered.

pub mod extensions;
pub mod functions;
pub mod move_stdlib;

use functions::NativeFunction;

/// Helper function to create a module's native function registry.
/// Converts function names to strings and packages them with their implementations.
pub fn make_module_natives(
    natives: impl IntoIterator<Item = (impl Into<String>, NativeFunction)>,
) -> impl Iterator<Item = (String, NativeFunction)> {
    natives
        .into_iter()
        .map(|(func_name, func)| (func_name.into(), func))
}
