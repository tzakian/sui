# Visitor Migration — Current State (2026-04-01)

## What's Done

### Core infrastructure (complete)
- `annotated_visitor.rs` — fully rewritten. Drivers hold compressed views. All accessor methods return view types.
- `annotated_value.rs` — `visit_deserialize` takes `MoveLayoutView`, `MoveStruct::visit_deserialize` takes `MoveStructView`. Added `MoveFieldLayoutView`, `is_type()` on views, `From<MoveLayoutView> for TypeTag`.
- `annotated_extractor.rs` — updated to use view-based accessors. Added `deserialize_struct_view`.

### sui-types (complete, compiles clean)
- `bounded_visitor.rs` — `.type_()`, `.name()`, compress-before-visit
- `rpc_visitor/mod.rs` — type tag comparisons instead of layout equality
- `rpc_visitor/proto.rs` — compress before `deserialize_value`
- `option_visitor.rs` — rewritten to use views
- `dynamic_field/visitor.rs` — rewritten, `Field` holds `MoveLayoutView`. Added `deserialize_view`.
- `balance_traversal.rs` — `.type_()` fix
- `object.rs` — compress before `visit_deserialize`

### sui-execution (compiles clean)
- `latest/object_runtime/mod.rs` — `.as_view()` for visit_deserialize
- `latest/test_scenario.rs` — uses views, `uid_type` instead of tree layout comparison
- `latest/sui-adapter/context.rs` — removed unnecessary inflate
- `v0-v3/object_runtime/mod.rs` — compress tree layout, then `.as_view()`
- `v0-v3/test_scenario.rs` — compress tree layout, use views for peek_field

### sui-display (partially done)
- `v1/mod.rs` — compress before visit_deserialize ✅
- `v2/visitor/extractor.rs` — `.type_()`, `.name()`, use `visit_deserialize` with view ✅
- `v2/visitor/address.rs` — type tag comparisons instead of layout equality ✅
- `v2/visitor/vec_map.rs` — `.type_()`, `.name()` fixes ✅
- `v2/interpreter.rs` — uses `deserialize_view` for Slice layouts ✅
- `v2/value.rs` — `layout()` returns `Option<AC::MoveTypeLayout>`, pattern matching rewritten for views ✅
  - BUT: `value.rs` has test code (~20 `OwnedSlice` constructions) that still use tree layouts — NOT YET FIXED

### Other crates (partially done)
- `sui-rpc-api/render.rs` — compress before `deserialize_value` and `OwnedSlice` ✅
- `sui-rpc-api/reader.rs` — compress for `OwnedSlice` ✅
- `sui-rpc-api/simulate/mod.rs` — compress before `deserialize_value` ✅
- `sui-rpc-resolver/json_visitor.rs` — compress before visit_deserialize ✅
- `sui-core/authority.rs` — fixed by agent ✅
- `sui-analytics-indexer/handlers/tables/df.rs` — fixed by agent ✅
- `sui-indexer-alt-jsonrpc/api/dynamic_fields/response.rs` — fixed by agent ✅

## What's Not Done / Currently Broken

### sui-indexer-alt-graphql (22+ errors, cascading)
Most errors are in `move_value.rs` and cascade into `dynamic_field.rs`, `move_object.rs`,
`coin_metadata.rs`, `address.rs`, `object.rs`. Root causes:
1. `OwnedSlice` constructions with tree layouts (need `AC::MoveTypeLayout::from(&layout)`)
2. `MoveType::from_layout()` expects tree `A::MoveTypeLayout` but receives compressed after inflate
3. `inflate()` error handling mismatches

The graphql types module is large and heavily uses `MoveTypeLayout` as part of `MoveType::from_layout()`.
Many call paths: `layout_impl()` → tree → needs compress for OwnedSlice or visit_deserialize.

### sui-display/v2/value.rs (test code)
~20 test constructions of `OwnedSlice` use tree `MoveTypeLayout` for the `layout` field.
Need `AC::MoveTypeLayout::from(...)` wrapping.

### sui-replay (1 error, likely simple)
`transaction_displays.rs` — probably a visit_deserialize or OwnedSlice with tree layout.

### Runtime visitor (NOT STARTED — task #9)
`runtime_visitor.rs` + `runtime_value.rs` need the same migration as annotated:
- Driver fields → compressed views
- `visit_deserialize` → takes view
- Downstream: `runtime_visitor_test.rs`, `values_impl.rs` (move-vm-runtime)

### Tests (NOT YET CHECKED)
- `annotated_visitor_test.rs` — needs `.type_()`, `.name()`, compress-before-visit
- `compressed_layout_test.rs` — likely fine (tests compressed types directly)
- Bounded visitor tests — partially fixed
- Display tests — not fixed
- Dynamic field visitor tests — fixed

## Build Status
```
cargo check -p move-core-types    ✅ (0 errors)
cargo check -p sui-types          ✅ (0 errors)
cargo check -p sui-core           ✅ (0 errors)
cargo check -p sui-display --lib  ✅ (0 errors)
cargo check -p sui-rpc-resolver   ✅ (0 errors)
cargo check -p sui-indexer-alt-graphql  ❌ (22 errors)
cargo check -p sui-replay         ❌ (not checked recently)
tests                              ❌ (not checked yet)
```

## Key Patterns for Remaining Fixes

### Tree layout → visit_deserialize
```rust
// Before:
MoveValue::visit_deserialize(bytes, &tree_layout, &mut visitor)
// After:
let compressed = AC::MoveTypeLayout::from(&tree_layout);
MoveValue::visit_deserialize(bytes, compressed.as_view(), &mut visitor)
```

### Tree layout → OwnedSlice
```rust
// Before:
OwnedSlice { layout: tree_layout, bytes }
// After:
OwnedSlice { layout: AC::MoveTypeLayout::from(&tree_layout), bytes }
```

### Compressed layout → visit_deserialize
```rust
// Before (when we had &AC::MoveTypeLayout):
MoveValue::visit_deserialize(bytes, &compressed, &mut visitor)
// After:
MoveValue::visit_deserialize(bytes, compressed.as_view(), &mut visitor)
```

### Driver accessor changes (all already done in visitors)
- `.struct_layout().type_` → `.struct_layout().type_()`
- `.peek_field().name` → `.peek_field().name()`
- `.peek_field().layout` → `.peek_field().layout()`
- `driver.struct_layout() == &tree_layout` → `*driver.struct_layout().type_() == tree_layout.type_`
