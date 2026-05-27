// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! "Find-by-layout" microbenchmark: same workload as `benches/find.rs`
//! (collect every `AccountAddress` that lives inside a struct shaped like
//! `UID`), but the visitor identifies "this is a UID" by **structural
//! layout equality** against a precomputed target layout — *not* by
//! `StructTag`.
//!
//! Layout matching is what you'd reach for if the type tag wasn't
//! authoritative (e.g. checking equivalence across compilation units, or
//! the type is generic and you care about a specific instantiation's full
//! shape).
//!
//! ## Variants
//!
//! The `*_direct` variants from `benches/find.rs` are intentionally absent:
//! they only expose the struct's `StructTag`, which is not enough for a
//! full-layout match.
//!
//! | Variant            | Visitor                              | Comparison                                                |
//! |--------------------|--------------------------------------|-----------------------------------------------------------|
//! | `tree`             | `annotated_visitor_tree`             | `&MoveStructLayout` derived `PartialEq` (deep tree walk)  |
//! | `owned`            | `annotated_visitor`                  | `MoveStructLayout::equivalent` (compressed, cross-pool)   |
//! | `unpacked_layout`  | `annotated_visitor_unpacked`         | `d.layout()? == target` (one Arc bump + cross-pool walk)  |
//! | `exp_layout`       | `annotated_visitor_exp`              | `d.layout()?` + custom structural walk on `ExpLayoutRef`  |
//!
//! Run with `cargo bench --bench find_by_layout`.

mod common;

use criterion::{Criterion, black_box, criterion_group, criterion_main};

use move_core_types::{
    account_address::AccountAddress,
    annotated_value::{
        MoveStructLayout as TreeMoveStructLayout, MoveTypeLayout as TreeMoveTypeLayout,
        MoveValue as AnnotatedMoveValue,
    },
    annotated_visitor as av_owned, annotated_visitor_exp as av_exp,
    annotated_visitor_gat as av_gat, annotated_visitor_tree as av_tree,
    annotated_visitor_unpacked as av_unpacked,
    compressed::annotated::{
        ExpMoveTypeLayout, ExpMoveTypeLayoutRef, MoveLayoutView, MoveStructLayout, MoveTypeLayout,
    },
    compressed::gat::annotated::{MoveLayoutView as GatView, MoveTypeLayout as GatLayout},
    compressed::gat::backend::arc_pool::AnnotatedArcPool,
};

use crate::common::{
    FIND_UIDS_N as N_ITEMS, find_uids_bytes, find_uids_expected, find_uids_layout, uid_layout,
};

// ---------------------------------------------------------------------------
// Tree visitor — derived `PartialEq` on `&MoveStructLayout`.
// ---------------------------------------------------------------------------

struct TreeFind {
    target: TreeMoveStructLayout,
    out: Vec<AccountAddress>,
    inside_uid: bool,
}

impl<'b, 'l> av_tree::Traversal<'b, 'l> for TreeFind {
    type Error = av_tree::Error;

    fn traverse_address(
        &mut self,
        _: &av_tree::ValueDriver<'_, 'b, 'l>,
        v: AccountAddress,
    ) -> Result<(), Self::Error> {
        if self.inside_uid {
            self.out.push(v);
        }
        Ok(())
    }

    fn traverse_struct(
        &mut self,
        d: &mut av_tree::StructDriver<'_, 'b, 'l>,
    ) -> Result<(), Self::Error> {
        let was_inside = self.inside_uid;
        if !was_inside && d.struct_layout() == &self.target {
            self.inside_uid = true;
        }
        while d.next_field(self)?.is_some() {}
        self.inside_uid = was_inside;
        Ok(())
    }
}

fn run_tree(
    bytes: &[u8],
    layout: &TreeMoveTypeLayout,
    target: &TreeMoveStructLayout,
) -> Vec<AccountAddress> {
    let mut v = TreeFind {
        target: target.clone(),
        out: Vec::with_capacity(N_ITEMS),
        inside_uid: false,
    };
    let mut cursor = std::io::Cursor::new(bytes);
    av_tree::visit_value(&mut cursor, layout, &mut v).unwrap();
    v.out
}

// ---------------------------------------------------------------------------
// Owned visitor — compressed `MoveStructLayout::equivalent`.
// ---------------------------------------------------------------------------

struct OwnedFind {
    target: MoveStructLayout,
    out: Vec<AccountAddress>,
    inside_uid: bool,
}

impl<'b> av_owned::Traversal<'b> for OwnedFind {
    type Error = av_owned::Error;

    fn traverse_address(
        &mut self,
        _: &av_owned::ValueDriver<'_, 'b>,
        v: AccountAddress,
    ) -> Result<(), Self::Error> {
        if self.inside_uid {
            self.out.push(v);
        }
        Ok(())
    }

    fn traverse_struct(
        &mut self,
        d: &mut av_owned::StructDriver<'_, 'b>,
    ) -> Result<(), Self::Error> {
        let was_inside = self.inside_uid;
        if !was_inside && d.struct_layout() == self.target {
            self.inside_uid = true;
        }
        while d.next_field(self)?.is_some() {}
        self.inside_uid = was_inside;
        Ok(())
    }
}

fn run_owned(
    bytes: &[u8],
    layout: &MoveTypeLayout,
    target: &MoveStructLayout,
) -> Vec<AccountAddress> {
    let mut v = OwnedFind {
        target: target.clone(),
        out: Vec::with_capacity(N_ITEMS),
        inside_uid: false,
    };
    AnnotatedMoveValue::visit_deserialize(bytes, layout.clone(), &mut v).unwrap();
    v.out
}

// ---------------------------------------------------------------------------
// Unpacked visitor — `d.layout()? == target`.
//
// `MoveTypeLayout::PartialEq` calls `equivalent`, which short-circuits on
// `Arc::ptr_eq` (irrelevant here — the target is built in a different pool)
// and otherwise walks both pools with a memoized set. So this measures the
// cross-pool structural-equivalence cost on top of the one Arc bump from
// `d.layout()`.
// ---------------------------------------------------------------------------

struct UnpackedLayoutFind {
    target: MoveTypeLayout,
    out: Vec<AccountAddress>,
    inside_uid: bool,
}

impl<'b> av_unpacked::Traversal<'b> for UnpackedLayoutFind {
    type Error = av_owned::Error;

    fn traverse_address(
        &mut self,
        _: &av_unpacked::ValueDriver<'b>,
        v: AccountAddress,
    ) -> Result<(), Self::Error> {
        if self.inside_uid {
            self.out.push(v);
        }
        Ok(())
    }

    fn traverse_struct(
        &mut self,
        d: &mut av_unpacked::StructDriver<'_, 'b>,
    ) -> Result<(), Self::Error> {
        let was_inside = self.inside_uid;
        if !was_inside {
            let layout = d.layout()?;
            if layout == self.target {
                self.inside_uid = true;
            }
        }
        while d.next_field(self)?.is_some() {}
        self.inside_uid = was_inside;
        Ok(())
    }
}

fn run_unpacked_layout(
    bytes: &[u8],
    layout: &MoveTypeLayout,
    target: &MoveTypeLayout,
) -> Vec<AccountAddress> {
    let mut v = UnpackedLayoutFind {
        target: target.clone(),
        out: Vec::with_capacity(N_ITEMS),
        inside_uid: false,
    };
    av_unpacked::visit_deserialize(bytes, layout, &mut v).unwrap();
    v.out
}

// ---------------------------------------------------------------------------
// Exp visitor — custom structural walk on `ExpMoveTypeLayoutRef`.
//
// `ExpMoveTypeLayout::PartialEq` only does `Arc::ptr_eq + root ==`, which
// is useless across pools. We materialize the current layout via
// `d.layout()?` and compare structurally against the precomputed target.
// ---------------------------------------------------------------------------

fn exp_layout_eq(a: ExpMoveTypeLayoutRef<'_>, b: ExpMoveTypeLayoutRef<'_>) -> bool {
    use ExpMoveTypeLayoutRef as L;
    match (a, b) {
        (L::Bool, L::Bool)
        | (L::U8, L::U8)
        | (L::U16, L::U16)
        | (L::U32, L::U32)
        | (L::U64, L::U64)
        | (L::U128, L::U128)
        | (L::U256, L::U256)
        | (L::Address, L::Address)
        | (L::Signer, L::Signer) => true,
        (L::Vector(va), L::Vector(vb)) => exp_layout_eq(va.element(), vb.element()),
        (L::Struct(sa), L::Struct(sb)) => {
            if sa.type_() != sb.type_() || sa.field_count() != sb.field_count() {
                return false;
            }
            sa.fields()
                .zip(sb.fields())
                .all(|(fa, fb)| fa.name() == fb.name() && exp_layout_eq(fa.layout(), fb.layout()))
        }
        (L::Enum(ea), L::Enum(eb)) => {
            if ea.type_() != eb.type_() || ea.variant_count() != eb.variant_count() {
                return false;
            }
            ea.variants().zip(eb.variants()).all(|(va, vb)| {
                if va.tag() != vb.tag() || va.name() != vb.name() {
                    return false;
                }
                match (va.fields(), vb.fields()) {
                    (None, None) => true,
                    (Some(fa), Some(fb)) => fa.into_iter().zip(fb.into_iter()).all(|(fa, fb)| {
                        fa.name() == fb.name() && exp_layout_eq(fa.layout(), fb.layout())
                    }),
                    _ => false,
                }
            })
        }
        _ => false,
    }
}

struct ExpLayoutFind {
    target: ExpMoveTypeLayout,
    out: Vec<AccountAddress>,
    inside_uid: bool,
}

impl<'b> av_exp::Traversal<'b> for ExpLayoutFind {
    type Error = av_owned::Error;

    fn traverse_address(
        &mut self,
        _: &av_exp::ValueDriver<'b>,
        v: AccountAddress,
    ) -> Result<(), Self::Error> {
        if self.inside_uid {
            self.out.push(v);
        }
        Ok(())
    }

    fn traverse_struct(&mut self, d: &mut av_exp::StructDriver<'_, 'b>) -> Result<(), Self::Error> {
        let was_inside = self.inside_uid;
        if !was_inside {
            let layout = d.layout()?;
            if exp_layout_eq(layout.as_view_ref(), self.target.as_view_ref()) {
                self.inside_uid = true;
            }
        }
        while d.next_field(self)?.is_some() {}
        self.inside_uid = was_inside;
        Ok(())
    }
}

fn run_exp_layout(
    bytes: &[u8],
    layout: &ExpMoveTypeLayout,
    target: &ExpMoveTypeLayout,
) -> Vec<AccountAddress> {
    let mut v = ExpLayoutFind {
        target: target.clone(),
        out: Vec::with_capacity(N_ITEMS),
        inside_uid: false,
    };
    av_exp::visit_deserialize(bytes, layout, &mut v).unwrap();
    v.out
}

// silence the unused-import lint for `MoveLayoutView` (kept for symmetry
// with `benches/find.rs` so the bench is easy to compare side-by-side).
#[allow(dead_code)]
fn _unused(v: MoveLayoutView) -> MoveLayoutView {
    v
}

// ---------------------------------------------------------------------------
// Gat visitor — custom structural walk on `gat::MoveLayoutView`.
//
// Like the exp variant: the gat layout doesn't expose a cross-pool
// structural `PartialEq`, so we materialize the current struct via
// `d.layout()?` and compare it against the precomputed target by walking
// both views in lock-step.
// ---------------------------------------------------------------------------

fn gat_layout_eq(a: GatView<'_, AnnotatedArcPool>, b: GatView<'_, AnnotatedArcPool>) -> bool {
    use GatView as L;
    match (a, b) {
        (L::Bool, L::Bool)
        | (L::U8, L::U8)
        | (L::U16, L::U16)
        | (L::U32, L::U32)
        | (L::U64, L::U64)
        | (L::U128, L::U128)
        | (L::U256, L::U256)
        | (L::Address, L::Address)
        | (L::Signer, L::Signer) => true,
        (L::Vector(va), L::Vector(vb)) => gat_layout_eq(va.as_view(), vb.as_view()),
        (L::Struct(sa), L::Struct(sb)) => {
            if sa.type_() != sb.type_() || sa.field_count() != sb.field_count() {
                return false;
            }
            sa.fields().zip(sb.fields()).all(|((na, la), (nb, lb))| {
                na == nb && gat_layout_eq(la.as_view(), lb.as_view())
            })
        }
        (L::Enum(ea), L::Enum(eb)) => {
            if ea.type_() != eb.type_() || ea.variant_count() != eb.variant_count() {
                return false;
            }
            ea.variants().zip(eb.variants()).all(|(va, vb)| {
                if va.tag() != vb.tag() || va.name() != vb.name() {
                    return false;
                }
                match (va.fields(), vb.fields()) {
                    (None, None) => true,
                    (Some(fa), Some(fb)) => {
                        fa.field_count() == fb.field_count()
                            && fa.fields().zip(fb.fields()).all(|((na, la), (nb, lb))| {
                                na == nb && gat_layout_eq(la.as_view(), lb.as_view())
                            })
                    }
                    _ => false,
                }
            })
        }
        _ => false,
    }
}

struct GatLayoutFind {
    target: GatLayout<AnnotatedArcPool>,
    out: Vec<AccountAddress>,
    inside_uid: bool,
}

impl<'a, 'b> av_gat::Traversal<'a, 'b, AnnotatedArcPool> for GatLayoutFind {
    type Error = av_owned::Error;

    fn traverse_address(
        &mut self,
        _: &av_gat::ValueDriver<'_, 'a, 'b, AnnotatedArcPool>,
        v: AccountAddress,
    ) -> Result<(), Self::Error> {
        if self.inside_uid {
            self.out.push(v);
        }
        Ok(())
    }

    fn traverse_struct(
        &mut self,
        d: &mut av_gat::StructDriver<'_, 'a, 'b, AnnotatedArcPool>,
    ) -> Result<(), Self::Error> {
        let was_inside = self.inside_uid;
        if !was_inside {
            let layout = d.layout()?;
            if gat_layout_eq(layout.as_view(), self.target.as_view()) {
                self.inside_uid = true;
            }
        }
        while d.next_field(self)?.is_some() {}
        self.inside_uid = was_inside;
        Ok(())
    }
}

fn run_gat_layout(
    bytes: &[u8],
    layout: &GatLayout<AnnotatedArcPool>,
    target: &GatLayout<AnnotatedArcPool>,
) -> Vec<AccountAddress> {
    let mut v = GatLayoutFind {
        target: target.clone(),
        out: Vec::with_capacity(N_ITEMS),
        inside_uid: false,
    };
    let mut cursor = std::io::Cursor::new(bytes);
    av_gat::visit_value(&mut cursor, layout.as_ref(), &mut v).unwrap();
    v.out
}

// ---------------------------------------------------------------------------
// Bench driver
// ---------------------------------------------------------------------------

fn bench(c: &mut Criterion) {
    // Outer fixture (the value being scanned).
    let layout = find_uids_layout();
    let tree_layout = layout.inflate().unwrap();
    let exp_layout = ExpMoveTypeLayout::try_from(&tree_layout).unwrap();
    let gat_layout = GatLayout::<AnnotatedArcPool>::try_from(&tree_layout).unwrap();
    let bytes = find_uids_bytes();
    let expected = find_uids_expected();

    // Standalone UID target — built in an independent pool from `layout`,
    // so cross-pool equality has to actually walk.
    let target_owned = uid_layout();
    let target_tree = target_owned.inflate().unwrap();
    let target_exp = ExpMoveTypeLayout::try_from(&target_tree).unwrap();
    let target_gat = GatLayout::<AnnotatedArcPool>::try_from(&target_tree).unwrap();
    let target_owned_struct = target_owned.clone().into_struct().unwrap();
    let TreeMoveTypeLayout::Struct(target_tree_struct) = &target_tree else {
        panic!("UID target should be a struct");
    };

    // Correctness sanity-check before timing.
    assert_eq!(
        run_tree(&bytes, &tree_layout, target_tree_struct),
        expected,
        "tree"
    );
    assert_eq!(
        run_owned(&bytes, &layout, &target_owned_struct),
        expected,
        "owned"
    );
    assert_eq!(
        run_unpacked_layout(&bytes, &layout, &target_owned),
        expected,
        "unpacked_layout"
    );
    assert_eq!(
        run_exp_layout(&bytes, &exp_layout, &target_exp),
        expected,
        "exp_layout"
    );
    assert_eq!(
        run_gat_layout(&bytes, &gat_layout, &target_gat),
        expected,
        "gat_layout"
    );

    let mut group = c.benchmark_group("find_uids_by_layout");
    group.bench_function("tree", |b| {
        b.iter(|| {
            run_tree(
                black_box(&bytes),
                black_box(&tree_layout),
                black_box(target_tree_struct),
            )
        })
    });
    group.bench_function("owned", |b| {
        b.iter(|| {
            run_owned(
                black_box(&bytes),
                black_box(&layout),
                black_box(&target_owned_struct),
            )
        })
    });
    group.bench_function("unpacked_layout", |b| {
        b.iter(|| {
            run_unpacked_layout(
                black_box(&bytes),
                black_box(&layout),
                black_box(&target_owned),
            )
        })
    });
    group.bench_function("exp_layout", |b| {
        b.iter(|| {
            run_exp_layout(
                black_box(&bytes),
                black_box(&exp_layout),
                black_box(&target_exp),
            )
        })
    });
    group.bench_function("gat_layout", |b| {
        b.iter(|| {
            run_gat_layout(
                black_box(&bytes),
                black_box(&gat_layout),
                black_box(&target_gat),
            )
        })
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
