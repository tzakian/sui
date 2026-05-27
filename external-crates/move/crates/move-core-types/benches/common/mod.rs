// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Shared bench scaffolding: shape catalog and annotated-layout builders
//! used by every bench file.
//!
//! ## Shapes
//!
//! Each shape exercises a different cost dimension of layout traversal:
//!
//! | Shape            | Description                                                      | Stresses                                          |
//! |------------------|------------------------------------------------------------------|---------------------------------------------------|
//! | `leaf`           | A single primitive (`u64`)                                       | Per-call overhead; baseline                       |
//! | `shallow_struct` | One struct with 8 mixed-primitive fields                         | Per-field iteration                               |
//! | `wide_struct`    | One struct with 64 mixed-primitive fields (8 cycles of 8 types)  | Iteration cost dominated by field count           |
//! | `deep_nested`    | 16 levels of `Struct { f: Struct { f: ... u64 } }`               | Recursive descent / per-level layout machinery    |
//! | `wide_enum`      | Enum with 32 known variants, each holding 4 primitive fields     | `as_view()` and `variant_by_tag()` on wide enums  |
//! | `realistic`      | Approximation of a Sui object: `UID` + `Balance` + `vector<u8>` + a small `Status` enum | Mixed shape similar to production data |

use move_core_types::{
    account_address::AccountAddress,
    compressed::annotated::{LayoutHandle, MoveTypeLayout, MoveTypeLayoutBuilder},
    compressed::gat::annotated::{BackendBuilder as GatBackendBuilder, MoveTypeLayout as GatLayout},
    compressed::gat::backend::arc_pool::AnnotatedArcPool,
    compressed::gat::backend::box_pool::{AnnotatedBoxPool, AnnotatedBoxPoolBuilder},
    identifier::Identifier,
    language_storage::StructTag,
};

/// Backend-abstracted (PR #26798) layout for a named shape, on the `ArcPool`
/// backend. Built by inflating the canonical annotated layout to tree form and
/// re-interning — done once, outside any timed region.
pub fn gat_arc_layout(name: &str) -> GatLayout<AnnotatedArcPool> {
    let tree = annotated_layout(name).inflate().expect("inflate");
    GatLayout::<AnnotatedArcPool>::try_from(&tree).expect("gat arc build")
}

/// Same shape on the `BoxPool` backend (owned slice, deep-clone). Built via the
/// generic builder since `BoxPool` doesn't expose the `TryFrom` convenience.
pub fn gat_box_layout(name: &str) -> GatLayout<AnnotatedBoxPool> {
    let tree = annotated_layout(name).inflate().expect("inflate");
    let mut b = AnnotatedBoxPoolBuilder::default();
    let root = b.intern_tree(&tree).expect("gat box intern");
    b.build(root)
}

/// Names of the shapes recognized by [`annotated_layout`]. Listed in the
/// order benches iterate them.
pub const SHAPE_NAMES: &[&str] = &[
    "leaf",
    "shallow_struct",
    "wide_struct",
    "deep_nested",
    "wide_enum",
    "realistic",
];

pub fn ident(s: &str) -> Identifier {
    Identifier::new(s).unwrap()
}

pub fn st(name: &str) -> StructTag {
    StructTag {
        address: AccountAddress::ONE,
        module: ident("m"),
        name: ident(name),
        type_params: vec![],
    }
}

// ---------------------------------------------------------------------------
// "Find UIDs" fixture, shared by `benches/find.rs` and
// `benches/find_by_layout.rs`.
//
//   Outer { items: vector<Item> }
//   Item  { id: UID, payload: u64, junk: vector<u8> }
//   UID   { id: ID { bytes: address } }
//
// BCS bytes: leb128(N) ++ N × ( address(32) ++ u64(8) ++ leb128(0) )
// ---------------------------------------------------------------------------

pub const FIND_UIDS_N: usize = 64;

/// Target `StructTag` used by find-uids benches.
pub fn uid_tag() -> StructTag {
    st("UID")
}

/// Owned compressed annotated layout for the find-uids fixture.
pub fn find_uids_layout() -> MoveTypeLayout {
    MoveTypeLayoutBuilder::with_builder::<_, anyhow::Error>(|b| {
        let addr = b.address();
        let id_inner = b.struct_layout(st("ID"), vec![(ident("bytes"), addr)])?;
        let uid = b.struct_layout(st("UID"), vec![(ident("id"), id_inner)])?;
        let payload = b.u64();
        let junk = {
            let u8h = b.u8();
            b.vector(u8h)?
        };
        let item: LayoutHandle = b.struct_layout(
            st("Item"),
            vec![
                (ident("id"), uid),
                (ident("payload"), payload),
                (ident("junk"), junk),
            ],
        )?;
        let items = b.vector(item)?;
        b.struct_layout(st("Outer"), vec![(ident("items"), items)])
    })
    .unwrap()
}

/// Standalone owned layout for *just* the UID struct — built in an
/// independent pool, so equality against a UID inside `find_uids_layout()`
/// can't take the `Arc::ptr_eq` short-circuit.
pub fn uid_layout() -> MoveTypeLayout {
    MoveTypeLayoutBuilder::with_builder::<_, anyhow::Error>(|b| {
        let addr = b.address();
        let id_inner = b.struct_layout(st("ID"), vec![(ident("bytes"), addr)])?;
        b.struct_layout(st("UID"), vec![(ident("id"), id_inner)])
    })
    .unwrap()
}

/// BCS bytes that decode against [`find_uids_layout`].
pub fn find_uids_bytes() -> Vec<u8> {
    let mut out = Vec::new();
    leb128::write::unsigned(&mut out, FIND_UIDS_N as u64).unwrap();
    for i in 0..FIND_UIDS_N {
        out.extend_from_slice(&[i as u8; 32]); // UID address
        out.extend_from_slice(&[0u8; 8]); // payload = 0u64
        leb128::write::unsigned(&mut out, 0).unwrap(); // junk: empty vec<u8>
    }
    out
}

/// Expected `Vec<AccountAddress>` from a find-uids traversal of
/// [`find_uids_bytes`].
pub fn find_uids_expected() -> Vec<AccountAddress> {
    (0..FIND_UIDS_N)
        .map(|i| AccountAddress::new([i as u8; 32]))
        .collect()
}

// ---------------------------------------------------------------------------
// Option-visit fixture: `vector<0x1::option::Option<u64>>`, all `Some(i)`.
// Used by `benches/option_visit.rs`.
// ---------------------------------------------------------------------------

pub const OPTION_N: usize = 64;

/// `0x1::option::Option<T>` struct tag, with `T` supplied as `type_params[0]`.
pub fn option_tag(inner: move_core_types::language_storage::TypeTag) -> StructTag {
    StructTag {
        address: AccountAddress::ONE,
        module: Identifier::new("option").unwrap(),
        name: Identifier::new("Option").unwrap(),
        type_params: vec![inner],
    }
}

/// `vector<Option<u64>>` compressed layout.
pub fn option_u64_outer_layout() -> MoveTypeLayout {
    MoveTypeLayoutBuilder::with_builder::<_, anyhow::Error>(|b| {
        let u64h = b.u64();
        let inner_vec = b.vector(u64h)?;
        let option = b.struct_layout(
            option_tag(move_core_types::language_storage::TypeTag::U64),
            vec![(Identifier::new("vec").unwrap(), inner_vec)],
        )?;
        b.vector(option)
    })
    .unwrap()
}

/// BCS bytes for a `vector<Option<u64>>` with [`OPTION_N`] `Some(i)` values.
///
/// `leb128(N)` ++ N × ( leb128(1) ++ u64_le(i) )
pub fn option_u64_outer_bytes() -> Vec<u8> {
    let mut out = Vec::new();
    leb128::write::unsigned(&mut out, OPTION_N as u64).unwrap();
    for i in 0..OPTION_N {
        leb128::write::unsigned(&mut out, 1).unwrap(); // Some
        out.extend_from_slice(&(i as u64).to_le_bytes());
    }
    out
}

/// Expected `Vec<u64>` from running a Some-collector over [`option_u64_outer_bytes`].
pub fn option_u64_expected() -> Vec<u64> {
    (0..OPTION_N as u64).collect()
}

/// Build the annotated [`MoveTypeLayout`] for a named shape from
/// [`SHAPE_NAMES`]. Panics on unknown name.
pub fn annotated_layout(name: &str) -> MoveTypeLayout {
    match name {
        "leaf" => leaf(),
        "shallow_struct" => shallow_struct(),
        "wide_struct" => wide_struct(),
        "deep_nested" => deep_nested(),
        "wide_enum" => wide_enum(),
        "realistic" => realistic(),
        _ => panic!("unknown shape: {name}"),
    }
}

fn leaf() -> MoveTypeLayout {
    MoveTypeLayout::u64()
}

fn shallow_struct() -> MoveTypeLayout {
    MoveTypeLayoutBuilder::with_builder::<_, anyhow::Error>(|b| {
        let fields = vec![
            (ident("f0"), b.bool()),
            (ident("f1"), b.u8()),
            (ident("f2"), b.u16()),
            (ident("f3"), b.u32()),
            (ident("f4"), b.u64()),
            (ident("f5"), b.u128()),
            (ident("f6"), b.address()),
            (ident("f7"), b.u256()),
        ];
        b.struct_layout(st("Shallow"), fields)
    })
    .unwrap()
}

fn wide_struct() -> MoveTypeLayout {
    MoveTypeLayoutBuilder::with_builder::<_, anyhow::Error>(|b| {
        let mut fields = Vec::with_capacity(64);
        for i in 0..64u32 {
            let h = match i % 8 {
                0 => b.bool(),
                1 => b.u8(),
                2 => b.u16(),
                3 => b.u32(),
                4 => b.u64(),
                5 => b.u128(),
                6 => b.u256(),
                _ => b.address(),
            };
            fields.push((ident(&format!("f{i}")), h));
        }
        b.struct_layout(st("Wide"), fields)
    })
    .unwrap()
}

fn deep_nested() -> MoveTypeLayout {
    MoveTypeLayoutBuilder::with_builder::<_, anyhow::Error>(|b| {
        let mut current: LayoutHandle = b.u64();
        for i in 0..16u32 {
            current = b.struct_layout(st(&format!("D{i}")), vec![(ident("f"), current)])?;
        }
        Ok::<_, anyhow::Error>(current)
    })
    .unwrap()
}

fn wide_enum() -> MoveTypeLayout {
    MoveTypeLayoutBuilder::with_builder::<_, anyhow::Error>(|b| {
        let mut variants = Vec::with_capacity(32);
        for i in 0..32u16 {
            let fields = vec![
                (ident("a"), b.bool()),
                (ident("b"), b.u64()),
                (ident("c"), b.address()),
                (ident("d"), b.u128()),
            ];
            variants.push((ident(&format!("V{i}")), i, Some(fields)));
        }
        b.enum_layout(st("WideEnum"), variants)
    })
    .unwrap()
}

fn realistic() -> MoveTypeLayout {
    // Outer { id: UID, balance: Balance<T>, name: vector<u8>, status: Status }
    //   UID    { id: ID { bytes: address } }
    //   Status = Active | Closed { reason: vector<u8> } | Pending { at: u64 }
    MoveTypeLayoutBuilder::with_builder::<_, anyhow::Error>(|b| {
        let addr = b.address();
        let id_inner = b.struct_layout(st("ID"), vec![(ident("bytes"), addr)])?;
        let uid = b.struct_layout(st("UID"), vec![(ident("id"), id_inner)])?;
        let value_h = b.u64();
        let balance = b.struct_layout(st("Balance"), vec![(ident("value"), value_h)])?;
        let bytes_vec = {
            let u8h = b.u8();
            b.vector(u8h)?
        };
        let bytes_vec_for_enum = {
            let u8h = b.u8();
            b.vector(u8h)?
        };
        let pending_field = b.u64();
        let status = b.enum_layout(
            st("Status"),
            vec![
                (ident("Active"), 0, Some(vec![])),
                (
                    ident("Closed"),
                    1,
                    Some(vec![(ident("reason"), bytes_vec_for_enum)]),
                ),
                (
                    ident("Pending"),
                    2,
                    Some(vec![(ident("at"), pending_field)]),
                ),
            ],
        )?;
        b.struct_layout(
            st("Outer"),
            vec![
                (ident("id"), uid),
                (ident("balance"), balance),
                (ident("name"), bytes_vec),
                (ident("status"), status),
            ],
        )
    })
    .unwrap()
}
