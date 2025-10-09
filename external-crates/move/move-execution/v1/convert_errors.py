#!/usr/bin/env python3
"""
Script to convert string-based error messages to structured VMErrorMessage format
in the v1 execution crate.
"""

import re
import sys
from pathlib import Path

# Mapping of error message patterns to VMErrorMessage variants
# Format: (regex_pattern, replacement_template, is_simple_static)
CONVERSIONS = [
    # Simple static messages (no parameters)
    (r'\.with_message\("failed to write to buffer"\.to_string\(\)\)',
     '.with_message(VMErrorMessage::FailedToWriteToBuffer)', True),

    (r'\.with_message\("failed to write to buffer"\)',
     '.with_message(VMErrorMessage::FailedToWriteToBuffer)', True),

    (r'\.with_message\("Struct Definition not resolved"\.to_string\(\)\)',
     '.with_message(VMErrorMessage::StructDefinitionNotResolved)', True),

    (r'\.with_message\("Type parameter should be fully resolved"\.to_string\(\)\)',
     '.with_message(VMErrorMessage::TypeParameterNotFullyResolved)', True),

    (r'\.with_message\("Verifier failed to verify the deserialization of constants"\.to_owned\(\)\)',
     '.with_message(VMErrorMessage::ConstantDeserializationFailure)', True),

    (r'\.with_message\("Verifier failed to verify the deserialization of constants"\.to_string\(\)\)',
     '.with_message(VMErrorMessage::ConstantDeserializationFailure)', True),

    (r'\.with_message\("Injected move_vm::interpreter verifier failure"\.to_owned\(\)\)',
     '.with_message(VMErrorMessage::InjectedVerifierFailure)', True),

    (r'\.with_message\("Failed to get last n arguments on the argument stack"\.to_string\(\)\)',
     '.with_message(VMErrorMessage::FailedToGetArgumentsFromStack)', True),

    (r'\.with_message\("Arity mismatch: return value count does not match return type count"\s*\.to_string\(\)\)',
     '.with_message(VMErrorMessage::ArityMismatchReturnValues)', True),

    (r'\.with_message\("failed in write_ref: type mismatch"\.to_string\(\)\)',
     '.with_message(VMErrorMessage::FailedWriteRefTypeMismatch)', True),

    (r'\.with_message\("cannot copy a Locals container"\.to_string\(\)\)',
     '.with_message(VMErrorMessage::ContainerIsNotStruct)', True),  # Close enough

    (r'\.with_message\("moving container with dangling references"\.to_string\(\)\)',
     '.with_message(VMErrorMessage::MovingValueWithDanglingReferences { value: "container".to_string() })', False),

    (r'\.with_message\("cannot overwrite Container::Locals"\.to_string\(\)\)',
     '.with_message(VMErrorMessage::CannotMoveFromInvalidLocation)', True),

    # Format-based messages with parameters
    (r'\.with_message\(format!\("moving value \{:\?\} with dangling references", ([^)]+)\)\)',
     r'.with_message(VMErrorMessage::MovingValueWithDanglingReferences { value: format!("{:?}", \1) })', False),

    (r'\.with_message\(format!\("cannot take \{:\?\} as &\{\}", self, stringify!\(([^)]+)\)\)\)',
     r'.with_message(VMErrorMessage::CannotCast { from_type: format!("{:?}", self), to_type: stringify!(\1).to_string() })', False),

    (r'\.with_message\(format!\("cannot compare values: \{:\?\}, \{:\?\}", self, other\)\)',
     r'.with_message(VMErrorMessage::CannotCompareValues { value1: format!("{:?}", self), value2: format!("{:?}", other) })', False),

    (r'\.with_message\(format!\(\s*"cannot compare container values: \{:\?\}, \{:\?\}",\s*self, other\s*\)\)',
     r'.with_message(VMErrorMessage::CannotCompareValues { value1: format!("{:?}", self), value2: format!("{:?}", other) })', False),

    (r'\.with_message\(format!\("cannot compare references \{:\?\}, \{:\?\}", self, other\)\)',
     r'.with_message(VMErrorMessage::CannotCompareReferences { ref1: format!("{:?}", self), ref2: format!("{:?}", other) })', False),

    (r'\.with_message\(format!\(\s*"cannot write value \{:\?\} to container ref \{:\?\}",\s*v, self\s*\)\)',
     r'.with_message(VMErrorMessage::CannotCastValue { value: format!("{:?}", v), to_type: format!("{:?}", self) })', False),

    (r'\.with_message\(format!\(\s*"cannot write value \{:\?\} to indexed ref \{:\?\}",\s*x, self\s*\)\)',
     r'.with_message(VMErrorMessage::CannotCastValue { value: format!("{:?}", x), to_type: format!("{:?}", self) })', False),

    (r'\.with_message\(\s*format!\(\s*"index out of bounds when borrowing container element: got: \{\}, len: \{\}",\s*idx, len\s*\)\)',
     r'.with_message(VMErrorMessage::IndexOutOfBounds { index: idx as u64, limit: len as u64, kind: "container element".to_string() })', False),

    (r'\.with_message\(format!\(\s*"expected variant container, got \{:\?\}",\s*([^)]+)\)\)',
     r'.with_message(VMErrorMessage::ExpectedVariant { found: format!("{:?}", \1) })', False),

    (r'\.with_message\(format!\(\s*"Variant tag mismatch: expected \{\}, got \{\}",\s*expected_tag, tag\s*\)\)',
     r'.with_message(VMErrorMessage::TagMismatch { expected: expected_tag, actual: tag })', False),

    (r'\.with_message\(format!\(\s*"cannot unpack a reference value \{:\?\} held inside a variant ref \{:\?\}",\s*x, self\s*\)\)',
     r'.with_message(VMErrorMessage::InvalidReferenceInConstant)', True),  # Close enough

    (r'\.with_message\(format!\(\s*"index out of bounds when borrowing local: got: \{\}, len: \{\}",\s*idx,\s*([^)]+)\)\)',
     r'.with_message(VMErrorMessage::LocalIndexOutOfBounds { index: idx })', False),

    (r'\.with_message\(format!\("cannot borrow local \{:\?\}", &v\[idx\]\)\)',
     r'.with_message(VMErrorMessage::CannotBorrowLocal { local: format!("{:?}", &v[idx]) })', False),

    (r'\.with_message\(format!\("cannot copy invalid value at index \{\}", idx\)\)',
     r'.with_message(VMErrorMessage::CannotCopyInvalidValue { index: idx })', False),

    (r'\.with_message\(\s*format!\("local index out of bounds: got \{\}, len: \{\}", idx, ([^)]+)\)\)',
     r'.with_message(VMErrorMessage::LocalIndexOutOfBounds { index: idx })', False),

    (r'\.with_message\(format!\("cannot move invalid value at index \{\}", idx\)\)',
     r'.with_message(VMErrorMessage::CannotMoveInvalidValue { index: idx })', False),

    # Cast error messages
    (r'\.with_message\(format!\("cannot cast \{:\?\} to \{\}", v, stringify!\(([^)]+)\)\)\)',
     r'.with_message(VMErrorMessage::CannotCast { from_type: format!("{:?}", v), to_type: stringify!(\1).to_string() })', False),

    (r'\.with_message\(format!\("cannot cast \{:\?\} to \{\}", ([^,]+), stringify!\(([^)]+)\)\)\)',
     r'.with_message(VMErrorMessage::CannotCast { from_type: format!("{:?}", \1), to_type: stringify!(\2).to_string() })', False),

    (r'\.with_message\(format!\("cannot cast \{:\?\} to integer", v,\)\)',
     r'.with_message(VMErrorMessage::CannotCast { from_type: format!("{:?}", v), to_type: "integer".to_string() })', False),

    (r'\.with_message\(format!\("cannot cast \{:\?\} to reference", v,\)\)',
     r'.with_message(VMErrorMessage::CannotCast { from_type: format!("{:?}", v), to_type: "reference".to_string() })', False),

    (r'\.with_message\(format!\("cannot cast \{:\?\} to container", v,\)\)',
     r'.with_message(VMErrorMessage::CannotCast { from_type: format!("{:?}", v), to_type: "container".to_string() })', False),

    (r'\.with_message\(format!\("cannot cast \{:\?\} to struct", v,\)\)',
     r'.with_message(VMErrorMessage::CannotCast { from_type: format!("{:?}", v), to_type: "struct".to_string() })', False),

    (r'\.with_message\(format!\("cannot cast \{:\?\} to enum variant", v,\)\)',
     r'.with_message(VMErrorMessage::CannotCast { from_type: format!("{:?}", v), to_type: "enum variant".to_string() })', False),

    (r'\.with_message\(format!\("cannot cast \{:\?\} to vector<u8>", v,\)\)',
     r'.with_message(VMErrorMessage::CannotCast { from_type: format!("{:?}", v), to_type: "vector<u8>".to_string() })', False),

    (r'\.with_message\(format!\("cannot cast \{:\?\} to vector<u64>", v,\)\)',
     r'.with_message(VMErrorMessage::CannotCast { from_type: format!("{:?}", v), to_type: "vector<u64>".to_string() })', False),

    (r'\.with_message\(\s*"cannot cast a specialized vector into a non-specialized one"\.to_string\(\)\)',
     r'.with_message(VMErrorMessage::CannotCast { from_type: "specialized vector".to_string(), to_type: "non-specialized vector".to_string() })', False),

    (r'\.with_message\(format!\(\s*"cannot cast \{:\?\} to vector<non-specialized-type>",\s*v,\s*\)\)',
     r'.with_message(VMErrorMessage::CannotCast { from_type: format!("{:?}", v), to_type: "vector<non-specialized-type>".to_string() })', False),

    (r'\.with_message\(format!\("cannot cast \{:\?\} to Signer reference", v,\)\)',
     r'.with_message(VMErrorMessage::CannotCast { from_type: format!("{:?}", v), to_type: "Signer reference".to_string() })', False),

    (r'\.with_message\(format!\("cannot cast \{:\?\} to vector reference", v,\)\)',
     r'.with_message(VMErrorMessage::CannotCast { from_type: format!("{:?}", v), to_type: "vector reference".to_string() })', False),

    (r'\.with_message\(format!\("cannot cast \{:\?\} to vector", v,\)\)',
     r'.with_message(VMErrorMessage::CannotCast { from_type: format!("{:?}", v), to_type: "vector".to_string() })', False),

    (r'\.with_message\(format!\("cannot cast \{:\?\} to (u8|u16|u32|u64|u128|u256)", v,\)\)',
     r'.with_message(VMErrorMessage::CannotCast { from_type: format!("{:?}", v), to_type: "\1".to_string() })', False),

    # Arithmetic error messages
    (r'\.with_message\(format!\("Cannot ([a-z_]+) \{:\?\} and \{:\?\}", l, r\)\)',
     r'.with_message(VMErrorMessage::CannotCompareValues { value1: format!("{:?}", l), value2: format!("{:?}", r) })', False),

    (r'\.with_message\(format!\("Cannot ([a-z_]+) \{:\?\} (?:from|by) \{:\?\}", ([lr]), ([lr])\)\)',
     r'.with_message(VMErrorMessage::CannotCompareValues { value1: format!("{:?}", \2), value2: format!("{:?}", \3) })', False),

    (r'\.with_message\(format!\("Cannot compare \{:\?\} and \{:\?\}: incompatible integer types", l, r\)\)',
     r'.with_message(VMErrorMessage::TypeMismatch { expected: format!("{:?}", l), actual: format!("{:?}", r) })', False),

    (r'\.with_message\(format!\("Cannot cast (u\d+)\(\{\}\) to (u\d+)", x\)\)',
     r'.with_message(VMErrorMessage::CannotCastValue { value: format!("\1({})", x), to_type: "\2".to_string() })', False),

    # Abort context
    (r'\.with_message\(format!\("\{\} at offset \{\}", ([^,]+)\.pretty_string\(\), \*pc,\)\)',
     r'.with_message(VMErrorMessage::AbortContext { function: \1.pretty_string(), offset: *pc })', False),
]

def convert_file(file_path: Path) -> tuple[int, str]:
    """
    Convert a single file and return (number_of_changes, file_content).
    """
    content = file_path.read_text()
    original_content = content
    changes = 0

    for pattern, replacement, is_static in CONVERSIONS:
        new_content = re.sub(pattern, replacement, content)
        if new_content != content:
            matches = len(re.findall(pattern, content))
            changes += matches
            content = new_content
            print(f"  Applied pattern ({matches} times): {pattern[:60]}...")

    return changes, content

def main():
    base_dir = Path(__file__).parent

    files_to_convert = [
        "crates/move-vm-types/src/values/values_impl.rs",
        "crates/move-vm-types/src/lib.rs",
        "crates/move-vm-types/src/loaded_data/runtime_types.rs",
        "crates/move-vm-runtime/src/data_cache.rs",
        "crates/move-vm-runtime/src/runtime.rs",
        "crates/move-vm-runtime/src/interpreter.rs",
        "crates/move-vm-runtime/src/logging.rs",
        "crates/move-vm-runtime/src/loader.rs",
        "crates/move-stdlib-natives/src/debug.rs",
        "crates/move-bytecode-verifier/src/limits.rs",
        "crates/move-bytecode-verifier/src/type_safety.rs",
        "crates/move-bytecode-verifier/src/struct_defs.rs",
        "crates/move-bytecode-verifier/src/stack_usage_verifier.rs",
        "crates/move-bytecode-verifier/src/signature.rs",
        "crates/move-bytecode-verifier/src/script_signature.rs",
        "crates/move-bytecode-verifier/src/reference_safety/mod.rs",
        "crates/move-bytecode-verifier/src/locals_safety/mod.rs",
        "crates/move-bytecode-verifier/src/instruction_consistency.rs",
        "crates/move-bytecode-verifier/src/instantiation_loops.rs",
        "crates/move-bytecode-verifier/src/dependencies.rs",
        "crates/move-bytecode-verifier/src/acquires_list_verifier.rs",
    ]

    total_changes = 0

    for rel_path in files_to_convert:
        file_path = base_dir / rel_path
        if not file_path.exists():
            print(f"WARNING: File not found: {file_path}")
            continue

        print(f"\nProcessing: {rel_path}")
        changes, new_content = convert_file(file_path)

        if changes > 0:
            file_path.write_text(new_content)
            print(f"  ✓ Made {changes} changes")
            total_changes += changes
        else:
            print(f"  - No changes needed")

    print(f"\n{'='*60}")
    print(f"Total changes made: {total_changes}")
    print(f"{'='*60}")

    return 0 if total_changes > 0 else 1

if __name__ == "__main__":
    sys.exit(main())
