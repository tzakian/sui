#!/bin/bash
# Script to convert simple static error messages to VMErrorMessage format
# Run this from the v1 directory

set -e

echo "Converting simple static error messages to VMErrorMessage format..."
echo "=================================================================="

# Array of files to process
FILES=(
    "crates/move-vm-types/src/values/values_impl.rs"
    "crates/move-vm-types/src/lib.rs"
    "crates/move-vm-types/src/loaded_data/runtime_types.rs"
    "crates/move-vm-runtime/src/data_cache.rs"
    "crates/move-vm-runtime/src/runtime.rs"
    "crates/move-vm-runtime/src/interpreter.rs"
    "crates/move-vm-runtime/src/logging.rs"
    "crates/move-vm-runtime/src/loader.rs"
    "crates/move-stdlib-natives/src/debug.rs"
    "crates/move-bytecode-verifier/src/type_safety.rs"
    "crates/move-bytecode-verifier/src/struct_defs.rs"
    "crates/move-bytecode-verifier/src/stack_usage_verifier.rs"
    "crates/move-bytecode-verifier/src/signature.rs"
    "crates/move-bytecode-verifier/src/script_signature.rs"
    "crates/move-bytecode-verifier/src/reference_safety/mod.rs"
    "crates/move-bytecode-verifier/src/locals_safety/mod.rs"
    "crates/move-bytecode-verifier/src/instruction_consistency.rs"
    "crates/move-bytecode-verifier/src/instantiation_loops.rs"
    "crates/move-bytecode-verifier/src/dependencies.rs"
    "crates/move-bytecode-verifier/src/acquires_list_verifier.rs"
)

# Function to add VMErrorMessage import
add_vmerrormessage_import() {
    local file=$1
    if ! grep -q "VMErrorMessage" "$file" 2>/dev/null; then
        # Try to add VMErrorMessage to existing errors import
        if grep -q "use move_binary_format::errors::{" "$file"; then
            # Multi-line import
            if grep -q "use move_binary_format::errors::{.*PartialVMError" "$file"; then
                sed -i.bak 's/PartialVMError/PartialVMError, VMErrorMessage/' "$file"
            fi
        elif grep -q "use move_binary_format::errors::PartialVMError" "$file"; then
            # Single import - convert to multi-line
            sed -i.bak 's/use move_binary_format::errors::PartialVMError/use move_binary_format::errors::{PartialVMError, VMErrorMessage}/' "$file"
        fi
    fi
}

# Simple static conversions (safe to automate)
convert_simple_static() {
    local file=$1

    # Check if file exists
    if [ ! -f "$file" ]; then
        echo "  WARNING: File not found: $file"
        return
    fi

    echo ""
    echo "Processing: $file"

    local changes=0

    # Failed to write to buffer
    if grep -q '\.with_message("failed to write to buffer"' "$file"; then
        perl -i.bak -pe 's/\.with_message\("failed to write to buffer"(?:\.to_string\(\))?\)/.with_message(VMErrorMessage::FailedToWriteToBuffer)/g' "$file"
        changes=$((changes + 1))
        echo "  ✓ Converted: FailedToWriteToBuffer"
    fi

    # Struct Definition not resolved
    if grep -q '\.with_message("Struct Definition not resolved"' "$file"; then
        perl -i.bak -pe 's/\.with_message\("Struct Definition not resolved"\.to_string\(\)\)/.with_message(VMErrorMessage::StructDefinitionNotResolved)/g' "$file"
        changes=$((changes + 1))
        echo "  ✓ Converted: StructDefinitionNotResolved"
    fi

    # Type parameter should be fully resolved
    if grep -q '\.with_message("Type parameter should be fully resolved"' "$file"; then
        perl -i.bak -pe 's/\.with_message\("Type parameter should be fully resolved"\.to_string\(\)\)/.with_message(VMErrorMessage::TypeParameterNotFullyResolved)/g' "$file"
        changes=$((changes + 1))
        echo "  ✓ Converted: TypeParameterNotFullyResolved"
    fi

    # Verifier failed to verify the deserialization of constants
    if grep -q '\.with_message("Verifier failed to verify the deserialization of constants"' "$file"; then
        perl -i.bak -pe 's/\.with_message\("Verifier failed to verify the deserialization of constants"\.to_owned\(\)\)/.with_message(VMErrorMessage::ConstantDeserializationFailure)/g' "$file"
        changes=$((changes + 1))
        echo "  ✓ Converted: ConstantDeserializationFailure"
    fi

    # Injected verifier failure
    if grep -q '\.with_message("Injected move_vm::interpreter verifier failure"' "$file"; then
        perl -i.bak -pe 's/\.with_message\("Injected move_vm::interpreter verifier failure"\.to_owned\(\)\)/.with_message(VMErrorMessage::InjectedVerifierFailure)/g' "$file"
        changes=$((changes + 1))
        echo "  ✓ Converted: InjectedVerifierFailure"
    fi

    # Failed to get last n arguments
    if grep -q '\.with_message("Failed to get last n arguments on the argument stack"' "$file"; then
        perl -i.bak -pe 's/\.with_message\("Failed to get last n arguments on the argument stack"\.to_string\(\)\)/.with_message(VMErrorMessage::FailedToGetArgumentsFromStack)/g' "$file"
        changes=$((changes + 1))
        echo "  ✓ Converted: FailedToGetArgumentsFromStack"
    fi

    # Arity mismatch
    if grep -q '\.with_message(\s*"Arity mismatch: return value count does not match return type count"' "$file"; then
        perl -i.bak -pe 's/\.with_message\(\s*"Arity mismatch: return value count does not match return type count"\s*\.to_string\(\)\s*,?\)/.with_message(VMErrorMessage::ArityMismatchReturnValues)/g' "$file"
        changes=$((changes + 1))
        echo "  ✓ Converted: ArityMismatchReturnValues"
    fi

    # Failed in write_ref: type mismatch
    if grep -q '\.with_message(\s*"failed.*write_ref.*type mismatch"' "$file"; then
        perl -i.bak -pe 's/\.with_message\(\s*"failed.*?write_ref.*?type mismatch"\.to_string\(\)\s*\)/.with_message(VMErrorMessage::FailedWriteRefTypeMismatch)/g' "$file"
        changes=$((changes + 1))
        echo "  ✓ Converted: FailedWriteRefTypeMismatch"
    fi

    if [ $changes -gt 0 ]; then
        add_vmerrormessage_import "$file"
        echo "  ✓ Made $changes simple conversions"
        rm -f "${file}.bak"
    else
        echo "  - No simple conversions needed (may have complex format! calls)"
    fi
}

# Process all files
for file in "${FILES[@]}"; do
    convert_simple_static "$file"
done

echo ""
echo "=================================================================="
echo "Simple conversions complete!"
echo "Note: Complex format!() expressions need manual conversion."
echo "See ERROR_CONVERSION_GUIDE.md for details."
echo "=================================================================="
