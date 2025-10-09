// Copyright (c) The Diem Core Contributors
// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    IndexKind,
    file_format::{CodeOffset, FunctionDefinitionIndex, TableIndex},
};
use move_core_types::{
    account_address::AccountAddress,
    language_storage::ModuleId,
    vm_status::{StatusCode, StatusType},
};
use std::fmt;

/// Structured error messages for VM errors
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum VMErrorMessage {
    // Special: Message chaining for context building
    MessageChain(Vec<VMErrorMessage>),

    // Bounds/Index Errors
    IndexOutOfBounds {
        index: u64,
        limit: u64,
        kind: String,
    },
    IndexOutOfBoundsWithContext {
        index: u64,
        limit: u64,
        kind: String,
        function: u16,
        offset: u16,
    },

    // Type Errors
    CannotCast {
        from_type: String,
        to_type: String,
    },
    CannotCastValue {
        value: String,
        to_type: String,
    },
    ExpectedStruct {
        found: String,
    },
    ExpectedVector {
        found: String,
    },
    ExpectedVariant {
        found: String,
    },
    TypeMismatch {
        expected: String,
        actual: String,
    },
    TypeParameterCountMismatch {
        expected: usize,
        actual: usize,
        context: Option<String>,
    },

    // Comparison Errors
    CannotCompareValues {
        value1: String,
        value2: String,
    },
    CannotCompareReferences {
        ref1: String,
        ref2: String,
    },

    // Limit Errors
    VectorSizeLimit {
        limit: u64,
    },
    SizeExceeded {
        actual: usize,
        limit: usize,
        context: String,
    },
    JumpTableLengthMismatch {
        table_length: usize,
        variant_count: usize,
    },
    TooComplex {
        name: String,
        current: u128,
        new: u128,
        max: u128,
    },

    // Malformed Bytecode (static messages)
    BadUleb,
    UlebTooLarge,
    ErrorReadingTable,
    BadIdentifierPoolSize,
    InvalidIdentifier,
    BadAddressPoolSize,
    InvalidAddressFormat,
    UnexpectedEndOfTable,
    BadByteBlob,
    UnexpectedEOF,
    MaxRecursionDepth,
    InvalidFieldInfoInStruct,
    InvalidFieldInfoInEnum,
    EnumTypeWithNoVariants,
    InvalidVisibilityByte,
    InvalidJumpTableType,
    BadBinaryHeader,
    BinaryHeaderNotAllowed,
    BinaryHeaderTooShort,
    UnexpectedBinaryHeader,
    TableSizeTooBig,
    StructInstWithZeroArity,

    // Version Errors
    UnexpectedVariantOpcodeInVersion {
        version: u32,
    },
    MetadataNotApplicableInVersion {
        version: u32,
    },
    EnumNotSupportedInVersion {
        version: u32,
    },
    FriendNotApplicableInVersion {
        version: u32,
    },
    IntegerTypesNotSupportedInVersion {
        version: u32,
    },
    VectorOperationsNotSupportedInVersion {
        version: u32,
    },
    IntegerLoadCastNotSupportedInVersion {
        version: u32,
    },

    // Resolution Errors
    CannotFindInCache {
        identifier: String,
    },
    CannotFindPackageInCache {
        package: String,
    },
    MissingNativeFunction {
        name: String,
    },
    MissingNativeFunctionUnnamed,
    StructDefinitionNotResolved,
    InstantiationSignatureNotFound,
    NoTypeTag {
        type_info: String,
    },
    NoTypeLayout {
        type_info: String,
    },
    MissingMapping {
        type_param: String,
    },

    // Invalid State Errors
    CannotCopyInvalidValue {
        index: usize,
    },
    CannotMoveInvalidValue {
        index: usize,
    },
    CannotBorrowLocal {
        local: String,
    },
    FailedWriteRefTypeMismatch,
    MovingValueWithDanglingReferences {
        value: String,
    },
    InvalidValueInConstant,
    InvalidReferenceInConstant,

    // Container Type Errors
    ContainerIsNotStruct,
    ContainerIsNotVector,
    ContainerIsNotSigner,
    ValueIsNotVariant {
        value: String,
    },
    ValueIsNotVector {
        value: String,
    },
    ValueIsNotPrimitiveVector {
        value: String,
    },
    InvalidIndexedReference {
        reference: String,
    },

    // Tag/Variant Errors
    TagMismatch {
        expected: u16,
        actual: u16,
    },
    InvalidTypeParamForVector {
        type_param: String,
    },

    // Execution Context
    AbortContext {
        function: String,
        offset: u16,
    },
    FailedToGetArgumentsFromStack,

    // Linking Errors
    RelocationError {
        module_id: String,
        error: String,
    },
    PackageNotFoundAfterLoading,

    // Package Validation
    EmptyPackage,

    // Internal/Debug Errors
    FailedToWriteToBuffer,
    InjectedVerifierFailure,
    VerifierFailedConstantDeserialization,
    ArityMismatchReturnValues,
    RecursiveType,
    TypeParameterNotFullyResolved,
    VecPackUnpackArgumentOutOfRange,
    LookingForFieldInNativeStructure,
    PhantomParameterLengthMismatch,
    CurrentFunctionNotSetDuringBoundsChecking {
        offset: u16,
    },
    SafeUnwrapNone {
        file: &'static str,
        line: u32,
    },
    SafeUnwrapErr {
        file: &'static str,
        line: u32,
        error: String,
    },
    SafeAssertFailed {
        file: &'static str,
        line: u32,
    },
    LoopInInstantiationGraph {
        message: String,
    },
    ScriptVisibleFunctionCalledFromNonScriptVisible,
    DuplicateModuleName {
        module: String,
    },
    DuplicateStructDefinition {
        struct_name: String,
    },
    DuplicateStructHandleIndex {
        handle_idx: u16,
        def1: String,
        def2: usize,
    },
    DuplicateEnumHandleIndex {
        handle_idx: u16,
        def1: String,
        def2: usize,
    },
    ReferenceFieldInRecursiveStruct,
    FunctionNotFoundInVerifyModuleScriptFunction,
    DuplicateKey {
        key: String,
    },
    InvalidIndex {
        index: String,
    },
    LocalIndexOutOfBounds {
        index: usize,
    },
    LocalIndexUnset {
        index: usize,
    },
    DefiningIdConflict {
        defining_id: String,
        addr1: String,
        addr2: String,
    },
    PackageNotFound {
        package: String,
    },
    ModuleNotFound {
        module: String,
    },
    FunctionNotFound {
        function: String,
    },
    TypeNotFound {
        type_name: String,
    },
    DefiningIdNotFound {
        defining_id: String,
        type_tag: String,
    },
    RuntimeIdMismatch {
        defining_id: String,
        runtime_id_expected: String,
        runtime_id_actual: String,
    },
    DefiningIdMismatch {
        defining_id_expected: String,
        defining_id_actual: String,
    },
    FieldTypeLengthMismatch {
        context: String,
    },
    CannotMoveFromInvalidLocation,
    HeapIndexInvalid {
        index: String,
    },
    IntererLimitReached {
        ident: String,
        error: String,
    },
    FailedToFindKeyInInterner {
        key_type: String,
    },
    SignatureLookupFailed {
        context: String,
    },
    SignatureArity {
        expected: usize,
        actual: usize,
        context: String,
    },
    FunctionLookupFailed {
        context: String,
    },
    UnexpectedStorageError {
        error: String,
    },
    ExternalResolutionRequestError {
        context: String,
    },
    IndexLookupFailed {
        context: String,
    },
    ConstantDeserializationFailure,
    MultiplePackagesLoaded,
    NativeExtensionNotFound {
        extension: String,
    },
    DowncastError {
        context: String,
    },
    IndexOutOfBoundsInVec,
    IndexOutOfBoundsInPrimVec,
    VectorU8Expected,
    NotAResource,
    TypeSubstitutionFailed {
        len: usize,
        got: usize,
    },
    UnableToLoadConstTypeSignature,
    VecMutBorrowExpectsVectorReference,
    VecImmBorrowExpectsVectorReference,
    NativeFunctionGasParametersNotSpecified,
    CannotCastToInt {
        value: String,
    },
    CannotCastToVecValue {
        value: String,
    },
    ExpectedVectorContainer {
        found: String,
    },
    ExpectedVectorInReference {
        found: String,
    },
    SignerExpected,
    VectorElemLayoutMismatch {
        expected: String,
        got: String,
    },
    WriteMacroFailed {
        error: String,
    },
    CouldNotConvertVecMoveValueToVecU8,
    CouldNotGetInnerTypeOfVector,
    ExpectedMoveValueVectorOfU8,
    ExpectedMoveValueMoveStruct,
    ExpectedMoveDatatypeLayoutStruct,
    ExpectedMoveDatatypeLayoutEnum,
    CouldNotParseUtf8Bytes {
        error: String,
    },
    ExpectedStringStructWithOneField {
        num_fields: usize,
    },
    ExpectedStringStructWithBytesField {
        field_name: String,
    },
}

impl fmt::Display for VMErrorMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Message chaining
            VMErrorMessage::MessageChain(messages) => {
                for (i, msg) in messages.iter().enumerate() {
                    if i > 0 {
                        write!(f, "\n")?;
                    }
                    write!(f, "{}", msg)?;
                }
                Ok(())
            }

            // Bounds/Index Errors
            VMErrorMessage::IndexOutOfBounds { index, limit, kind } => {
                write!(
                    f,
                    "Index {} out of bounds for {} while indexing {}",
                    index, limit, kind
                )
            }
            VMErrorMessage::IndexOutOfBoundsWithContext {
                index,
                limit,
                kind,
                function,
                offset,
            } => {
                write!(
                    f,
                    "Index {} out of bounds for {} at bytecode offset {} in function {} while indexing {}",
                    index, limit, offset, function, kind
                )
            }

            // Type Errors
            VMErrorMessage::CannotCast { from_type, to_type } => {
                write!(f, "cannot cast {} to {}", from_type, to_type)
            }
            VMErrorMessage::CannotCastValue { value, to_type } => {
                write!(f, "cannot cast {} to {}", value, to_type)
            }
            VMErrorMessage::ExpectedStruct { found } => {
                write!(f, "Expected a Struct, found {}", found)
            }
            VMErrorMessage::ExpectedVector { found } => {
                write!(f, "Expected a Vector, found {}", found)
            }
            VMErrorMessage::ExpectedVariant { found } => {
                write!(f, "Expected a Variant, found {}", found)
            }
            VMErrorMessage::TypeMismatch { expected, actual } => {
                write!(f, "expected {} got {}", expected, actual)
            }
            VMErrorMessage::TypeParameterCountMismatch {
                expected,
                actual,
                context,
            } => {
                if let Some(ctx) = context {
                    write!(
                        f,
                        "expected {} type parameters got {} ({})",
                        expected, actual, ctx
                    )
                } else {
                    write!(f, "expected {} type parameters got {}", expected, actual)
                }
            }

            // Comparison Errors
            VMErrorMessage::CannotCompareValues { value1, value2 } => {
                write!(f, "cannot compare values: {}, {}", value1, value2)
            }
            VMErrorMessage::CannotCompareReferences { ref1, ref2 } => {
                write!(f, "cannot compare references {}, {}", ref1, ref2)
            }

            // Limit Errors
            VMErrorMessage::VectorSizeLimit { limit } => {
                write!(f, "vector size limit is {}", limit)
            }
            VMErrorMessage::SizeExceeded {
                actual,
                limit,
                context,
            } => {
                write!(f, "Exceeded size ({} > {}) in {}", actual, limit, context)
            }
            VMErrorMessage::JumpTableLengthMismatch {
                table_length,
                variant_count,
            } => {
                write!(
                    f,
                    "Jump table length {} does not equal number of variants {}",
                    table_length, variant_count
                )
            }
            VMErrorMessage::TooComplex {
                name,
                current,
                new,
                max,
            } => {
                write!(
                    f,
                    "program too complex (in `{}` with `{} current + {} new > {} max`)",
                    name, current, new, max
                )
            }

            // Malformed Bytecode (static messages)
            VMErrorMessage::BadUleb => write!(f, "Bad Uleb"),
            VMErrorMessage::UlebTooLarge => write!(f, "Uleb greater than max requested"),
            VMErrorMessage::ErrorReadingTable => write!(f, "Error reading table"),
            VMErrorMessage::BadIdentifierPoolSize => write!(f, "Bad Identifier pool size"),
            VMErrorMessage::InvalidIdentifier => write!(f, "Invalid Identifier"),
            VMErrorMessage::BadAddressPoolSize => write!(f, "Bad Address Identifier pool size"),
            VMErrorMessage::InvalidAddressFormat => write!(f, "Invalid Address format"),
            VMErrorMessage::UnexpectedEndOfTable => write!(f, "Unexpected end of table"),
            VMErrorMessage::BadByteBlob => write!(f, "Bad byte blob size"),
            VMErrorMessage::UnexpectedEOF => write!(f, "Unexpected EOF"),
            VMErrorMessage::MaxRecursionDepth => write!(f, "Maximum recursion depth reached"),
            VMErrorMessage::InvalidFieldInfoInStruct => write!(f, "Invalid field info in struct"),
            VMErrorMessage::InvalidFieldInfoInEnum => write!(f, "Invalid field info in enum"),
            VMErrorMessage::EnumTypeWithNoVariants => write!(f, "Enum type with no variants"),
            VMErrorMessage::InvalidVisibilityByte => write!(f, "Invalid visibility byte"),
            VMErrorMessage::InvalidJumpTableType => write!(f, "Invalid jump table type"),
            VMErrorMessage::BadBinaryHeader => write!(f, "Bad binary header"),
            VMErrorMessage::BinaryHeaderNotAllowed => write!(f, "Binary header not allowed"),
            VMErrorMessage::BinaryHeaderTooShort => write!(f, "Binary header too short"),
            VMErrorMessage::UnexpectedBinaryHeader => write!(f, "Unexpected binary header"),
            VMErrorMessage::TableSizeTooBig => write!(f, "Table size too big"),
            VMErrorMessage::StructInstWithZeroArity => write!(f, "Struct inst with arity 0"),

            // Version Errors
            VMErrorMessage::UnexpectedVariantOpcodeInVersion { version } => {
                write!(f, "Unexpected variant opcode in version {}", version)
            }
            VMErrorMessage::MetadataNotApplicableInVersion { version } => {
                write!(
                    f,
                    "metadata declarations not applicable in bytecode version {}",
                    version
                )
            }
            VMErrorMessage::EnumNotSupportedInVersion { version } => {
                write!(
                    f,
                    "Enum declarations not supported in bytecode versions less than {}",
                    version
                )
            }
            VMErrorMessage::FriendNotApplicableInVersion { version } => {
                write!(
                    f,
                    "Friend declarations not applicable in bytecode version {}",
                    version
                )
            }
            VMErrorMessage::IntegerTypesNotSupportedInVersion { version } => {
                write!(
                    f,
                    "u16, u32, u256 integers not supported in bytecode version {}",
                    version
                )
            }
            VMErrorMessage::VectorOperationsNotSupportedInVersion { version } => {
                write!(
                    f,
                    "Vector operations not available before bytecode version {}",
                    version
                )
            }
            VMErrorMessage::IntegerLoadCastNotSupportedInVersion { version } => {
                write!(
                    f,
                    "Loading or casting u16, u32, u256 integers not supported in bytecode version {}",
                    version
                )
            }

            // Resolution Errors
            VMErrorMessage::CannotFindInCache { identifier } => {
                write!(f, "Cannot find {} in cache", identifier)
            }
            VMErrorMessage::CannotFindPackageInCache { package } => {
                write!(f, "Cannot find package {} in data cache", package)
            }
            VMErrorMessage::MissingNativeFunction { name } => {
                write!(f, "Missing Native Function `{}`", name)
            }
            VMErrorMessage::MissingNativeFunctionUnnamed => {
                write!(f, "Missing Native Function")
            }
            VMErrorMessage::StructDefinitionNotResolved => {
                write!(f, "Struct Definition not resolved")
            }
            VMErrorMessage::InstantiationSignatureNotFound => {
                write!(f, "Instantiation signature not found")
            }
            VMErrorMessage::NoTypeTag { type_info } => {
                write!(f, "no type tag for {}", type_info)
            }
            VMErrorMessage::NoTypeLayout { type_info } => {
                write!(f, "no type layout for {}", type_info)
            }
            VMErrorMessage::MissingMapping { type_param } => {
                write!(f, "{} missing mapping", type_param)
            }

            // Invalid State Errors
            VMErrorMessage::CannotCopyInvalidValue { index } => {
                write!(f, "cannot copy invalid value at index {}", index)
            }
            VMErrorMessage::CannotMoveInvalidValue { index } => {
                write!(f, "cannot move invalid value at index {}", index)
            }
            VMErrorMessage::CannotBorrowLocal { local } => {
                write!(f, "cannot borrow local {}", local)
            }
            VMErrorMessage::FailedWriteRefTypeMismatch => {
                write!(f, "failed in write_ref: type mismatch")
            }
            VMErrorMessage::MovingValueWithDanglingReferences { value } => {
                write!(f, "moving value {} with dangling references", value)
            }
            VMErrorMessage::InvalidValueInConstant => {
                write!(f, "invalid value in constant")
            }
            VMErrorMessage::InvalidReferenceInConstant => {
                write!(f, "invalid reference in constant")
            }

            // Container Type Errors
            VMErrorMessage::ContainerIsNotStruct => {
                write!(f, "Container is not a struct")
            }
            VMErrorMessage::ContainerIsNotVector => {
                write!(f, "Container is not a vector")
            }
            VMErrorMessage::ContainerIsNotSigner => {
                write!(f, "Container is not a signer")
            }
            VMErrorMessage::ValueIsNotVariant { value } => {
                write!(f, "{} is not a variant", value)
            }
            VMErrorMessage::ValueIsNotVector { value } => {
                write!(f, "{} is not a vector", value)
            }
            VMErrorMessage::ValueIsNotPrimitiveVector { value } => {
                write!(f, "{} is not a primitive vector", value)
            }
            VMErrorMessage::InvalidIndexedReference { reference } => {
                write!(f, "invalid indexed reference: {}", reference)
            }

            // Tag/Variant Errors
            VMErrorMessage::TagMismatch { expected, actual } => {
                write!(f, "tag mismatch: expected {}, got {}", expected, actual)
            }
            VMErrorMessage::InvalidTypeParamForVector { type_param } => {
                write!(f, "invalid type param for vector: {}", type_param)
            }

            // Execution Context
            VMErrorMessage::AbortContext { function, offset } => {
                write!(f, "{} at offset {}", function, offset)
            }
            VMErrorMessage::FailedToGetArgumentsFromStack => {
                write!(f, "Failed to get last n arguments on the argument stack")
            }

            // Linking Errors
            VMErrorMessage::RelocationError { module_id, error } => {
                write!(f, "Error relocating {}: {}", module_id, error)
            }
            VMErrorMessage::PackageNotFoundAfterLoading => {
                write!(f, "Package not found in cache after loading")
            }

            // Package Validation
            VMErrorMessage::EmptyPackage => {
                write!(f, "Empty packages are not allowed.")
            }

            // Internal/Debug Errors
            VMErrorMessage::FailedToWriteToBuffer => {
                write!(f, "failed to write to buffer")
            }
            VMErrorMessage::InjectedVerifierFailure => {
                write!(f, "Injected move_vm::interpreter verifier failure")
            }
            VMErrorMessage::VerifierFailedConstantDeserialization => {
                write!(
                    f,
                    "Verifier failed to verify the deserialization of constants"
                )
            }
            VMErrorMessage::ArityMismatchReturnValues => {
                write!(
                    f,
                    "Arity mismatch: return value count does not match return type count"
                )
            }
            VMErrorMessage::RecursiveType => {
                write!(f, "Recursive type?")
            }
            VMErrorMessage::TypeParameterNotFullyResolved => {
                write!(f, "Type parameter should be fully resolved")
            }
            VMErrorMessage::VecPackUnpackArgumentOutOfRange => {
                write!(f, "VecPack/VecUnpack argument out of range")
            }
            VMErrorMessage::LookingForFieldInNativeStructure => {
                write!(f, "Looking for field in native structure")
            }
            VMErrorMessage::PhantomParameterLengthMismatch => {
                write!(
                    f,
                    "the length of `declared_phantom_parameters` doesn't match the length of `type_arguments`"
                )
            }
            VMErrorMessage::CurrentFunctionNotSetDuringBoundsChecking { offset } => {
                write!(
                    f,
                    "Indexing into bytecode {} during bounds checking but 'current_function' was not set",
                    offset
                )
            }
            VMErrorMessage::SafeUnwrapNone { file, line } => {
                write!(
                    f,
                    "{file}:{line} safe_unwrap failed: expected Some, got None"
                )
            }
            VMErrorMessage::SafeUnwrapErr { file, line, error } => {
                write!(f, "{file}:{line} safe_unwrap_err failed: {}", error)
            }
            VMErrorMessage::SafeAssertFailed { file, line } => {
                write!(f, "{file}:{line} safe_assert failed")
            }
            VMErrorMessage::LoopInInstantiationGraph { message } => {
                write!(f, "{}", message)
            }
            VMErrorMessage::ScriptVisibleFunctionCalledFromNonScriptVisible => {
                write!(
                    f,
                    "script-visible functions can only be called from scripts or other script-visible functions"
                )
            }
            VMErrorMessage::DuplicateModuleName { module } => {
                write!(f, "Duplicate module name {}", module)
            }
            VMErrorMessage::DuplicateStructDefinition { struct_name } => {
                write!(f, "Duplicate struct definition {}", struct_name)
            }
            VMErrorMessage::DuplicateStructHandleIndex {
                handle_idx,
                def1,
                def2,
            } => {
                write!(
                    f,
                    "Duplicate struct handle index {} for struct definitions {} and {}",
                    handle_idx, def1, def2
                )
            }
            VMErrorMessage::DuplicateEnumHandleIndex {
                handle_idx,
                def1,
                def2,
            } => {
                write!(
                    f,
                    "Duplicate enum handle index {} for enum definitions {} and {}",
                    handle_idx, def1, def2
                )
            }
            VMErrorMessage::ReferenceFieldInRecursiveStruct => {
                write!(f, "Reference field when checking recursive structs")
            }
            VMErrorMessage::FunctionNotFoundInVerifyModuleScriptFunction => {
                write!(f, "function not found in verify_module_script_function")
            }
            VMErrorMessage::DuplicateKey { key } => {
                write!(f, "Duplicate key {}", key)
            }
            VMErrorMessage::InvalidIndex { index } => {
                write!(f, "Invalid index: {}", index)
            }
            VMErrorMessage::LocalIndexOutOfBounds { index } => {
                write!(f, "Local index out of bounds: {}", index)
            }
            VMErrorMessage::LocalIndexUnset { index } => {
                write!(f, "Local index {} is unset", index)
            }
            VMErrorMessage::DefiningIdConflict {
                defining_id,
                addr1,
                addr2,
            } => {
                write!(
                    f,
                    "Defining ID {} found for {} and {}",
                    defining_id, addr1, addr2
                )
            }
            VMErrorMessage::PackageNotFound { package } => {
                write!(f, "Package {} not found", package)
            }
            VMErrorMessage::ModuleNotFound { module } => {
                write!(f, "Module {} not found", module)
            }
            VMErrorMessage::FunctionNotFound { function } => {
                write!(f, "Could not find function {}", function)
            }
            VMErrorMessage::TypeNotFound { type_name } => {
                write!(f, "Could not find type {}", type_name)
            }
            VMErrorMessage::DefiningIdNotFound {
                defining_id,
                type_tag,
            } => {
                write!(
                    f,
                    "Defining ID {} for type {} not found in loaded packages",
                    defining_id, type_tag
                )
            }
            VMErrorMessage::RuntimeIdMismatch {
                defining_id,
                runtime_id_expected,
                runtime_id_actual,
            } => {
                write!(
                    f,
                    "Runtime ID resolution of {} => {} does not match runtime ID of loaded type: {}",
                    defining_id, runtime_id_expected, runtime_id_actual
                )
            }
            VMErrorMessage::DefiningIdMismatch {
                defining_id_expected,
                defining_id_actual,
            } => {
                write!(
                    f,
                    "Defining ID {} does not match defining ID of loaded type: {}",
                    defining_id_expected, defining_id_actual
                )
            }
            VMErrorMessage::FieldTypeLengthMismatch { context } => {
                write!(
                    f,
                    "Field types did not match the length of field names in {}",
                    context
                )
            }
            VMErrorMessage::CannotMoveFromInvalidLocation => {
                write!(f, "Cannot move from an invalid memory location")
            }
            VMErrorMessage::HeapIndexInvalid { index } => {
                write!(f, "Heap index invalid: {}", index)
            }
            VMErrorMessage::IntererLimitReached { ident, error } => {
                write!(f, "Failed to intern {} ident; error: {:?}.", ident, error)
            }
            VMErrorMessage::FailedToFindKeyInInterner { key_type } => {
                write!(f, "Failed to find {} key in ident interner.", key_type)
            }
            VMErrorMessage::SignatureLookupFailed { context } => {
                write!(f, "{}", context)
            }
            VMErrorMessage::SignatureArity {
                expected,
                actual,
                context,
            } => {
                write!(
                    f,
                    "Signature arity mismatch: expected {}, got {} ({})",
                    expected, actual, context
                )
            }
            VMErrorMessage::FunctionLookupFailed { context } => {
                write!(f, "{}", context)
            }
            VMErrorMessage::UnexpectedStorageError { error } => {
                write!(f, "Unexpected storage error: {}", error)
            }
            VMErrorMessage::ExternalResolutionRequestError { context } => {
                write!(f, "{}", context)
            }
            VMErrorMessage::IndexLookupFailed { context } => {
                write!(f, "{}", context)
            }
            VMErrorMessage::ConstantDeserializationFailure => {
                write!(
                    f,
                    "Verifier failed to verify the deserialization of constants"
                )
            }
            VMErrorMessage::MultiplePackagesLoaded => {
                write!(
                    f,
                    "More than one package was loaded when only one was requested"
                )
            }
            VMErrorMessage::NativeExtensionNotFound { extension } => {
                write!(f, "native extension not found: {}", extension)
            }
            VMErrorMessage::DowncastError { context } => {
                write!(f, "downcast error: {}", context)
            }
            VMErrorMessage::IndexOutOfBoundsInVec => {
                write!(f, "Index out of bounds in Vec")
            }
            VMErrorMessage::IndexOutOfBoundsInPrimVec => {
                write!(f, "Index out of bounds in PrimVec")
            }
            VMErrorMessage::VectorU8Expected => {
                write!(f, "expected vector<u8>")
            }
            VMErrorMessage::NotAResource => {
                write!(f, "failed to publish fresh: not a resource")
            }
            VMErrorMessage::TypeSubstitutionFailed { len, got } => {
                write!(
                    f,
                    "type substitution failed: index out of bounds -- len {} got {}",
                    len, got
                )
            }
            VMErrorMessage::UnableToLoadConstTypeSignature => {
                write!(f, "Unable to load const type signature")
            }
            VMErrorMessage::VecMutBorrowExpectsVectorReference => {
                write!(f, "VecMutBorrow expects a vector reference")
            }
            VMErrorMessage::VecImmBorrowExpectsVectorReference => {
                write!(f, "VecImmBorrow expects a vector reference")
            }
            VMErrorMessage::NativeFunctionGasParametersNotSpecified => {
                write!(f, "native function gas parameters not specified")
            }
            VMErrorMessage::CannotCastToInt { value } => {
                write!(f, "cannot cast {:?} to integer", value)
            }
            VMErrorMessage::CannotCastToVecValue { value } => {
                write!(f, "cannot cast {:?} to Vec<Value>", value)
            }
            VMErrorMessage::ExpectedVectorContainer { found } => {
                write!(f, "Expected a vector container, found {:?}", found)
            }
            VMErrorMessage::ExpectedVectorInReference { found } => {
                write!(
                    f,
                    "Expected a vector container in reference, found {:?}",
                    found
                )
            }
            VMErrorMessage::SignerExpected => {
                write!(f, "Container is not a signer")
            }
            VMErrorMessage::VectorElemLayoutMismatch { expected, got } => {
                write!(
                    f,
                    "vector elem layout mismatch, expected {:?}, got {:?}",
                    expected, got
                )
            }
            VMErrorMessage::WriteMacroFailed { error } => {
                write!(f, "write! macro failed with: {}", error)
            }
            VMErrorMessage::CouldNotConvertVecMoveValueToVecU8 => {
                write!(f, "Could not convert Vec<MoveValue> to Vec<u8>")
            }
            VMErrorMessage::CouldNotGetInnerTypeOfVector => {
                write!(f, "Could not get the inner Type of a vector's Type")
            }
            VMErrorMessage::ExpectedMoveValueVectorOfU8 => {
                write!(f, "Expected a MoveValue::Vector of u8's")
            }
            VMErrorMessage::ExpectedMoveValueMoveStruct => {
                write!(f, "Expected MoveValue::MoveStruct")
            }
            VMErrorMessage::ExpectedMoveDatatypeLayoutStruct => {
                write!(f, "Expected MoveDatatypeLayout::Struct")
            }
            VMErrorMessage::ExpectedMoveDatatypeLayoutEnum => {
                write!(f, "Expected MoveDatatypeLayout::Enum")
            }
            VMErrorMessage::CouldNotParseUtf8Bytes { error } => {
                write!(f, "Could not parse UTF8 bytes: {}", error)
            }
            VMErrorMessage::ExpectedStringStructWithOneField { num_fields } => {
                write!(
                    f,
                    "Expected std::string::String struct to have just one field, got {}",
                    num_fields
                )
            }
            VMErrorMessage::ExpectedStringStructWithBytesField { field_name } => {
                write!(
                    f,
                    "Expected std::string::String struct to have a `bytes` field, got {}",
                    field_name
                )
            }
        }
    }
}

pub type VMResult<T> = ::std::result::Result<T, VMError>;
pub type BinaryLoaderResult<T> = ::std::result::Result<T, PartialVMError>;
pub type PartialVMResult<T> = ::std::result::Result<T, PartialVMError>;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Location {
    Undefined,
    // The `AccountAddress` inside of the `Module`'s `ModuleId` is the original id
    Module(ModuleId),
    // The `AccountAddress` inside of the `Package` is the version id of the package
    Package(AccountAddress),
}

/// A representation of the execution state (e.g., stack trace) at an
/// error point.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ExecutionState {
    stack_trace: Vec<(ModuleId, FunctionDefinitionIndex, CodeOffset)>,
    // we may consider adding more state if necessary
}

impl ExecutionState {
    pub fn new(stack_trace: Vec<(ModuleId, FunctionDefinitionIndex, CodeOffset)>) -> Self {
        Self { stack_trace }
    }

    pub fn stack_trace(&self) -> &Vec<(ModuleId, FunctionDefinitionIndex, CodeOffset)> {
        &self.stack_trace
    }
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct VMError(Box<VMError_>);

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
struct VMError_ {
    major_status: StatusCode,
    sub_status: Option<u64>,
    message: Option<VMErrorMessage>,
    exec_state: Option<ExecutionState>,
    location: Location,
    indices: Vec<(IndexKind, TableIndex)>,
    offsets: Vec<(FunctionDefinitionIndex, CodeOffset)>,
}

impl VMError {
    pub fn major_status(&self) -> StatusCode {
        self.0.major_status
    }

    pub fn sub_status(&self) -> Option<u64> {
        self.0.sub_status
    }

    pub fn message(&self) -> Option<&VMErrorMessage> {
        self.0.message.as_ref()
    }

    pub fn exec_state(&self) -> Option<&ExecutionState> {
        self.0.exec_state.as_ref()
    }

    pub fn remove_exec_state(&mut self) {
        self.0.exec_state = None;
    }

    pub fn location(&self) -> &Location {
        &self.0.location
    }

    pub fn indices(&self) -> &Vec<(IndexKind, TableIndex)> {
        &self.0.indices
    }

    pub fn offsets(&self) -> &Vec<(FunctionDefinitionIndex, CodeOffset)> {
        &self.0.offsets
    }

    pub fn status_type(&self) -> StatusType {
        self.0.major_status.status_type()
    }

    #[allow(clippy::type_complexity)]
    pub fn all_data(
        self,
    ) -> (
        StatusCode,
        Option<u64>,
        Option<VMErrorMessage>,
        Option<ExecutionState>,
        Location,
        Vec<(IndexKind, TableIndex)>,
        Vec<(FunctionDefinitionIndex, CodeOffset)>,
    ) {
        let VMError_ {
            major_status,
            sub_status,
            message,
            exec_state,
            location,
            indices,
            offsets,
        } = *self.0;
        (
            major_status,
            sub_status,
            message,
            exec_state,
            location,
            indices,
            offsets,
        )
    }

    pub fn to_partial(self) -> PartialVMError {
        let VMError_ {
            major_status,
            sub_status,
            message,
            exec_state,
            indices,
            offsets,
            ..
        } = *self.0;
        PartialVMError(Box::new(PartialVMError_ {
            major_status,
            sub_status,
            message,
            exec_state,
            indices,
            offsets,
        }))
    }
}

impl fmt::Debug for VMError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Debug for VMError_ {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            major_status,
            sub_status,
            message,
            exec_state,
            location,
            indices,
            offsets,
        } = self;
        f.debug_struct("VMError")
            .field("major_status", major_status)
            .field("sub_status", sub_status)
            .field("message", message)
            .field("exec_state", exec_state)
            .field("location", location)
            .field("indices", indices)
            .field("offsets", offsets)
            .finish()
    }
}

impl std::error::Error for VMError {}

#[derive(Clone)]
pub struct PartialVMError(Box<PartialVMError_>);

#[derive(Clone)]
struct PartialVMError_ {
    major_status: StatusCode,
    sub_status: Option<u64>,
    message: Option<VMErrorMessage>,
    exec_state: Option<ExecutionState>,
    indices: Vec<(IndexKind, TableIndex)>,
    offsets: Vec<(FunctionDefinitionIndex, CodeOffset)>,
}

impl PartialVMError {
    #[allow(clippy::type_complexity)]
    pub fn all_data(
        self,
    ) -> (
        StatusCode,
        Option<u64>,
        Option<VMErrorMessage>,
        Option<ExecutionState>,
        Vec<(IndexKind, TableIndex)>,
        Vec<(FunctionDefinitionIndex, CodeOffset)>,
    ) {
        let PartialVMError_ {
            major_status,
            sub_status,
            message,
            exec_state,
            indices,
            offsets,
        } = *self.0;
        (
            major_status,
            sub_status,
            message,
            exec_state,
            indices,
            offsets,
        )
    }

    pub fn finish(self, location: Location) -> VMError {
        let PartialVMError_ {
            major_status,
            sub_status,
            message,
            exec_state,
            indices,
            offsets,
        } = *self.0;
        VMError(Box::new(VMError_ {
            major_status,
            sub_status,
            message,
            exec_state,
            location,
            indices,
            offsets,
        }))
    }

    pub fn new(major_status: StatusCode) -> Self {
        Self(Box::new(PartialVMError_ {
            major_status,
            sub_status: None,
            message: None,
            exec_state: None,
            indices: vec![],
            offsets: vec![],
        }))
    }

    pub fn major_status(&self) -> StatusCode {
        self.0.major_status
    }

    pub fn with_sub_status(mut self, sub_status: u64) -> Self {
        debug_assert!(self.0.sub_status.is_none());
        self.0.sub_status = Some(sub_status);
        self
    }

    pub fn with_message(mut self, message: VMErrorMessage) -> Self {
        debug_assert!(self.0.message.is_none());
        self.0.message = Some(message);
        self
    }

    pub fn with_exec_state(mut self, exec_state: ExecutionState) -> Self {
        debug_assert!(self.0.exec_state.is_none());
        self.0.exec_state = Some(exec_state);
        self
    }

    pub fn at_index(mut self, kind: IndexKind, index: TableIndex) -> Self {
        self.0.indices.push((kind, index));
        self
    }

    pub fn at_indices(mut self, additional_indices: Vec<(IndexKind, TableIndex)>) -> Self {
        self.0.indices.extend(additional_indices);
        self
    }

    pub fn at_code_offset(mut self, function: FunctionDefinitionIndex, offset: CodeOffset) -> Self {
        self.0.offsets.push((function, offset));
        self
    }

    pub fn at_code_offsets(
        mut self,
        additional_offsets: Vec<(FunctionDefinitionIndex, CodeOffset)>,
    ) -> Self {
        self.0.offsets.extend(additional_offsets);
        self
    }

    /// Append the message `message` to the message field of the VM status, and insert a separator
    /// if the original message is non-empty.
    pub fn append_message_with_separator(
        mut self,
        _separator: char,
        additional_message: VMErrorMessage,
    ) -> Self {
        match self.0.message.take() {
            Some(VMErrorMessage::MessageChain(mut chain)) => {
                // Already a chain, append to it
                chain.push(additional_message);
                self.0.message = Some(VMErrorMessage::MessageChain(chain));
            }
            Some(existing_msg) => {
                // Convert single message to chain
                self.0.message = Some(VMErrorMessage::MessageChain(vec![
                    existing_msg,
                    additional_message,
                ]));
            }
            None => {
                // No existing message, just set it
                self.0.message = Some(additional_message);
            }
        }
        self
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Location::Undefined => write!(f, "UNDEFINED"),
            Location::Module(id) => write!(f, "Module {:?}", id),
            Location::Package(addr) => write!(f, "Package {:?}", addr),
        }
    }
}

impl fmt::Display for PartialVMError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut status = format!("PartialVMError with status {:#?}", self.0.major_status);

        if let Some(sub_status) = self.0.sub_status {
            status = format!("{} with sub status {}", status, sub_status);
        }

        if let Some(msg) = &self.0.message {
            status = format!("{} and message {}", status, msg);
        }

        for (kind, index) in &self.0.indices {
            status = format!("{} at index {} for {}", status, index, kind);
        }
        for (fdef, code_offset) in &self.0.offsets {
            status = format!(
                "{} at code offset {} in function definition {}",
                status, code_offset, fdef
            );
        }

        write!(f, "{}", status)
    }
}

impl fmt::Display for VMError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut status = format!("VMError with status {:#?}", self.0.major_status);

        if let Some(sub_status) = self.0.sub_status {
            status = format!("{} with sub status {}", status, sub_status);
        }

        status = format!("{} at location {}", status, self.0.location);

        if let Some(msg) = &self.0.message {
            status = format!("{} and message {}", status, msg);
        }

        for (kind, index) in &self.0.indices {
            status = format!("{} at index {} for {}", status, index, kind);
        }
        for (fdef, code_offset) in &self.0.offsets {
            status = format!(
                "{} at code offset {} in function definition {}",
                status, code_offset, fdef
            );
        }

        write!(f, "{}", status)
    }
}

////////////////////////////////////////////////////////////////////////////
// Conversion functions from internal VM statuses into external VM statuses
////////////////////////////////////////////////////////////////////////////

pub fn offset_out_of_bounds(
    status: StatusCode,
    kind: IndexKind,
    target_offset: usize,
    target_pool_len: usize,
    cur_function: FunctionDefinitionIndex,
    cur_bytecode_offset: CodeOffset,
) -> PartialVMError {
    PartialVMError::new(status)
        .with_message(VMErrorMessage::IndexOutOfBoundsWithContext {
            index: target_offset as u64,
            limit: target_pool_len as u64,
            kind: kind.to_string(),
            function: cur_function.0,
            offset: cur_bytecode_offset,
        })
        .at_code_offset(cur_function, cur_bytecode_offset)
}

pub fn bounds_error(
    status: StatusCode,
    kind: IndexKind,
    idx: TableIndex,
    len: usize,
) -> PartialVMError {
    PartialVMError::new(status)
        .at_index(kind, idx)
        .with_message(VMErrorMessage::IndexOutOfBounds {
            index: idx as u64,
            limit: len as u64,
            kind: kind.to_string(),
        })
}

pub fn verification_error(status: StatusCode, kind: IndexKind, idx: TableIndex) -> PartialVMError {
    PartialVMError::new(status).at_index(kind, idx)
}

impl fmt::Debug for PartialVMError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Debug for PartialVMError_ {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            major_status,
            sub_status,
            message,
            exec_state,
            indices,
            offsets,
        } = self;
        f.debug_struct("PartialVMError")
            .field("major_status", major_status)
            .field("sub_status", sub_status)
            .field("message", message)
            .field("exec_state", exec_state)
            .field("indices", indices)
            .field("offsets", offsets)
            .finish()
    }
}

impl std::error::Error for PartialVMError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}
