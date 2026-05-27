// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `OptionVisitor`-style traversal: walk a `vector<Option<T>>` and, at
//! each child struct, recognise it as `0x1::option::Option<T>` and pull
//! out its inner value.
//!
//! Two recognition styles, paired with each of the four "layout-capable"
//! visitor variants used in [`benches/find_by_layout`]:
//!
//! ## Styles
//!
//! - `layout`: full structural check (the production `is_option`):
//!     1. struct's `(address, module, name)` equals `(0x1, option, Option)`
//!     2. exactly one `type_param`
//!     3. exactly one field named `"vec"`
//!     4. that field is `vector<T>` where `T` matches `type_param`
//! - `type`:   only the `(address, module, name) + type_params.len()` check.
//!
//! ## Variants
//!
//! For each style, the visitor that traverses the outer value runs in
//! these four flavours:
//!
//! | Variant            | Visitor                              | How the struct's layout is reached            |
//! |--------------------|--------------------------------------|-----------------------------------------------|
//! | `tree`             | `annotated_visitor_tree`             | `d.struct_layout() -> &MoveStructLayout`      |
//! | `owned`            | `annotated_visitor`                  | `d.struct_layout() -> MoveStructLayout`       |
//! | `ref`              | `annotated_visitor_ref`              | `d.struct_layout() -> MoveStructLayoutRef<'a>`|
//! | `unpacked`         | `annotated_visitor_unpacked`         | `d.layout()? -> MoveTypeLayout` (Arc bump)    |
//!
//! Run with `cargo bench --bench option_visit`.

mod common;

use criterion::{Criterion, black_box, criterion_group, criterion_main};

use move_core_types::{
    account_address::AccountAddress,
    annotated_value::{
        self as TV, MoveTypeLayout as TreeMoveTypeLayout, MoveValue as AnnotatedMoveValue,
    },
    annotated_visitor as av_owned, annotated_visitor_gat as av_gat,
    annotated_visitor_ref as av_ref, annotated_visitor_tree as av_tree,
    annotated_visitor_unpacked as av_unpacked,
    compressed::annotated::{
        MoveLayoutView, MoveLayoutViewRef, MoveStructLayout, MoveStructLayoutRef,
        MoveTypeLayout, MoveTypeLayoutRef,
    },
    compressed::gat::annotated::{
        MoveLayoutView as GatView, MoveStructLayout as GatStruct, MoveTypeLayout as GatLayout,
        TypeLayout as GatTypeLayout,
    },
    compressed::gat::backend::arc_pool::AnnotatedArcPool,
    language_storage::TypeTag,
};

use crate::common::{
    OPTION_N as N, option_tag, option_u64_expected, option_u64_outer_bytes, option_u64_outer_layout,
};

// ---------------------------------------------------------------------------
// `is_option` — five flavours so we don't pay for needless conversions.
// Each pair `_layout`/`_type` mirrors the bench's two recognition styles.
// ---------------------------------------------------------------------------

const STD_OPTION_ADDR: AccountAddress = AccountAddress::ONE;
const STD_OPTION_MODULE: &str = "option";
const STD_OPTION_NAME: &str = "Option";

fn tag_matches_option(t: &move_core_types::language_storage::StructTag) -> bool {
    t.address == STD_OPTION_ADDR
        && t.module.as_str() == STD_OPTION_MODULE
        && t.name.as_str() == STD_OPTION_NAME
        && t.type_params.len() == 1
}

// --- compressed (owned & unpacked-via-d.layout()) ---

fn is_option_compressed_layout(s: &MoveStructLayout) -> bool {
    let ty = s.type_();
    if !tag_matches_option(ty) {
        return false;
    }
    let type_param = &ty.type_params[0];
    if s.field_count() != 1 {
        return false;
    }
    let Some((name, field_layout)) = s.fields().next() else {
        return false;
    };
    if name.as_str() != "vec" {
        return false;
    }
    match field_layout.as_view() {
        MoveLayoutView::Vector(elem) => elem.is_type(type_param),
        _ => false,
    }
}

// --- tree ---

fn is_option_tree_layout(s: &TV::MoveStructLayout) -> bool {
    if !tag_matches_option(&s.type_) {
        return false;
    }
    let type_param = &s.type_.type_params[0];
    if s.fields.len() != 1 {
        return false;
    }
    let field = &s.fields[0];
    if field.name.as_str() != "vec" {
        return false;
    }
    match &field.layout {
        TreeMoveTypeLayout::Vector(elem) => elem.is_type(type_param),
        _ => false,
    }
}

// --- ref ---

fn ref_layout_matches_type_tag(layout: MoveTypeLayoutRef<'_>, tag: &TypeTag) -> bool {
    use MoveLayoutViewRef as L;
    match (layout.as_view(), tag) {
        (L::Bool, TypeTag::Bool)
        | (L::U8, TypeTag::U8)
        | (L::U16, TypeTag::U16)
        | (L::U32, TypeTag::U32)
        | (L::U64, TypeTag::U64)
        | (L::U128, TypeTag::U128)
        | (L::U256, TypeTag::U256)
        | (L::Address, TypeTag::Address)
        | (L::Signer, TypeTag::Signer) => true,
        (L::Vector(inner), TypeTag::Vector(t)) => ref_layout_matches_type_tag(inner, t),
        (L::Struct(s), TypeTag::Struct(target)) => s.type_() == target.as_ref(),
        _ => false,
    }
}

fn is_option_ref_layout(s: MoveStructLayoutRef<'_>) -> bool {
    if !tag_matches_option(s.type_()) {
        return false;
    }
    let type_param = &s.type_().type_params[0];
    if s.field_count() != 1 {
        return false;
    }
    let Some((name, field_layout)) = s.fields().next() else {
        return false;
    };
    if name.as_str() != "vec" {
        return false;
    }
    match field_layout.as_view() {
        MoveLayoutViewRef::Vector(inner) => ref_layout_matches_type_tag(inner, type_param),
        _ => false,
    }
}

// --- gat ---

fn is_option_gat_layout<T: GatTypeLayout>(s: GatStruct<'_, T>) -> bool {
    let ty = s.type_();
    if !tag_matches_option(ty) {
        return false;
    }
    let type_param = &ty.type_params[0];
    if s.field_count() != 1 {
        return false;
    }
    let Some((name, field_layout)) = s.fields().next() else {
        return false;
    };
    if name.as_str() != "vec" {
        return false;
    }
    match field_layout.as_view() {
        GatView::Vector(elem) => elem.as_view().is_type_tag(type_param),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Visitors — each impl exists in two flavours (layout / type). We
// parameterise via a bool field to keep the code small.
// ---------------------------------------------------------------------------

// `full_layout = true`  → run `is_option_*_layout`.
// `full_layout = false` → only check the `StructTag` via `tag_matches_option`.

// --- tree ---

struct TreeFind {
    out: Vec<u64>,
    inside: bool,
    full_layout: bool,
}

impl<'b, 'l> av_tree::Traversal<'b, 'l> for TreeFind {
    type Error = av_tree::Error;

    fn traverse_u64(
        &mut self,
        _: &av_tree::ValueDriver<'_, 'b, 'l>,
        v: u64,
    ) -> Result<(), Self::Error> {
        if self.inside {
            self.out.push(v);
        }
        Ok(())
    }

    fn traverse_struct(
        &mut self,
        d: &mut av_tree::StructDriver<'_, 'b, 'l>,
    ) -> Result<(), Self::Error> {
        let was_inside = self.inside;
        let s = d.struct_layout();
        let matched = if self.full_layout {
            is_option_tree_layout(s)
        } else {
            tag_matches_option(&s.type_)
        };
        if matched {
            self.inside = true;
        }
        while d.next_field(self)?.is_some() {}
        self.inside = was_inside;
        Ok(())
    }
}

fn run_tree(bytes: &[u8], layout: &TreeMoveTypeLayout, full_layout: bool) -> Vec<u64> {
    let mut v = TreeFind {
        out: Vec::with_capacity(N),
        inside: false,
        full_layout,
    };
    let mut cursor = std::io::Cursor::new(bytes);
    av_tree::visit_value(&mut cursor, layout, &mut v).unwrap();
    v.out
}

// --- owned ---

struct OwnedFind {
    out: Vec<u64>,
    inside: bool,
    full_layout: bool,
}

impl<'b> av_owned::Traversal<'b> for OwnedFind {
    type Error = av_owned::Error;

    fn traverse_u64(
        &mut self,
        _: &av_owned::ValueDriver<'_, 'b>,
        v: u64,
    ) -> Result<(), Self::Error> {
        if self.inside {
            self.out.push(v);
        }
        Ok(())
    }

    fn traverse_struct(
        &mut self,
        d: &mut av_owned::StructDriver<'_, 'b>,
    ) -> Result<(), Self::Error> {
        let was_inside = self.inside;
        let s = d.struct_layout();
        let matched = if self.full_layout {
            is_option_compressed_layout(&s)
        } else {
            tag_matches_option(s.type_())
        };
        if matched {
            self.inside = true;
        }
        while d.next_field(self)?.is_some() {}
        self.inside = was_inside;
        Ok(())
    }
}

fn run_owned(bytes: &[u8], layout: &MoveTypeLayout, full_layout: bool) -> Vec<u64> {
    let mut v = OwnedFind {
        out: Vec::with_capacity(N),
        inside: false,
        full_layout,
    };
    AnnotatedMoveValue::visit_deserialize(bytes, layout.clone(), &mut v).unwrap();
    v.out
}

// --- unpacked ---

struct UnpackedFind {
    out: Vec<u64>,
    inside: bool,
    full_layout: bool,
}

impl<'b> av_unpacked::Traversal<'b> for UnpackedFind {
    type Error = av_owned::Error;

    fn traverse_u64(
        &mut self,
        _: &av_unpacked::ValueDriver<'b>,
        v: u64,
    ) -> Result<(), Self::Error> {
        if self.inside {
            self.out.push(v);
        }
        Ok(())
    }

    fn traverse_struct(
        &mut self,
        d: &mut av_unpacked::StructDriver<'_, 'b>,
    ) -> Result<(), Self::Error> {
        let was_inside = self.inside;
        let matched = if self.full_layout {
            let layout = d.layout()?;
            match layout.as_view() {
                MoveLayoutView::Struct(s) => is_option_compressed_layout(&s),
                _ => false,
            }
        } else {
            tag_matches_option(d.struct_type())
        };
        if matched {
            self.inside = true;
        }
        while d.next_field(self)?.is_some() {}
        self.inside = was_inside;
        Ok(())
    }
}

fn run_unpacked(bytes: &[u8], layout: &MoveTypeLayout, full_layout: bool) -> Vec<u64> {
    let mut v = UnpackedFind {
        out: Vec::with_capacity(N),
        inside: false,
        full_layout,
    };
    av_unpacked::visit_deserialize(bytes, layout, &mut v).unwrap();
    v.out
}

// --- ref ---

struct RefFind {
    out: Vec<u64>,
    inside: bool,
    full_layout: bool,
}

impl<'a, 'b> av_ref::Traversal<'a, 'b> for RefFind {
    type Error = av_owned::Error;

    fn traverse_u64(
        &mut self,
        _: &av_ref::ValueDriver<'_, 'a, 'b>,
        v: u64,
    ) -> Result<(), Self::Error> {
        if self.inside {
            self.out.push(v);
        }
        Ok(())
    }

    fn traverse_struct(
        &mut self,
        d: &mut av_ref::StructDriver<'_, 'a, 'b>,
    ) -> Result<(), Self::Error> {
        let was_inside = self.inside;
        let s = d.struct_layout();
        let matched = if self.full_layout {
            is_option_ref_layout(s)
        } else {
            tag_matches_option(s.type_())
        };
        if matched {
            self.inside = true;
        }
        while d.next_field(self)?.is_some() {}
        self.inside = was_inside;
        Ok(())
    }
}

fn run_ref(bytes: &[u8], layout: &MoveTypeLayout, full_layout: bool) -> Vec<u64> {
    let mut v = RefFind {
        out: Vec::with_capacity(N),
        inside: false,
        full_layout,
    };
    let mut cursor = std::io::Cursor::new(bytes);
    av_ref::visit_value(&mut cursor, layout.as_layout_ref(), &mut v).unwrap();
    v.out
}

// --- gat ---

struct GatFind {
    out: Vec<u64>,
    inside: bool,
    full_layout: bool,
}

impl<'a, 'b, T: GatTypeLayout> av_gat::Traversal<'a, 'b, T> for GatFind {
    type Error = av_owned::Error;

    fn traverse_u64(
        &mut self,
        _: &av_gat::ValueDriver<'_, 'a, 'b, T>,
        v: u64,
    ) -> Result<(), Self::Error> {
        if self.inside {
            self.out.push(v);
        }
        Ok(())
    }

    fn traverse_struct(
        &mut self,
        d: &mut av_gat::StructDriver<'_, 'a, 'b, T>,
    ) -> Result<(), Self::Error> {
        let was_inside = self.inside;
        let s = d.struct_layout();
        let matched = if self.full_layout {
            is_option_gat_layout(s)
        } else {
            tag_matches_option(s.type_())
        };
        if matched {
            self.inside = true;
        }
        while d.next_field(self)?.is_some() {}
        self.inside = was_inside;
        Ok(())
    }
}

fn run_gat<T: GatTypeLayout>(bytes: &[u8], layout: &GatLayout<T>, full_layout: bool) -> Vec<u64> {
    let mut v = GatFind {
        out: Vec::with_capacity(N),
        inside: false,
        full_layout,
    };
    let mut cursor = std::io::Cursor::new(bytes);
    av_gat::visit_value(&mut cursor, layout.as_ref(), &mut v).unwrap();
    v.out
}

// ---------------------------------------------------------------------------
// Bench driver
// ---------------------------------------------------------------------------

fn bench(c: &mut Criterion) {
    let _ = option_tag; // re-exported for downstream use; silence dead-code in this file.

    let layout = option_u64_outer_layout();
    let tree_layout = layout.inflate().unwrap();
    let gat_layout = GatLayout::<AnnotatedArcPool>::try_from(&tree_layout).unwrap();
    let bytes = option_u64_outer_bytes();
    let expected = option_u64_expected();

    // Correctness sanity-check every variant in both styles before timing.
    for &full in &[true, false] {
        assert_eq!(
            run_tree(&bytes, &tree_layout, full),
            expected,
            "tree full={full}"
        );
        assert_eq!(
            run_owned(&bytes, &layout, full),
            expected,
            "owned full={full}"
        );
        assert_eq!(
            run_ref(&bytes, &layout, full),
            expected,
            "ref full={full}"
        );
        assert_eq!(
            run_unpacked(&bytes, &layout, full),
            expected,
            "unpacked full={full}"
        );
        assert_eq!(
            run_gat(&bytes, &gat_layout, full),
            expected,
            "gat full={full}"
        );
    }

    let mut group = c.benchmark_group("option_visit");
    for (suffix, full) in [("layout", true), ("type", false)] {
        group.bench_function(format!("tree/{suffix}"), |b| {
            b.iter(|| run_tree(black_box(&bytes), black_box(&tree_layout), full))
        });
        group.bench_function(format!("owned/{suffix}"), |b| {
            b.iter(|| run_owned(black_box(&bytes), black_box(&layout), full))
        });
        group.bench_function(format!("ref/{suffix}"), |b| {
            b.iter(|| run_ref(black_box(&bytes), black_box(&layout), full))
        });
        group.bench_function(format!("unpacked/{suffix}"), |b| {
            b.iter(|| run_unpacked(black_box(&bytes), black_box(&layout), full))
        });
        group.bench_function(format!("gat/{suffix}"), |b| {
            b.iter(|| run_gat(black_box(&bytes), black_box(&gat_layout), full))
        });
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
