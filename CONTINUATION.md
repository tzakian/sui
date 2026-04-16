# Continuation: Migrate type layout resolution from Executor to sui-package-resolver

## What We're Doing

Migrating all type layout resolution from the `Executor` trait's `type_layout_resolver` method
(which creates a VM-backed `TypeLayoutResolver`) to `sui-package-resolver::Resolver` (which resolves
types from package bytecode without the VM). This decouples layout resolution from the execution layer.

## Current State

**Branch:** detached HEAD, 76 files changed, does NOT compile (~20 errors remaining).
**Plan file:** `~/.claude/plans/snoopy-wondering-dawn.md` (outdated — approach evolved significantly)

### What Compiles

The individual adapter packages (`sui-adapter-{latest,v3,v2,v1,v0}`), `sui-execution`,
`sui-package-resolver`, `sui-json-rpc-types` all compile. The errors are in `sui-core`
(specifically `authority.rs` and `execution_cache.rs`) from the async cascade not being
fully propagated.

### Key Architectural Decisions Made

1. **`LayoutResolver` trait is now async** (`#[async_trait(?Send)]` in `sui-types/src/layout_resolver.rs`).
   The `?Send` was chosen because the executor-based `TypeLayoutResolver` types hold non-Send VM references.

2. **`Resolver<S: PackageStore>` implements `LayoutResolver` directly** (in `sui-package-resolver/src/lib.rs`).
   No wrapper struct needed. Call sites construct a `Resolver` and pass `&mut resolver` wherever
   `&mut dyn LayoutResolver` is expected.

3. **`'static` removed from `PackageStore` trait**. This allows passing references
   (`&InnerTemporaryStore`, `&dyn BackingPackageStore`, etc.) into `BackingPackageStoreAdapter`
   without cloning or Arc-ing. The `Arc<dyn PackageStore>` usages in indexer/GraphQL crates still
   work because `Arc<dyn Trait>` defaults to `'static`.

4. **`Object::get_total_sui` is async**. This cascades through `temporary_store.rs` (5 versions),
   `execution_engine.rs` (5 versions), the `Executor` trait, and all callers.

5. **`SuiTransactionBlockEvents::try_from` and `DevInspectResults::new` are async** with
   `&mut dyn LayoutResolver`. The `_with_resolver` variant methods were added and then removed
   once we realized `Resolver` implements `LayoutResolver` directly.

6. **`RpcStateReader` trait layout methods are async** (`get_type_layout`, `get_struct_layout`,
   `get_struct_layout_with_overlay`). All callers in `sui-rpc-api` have been updated.

7. **Executor trait methods are async** (`execute_transaction_to_effects`,
   `execute_transaction_to_effects_and_execution_error`, `dev_inspect_transaction`).
   Uses `#[async_trait(?Send)]`.

### Infrastructure Created

**`crates/sui-package-resolver/src/backing_store.rs`:**
- `BackingPackageStoreAdapter<S>` — wraps sync `BackingPackageStore` as async `PackageStore`
- `FallbackPackageStore<P, F>` — checks primary then fallback (replaces `PackageStoreWithFallback`)

**`crates/sui-package-resolver/src/lib.rs`:**
- `Resolver::datatype_layout(&self, &StructTag)` — convenience method
- `impl LayoutResolver for Resolver<S: PackageStore>` — delegates to `datatype_layout`
- `MoveDatatypeLayout` import added

**`crates/sui-types/src/full_checkpoint_content.rs`:**
- `impl BackingPackageStore for ObjectSet` — allows using `ObjectSet` as a package overlay

**`crates/sui-types/src/storage/mod.rs`:**
- `PostExecutionPackageResolver` changed to use `Arc<dyn BackingPackageStore + Send + Sync>`

### Call Sites Migrated

These call sites now use `Resolver` + `BackingPackageStoreAdapter` instead of
`executor.type_layout_resolver(...)`:

- `authority.rs`: `dev_inspect_transaction_block`, `dry_exec_transaction_impl`,
  `handle_transaction_info_request`, `query_events`, `process_object_index`,
  `make_transaction_block_events`, `create_owner_index_if_empty`, `get_object_layout`
- `read_api.rs`: `get_events` closure, `to_sui_transaction_events`
- `transaction_execution_api.rs`: `handle_post_orchestration`
- `checkpoint_executor/mod.rs`: `process_checkpoint_data`
- `storage.rs`: `get_struct_layout_with_overlay`
- `rpc_index.rs`: `index_checkpoint` (signature changed to `&Resolver<impl PackageStore>`)
- `sui-replay/src/displays/transaction_displays.rs`
- `transaction-fuzzer/src/account_universe.rs`
- `sui-e2e-tests/tests/dynamic_committee_tests.rs` (2 sites)

### Call Sites NOT Yet Migrated

- `authority_store.rs:1221, 1268` — `expensive_check_sui_conservation` (uses `thread::scope`,
  needs conversion to tokio tasks)

### What Remains To Fix (compilation errors)

All remaining errors are async cascade in `crates/sui-core/src/authority.rs` and
`crates/sui-core/src/execution_cache.rs`:

1. **`authority.rs:3488` — Send issue.** `tokio::spawn(async move { ... })` captures
   `&mut dyn LayoutResolver` which is `?Send`. The `post_process_one_tx` function spawns a tokio
   task. Since `LayoutResolver` uses `#[async_trait(?Send)]`, the future isn't Send.
   **Fix options:**
   - Change `LayoutResolver` to `#[async_trait]` (Send) — but executor `TypeLayoutResolver` types
     may not be Send (they hold VM references with lifetimes)
   - Use `tokio::task::spawn_local` instead of `tokio::spawn`
   - Use `block_in_place` to bridge back to sync for this specific call site
   - Restructure to not hold the resolver across the spawn boundary

2. **`execution_cache.rs` — `ExecutionCacheReconfigAPI` trait.** The
   `expensive_check_sui_conservation` method was made async, but the trait uses native async
   (edition 2024). The `dyn ExecutionCacheReconfigAPI` usages may not work with async methods
   (object safety). Need to check if the trait is used as `dyn` — if so, might need `#[async_trait]`.

3. **Various authority.rs cascade points** (~15 locations) where async hasn't propagated through
   `check_system_consistency`, `reconfigure`, and related functions.

### Key Learnings / Gotchas

- **Don't delegate mechanical edits to sub-agents** — they take 5-10 minutes for what are
  simple find-and-replace operations. Do batch edits directly.

- **The `?Send` vs `Send` tension on `LayoutResolver`:** The executor's `TypeLayoutResolver`
  types hold borrowed VM references that aren't Send. But `tokio::spawn` requires Send futures.
  This creates a tension. The `post_process_one_tx` function previously used `spawn_blocking`
  (which runs sync code) — now it uses `tokio::spawn` (which requires Send). May need to
  restructure this specific site.

- **`'static` on `PackageStore` was the main blocker** for passing references. Removing it
  unlocked clean reference-based usage everywhere without cloning or Arc-ing.

- **Circular dependency prevents `sui-types` from using `Resolver`/`PackageStore` directly.**
  That's why `LayoutResolver` (in sui-types) is the abstraction, and `Resolver` implements it
  in sui-package-resolver.

- **`ObjectSet` and `CheckpointData` can be used as `BackingPackageStore` overlays** by
  implementing the trait and wrapping in `BackingPackageStoreAdapter`. `ObjectSet` got a new
  `BackingPackageStore` impl.

- **`PostExecutionPackageResolver` needed `Send + Sync`** — the inner `Arc<dyn BackingPackageStore>`
  was changed to `Arc<dyn BackingPackageStore + Send + Sync>`.

- **When moving ownership of `InnerTemporaryStore` into the resolver**, clone fields you still
  need (like `events`) before the move. Or pass a reference instead (now that `'static` is removed).

- **`rpc_index.rs::index_checkpoint`** takes a resolver parameter that is actually unused
  (`_resolver`). Changed signature for forward compatibility but the resolver does nothing there.

### Files Changed (76 total)

Key files:
```
crates/sui-package-resolver/src/lib.rs              # LayoutResolver impl, datatype_layout, 'static removal
crates/sui-package-resolver/src/backing_store.rs    # BackingPackageStoreAdapter, FallbackPackageStore
crates/sui-types/src/layout_resolver.rs             # async trait
crates/sui-types/src/object.rs                      # async get_total_sui
crates/sui-types/src/storage/mod.rs                 # PostExecutionPackageResolver Send+Sync
crates/sui-types/src/storage/read_store.rs          # async RpcStateReader methods
crates/sui-types/src/full_checkpoint_content.rs     # BackingPackageStore for ObjectSet
crates/sui-core/src/authority.rs                    # Main call site migrations
crates/sui-core/src/storage.rs                      # get_struct_layout_with_overlay migration
crates/sui-core/src/rpc_index.rs                    # Signature change to Resolver
crates/sui-core/src/execution_cache.rs              # async expensive_check_sui_conservation
crates/sui-core/src/checkpoints/checkpoint_executor/mod.rs  # process_checkpoint_data migration
crates/sui-json-rpc-types/src/sui_transaction.rs    # async try_from, async new
crates/sui-json-rpc/src/authority_state.rs          # async StateRead methods
crates/sui-json-rpc/src/read_api.rs                 # Call site migrations
crates/sui-json-rpc/src/transaction_execution_api.rs # Call site migration
sui-execution/src/executor.rs                       # async Executor trait
sui-execution/src/{latest,v0,v1,v2,v3}.rs          # async impls
sui-execution/{latest,v0,v1,v2,v3}/sui-adapter/src/type_layout_resolver.rs  # async
sui-execution/{latest,v0,v1,v2,v3}/sui-adapter/src/temporary_store.rs      # async cascade
sui-execution/{latest,v0,v1,v2,v3}/sui-adapter/src/execution_engine.rs     # async cascade
crates/sui-rpc-api/src/grpc/v2/...                  # Various async cascades
crates/sui-replay/src/displays/transaction_displays.rs  # Migration
crates/sui-replay-2/src/execution.rs                # async cascade
crates/transaction-fuzzer/src/account_universe.rs   # Migration
crates/sui-e2e-tests/tests/dynamic_committee_tests.rs  # Migration
```

### Next Steps (in order)

1. **Fix the ~20 remaining compilation errors** — mostly async cascade through `authority.rs`
   functions like `check_system_consistency`, `reconfigure`, etc. These are mechanical `.await`
   additions and `fn` → `async fn` changes.

2. **Resolve the Send issue at `authority.rs:3488`** — the `tokio::spawn` in `post_process_one_tx`
   captures a `?Send` future. Decide on approach (see options above).

3. **Resolve `execution_cache.rs` object safety** — if `ExecutionCacheReconfigAPI` is used as
   `dyn`, async methods won't work. May need `#[async_trait]` or a different approach.

4. **Consider reverting `rpc_index.rs` signature change** — now that `Resolver` implements
   `LayoutResolver`, `index_checkpoint` could go back to `&mut dyn LayoutResolver` instead of
   `&Resolver<impl PackageStore>`. Same for `try_create_dynamic_field_info` if its signature
   was changed.

5. **Run tests** — once compilation is clean:
   ```
   SUI_SKIP_SIMTESTS=1 cargo nextest run -p sui-core -p sui-json-rpc -p sui-package-resolver
   cargo simtest -p sui-e2e-tests
   cargo xclippy
   ```

6. **Phase 4 cleanup** — remove `Executor::type_layout_resolver` method, delete `TypeLayoutStore`,
   remove old overlay stores, consider removing `LayoutResolver` trait entirely in favor of
   calling `Resolver` methods directly.
