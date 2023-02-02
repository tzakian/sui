// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

module sui::ecdsa_r1 {

    /// Error if the public key cannot be recovered from the signature. 
    const EFailToRecoverPubKey: u64 = 0;

    /// Error if the signature is invalid.
    const EInvalidSignature: u64 = 1;

    /// @param signature: A 65-bytes signature in form (r, s, v) that is signed using
    /// Secp256r1. Reference implementation on signature generation using RFC6979:
    /// https://github.com/MystenLabs/fastcrypto/blob/74aec4886e62122a5b769464c2bea5f803cf8ecc/fastcrypto/src/secp256r1/mod.rs
    /// The accepted v values are {0, 1, 2, 3}.
    ///
    /// @param hashed_msg: the hashed 32-bytes message. The message must be hashed instead
    /// of plain text to be secure.
    ///
    /// If the signature is valid, return the corresponding recovered Secpk256r1 public
    /// key, otherwise throw error. This is similar to ecrecover in Ethereum, can only be
    /// applied to Secp256r1 signatures.
    public native fun ecrecover(signature: &vector<u8>, msg: &vector<u8>): vector<u8>;

    /// @param signature: A 64-bytes signature in form (r, s) that is signed using
    /// Secp256r1. This is an non-recoverable signature without recovery id.
    /// Reference implementation on signature generation using RFC6979:
    /// https://github.com/MystenLabs/fastcrypto/blob/74aec4886e62122a5b769464c2bea5f803cf8ecc/fastcrypto/src/secp256r1/mod.rs
    ///
    /// @param public_key: The public key to verify the signature against
    /// @param hashed_msg: The hashed 32-bytes message, same as what the signature is signed against.
    ///
    /// If the signature is valid to the pubkey and hashed message, return true. Else false.
    public native fun secp256r1_verify(signature: &vector<u8>, public_key: &vector<u8>, msg: &vector<u8>): bool;

    /// @param signature: A 65-bytes signature in form (r, s, v) that is signed using
    /// Secp256r1. This is an recoverable signature with recovery id denoted as v.
    /// Reference implementation on signature generation using RFC6979:
    /// https://github.com/MystenLabs/fastcrypto/blob/74aec4886e62122a5b769464c2bea5f803cf8ecc/fastcrypto/src/secp256r1/recoverable.rs#L35
    ///
    /// @param public_key: The public key to verify the signature against
    /// @param hashed_msg: The hashed 32-bytes message, same as what the signature is signed against.
    ///
    /// If the signature is valid to the pubkey and hashed message, return true. Else false.
    public native fun secp256r1_verify_recoverable(signature: &vector<u8>, public_key: &vector<u8>, msg: &vector<u8>): bool;
}
