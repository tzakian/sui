// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! "Find-a-type" microbenchmark: walk a value and collect every
//! `AccountAddress` that lives inside a struct tagged `UID`.
//!
//! Production analog: `find_all_uids`-style traversals.
//!
//! ## Variants
//!
//! All variants run the same workload (output: `Vec<AccountAddress>` of
//! length `N_ITEMS`). They differ only in *how* the visitor recognises
//! "this struct is a UID":
//!
//! | Variant            | Visitor                                | Identification path                                     |
//! |--------------------|----------------------------------------|---------------------------------------------------------|
//! | `owned`            | `annotated_visitor`                    | `d.struct_layout().type_()` — direct accessor           |
//! | `ref`              | `annotated_visitor_ref`                | `d.struct_layout().type_()` — direct accessor           |
//! | `unpacked_direct`  | `annotated_visitor_unpacked`           | `d.struct_type()` — direct accessor (one ptr deref)     |
//! | `unpacked_layout`  | `annotated_visitor_unpacked`           | `d.layout()?.as_view()` — materialize + match (Arc bump)|
//! | `exp_direct`       | `annotated_visitor_exp`                | `d.struct_type()` — direct accessor                     |
//! | `exp_layout`       | `annotated_visitor_exp`                | `d.layout()?.as_view_ref()` — materialize + match       |
//!
//! Run with `cargo bench --bench find`.

mod common;

use criterion::{Criterion, black_box, criterion_group, criterion_main};

use move_core_types::{
    account_address::AccountAddress,
    annotated_value::{MoveTypeLayout as TreeMoveTypeLayout, MoveValue as AnnotatedMoveValue},
    annotated_visitor as av_owned, annotated_visitor_exp as av_exp,
    annotated_visitor_gat as av_gat, annotated_visitor_ref as av_ref,
    annotated_visitor_tree as av_tree, annotated_visitor_unpacked as av_unpacked,
    compressed::annotated::{
        ExpMoveTypeLayout, ExpMoveTypeLayoutRef, MoveLayoutView, MoveTypeLayout,
    },
    compressed::gat::annotated::{MoveTypeLayout as GatLayout, TypeLayout as GatTypeLayout},
    compressed::gat::backend::arc_pool::AnnotatedArcPool,
    language_storage::StructTag,
};

use crate::common::{
    FIND_UIDS_N as N_ITEMS, find_uids_bytes, find_uids_expected, find_uids_layout,
    uid_tag as uid_type,
};

// ---------------------------------------------------------------------------
// Owned visitor (`annotated_visitor`).
// ---------------------------------------------------------------------------

struct OwnedFind {
    target: StructTag,
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
        if !was_inside && d.struct_layout().type_() == &self.target {
            self.inside_uid = true;
        }
        while d.next_field(self)?.is_some() {}
        self.inside_uid = was_inside;
        Ok(())
    }
}

fn run_owned(bytes: &[u8], layout: &MoveTypeLayout) -> Vec<AccountAddress> {
    let mut v = OwnedFind {
        target: uid_type(),
        out: Vec::with_capacity(N_ITEMS),
        inside_uid: false,
    };
    AnnotatedMoveValue::visit_deserialize(bytes, layout.clone(), &mut v).unwrap();
    v.out
}

// ---------------------------------------------------------------------------
// Ref visitor (`annotated_visitor_ref`).
// ---------------------------------------------------------------------------

struct RefFind {
    target: StructTag,
    out: Vec<AccountAddress>,
    inside_uid: bool,
}

impl<'a, 'b> av_ref::Traversal<'a, 'b> for RefFind {
    type Error = av_owned::Error;

    fn traverse_address(
        &mut self,
        _: &av_ref::ValueDriver<'_, 'a, 'b>,
        v: AccountAddress,
    ) -> Result<(), Self::Error> {
        if self.inside_uid {
            self.out.push(v);
        }
        Ok(())
    }

    fn traverse_struct(
        &mut self,
        d: &mut av_ref::StructDriver<'_, 'a, 'b>,
    ) -> Result<(), Self::Error> {
        let was_inside = self.inside_uid;
        if !was_inside && d.struct_layout().type_() == &self.target {
            self.inside_uid = true;
        }
        while d.next_field(self)?.is_some() {}
        self.inside_uid = was_inside;
        Ok(())
    }
}

fn run_ref(bytes: &[u8], layout: &MoveTypeLayout) -> Vec<AccountAddress> {
    let mut v = RefFind {
        target: uid_type(),
        out: Vec::with_capacity(N_ITEMS),
        inside_uid: false,
    };
    let mut cursor = std::io::Cursor::new(bytes);
    av_ref::visit_value(&mut cursor, layout.as_layout_ref(), &mut v).unwrap();
    v.out
}

// ---------------------------------------------------------------------------
// Tree visitor (`annotated_visitor_tree` — the uncompressed `Box`-nested
// layout from origin/main).
// ---------------------------------------------------------------------------

struct TreeFind {
    target: StructTag,
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
        if !was_inside && d.struct_layout().type_ == self.target {
            self.inside_uid = true;
        }
        while d.next_field(self)?.is_some() {}
        self.inside_uid = was_inside;
        Ok(())
    }
}

fn run_tree(bytes: &[u8], layout: &TreeMoveTypeLayout) -> Vec<AccountAddress> {
    let mut v = TreeFind {
        target: uid_type(),
        out: Vec::with_capacity(N_ITEMS),
        inside_uid: false,
    };
    let mut cursor = std::io::Cursor::new(bytes);
    av_tree::visit_value(&mut cursor, layout, &mut v).unwrap();
    v.out
}

// ---------------------------------------------------------------------------
// Unpacked visitor (`annotated_visitor_unpacked`).
// ---------------------------------------------------------------------------

/// Identification: `d.struct_type()` (cached `*const StructTag`).
struct UnpackedDirectFind {
    target: StructTag,
    out: Vec<AccountAddress>,
    inside_uid: bool,
}

impl<'b> av_unpacked::Traversal<'b> for UnpackedDirectFind {
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
        if !was_inside && d.struct_type() == &self.target {
            self.inside_uid = true;
        }
        while d.next_field(self)?.is_some() {}
        self.inside_uid = was_inside;
        Ok(())
    }
}

fn run_unpacked_direct(bytes: &[u8], layout: &MoveTypeLayout) -> Vec<AccountAddress> {
    let mut v = UnpackedDirectFind {
        target: uid_type(),
        out: Vec::with_capacity(N_ITEMS),
        inside_uid: false,
    };
    av_unpacked::visit_deserialize(bytes, layout, &mut v).unwrap();
    v.out
}

/// Identification: `d.layout()?.as_view()` then match on `MoveLayoutView::Struct(s).type_()`.
/// One `Arc` refcount bump per visited struct.
struct UnpackedLayoutFind {
    target: StructTag,
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
            if let MoveLayoutView::Struct(s) = layout.as_view()
                && s.type_() == &self.target
            {
                self.inside_uid = true;
            }
        }
        while d.next_field(self)?.is_some() {}
        self.inside_uid = was_inside;
        Ok(())
    }
}

fn run_unpacked_layout(bytes: &[u8], layout: &MoveTypeLayout) -> Vec<AccountAddress> {
    let mut v = UnpackedLayoutFind {
        target: uid_type(),
        out: Vec::with_capacity(N_ITEMS),
        inside_uid: false,
    };
    av_unpacked::visit_deserialize(bytes, layout, &mut v).unwrap();
    v.out
}

// ---------------------------------------------------------------------------
// Exp visitor (`annotated_visitor_exp`).
// ---------------------------------------------------------------------------

struct ExpDirectFind {
    target: StructTag,
    out: Vec<AccountAddress>,
    inside_uid: bool,
}

impl<'b> av_exp::Traversal<'b> for ExpDirectFind {
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
        if !was_inside && d.struct_type() == &self.target {
            self.inside_uid = true;
        }
        while d.next_field(self)?.is_some() {}
        self.inside_uid = was_inside;
        Ok(())
    }
}

fn run_exp_direct(bytes: &[u8], layout: &ExpMoveTypeLayout) -> Vec<AccountAddress> {
    let mut v = ExpDirectFind {
        target: uid_type(),
        out: Vec::with_capacity(N_ITEMS),
        inside_uid: false,
    };
    av_exp::visit_deserialize(bytes, layout, &mut v).unwrap();
    v.out
}

struct ExpLayoutFind {
    target: StructTag,
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
            if let ExpMoveTypeLayoutRef::Struct(s) = layout.as_view_ref()
                && s.type_() == &self.target
            {
                self.inside_uid = true;
            }
        }
        while d.next_field(self)?.is_some() {}
        self.inside_uid = was_inside;
        Ok(())
    }
}

fn run_exp_layout(bytes: &[u8], layout: &ExpMoveTypeLayout) -> Vec<AccountAddress> {
    let mut v = ExpLayoutFind {
        target: uid_type(),
        out: Vec::with_capacity(N_ITEMS),
        inside_uid: false,
    };
    av_exp::visit_deserialize(bytes, layout, &mut v).unwrap();
    v.out
}

// ---------------------------------------------------------------------------
// Gat visitor (`annotated_visitor_gat`, PR #26798 backend-abstracted layout).
// Identification: `d.struct_layout().type_()` — direct accessor, peer of `ref`.
// ---------------------------------------------------------------------------

struct GatFind {
    target: StructTag,
    out: Vec<AccountAddress>,
    inside_uid: bool,
}

impl<'a, 'b, T: GatTypeLayout> av_gat::Traversal<'a, 'b, T> for GatFind {
    type Error = av_owned::Error;

    fn traverse_address(
        &mut self,
        _: &av_gat::ValueDriver<'_, 'a, 'b, T>,
        v: AccountAddress,
    ) -> Result<(), Self::Error> {
        if self.inside_uid {
            self.out.push(v);
        }
        Ok(())
    }

    fn traverse_struct(
        &mut self,
        d: &mut av_gat::StructDriver<'_, 'a, 'b, T>,
    ) -> Result<(), Self::Error> {
        let was_inside = self.inside_uid;
        if !was_inside && d.struct_layout().type_() == &self.target {
            self.inside_uid = true;
        }
        while d.next_field(self)?.is_some() {}
        self.inside_uid = was_inside;
        Ok(())
    }
}

fn run_gat<T: GatTypeLayout>(bytes: &[u8], layout: &GatLayout<T>) -> Vec<AccountAddress> {
    let mut v = GatFind {
        target: uid_type(),
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

fn bench_find(c: &mut Criterion) {
    let layout = find_uids_layout();
    let tree_layout = layout.inflate().unwrap();
    let exp_layout = ExpMoveTypeLayout::try_from(&tree_layout).unwrap();
    let gat_layout = GatLayout::<AnnotatedArcPool>::try_from(&tree_layout).unwrap();
    let bytes = find_uids_bytes();
    let expected = find_uids_expected();

    // Correctness sanity-check every variant before timing.
    assert_eq!(run_owned(&bytes, &layout), expected, "owned");
    assert_eq!(run_ref(&bytes, &layout), expected, "ref");
    assert_eq!(run_tree(&bytes, &tree_layout), expected, "tree");
    assert_eq!(
        run_unpacked_direct(&bytes, &layout),
        expected,
        "unpacked_direct"
    );
    assert_eq!(
        run_unpacked_layout(&bytes, &layout),
        expected,
        "unpacked_layout"
    );
    assert_eq!(run_exp_direct(&bytes, &exp_layout), expected, "exp_direct");
    assert_eq!(run_exp_layout(&bytes, &exp_layout), expected, "exp_layout");
    assert_eq!(run_gat(&bytes, &gat_layout), expected, "gat");

    let mut group = c.benchmark_group("find_uids");
    group.bench_function("owned", |b| {
        b.iter(|| run_owned(black_box(&bytes), black_box(&layout)))
    });
    group.bench_function("ref", |b| {
        b.iter(|| run_ref(black_box(&bytes), black_box(&layout)))
    });
    group.bench_function("tree", |b| {
        b.iter(|| run_tree(black_box(&bytes), black_box(&tree_layout)))
    });
    group.bench_function("unpacked_direct", |b| {
        b.iter(|| run_unpacked_direct(black_box(&bytes), black_box(&layout)))
    });
    group.bench_function("unpacked_layout", |b| {
        b.iter(|| run_unpacked_layout(black_box(&bytes), black_box(&layout)))
    });
    group.bench_function("exp_direct", |b| {
        b.iter(|| run_exp_direct(black_box(&bytes), black_box(&exp_layout)))
    });
    group.bench_function("exp_layout", |b| {
        b.iter(|| run_exp_layout(black_box(&bytes), black_box(&exp_layout)))
    });
    group.bench_function("gat", |b| {
        b.iter(|| run_gat(black_box(&bytes), black_box(&gat_layout)))
    });
    group.finish();
}

criterion_group!(benches, bench_find);
criterion_main!(benches);
