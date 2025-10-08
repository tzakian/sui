// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

//! Machine state management for the Move interpreter.
//!
//! This module defines the core execution state of the interpreter including
//! the call stack, current frame management, and overall machine state.
//! It tracks the execution context as the interpreter processes bytecode instructions.
//!
//! Key components:
//! - **MachineState**: Top-level interpreter state
//! - **CallStack**: Stack of function frames for nested calls
//! - **Frame**: Individual function execution context
//! - Program counter and operand stack management

//! Virtual machine execution state management.
//!
//! This module provides the core state structures for the Move VM interpreter:
//!
//! - **MachineState**: The top-level execution context containing call stack and operand stack
//! - **CallStack**: Manages function call frames and their lifetimes
//! - **CallFrame**: Individual function execution context with locals and program counter
//! - **ValueStack**: The operand stack for intermediate computation values
//!
//! These structures work together to provide a complete execution environment that
//! tracks program state, manages memory, and enforces VM limits like stack depth.

use crate::{
    execution::{
        dispatch_tables::VMDispatchTables,
        interpreter::{locals::MachineHeap, set_err_info},
        values::values_impl::{self as values, VMValueCast, Value},
    },
    jit::execution::ast::{Function, Type},
    shared::{
        constants::{CALL_STACK_SIZE_LIMIT, OPERAND_STACK_SIZE_LIMIT},
        views::TypeView,
        vm_pointer::VMPointer,
    },
};
use move_binary_format::errors::*;
use move_core_types::{
    language_storage::TypeTag,
    vm_status::{StatusCode, StatusType},
};

use std::{cmp::min, fmt::Write};
use tracing::error;

use super::locals::StackFrame;

macro_rules! debug_write {
    ($($toks: tt)*) => {
        write!($($toks)*).map_err(|_|
            PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR)
                .with_message("failed to write to buffer".to_string())
        )
    };
}

macro_rules! debug_writeln {
    ($($toks: tt)*) => {
        writeln!($($toks)*).map_err(|_|
            PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR)
                .with_message("failed to write to buffer".to_string())
        )
    };
}

// -------------------------------------------------------------------------------------------------
// Types
// -------------------------------------------------------------------------------------------------

/// The complete execution state for running Move bytecode.
/// Manages both the call stack (for function calls) and operand stack (for computations).
/// Each MachineState represents a single thread of execution in the VM.
pub(crate) struct MachineState {
    pub(crate) call_stack: CallStack,
    /// Operand stack, where Move `Value`s are stored for stack operations.
    pub(crate) operand_stack: ValueStack,
}

/// The operand stack where intermediate values are stored during computation.
/// Used for bytecode operations that manipulate values before storing them in locals.
pub(crate) struct ValueStack {
    pub(crate) value: Vec<Value>,
}

/// The call stack manages active function invocations and their execution contexts.
/// Each function call creates a new frame with its own locals and program counter.
pub(crate) struct CallStack {
    /// The current frame we are computing in.
    pub(crate) current_frame: CallFrame,
    /// The current heap.
    pub(crate) heap: MachineHeap,
    /// The stack of active functions.
    pub(crate) frames: Vec<CallFrame>,
}

/// A single function's execution context containing its local variables,
/// program counter, type arguments, and reference to the function being executed.
#[derive(Debug)]
pub(crate) struct CallFrame {
    pub(crate) pc: u16,
    pub(crate) function: VMPointer<Function>,
    pub(crate) stack_frame: StackFrame,
    pub(crate) ty_args: Vec<Type>,
}

/// Helper struct for resolving type information during execution.
/// Combines a type reference with dispatch tables to enable type tag conversion.
pub(super) struct ResolvableType<'a, 'b> {
    pub(super) ty: &'a Type,
    pub(super) vtables: &'b VMDispatchTables,
}

// -------------------------------------------------------------------------------------------------
// impl Blocks
// -------------------------------------------------------------------------------------------------

impl MachineState {
    /// Creates a new machine state with the given call stack.
    /// Initializes an empty operand stack for computation.
    pub(super) fn new(call_stack: CallStack) -> Self {
        MachineState {
            operand_stack: ValueStack::new(),
            call_stack,
        }
    }

    /// Pushes a value onto the operand stack, checking stack size limits.
    /// Returns an error if the stack would overflow.
    #[inline]
    pub fn push_operand(&mut self, value: Value) -> PartialVMResult<()> {
        self.operand_stack.push(value)
    }

    /// Pops a value from the operand stack.
    /// Returns an error if the stack is empty.
    #[inline]
    pub fn pop_operand(&mut self) -> PartialVMResult<Value> {
        self.operand_stack.pop()
    }

    /// Pops a value from the operand stack and casts it to the specified type.
    /// Returns an error if the stack is empty or the cast fails.
    #[inline]
    pub fn pop_operand_as<T>(&mut self) -> PartialVMResult<T>
    where
        Value: VMValueCast<T>,
    {
        self.operand_stack.pop_as()
    }

    /// Pops n values from the operand stack and returns them in a vector.
    /// Returns an error if there aren't enough values on the stack.
    #[inline]
    pub fn pop_n_operands(&mut self, n: u16) -> PartialVMResult<Vec<Value>> {
        self.operand_stack.pop_n(n)
    }

    /// Returns an iterator over the last n values on the operand stack without removing them.
    /// Used for gas charging and validation before operations.
    #[inline]
    pub fn last_n_operands(
        &self,
        n: usize,
    ) -> PartialVMResult<impl ExactSizeIterator<Item = &Value>> {
        self.operand_stack.last_n(n)
    }

    /// Pushes a new function call frame onto the call stack.
    /// The current frame becomes the new frame and the old frame is saved.
    /// Returns an error if the call stack would overflow.
    #[inline]
    pub fn push_call(
        &mut self,
        function: VMPointer<Function>,
        ty_args: Vec<Type>,
        args: Vec<Value>,
    ) -> VMResult<()> {
        self.call_stack.push_call(function, ty_args, args)
    }

    /// Checks if there are saved frames that can be popped from the call stack.
    /// Returns false when only the initial frame remains.
    #[inline]
    pub(super) fn can_pop_call_frame(&self) -> bool {
        !self.call_stack.frames.is_empty()
    }

    /// Pops the current call frame and restores the previous one.
    /// Returns an error if there's no frame to restore (shouldn't happen in normal execution).
    #[inline]
    pub(super) fn pop_call_frame(&mut self) -> VMResult<()> {
        self.call_stack.pop_frame()
    }

    //
    // Debugging and logging helpers.
    //

    /// Generates a core dump for invariant violations and verification errors.
    /// Logs the complete execution state to help with debugging critical failures.
    pub fn maybe_core_dump(&self, err: VMError) -> VMError {
        let err = if err.status_type() == StatusType::Verification {
            error!("Verification error during runtime: {:?}", err);
            let new_err = PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR);
            let new_err = match err.message() {
                None => new_err.with_message("No message provided for core dump".to_owned()),
                Some(msg) => new_err.with_message(msg.to_owned()),
            };
            new_err.finish(err.location().clone())
        } else {
            err
        };
        if err.status_type() == StatusType::InvariantViolation {
            let state = self.internal_state_str();
            error!(
                "Error: {:?}\nCORE DUMP: >>>>>>>>>>>>\n{}\n<<<<<<<<<<<<\n",
                err, state,
            );
        }
        err
    }

    #[allow(dead_code)]
    pub(super) fn debug_print_frame<B: Write>(
        &self,
        buf: &mut B,
        vtables: &VMDispatchTables,
        idx: usize,
        frame: &CallFrame,
    ) -> PartialVMResult<()> {
        // Print out the function name with type arguments.
        let _func_ptr = &frame.function;
        let func = frame.function();

        debug_write!(buf, "    [{}] ", idx)?;
        let module = func.module_id();
        debug_write!(buf, "{}::{}::", module.address(), module.name(),)?;

        debug_write!(buf, "{}", func.name())?;
        let ty_args = frame.ty_args();
        let mut ty_tags = vec![];
        for ty in ty_args {
            ty_tags.push(vtables.type_to_type_tag(ty)?);
        }
        if !ty_tags.is_empty() {
            debug_write!(buf, "<")?;
            let mut it = ty_tags.iter();
            if let Some(tag) = it.next() {
                debug_write!(buf, "{}", tag)?;
                for tag in it {
                    debug_write!(buf, ", ")?;
                    debug_write!(buf, "{}", tag)?;
                }
            }
            debug_write!(buf, ">")?;
        }
        debug_writeln!(buf)?;

        // Print out the current instruction.
        debug_writeln!(buf)?;
        debug_writeln!(buf, "        Code:")?;
        let pc = frame.pc as usize;
        let code = func.code();
        let before = if pc > 3 { pc - 3 } else { 0 };
        let after = min(code.len(), pc + 4);
        for (idx, instr) in code.iter().enumerate().take(pc).skip(before) {
            debug_writeln!(buf, "            [{}] {:?}", idx, instr)?;
        }
        debug_writeln!(buf, "          > [{}] {:?}", pc, &code[pc])?;
        for (idx, instr) in code.iter().enumerate().take(after).skip(pc + 1) {
            debug_writeln!(buf, "            [{}] {:?}", idx, instr)?;
        }

        // Print out the locals.
        debug_writeln!(buf)?;
        debug_writeln!(buf, "        Locals:")?;
        if func.local_count() > 0 {
            values::debug::print_stack_frame(buf, &frame.stack_frame)?;
            debug_writeln!(buf)?;
        } else {
            debug_writeln!(buf, "            (none)")?;
        }

        debug_writeln!(buf)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn debug_print_stack_trace<B: Write>(
        &self,
        buf: &mut B,
        vtables: &VMDispatchTables,
    ) -> PartialVMResult<()> {
        debug_writeln!(buf, "Call Stack:")?;
        self.debug_print_frame(buf, vtables, 0, &self.call_stack.current_frame)?;
        for (i, frame) in self.call_stack.frames.iter().enumerate() {
            self.debug_print_frame(buf, vtables, i + 1, frame)?;
        }
        debug_writeln!(buf, "Operand Stack:")?;
        for (idx, val) in self.operand_stack.value.iter().enumerate() {
            // TODO: Currently we do not know the types of the values on the operand stack.
            // Revisit.
            debug_write!(buf, "    [{}] ", idx)?;
            values::debug::print_value(buf, val)?;
            debug_writeln!(buf)?;
        }
        Ok(())
    }

    /// Generates a detailed string representation of the interpreter's internal state.
    /// Includes call stack, current bytecode, local variables, and operand stack.
    /// Used for core dumps and debugging.
    fn internal_state_str(&self) -> String {
        let mut internal_state = "Call stack:\n".to_string();

        for (i, frame) in self.call_stack.frames.iter().enumerate() {
            internal_state.push_str(
                format!(
                    " frame #{}: {} [pc = {}]\n",
                    i,
                    frame.function().pretty_string(),
                    frame.pc,
                )
                .as_str(),
            );
        }
        internal_state.push_str(
            format!(
                "*frame #{}: {} [pc = {}]:\n",
                self.call_stack.frames.len(),
                self.call_stack.current_frame.function().pretty_string(),
                self.call_stack.current_frame.pc,
            )
            .as_str(),
        );
        let code = self.call_stack.current_frame.function().code();
        let pc = self.call_stack.current_frame.pc as usize;
        if pc < code.len() {
            let mut i = 0;
            for bytecode in &code[..pc] {
                internal_state.push_str(format!("{}> {:?}\n", i, bytecode).as_str());
                i += 1;
            }
            internal_state.push_str(format!("{}* {:?}\n", i, code[pc]).as_str());
        }
        internal_state
            .push_str(format!("Locals:\n{}\n", self.call_stack.current_frame.stack_frame).as_str());
        internal_state.push_str("Operand Stack:\n");
        for value in &self.operand_stack.value {
            internal_state.push_str(format!("{}\n", value).as_str());
        }
        internal_state
    }

    /// Converts a partial error to a full VMError by adding the current execution location.
    pub(super) fn set_location(&self, err: PartialVMError) -> VMError {
        err.finish(self.call_stack.current_frame.location())
    }

    /// Returns the complete execution state for error reporting and debugging.
    pub(super) fn get_internal_state(&self) -> ExecutionState {
        self.get_stack_frames(usize::MAX)
    }

    /// Gets a limited number of stack frames for error reporting.
    /// Returns frames in reverse order (outermost frame last) for standard stack trace format.
    pub fn get_stack_frames(&self, count: usize) -> ExecutionState {
        // collect frames in the reverse order as this is what is
        // normally expected from the stack trace (outermost frame
        // is the last one)
        let stack_trace = self
            .call_stack
            .frames
            .iter()
            .rev()
            .take(count)
            .map(|frame| {
                let fun = frame.function();
                (fun.module_id().clone(), fun.index(), frame.pc)
            })
            .collect();
        ExecutionState::new(stack_trace)
    }
}

impl ValueStack {
    /// Creates a new empty operand stack with no values.
    fn new() -> Self {
        ValueStack { value: vec![] }
    }

    /// Pushes a value onto the operand stack, enforcing size limits.
    /// Returns an error if the stack would exceed the maximum size.
    fn push(&mut self, value: Value) -> PartialVMResult<()> {
        if self.value.len() < OPERAND_STACK_SIZE_LIMIT {
            self.value.push(value);
            Ok(())
        } else {
            Err(PartialVMError::new(StatusCode::EXECUTION_STACK_OVERFLOW))
        }
    }

    /// Pops a value from the operand stack.
    /// Returns an error if the stack is empty.
    fn pop(&mut self) -> PartialVMResult<Value> {
        self.value
            .pop()
            .ok_or_else(|| PartialVMError::new(StatusCode::EMPTY_VALUE_STACK))
    }

    /// Pops a value from the stack and attempts to cast it to the specified type.
    /// Returns an error if the stack is empty or the cast fails.
    fn pop_as<T>(&mut self) -> PartialVMResult<T>
    where
        Value: VMValueCast<T>,
    {
        VMValueCast::cast(self.pop()?)
    }

    /// Pops n values from the stack and returns them as a vector.
    /// Returns an error if there aren't enough values on the stack.
    fn pop_n(&mut self, n: u16) -> PartialVMResult<Vec<Value>> {
        let remaining_stack_size = self
            .value
            .len()
            .checked_sub(n as usize)
            .ok_or_else(|| PartialVMError::new(StatusCode::EMPTY_VALUE_STACK))?;
        let args = self.value.split_off(remaining_stack_size);
        Ok(args)
    }

    /// Returns an iterator over the last n values on the stack without removing them.
    /// Used for inspecting values before operations.
    fn last_n(&self, n: usize) -> PartialVMResult<impl ExactSizeIterator<Item = &Value>> {
        if self.value.len() < n {
            return Err(PartialVMError::new(StatusCode::EMPTY_VALUE_STACK)
                .with_message("Failed to get last n arguments on the argument stack".to_string()));
        }
        Ok(self.value[(self.value.len() - n)..].iter())
    }

    pub(crate) fn len(&self) -> usize {
        self.value.len()
    }

    pub(crate) fn value_at(&self, n: usize) -> Option<&Value> {
        self.value.get(n)
    }
}

impl CallStack {
    /// Creates a new call stack with an initial function frame.
    /// Sets up the machine heap and allocates the first stack frame with the provided arguments.
    pub fn new(
        function: VMPointer<Function>,
        ty_args: Vec<Type>,
        args: Vec<Value>,
    ) -> PartialVMResult<Self> {
        let mut heap = MachineHeap::new();

        let stack_frame = heap.allocate_stack_frame(args, function.local_count())?;
        let current_frame = CallFrame {
            pc: 0,
            stack_frame,
            function,
            ty_args,
        };

        Ok(Self {
            current_frame,
            heap,
            frames: vec![],
        })
    }

    /// Pushes a new call frame onto the stack, making it the current frame.
    /// Allocates a new stack frame for the function's locals and validates call stack depth.
    #[inline]
    pub fn push_call(
        &mut self,
        function: VMPointer<Function>,
        ty_args: Vec<Type>,
        args: Vec<Value>,
    ) -> VMResult<()> {
        let stack_frame = self
            .heap
            .allocate_stack_frame(args, function.local_count())
            .map_err(|err| set_err_info!(&self.current_frame, err))?;
        let new_frame = CallFrame {
            pc: 0,
            stack_frame,
            function,
            ty_args,
        };
        if self.frames.len() < CALL_STACK_SIZE_LIMIT {
            let prev_frame = std::mem::replace(&mut self.current_frame, new_frame);
            self.frames.push(prev_frame);
            Ok(())
        } else {
            let err = PartialVMError::new(StatusCode::CALL_STACK_OVERFLOW);
            let err = set_err_info!(new_frame, err);
            Err(err)
        }
    }

    /// Pops the current frame and restores the previous one.
    /// Frees the stack frame memory and handles cleanup.
    #[inline]
    fn pop_frame(&mut self) -> VMResult<()> {
        let Some(return_frame) = self.frames.pop() else {
            let err = PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR);
            let err = set_err_info!(self.current_frame, err);
            return Err(err);
        };
        let frame = std::mem::replace(&mut self.current_frame, return_frame);
        let index = frame.function().index();
        let pc = frame.pc;
        let loc = frame.location();
        self.heap
            .free_stack_frame(frame.stack_frame)
            .map_err(|e| e.at_code_offset(index, pc).finish(loc))
    }
}

impl CallFrame {
    /// Returns a reference to the function being executed in this frame.
    pub(super) fn function<'a>(&self) -> &'a Function {
        self.function.to_ref()
    }

    /// Returns the type arguments used to instantiate this function call.
    pub(super) fn ty_args(&self) -> &[Type] {
        &self.ty_args
    }

    /// Returns the location information for error reporting.
    pub(super) fn location(&self) -> Location {
        Location::Module(self.function().module_id().clone())
    }
}

// -------------------------------------------------------------------------------------------------
// Other impls
// -------------------------------------------------------------------------------------------------

impl TypeView for ResolvableType<'_, '_> {
    fn to_type_tag(&self) -> TypeTag {
        self.vtables.type_to_type_tag(self.ty).unwrap()
    }
}
