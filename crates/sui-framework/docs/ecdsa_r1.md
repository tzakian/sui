
<a name="0x2_ecdsa_r1"></a>

# Module `0x2::ecdsa_r1`



-  [Constants](#@Constants_0)
-  [Function `ecrecover`](#0x2_ecdsa_r1_ecrecover)
-  [Function `secp256r1_verify`](#0x2_ecdsa_r1_secp256r1_verify)
-  [Function `secp256r1_verify_recoverable`](#0x2_ecdsa_r1_secp256r1_verify_recoverable)


<pre><code></code></pre>



<a name="@Constants_0"></a>

## Constants


<a name="0x2_ecdsa_r1_EFailToRecoverPubKey"></a>

Error if the public key cannot be recovered from the signature.


<pre><code><b>const</b> <a href="ecdsa_r1.md#0x2_ecdsa_r1_EFailToRecoverPubKey">EFailToRecoverPubKey</a>: u64 = 0;
</code></pre>



<a name="0x2_ecdsa_r1_EInvalidSignature"></a>

Error if the signature is invalid.


<pre><code><b>const</b> <a href="ecdsa_r1.md#0x2_ecdsa_r1_EInvalidSignature">EInvalidSignature</a>: u64 = 1;
</code></pre>



<a name="0x2_ecdsa_r1_ecrecover"></a>

## Function `ecrecover`

@param signature: A 65-bytes signature in form (r, s, v) that is signed using
Secp256r1. Reference implementation on signature generation using RFC6979:
https://github.com/MystenLabs/fastcrypto/blob/74aec4886e62122a5b769464c2bea5f803cf8ecc/fastcrypto/src/secp256r1/mod.rs
The accepted v values are {0, 1, 2, 3}.

@param hashed_msg: the hashed 32-bytes message. The message must be hashed instead
of plain text to be secure.

If the signature is valid, return the corresponding recovered Secpk256r1 public
key, otherwise throw error. This is similar to ecrecover in Ethereum, can only be
applied to Secp256r1 signatures.


<pre><code><b>public</b> <b>fun</b> <a href="ecdsa_r1.md#0x2_ecdsa_r1_ecrecover">ecrecover</a>(signature: &<a href="">vector</a>&lt;u8&gt;, msg: &<a href="">vector</a>&lt;u8&gt;): <a href="">vector</a>&lt;u8&gt;
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>native</b> <b>fun</b> <a href="ecdsa_r1.md#0x2_ecdsa_r1_ecrecover">ecrecover</a>(signature: &<a href="">vector</a>&lt;u8&gt;, msg: &<a href="">vector</a>&lt;u8&gt;): <a href="">vector</a>&lt;u8&gt;;
</code></pre>



</details>

<a name="0x2_ecdsa_r1_secp256r1_verify"></a>

## Function `secp256r1_verify`

@param signature: A 64-bytes signature in form (r, s) that is signed using
Secp256r1. This is an non-recoverable signature without recovery id.
Reference implementation on signature generation using RFC6979:
https://github.com/MystenLabs/fastcrypto/blob/74aec4886e62122a5b769464c2bea5f803cf8ecc/fastcrypto/src/secp256r1/mod.rs

@param public_key: The public key to verify the signature against
@param hashed_msg: The hashed 32-bytes message, same as what the signature is signed against.

If the signature is valid to the pubkey and hashed message, return true. Else false.


<pre><code><b>public</b> <b>fun</b> <a href="ecdsa_r1.md#0x2_ecdsa_r1_secp256r1_verify">secp256r1_verify</a>(signature: &<a href="">vector</a>&lt;u8&gt;, public_key: &<a href="">vector</a>&lt;u8&gt;, msg: &<a href="">vector</a>&lt;u8&gt;): bool
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>native</b> <b>fun</b> <a href="ecdsa_r1.md#0x2_ecdsa_r1_secp256r1_verify">secp256r1_verify</a>(signature: &<a href="">vector</a>&lt;u8&gt;, public_key: &<a href="">vector</a>&lt;u8&gt;, msg: &<a href="">vector</a>&lt;u8&gt;): bool;
</code></pre>



</details>

<a name="0x2_ecdsa_r1_secp256r1_verify_recoverable"></a>

## Function `secp256r1_verify_recoverable`

@param signature: A 65-bytes signature in form (r, s, v) that is signed using
Secp256r1. This is an recoverable signature with recovery id denoted as v.
Reference implementation on signature generation using RFC6979:
https://github.com/MystenLabs/fastcrypto/blob/74aec4886e62122a5b769464c2bea5f803cf8ecc/fastcrypto/src/secp256r1/recoverable.rs#L35

@param public_key: The public key to verify the signature against
@param hashed_msg: The hashed 32-bytes message, same as what the signature is signed against.

If the signature is valid to the pubkey and hashed message, return true. Else false.


<pre><code><b>public</b> <b>fun</b> <a href="ecdsa_r1.md#0x2_ecdsa_r1_secp256r1_verify_recoverable">secp256r1_verify_recoverable</a>(signature: &<a href="">vector</a>&lt;u8&gt;, public_key: &<a href="">vector</a>&lt;u8&gt;, msg: &<a href="">vector</a>&lt;u8&gt;): bool
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>native</b> <b>fun</b> <a href="ecdsa_r1.md#0x2_ecdsa_r1_secp256r1_verify_recoverable">secp256r1_verify_recoverable</a>(signature: &<a href="">vector</a>&lt;u8&gt;, public_key: &<a href="">vector</a>&lt;u8&gt;, msg: &<a href="">vector</a>&lt;u8&gt;): bool;
</code></pre>



</details>
