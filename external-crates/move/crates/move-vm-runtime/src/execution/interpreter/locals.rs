// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

//! Local variable management for the Move interpreter.
//!
//! This module handles storage and access to local variables within function frames.
//! Each function frame maintains its own set of locals including parameters and
//! variables declared within the function body. The module ensures safe access
//! to locals with proper borrow checking and move semantics.
//!
//! Key features:
//! - Indexed access to local variables
//! - Move and copy operations respecting ownership
//! - Reference creation and management
//! - Safe destruction of locals when frames exit

//! Memory management structures for the Move VM interpreter.
//!
//! This module provides the heap and stack frame abstractions that manage
//! local variables and references during bytecode execution. The VM maintains
//! two separate memory spaces:
//!
//! - **BaseHeap**: Stores values that outlive individual function calls, such as
//!   arguments passed to native functions and objects referenced across frames.
//! - **MachineHeap**: Manages stack frames for function local variables during execution.
//!
//! Stack frames contain local variables for a single function call, with proper
//! lifetime management to ensure references don't outlive their referenced values.

#![allow(unsafe_code)]

use crate::execution::values::{MemBox, values_impl::Value};

use move_binary_format::errors::{PartialVMError, PartialVMResult};
use move_core_types::vm_status::StatusCode;

use std::collections::HashMap;

// -------------------------------------------------------------------------------------------------
// Heap
// -------------------------------------------------------------------------------------------------

/// The Move VM's base heap stores values that persist beyond individual function calls.
/// This includes arguments to native functions and values that can be referenced
/// across different execution contexts. Each value is assigned a unique ID for tracking.
#[derive(Debug)]
pub struct BaseHeap {
    next_id: usize,
    values: HashMap<BaseHeapId, MemBox<Value>>,
}

/// Unique identifier for values stored in the BaseHeap.
/// Used to track and access values that outlive individual function calls.
#[derive(Clone, Copy, Debug, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub struct BaseHeapId(usize);

/// The runtime machine heap manages stack frames during function execution.
/// Currently simplified to just allocate and deallocate stack frames,
/// but provides the foundation for more sophisticated memory management.
#[derive(Debug)]
pub struct MachineHeap {}

/// A stack frame containing local variables for a single function call.
/// Manages the lifetime of local variables and ensures proper cleanup
/// when the function returns or encounters an error.
#[derive(Debug)]
pub struct StackFrame {
    slice: Vec<MemBox<Value>>,
}

// -------------------------------------------------------------------------------------------------
// Base (Machine-External) Heap
// -------------------------------------------------------------------------------------------------

impl Default for BaseHeap {
    fn default() -> Self {
        Self::new()
    }
}

impl BaseHeap {
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
            next_id: 0,
        }
    }

    /// Allocates a slot for a value in the base heap and returns its ID.
    /// The value will persist until explicitly removed or the heap is dropped.
    pub fn allocate_value(&mut self, value: Value) -> BaseHeapId {
        let next_id = BaseHeapId(self.next_id);
        self.next_id += 1;
        self.values.insert(next_id, MemBox::new(value));
        next_id
    }

    /// Allocates a value in the base heap and immediately creates a reference to it.
    /// Returns both the heap ID and a reference value for immediate use.
    pub fn allocate_and_borrow_loc(
        &mut self,
        value: Value,
    ) -> PartialVMResult<(BaseHeapId, Value)> {
        let id = self.allocate_value(value);
        let ref_ = self.borrow_loc(id)?;
        Ok((id, ref_))
    }

    /// Moves a value out of the base heap, replacing it with an invalid marker.
    /// The caller takes ownership of the value, and the slot becomes unusable.
    pub fn take_loc(&mut self, ndx: BaseHeapId) -> PartialVMResult<Value> {
        if self.is_invalid(ndx)? {
            return Err(
                PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR)
                    .with_message("Cannot move from an invalid memory location".to_string()),
            );
        }

        let Some(value_box) = self.values.get_mut(&ndx) else {
            return Err(
                PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR)
                    .with_message(format!("Invalid index: {}", ndx)),
            );
        };
        Ok(value_box.replace(Value::invalid()))
    }

    /// Creates a reference to a value in the base heap without moving it.
    /// The original value remains in the heap and can be borrowed multiple times.
    pub fn borrow_loc(&self, ndx: BaseHeapId) -> PartialVMResult<Value> {
        self.values
            .get(&ndx)
            .ok_or_else(|| {
                PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR)
                    .with_message(format!("Heap index invalid: {}", ndx))
            })
            .map(|value| value.as_ref_value())
    }

    /// Checks if the value at the given location has been moved out (invalid).
    /// Invalid values cannot be used and will cause runtime errors if accessed.
    pub fn is_invalid(&self, ndx: BaseHeapId) -> PartialVMResult<bool> {
        self.values
            .get(&ndx)
            .map(|value| matches!(&*value.borrow(), &Value::Invalid))
            .ok_or_else(|| {
                PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR)
                    .with_message(format!("Invalid index: {}", ndx))
            })
    }
}

// -------------------------------------------------------------------------------------------------
// Machine (Runtime) Heap
// -------------------------------------------------------------------------------------------------

impl Default for MachineHeap {
    fn default() -> Self {
        Self::new()
    }
}

impl MachineHeap {
    pub fn new() -> Self {
        Self {}
    }

    /// Allocates a new stack frame for function local variables.
    /// Initializes the frame with the provided parameter values and fills
    /// remaining slots with invalid markers.
    pub fn allocate_stack_frame(
        &mut self,
        params: Vec<Value>,
        size: usize,
    ) -> PartialVMResult<StackFrame> {
        // Calculate how many invalid values need to be added
        let invalids_len = size - params.len();

        // Initialize the stack frame with the provided parameters and fill remaining slots with `Invalid`
        let local_values = params
            .into_iter()
            .chain((0..invalids_len).map(|_| Value::invalid())) // Fill the rest with `Invalid`
            .map(MemBox::new) // Make them into MemBoxes
            .collect::<Vec<MemBox<Value>>>();

        Ok(StackFrame {
            slice: local_values,
        })
    }

    /// Frees a stack frame and releases its resources.
    /// Currently a no-op since frames are automatically cleaned up on drop.
    pub fn free_stack_frame(&mut self, _frame: StackFrame) -> PartialVMResult<()> {
        Ok(())
    }
}

// -------------------------------------------------------------------------------------------------
// Stack Frame
// -------------------------------------------------------------------------------------------------

impl StackFrame {
    pub(crate) fn iter(&self) -> std::slice::Iter<'_, MemBox<Value>> {
        self.slice.iter()
    }

    /// Creates a copy of the value at the specified local variable index.
    /// The original value remains in the frame unchanged.
    pub fn copy_loc(&self, ndx: usize) -> PartialVMResult<Value> {
        self.get_valid(ndx).map(|value| value.borrow().copy_value())
    }

    /// Moves a value out of a local variable slot, marking it as invalid.
    /// The caller takes ownership and the slot cannot be used again.
    pub fn move_loc(&mut self, ndx: usize) -> PartialVMResult<Value> {
        let value_slot = self.get_valid_mut(ndx)?;
        Ok(std::mem::replace(
            &mut *value_slot.borrow_mut(),
            Value::invalid(),
        ))
    }

    /// Creates a reference to a local variable without moving the value.
    /// The original value remains accessible in the frame.
    pub fn borrow_loc(&mut self, ndx: usize) -> PartialVMResult<Value> {
        self.get_valid_mut(ndx).map(|value| value.as_ref_value())
    }

    /// Stores a value in the specified local variable slot.
    /// Overwrites any existing value at that location.
    pub fn store_loc(&mut self, ndx: usize, x: Value) -> PartialVMResult<()> {
        if ndx >= self.slice.len() {
            return Err(PartialVMError::new(StatusCode::INTERNAL_TYPE_ERROR)
                .with_message(format!("Local index out of bounds: {}", ndx)));
        }
        let _ = self.slice[ndx].replace(x);
        Ok(())
    }

    /// Gets a valid value reference by index, ensuring it's not out of bounds or invalid.
    /// Returns an error if the index is invalid or the slot contains an invalid value.
    fn get_valid(&self, ndx: usize) -> PartialVMResult<&MemBox<Value>> {
        self.slice
            .get(ndx)
            .ok_or_else(|| {
                PartialVMError::new(StatusCode::INTERNAL_TYPE_ERROR)
                    .with_message(format!("Local index out of bounds: {}", ndx))
            })
            .and_then(|value| {
                if matches!(&*value.borrow(), &Value::Invalid) {
                    Err(PartialVMError::new(StatusCode::INTERNAL_TYPE_ERROR)
                        .with_message(format!("Local index {} is unset", ndx)))
                } else {
                    Ok(value)
                }
            })
    }

    /// Gets a mutable reference to a valid value by index.
    /// Returns an error if the index is invalid or the slot contains an invalid value.
    fn get_valid_mut(&mut self, ndx: usize) -> PartialVMResult<&mut MemBox<Value>> {
        self.slice
            .get_mut(ndx)
            .ok_or_else(|| {
                PartialVMError::new(StatusCode::INTERNAL_TYPE_ERROR)
                    .with_message(format!("Local index out of bounds: {}", ndx))
            })
            .and_then(|value| {
                if matches!(&*value.borrow(), &Value::Invalid) {
                    Err(PartialVMError::new(StatusCode::INTERNAL_TYPE_ERROR)
                        .with_message(format!("Local index {} is unset", ndx)))
                } else {
                    Ok(value)
                }
            })
    }

    /// Drops all non-reference values from the stack frame for cleanup.
    /// References are invalidated but not dropped to avoid use-after-free issues.
    /// Returns an iterator over the dropped values for gas charging purposes.
    pub fn drop_all_values(&mut self) -> impl Iterator<Item = Value> {
        self.slice
            .iter_mut()
            .filter_map(|value| match &mut *value.borrow_mut() {
                Value::Invalid => None,
                value @ Value::Reference(_) => {
                    *value = Value::Invalid;
                    None
                }
                value @ (Value::U8(_)
                | Value::U16(_)
                | Value::U32(_)
                | Value::U64(_)
                | Value::U128(_)
                | Value::U256(_)
                | Value::Bool(_)
                | Value::Address(_)
                | Value::Vec(_)
                | Value::PrimVec(_)
                | Value::Struct(_)
                | Value::Variant(_)) => {
                    let result = std::mem::replace(value, Value::Invalid);
                    Some(result)
                }
            })
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[cfg(test)]
    #[allow(non_snake_case)]
    /// This is strictly for testing cycle dropping.
    /// If you ever mark this not #[cfg(test)] you will have your VM implementor card revoked.
    pub(crate) fn UNSAFE_copy_local_box(&mut self, ndx: usize) -> MemBox<Value> {
        self.slice[ndx].UNSAFE_ptr_clone()
    }
}

// -------------------------------------------------------------------------------------------------
// Display
// -------------------------------------------------------------------------------------------------

impl std::fmt::Display for StackFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "StackFrame(size: {})", self.slice.len())?;
        for (i, value) in self.slice.iter().enumerate() {
            writeln!(f, "  [{}]: {:?}", i, value)?;
        }
        Ok(())
    }
}

impl std::fmt::Display for BaseHeapId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "base#{}", self.0)
    }
}
