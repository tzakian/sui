# V1 Error Message Conversion Guide

## Overview
This guide documents the conversion of string-based error messages to structured `VMErrorMessage` format in the v1 execution crate. The conversion follows the same pattern established in v2.

## Files Requiring Conversion

### Move VM Types
1. **crates/move-vm-types/src/values/values_impl.rs** - ~70+ occurrences
2. **crates/move-vm-types/src/lib.rs**
3. **crates/move-vm-types/src/loaded_data/runtime_types.rs**

### Move VM Runtime
4. **crates/move-vm-runtime/src/data_cache.rs**
5. **crates/move-vm-runtime/src/runtime.rs**
6. **crates/move-vm-runtime/src/interpreter.rs** - ~15+ occurrences
7. **crates/move-vm-runtime/src/logging.rs**
8. **crates/move-vm-runtime/src/loader.rs** - ~10+ occurrences

### Move Stdlib Natives
9. **crates/move-stdlib-natives/src/vector.rs** - ✅ COMPLETE (already using VMErrorMessage)
10. **crates/move-stdlib-natives/src/debug.rs** - ~12+ occurrences

### Move Bytecode Verifier
11. **crates/move-bytecode-verifier/src/limits.rs** - ✅ COMPLETE
12. **crates/move-bytecode-verifier/src/type_safety.rs**
13. **crates/move-bytecode-verifier/src/struct_defs.rs**
14. **crates/move-bytecode-verifier/src/stack_usage_verifier.rs**
15. **crates/move-bytecode-verifier/src/signature.rs**
16. **crates/move-bytecode-verifier/src/script_signature.rs**
17. **crates/move-bytecode-verifier/src/reference_safety/mod.rs**
18. **crates/move-bytecode-verifier/src/locals_safety/mod.rs**
19. **crates/move-bytecode-verifier/src/instruction_consistency.rs**
20. **crates/move-bytecode-verifier/src/instantiation_loops.rs**
21. **crates/move-bytecode-verifier/src/dependencies.rs**
22. **crates/move-bytecode-verifier/src/acquires_list_verifier.rs**

## Common Conversion Patterns

### 1. Simple Static Messages

#### Pattern: Failed to write to buffer
```rust
// Before:
.with_message("failed to write to buffer".to_string())

// After:
.with_message(VMErrorMessage::FailedToWriteToBuffer)
```

#### Pattern: Struct Definition Not Resolved
```rust
// Before:
.with_message("Struct Definition not resolved".to_string())

// After:
.with_message(VMErrorMessage::StructDefinitionNotResolved)
```

#### Pattern: Type Parameter Unresolved
```rust
// Before:
.with_message("Type parameter should be fully resolved".to_string())

// After:
.with_message(VMErrorMessage::TypeParameterNotFullyResolved)
```

#### Pattern: Constant Deserialization
```rust
// Before:
.with_message("Verifier failed to verify the deserialization of constants".to_owned())

// After:
.with_message(VMErrorMessage::ConstantDeserializationFailure)
```

#### Pattern: Injected Verifier Failure
```rust
// Before:
.with_message("Injected move_vm::interpreter verifier failure".to_owned())

// After:
.with_message(VMErrorMessage::InjectedVerifierFailure)
```

#### Pattern: Stack Arguments Error
```rust
// Before:
.with_message("Failed to get last n arguments on the argument stack".to_string())

// After:
.with_message(VMErrorMessage::FailedToGetArgumentsFromStack)
```

#### Pattern: Arity Mismatch
```rust
// Before:
.with_message("Arity mismatch: return value count does not match return type count".to_string())

// After:
.with_message(VMErrorMessage::ArityMismatchReturnValues)
```

#### Pattern: Write Ref Type Mismatch
```rust
// Before:
.with_message("failed in write_ref: type mismatch".to_string())

// After:
.with_message(VMErrorMessage::FailedWriteRefTypeMismatch)
```

### 2. Dynamic Messages with Parameters

#### Pattern: Moving Value with Dangling References
```rust
// Before:
.with_message(format!("moving value {:?} with dangling references", r))

// After:
.with_message(VMErrorMessage::MovingValueWithDanglingReferences {
    value: format!("{:?}", r)
})
```

#### Pattern: Cannot Cast
```rust
// Before:
.with_message(format!("cannot take {:?} as &{}", self, stringify!($ty)))

// After:
.with_message(VMErrorMessage::CannotCast {
    from_type: format!("{:?}", self),
    to_type: stringify!($ty).to_string()
})
```

```rust
// Before:
.with_message(format!("cannot cast {:?} to {}", v, stringify!($ty)))

// After:
.with_message(VMErrorMessage::CannotCast {
    from_type: format!("{:?}", v),
    to_type: stringify!($ty).to_string()
})
```

```rust
// Before:
.with_message(format!("cannot cast {:?} to integer", v,))

// After:
.with_message(VMErrorMessage::CannotCast {
    from_type: format!("{:?}", v),
    to_type: "integer".to_string()
})
```

#### Pattern: Cannot Cast Value
```rust
// Before:
.with_message(format!("Cannot cast u16({}) to u8", x))

// After:
.with_message(VMErrorMessage::CannotCastValue {
    value: format!("u16({})", x),
    to_type: "u8".to_string()
})
```

#### Pattern: Cannot Compare
```rust
// Before:
.with_message(format!("cannot compare values: {:?}, {:?}", self, other))

// After:
.with_message(VMErrorMessage::CannotCompareValues {
    value1: format!("{:?}", self),
    value2: format!("{:?}", other)
})
```

```rust
// Before:
.with_message(format!("cannot compare references {:?}, {:?}", self, other))

// After:
.with_message(VMErrorMessage::CannotCompareReferences {
    ref1: format!("{:?}", self),
    ref2: format!("{:?}", other)
})
```

```rust
// Before:
.with_message(format!("Cannot compare {:?} and {:?}: incompatible integer types", l, r))

// After:
.with_message(VMErrorMessage::TypeMismatch {
    expected: format!("{:?}", l),
    actual: format!("{:?}", r)
})
```

#### Pattern: Index Out of Bounds
```rust
// Before:
.with_message(format!(
    "index out of bounds when borrowing container element: got: {}, len: {}",
    idx, len
))

// After:
.with_message(VMErrorMessage::IndexOutOfBounds {
    index: idx as u64,
    limit: len as u64,
    kind: "container element".to_string()
})
```

```rust
// Before:
.with_message(format!("local index out of bounds: got {}, len: {}", idx, v.len()))

// After:
.with_message(VMErrorMessage::LocalIndexOutOfBounds { index: idx })
```

#### Pattern: Tag/Variant Errors
```rust
// Before:
.with_message(format!(
    "expected variant container, got {:?}",
    self.0.container()
))

// After:
.with_message(VMErrorMessage::ExpectedVariant {
    found: format!("{:?}", self.0.container())
})
```

```rust
// Before:
.with_message(format!(
    "Variant tag mismatch: expected {}, got {}",
    expected_tag, tag
))

// After:
.with_message(VMErrorMessage::TagMismatch {
    expected: expected_tag,
    actual: tag
})
```

#### Pattern: Cannot Copy/Move Invalid Value
```rust
// Before:
.with_message(format!("cannot copy invalid value at index {}", idx))

// After:
.with_message(VMErrorMessage::CannotCopyInvalidValue { index: idx })
```

```rust
// Before:
.with_message(format!("cannot move invalid value at index {}", idx))

// After:
.with_message(VMErrorMessage::CannotMoveInvalidValue { index: idx })
```

#### Pattern: Cannot Borrow Local
```rust
// Before:
.with_message(format!("cannot borrow local {:?}", &v[idx]))

// After:
.with_message(VMErrorMessage::CannotBorrowLocal {
    local: format!("{:?}", &v[idx])
})
```

#### Pattern: Abort Context
```rust
// Before:
.with_message(format!("{} at offset {}", function.pretty_string(), *pc,))

// After:
.with_message(VMErrorMessage::AbortContext {
    function: function.pretty_string(),
    offset: *pc
})
```

#### Pattern: Vector Size Limit
```rust
// Before:
.with_message(format!("vector size limit is {}", lim))

// After:
.with_message(VMErrorMessage::VectorSizeLimit { limit: lim })
```

### 3. Required Import Addition

For each file being converted, ensure the import includes `VMErrorMessage`:

```rust
// Before:
use move_binary_format::{
    errors::{PartialVMError, PartialVMResult},
    // ...
};

// After:
use move_binary_format::{
    errors::{PartialVMError, PartialVMResult, VMErrorMessage},
    // ...
};
```

## New VMErrorMessage Variants Needed

Based on analysis of v1 code, the following new variants may need to be added to `VMErrorMessage`:

1. **Debug-related errors** (from debug.rs):
   - Errors related to formatting and type conversion in debug natives
   - May need generic error types or reuse existing ones

2. **Loader-related errors** (from loader.rs):
   - Missing dependency errors
   - Link/relocation errors
   - May need new variants or can reuse existing resolution errors

3. **Verifier-related errors** (from various verifier files):
   - Most can use existing variants
   - May need specific verifier invariant violation types

## Automation Approach

Due to the large number of files and occurrences (~150+ total), a semi-automated approach is recommended:

1. **Use regex-based search and replace** for simple patterns
2. **Manual review** for complex cases with nested format! macros
3. **Test compilation** after each file to ensure correctness
4. **Run tests** to verify behavior is preserved

## Example sed Commands

For simple conversions that are safe to automate:

```bash
# Convert "failed to write to buffer"
sed -i '' 's/.with_message("failed to write to buffer".to_string())/.with_message(VMErrorMessage::FailedToWriteToBuffer)/g' file.rs

# Convert "Struct Definition not resolved"
sed -i '' 's/.with_message("Struct Definition not resolved".to_string())/.with_message(VMErrorMessage::StructDefinitionNotResolved)/g' file.rs
```

## Status

- ✅ limits.rs - Converted (1 occurrence)
- ✅ vector.rs - Already correct (uses VMErrorMessage)
- ⏳ Remaining 20 files - Pending conversion

## Next Steps

1. Start with smaller files in bytecode-verifier
2. Then tackle stdlib-natives/debug.rs
3. Finally handle the large files in move-vm-types and move-vm-runtime
4. Add any missing VMErrorMessage variants to errors.rs as needed
5. Run full test suite to verify all conversions

## Important Notes

- **Do not change behavior** - only the error message format
- **Preserve all debugging information** - keep format!() where needed to include variable values
- **Test after each file** - ensure compilation succeeds
- **Update imports** - add VMErrorMessage to use statements
- **Be consistent** - use the same VMErrorMessage variant for the same error type across files
