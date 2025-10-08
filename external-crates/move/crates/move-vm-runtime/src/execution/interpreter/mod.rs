// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

//! Stack-based bytecode interpreter for Move execution.
//!
//! This module implements the core interpreter that executes Move bytecode instructions.
//! The interpreter manages a call stack of frames, each with its own operand stack and locals,
//! following a traditional stack machine architecture.
//!
//! Key components:
//! - **Evaluation engine**: Executes bytecode instructions
//! - **Call stack**: Manages function frames and returns
//! - **Locals management**: Handles function parameters and local variables
//! - **Machine state**: Maintains interpreter execution state
//!
//! The interpreter integrates with:
//! - Dispatch tables for function resolution
//! - Native extensions for host functions
//! - Gas metering for resource accounting
//! - Execution tracing for debugging

use crate::{
    execution::{
        dispatch_tables::VMDispatchTables,
        interpreter::state::{CallStack, MachineState},
        tracing::trace,
        values::Value,
    },
    jit::execution::ast::{Function, Type},
    natives::extensions::NativeContextExtensions,
    shared::{gas::GasMeter, vm_pointer::VMPointer},
};
use move_binary_format::errors::*;
use move_vm_config::runtime::VMConfig;
use std::sync::Arc;

mod eval;
pub(crate) mod helpers;
pub mod locals;
pub(crate) mod state;

/// Main entry point for executing a Move function.
///
/// Sets up the execution environment and runs either native or bytecode functions.
/// Handles initial frame setup, gas metering, and tracing throughout execution.
///
/// # Parameters
/// - `vtables`: Dispatch tables for function and type resolution
/// - `vm_config`: Configuration parameters for the VM
/// - `extensions`: Native function extensions
/// - `tracer`: Optional execution tracer for debugging
/// - `gas_meter`: Gas metering for resource accounting
/// - `function`: The function to execute
/// - `ty_args`: Type arguments for generic functions
/// - `args`: Runtime arguments to the function
///
/// # Returns
/// Vector of return values from the function execution
///
/// # Errors
/// Returns VM errors for execution failures, type mismatches, or gas exhaustion
pub(crate) fn run(
    vtables: &mut VMDispatchTables,
    vm_config: Arc<VMConfig>,
    extensions: &mut NativeContextExtensions,
    tracer: &mut Option<VMTracer<'_>>,
    gas_meter: &mut impl GasMeter,
    function: VMPointer<Function>,
    ty_args: Vec<Type>,
    args: Vec<Value>,
) -> VMResult<Vec<Value>> {
    let fun_ref = function.to_ref();
    trace(tracer, |tracer| {
        tracer.enter_initial_frame(
            vtables,
            &gas_meter.remaining_gas().into(),
            function.ptr_clone().to_ref(),
            &ty_args,
            &args,
        )
    });

    if fun_ref.is_native() {
        let return_result = eval::call_native_with_args(
            None,
            vtables,
            gas_meter,
            &vm_config.runtime_limits_config,
            extensions,
            fun_ref,
            &ty_args,
            args.into(),
        )
        .map_err(|e| {
            e.at_code_offset(fun_ref.index(), 0)
                .finish(Location::Module(fun_ref.module_id().clone()))
        });
        trace(tracer, |tracer| {
            tracer.exit_initial_native_frame(&return_result, &gas_meter.remaining_gas().into())
        });
        return_result.map(|values| values.into_iter().collect())
    } else {
        let call_stack = CallStack::new(function, ty_args, args).map_err(|e| {
            e.at_code_offset(fun_ref.index(), 0)
                .finish(Location::Module(fun_ref.module_id().clone()))
        })?;
        let state = MachineState::new(call_stack);
        eval::run(state, vtables, vm_config, extensions, tracer, gas_meter)
    }
}

macro_rules! set_err_info {
    ($frame:expr, $e:expr) => {{
        let function = $frame.function();
        $e.at_code_offset(function.index(), $frame.pc)
            .finish($frame.location())
    }};
}

pub(crate) use set_err_info;

use super::tracing::tracer::VMTracer;
