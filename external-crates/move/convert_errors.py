#!/usr/bin/env python3
"""
Script to convert old-style .with_message() error patterns to structured VMErrorMessage variants.
"""

import re
import sys
from pathlib import Path

# Common conversion patterns
CONVERSIONS = [
    # Simple static messages
    (r'\.with_message\("Recursive type\?".to_owned\(\)\)', '.with_message(VMErrorMessage::RecursiveType)'),
    (r'\.with_message\("Recursive type\?".to_string\(\)\)', '.with_message(VMErrorMessage::RecursiveType)'),

    # Package/Module/Function not found
    (r'\.with_message\(format!\("Package \{\} not found", ([^)]+)\)\)', r'.with_message(VMErrorMessage::PackageNotFound { package: \1.to_string() })'),
    (r'\.with_message\(format!\("Module \{\} not found", ([^)]+)\)\)', r'.with_message(VMErrorMessage::ModuleNotFound { module: \1.to_string() })'),
    (r'\.with_message\(format!\("Could not find function \{\}", ([^)]+)\)\)', r'.with_message(VMErrorMessage::FunctionNotFound { function: \1.to_string() })'),
    (r'\.with_message\(format!\("Could not find type \{\}", ([^)]+)\)\)', r'.with_message(VMErrorMessage::TypeNotFound { type_name: \1.to_string() })'),

    # Duplicate key
    (r'\.with_message\(format!\("Duplicate key \{\}", ([^)]+)\)\)', r'.with_message(VMErrorMessage::DuplicateKey { key: \1.to_string() })'),

    # Type layouts and tags
    (r'\.with_message\(format!\("no type tag for \{:?\?\}", ([^)]+)\)\)', r'.with_message(VMErrorMessage::NoTypeTag { type_info: format!("{:?}", \1) })'),
    (r'\.with_message\(format!\("no type layout for \{:?\?\}", ([^)]+)\)\)', r'.with_message(VMErrorMessage::NoTypeLayout { type_info: format!("{:?}", \1) })'),

    # Missing mapping
    (r'\.with_message\(format!\("\{:?\?\} missing mapping", ([^)]+)\)\)', r'.with_message(VMErrorMessage::MissingMapping { type_param: format!("{:?}", \1) })'),

    # Field length mismatch
    (r'\.with_message\("Field types did not match the length of field names in loaded enum variant".to_owned\(\)\)', '.with_message(VMErrorMessage::FieldTypeLengthMismatch { context: "loaded enum variant".to_string() })'),
    (r'\.with_message\("Field types did not match the length of field names in loaded struct".to_owned\(\)\)', '.with_message(VMErrorMessage::FieldTypeLengthMismatch { context: "loaded struct".to_string() })'),
]

def convert_file(file_path):
    """Convert a single file's error messages."""
    try:
        with open(file_path, 'r') as f:
            content = f.read()

        original_content = content

        # Apply conversions
        for pattern, replacement in CONVERSIONS:
            content = re.sub(pattern, replacement, content)

        # Check if VMErrorMessage import is needed
        if content != original_content and 'VMErrorMessage' in content:
            # Check if import exists
            if 'use move_binary_format::errors::' in content:
                # Add VMErrorMessage to existing import if not present
                if 'VMErrorMessage' not in content.split('use move_binary_format::errors::')[1].split(';')[0]:
                    content = re.sub(
                        r'(use move_binary_format::errors::\{[^}]+)',
                        r'\1, VMErrorMessage',
                        content
                    )

            # Write back if changes were made
            if content != original_content:
                with open(file_path, 'w') as f:
                    f.write(content)
                print(f"✓ Converted: {file_path}")
                return True
            else:
                print(f"✗ No changes: {file_path}")
                return False
        else:
            return False

    except Exception as e:
        print(f"✗ Error processing {file_path}: {e}")
        return False

def main():
    if len(sys.argv) < 2:
        print("Usage: python convert_errors.py <directory>")
        sys.exit(1)

    root_dir = Path(sys.argv[1])

    if not root_dir.exists():
        print(f"Directory does not exist: {root_dir}")
        sys.exit(1)

    # Find all .rs files
    rust_files = list(root_dir.rglob("*.rs"))

    print(f"Found {len(rust_files)} Rust files in {root_dir}")
    print("=" * 60)

    converted_count = 0
    for file_path in rust_files:
        if convert_file(file_path):
            converted_count += 1

    print("=" * 60)
    print(f"Converted {converted_count} files")

if __name__ == "__main__":
    main()
