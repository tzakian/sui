// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use std::{collections::HashMap, hash::Hash};

use move_binary_format::errors::PartialVMResult;

use crate::cache::arena::ArenaVec;

pub mod binary_cache;
pub mod constants;
pub mod gas;
pub mod linkage_context;
pub mod logging;
pub mod types;
pub mod views;
pub mod vm_pointer;

#[macro_export]
macro_rules! try_block {
    ($($body:tt)*) => {{
        #[allow(clippy::redundant_closure_call)]
        (|| {
            $($body)*
        })()
    }};
}

#[macro_export]
macro_rules! partial_vm_error {
    ($error_name:ident $(,)?) => {{
        move_binary_format::errors::PartialVMError::new(
            move_core_types::vm_status::StatusCode::$error_name,
        )
    }};
    ($error_name:ident, $($body:tt)*) => {{
        move_binary_format::errors::PartialVMError::new(
            move_core_types::vm_status::StatusCode::$error_name,
        ).with_message(
            format!($($body)*),
        )
    }};
}

// NB: this does the lookup separately from the insertion, as otherwise would require copying the
// key to retrieve the entry and support the error case.
#[allow(clippy::map_entry)]
/// Either returns a BTreeMap of unique keys, or a repeated key if the input keys are not unique.
pub fn unique_map<Key: Hash + Eq, Value>(
    values: impl IntoIterator<Item = (Key, Value)>,
) -> Result<HashMap<Key, Value>, Key> {
    let mut map = HashMap::new();
    for (k, v) in values {
        if map.contains_key(&k) {
            return Err(k);
        } else {
            map.insert(k, v);
        }
    }
    Ok(map)
}

pub trait SafeIndex<T> {
    fn at(&self, index: usize) -> PartialVMResult<&T>;
}

impl<T> SafeIndex<T> for Vec<T> {
    fn at(&self, index: usize) -> PartialVMResult<&T> {
        self.get(index).ok_or_else(|| {
            partial_vm_error!(
                INDEX_OUT_OF_BOUNDS,
                "Index {} out of bounds for vector of length {}",
                index,
                self.len()
            )
        })
    }
}

impl<T> SafeIndex<T> for &[T] {
    fn at(&self, index: usize) -> PartialVMResult<&T> {
        self.get(index).ok_or_else(|| {
            partial_vm_error!(
                INDEX_OUT_OF_BOUNDS,
                "Index {} out of bounds for slice of length {}",
                index,
                self.len()
            )
        })
    }
}

impl<T> SafeIndex<T> for ArenaVec<T> {
    fn at(&self, index: usize) -> PartialVMResult<&T> {
        self.get(index).ok_or_else(|| {
            partial_vm_error!(
                INDEX_OUT_OF_BOUNDS,
                "Index {} out of bounds for arena vector of length {}",
                index,
                self.len()
            )
        })
    }
}
