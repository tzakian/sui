// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

//! Execution tracing infrastructure for debugging and profiling.
//!
//! This module provides hooks for observing VM execution at various points,
//! enabling debugging tools, profilers, and execution analysis.
//! Tracing is optional and has minimal overhead when disabled.
//!
//! Tracing capabilities:
//! - Function entry and exit
//! - Instruction execution
//! - Value operations
//! - Gas consumption tracking

//! Execution tracing support for the Move VM.
//!
//! This module provides tracing capabilities to record and analyze VM execution.
//! Tracing can be enabled at compile time with the "tracing" feature flag and provides
//! detailed information about instruction execution, function calls, and state changes.
//!
//! The tracing system is designed to have minimal overhead when disabled and provides
//! a clean interface for optional tracing operations during VM execution.

use self::tracer::VMTracer;

pub mod tracer;

#[cfg(feature = "tracing")]
pub(crate) const TRACING_ENABLED: bool = true;

#[cfg(not(feature = "tracing"))]
pub(crate) const TRACING_ENABLED: bool = false;

/// Conditionally executes a tracing operation if tracing is enabled.
/// This function provides a zero-cost abstraction when tracing is disabled at compile time.
#[inline]
pub(crate) fn trace<'a, F: Fn(&mut VMTracer<'a>)>(tracer: &mut Option<VMTracer<'a>>, op: F) {
    if TRACING_ENABLED {
        if let Some(tracer) = tracer {
            op(tracer)
        }
    }
}
