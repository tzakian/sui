// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    data_store::PackageStore,
    static_programmable_transactions::{
        linkage::resolution::ResolutionTable,
        loading::ast::{LoadedFunction, Type},
    },
};
use indexmap::IndexSet;
use move_core_types::{
    account_address::AccountAddress,
    identifier::IdentStr,
    language_storage::{StructTag, TypeTag},
    resolver::IntraPackageName,
};
use move_vm_runtime::{execution as vm_runtime, runtime::MoveRuntime};
use quick_cache::sync::Cache as QCache;
use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap},
    sync::Arc,
};
use sui_protocol_config::ProtocolConfig;
use sui_types::{
    Identifier,
    base_types::ObjectID,
    error::ExecutionError,
    execution_status::{ExecutionErrorKind, TypeArgumentError},
    type_input::{StructInput, TypeInput},
};

/// Capacity (number of entries) for each LFU cache in `RuntimeCache`. Constant for now;
/// likely to become protocol-config-driven in the future.
const RUNTIME_CACHE_CAPACITY: usize = 1024;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct TypeLinkageCacheKey {
    // NB: We use a BTreeSet here to ensure that the order of the root IDs does not affect the
    // cache key.
    root_ids: BTreeSet<AccountAddress>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct LoadedFunctionKey {
    package: ObjectID,
    module: Identifier,
    function: Identifier,
    type_arguments: Vec<Type>,
}

/// Cross-transaction cache that wraps an `Arc<MoveRuntime>` and holds an LFU table per
/// `PerTxCache_` field. Values are populated only at the end of a transaction (via
/// `PerTxCache::flush_to_runtime_cache`) when the corresponding predicate accepts the entry.
/// During execution, per-tx lookups consult this cache on local miss and promote any hit
/// into the per-tx `HashMap`.
pub struct RuntimeCache {
    pub runtime: Arc<MoveRuntime>,

    pub(crate) type_resolution: QCache<TypeLinkageCacheKey, Arc<ResolutionTable>>,
    pub(crate) type_input_to_type: QCache<TypeInput, Type>,
    pub(crate) type_input_to_tag: QCache<TypeInput, Arc<TypeTag>>,
    pub(crate) tag_to_type: QCache<Arc<TypeTag>, Type>,
    pub(crate) type_to_tag: QCache<Type, Arc<TypeTag>>,
    pub(crate) vm_to_type: QCache<Arc<vm_runtime::Type>, Type>,
    pub(crate) type_to_vm: QCache<Type, Arc<vm_runtime::Type>>,
    pub(crate) function_cache: QCache<LoadedFunctionKey, Arc<LoadedFunction>>,
    pub(crate) defining_id_map: QCache<(ObjectID, Identifier, Identifier), ObjectID>,
}

impl RuntimeCache {
    pub fn new(runtime: Arc<MoveRuntime>) -> Self {
        Self {
            runtime,
            type_resolution: QCache::new(RUNTIME_CACHE_CAPACITY),
            type_input_to_type: QCache::new(RUNTIME_CACHE_CAPACITY),
            type_input_to_tag: QCache::new(RUNTIME_CACHE_CAPACITY),
            tag_to_type: QCache::new(RUNTIME_CACHE_CAPACITY),
            type_to_tag: QCache::new(RUNTIME_CACHE_CAPACITY),
            vm_to_type: QCache::new(RUNTIME_CACHE_CAPACITY),
            type_to_vm: QCache::new(RUNTIME_CACHE_CAPACITY),
            function_cache: QCache::new(RUNTIME_CACHE_CAPACITY),
            defining_id_map: QCache::new(RUNTIME_CACHE_CAPACITY),
        }
    }

    // Predicates: decide whether a per-tx entry should be persisted into the cross-tx LFU
    // at end of transaction. For now they all return `true`; in the future these can use
    // entry-specific signals (e.g. observed reuse, package version, etc.).

    fn keep_type_resolution(&self, _k: &TypeLinkageCacheKey, _v: &Arc<ResolutionTable>) -> bool {
        true
    }
    fn keep_type_input_to_type(&self, _k: &TypeInput, _v: &Type) -> bool {
        true
    }
    fn keep_type_input_to_tag(&self, _k: &TypeInput, _v: &Arc<TypeTag>) -> bool {
        true
    }
    fn keep_tag_type_pair(&self, _tag: &Arc<TypeTag>, _ty: &Type) -> bool {
        true
    }
    fn keep_vm_type_pair(&self, _vm_ty: &Arc<vm_runtime::Type>, _ty: &Type) -> bool {
        true
    }
    fn keep_function(&self, _k: &LoadedFunctionKey, _v: &Arc<LoadedFunction>) -> bool {
        true
    }
    fn keep_defining_id(&self, _k: &(ObjectID, Identifier, Identifier), _v: &ObjectID) -> bool {
        true
    }
}

pub struct PerTxCache<'pc> {
    protocol_config: &'pc ProtocolConfig,
    /// Optional cross-tx LFU cache. When present, lookups fall through on local miss and
    /// `flush_to_runtime_cache` writes accepted entries back at end of transaction.
    runtime_cache: Option<&'pc RuntimeCache>,
    inner: RefCell<PerTxCache_>,
}

/// Per-transaction memoization tables shared across the PTB pipeline.
///
/// The tables fall into four groups:
///
/// 1. **Linkage resolution** (`type_resolution_cache`): computing the transitive-dependency
///    `ResolutionTable` for a set of root addresses touches every transitive dependency of
///    those packages; the same set of roots may be queried many times within a single PTB
///    (once per object input, once per type argument on a call, once per `get_type_linkage`,
///    etc.). Caching the `ResolutionTable` (rather than the derived `ExecutableLinkage`) lets
///    input-resolution analysis fold cached minis into the PTB-wide table without re-walking,
///    and lets `Env::get_type_linkage` derive its `ExecutableLinkage` from the same source.
///
/// 2. **TypeInput resolution** (`type_input_to_type`): user-supplied `TypeInput`s are resolved to
///    adapter `Type`s via a tag lookup + VM load. Kept one-way because multiple `TypeInput`s can
///    resolve to the same `Type` so a reverse map would be ambiguous.
///
/// 3. **Bijective type conversions** (`tag_to_type` / `type_to_tag`, `vm_to_type` / `type_to_vm`):
///    adapter `Type`, `TypeTag`, and `vm_runtime::Type` all carry defining IDs, so the mappings
///    between them are one-to-one for fully-resolved types. Each bijection is kept as two maps
///    sharing `Arc`-owned value sides, and populated atomically through a single helper
///    (`insert_tag_type_pair`, `insert_vm_type_pair`) so the two directions cannot drift.
///
/// 4. **Loaded function resolution** (`function_cache`): resolving a Move call to a
///    `LoadedFunction` involves call-linkage computation, VM instantiation against that linkage,
///    function-def lookup, and type-parameter substitution. The full result is a pure function
///    of `(version-specific package, module, function, type arguments)`, so we cache the
///    `Arc<LoadedFunction>` keyed on that tuple.
///
/// All fields are `HashMap`-backed: the keys are either structural type trees or defining-ID
/// tuples, so hashing dominates over ordered iteration, and `vm_runtime::Type` in particular
/// only implements `Hash + Eq` (not `Ord`).
struct PerTxCache_ {
    /// Per-tag mini `ResolutionTable`s keyed by the root-address set walked. Used both to
    /// fold into the PTB-wide table during input-resolution analysis and to derive the
    /// `ExecutableLinkage` that `Env::get_type_linkage` returns on the error path.
    type_resolution_cache: HashMap<TypeLinkageCacheKey, Arc<ResolutionTable>>,

    /// TypeInput -> adapter Type. One-way only.
    type_input_to_type: HashMap<TypeInput, Type>,

    /// TypeInput -> defining-ID `TypeTag`. The rewrite walks each `TypeInput::Struct`,
    /// resolves its `address` through the package's `type_origin_table` to the defining ID
    /// for the named type, and recurses into type params. This is keyed on the raw
    /// user-supplied `TypeInput` because the same input is queried both during input
    /// resolution analysis (for linkage pre-warming) and during loading.
    type_input_to_tag: HashMap<TypeInput, Arc<TypeTag>>,

    /// Bijective Type <-> TypeTag. Populated atomically via `insert_tag_type_pair`.
    tag_to_type: HashMap<Arc<TypeTag>, Type>,
    type_to_tag: HashMap<Type, Arc<TypeTag>>,

    /// Bijective Type <-> vm_runtime::Type. Only fully-resolved types (no `TyParam`). Populated
    /// atomically via `insert_vm_type_pair`.
    vm_to_type: HashMap<Arc<vm_runtime::Type>, Type>,
    type_to_vm: HashMap<Type, Arc<vm_runtime::Type>>,

    /// `(package version-id, module, function, type arguments)` -> `Arc<LoadedFunction>`. Keyed on
    /// the version-specific package ID because private/entry functions can disappear across
    /// package versions, so different versions resolve differently.
    function_cache: HashMap<LoadedFunctionKey, Arc<LoadedFunction>>,

    defining_id_map: HashMap<(ObjectID, Identifier, Identifier), ObjectID>,
}

/// Early-returns `Ok($empty)` from the enclosing method when PTB caching is disabled. The second
/// argument is the stand-in result for that short-circuit: `None` for lookups, `()` for inserts,
/// or a freshly-allocated `(Arc, value)` pair for the paired inserts that must still hand a valid
/// return back to the caller.
macro_rules! gated {
    ($config:expr, $empty:expr) => {
        if !$config.enable_ptb_tx_cache() {
            return Ok($empty);
        }
    };
}

impl<'pc> PerTxCache<'pc> {
    pub(crate) fn new(
        protocol_config: &'pc ProtocolConfig,
        runtime_cache: Option<&'pc RuntimeCache>,
    ) -> Self {
        Self {
            protocol_config,
            runtime_cache,
            inner: RefCell::new(PerTxCache_ {
                type_resolution_cache: HashMap::new(),
                type_input_to_type: HashMap::new(),
                type_input_to_tag: HashMap::new(),
                tag_to_type: HashMap::new(),
                type_to_tag: HashMap::new(),
                vm_to_type: HashMap::new(),
                type_to_vm: HashMap::new(),
                function_cache: HashMap::new(),
                defining_id_map: HashMap::new(),
            }),
        }
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.protocol_config.enable_ptb_tx_cache()
    }

    fn borrow(&self) -> Result<std::cell::Ref<'_, PerTxCache_>, ExecutionError> {
        self.inner.try_borrow().map_err(|_| {
            make_invariant_violation!(
                "Should be able to borrow PerTxCache for access here as we are only accessing it"
            )
        })
    }

    fn borrow_mut(&self) -> Result<std::cell::RefMut<'_, PerTxCache_>, ExecutionError> {
        self.inner.try_borrow_mut().map_err(|_| {
            make_invariant_violation!(
                "Should be able to borrow PerTxCache for mutation here as we are only mutating it"
            )
        })
    }

    /// Look up the cached mini `ResolutionTable` for `key`, or compute and cache it via
    /// `compute` on miss. When caching is disabled, always invokes `compute` and returns its
    /// result wrapped in a fresh `Arc` without touching the table. On local miss, falls
    /// through to the cross-tx `RuntimeCache` and promotes a hit into the local table.
    pub(crate) fn get_or_compute_type_resolution(
        &self,
        key: TypeLinkageCacheKey,
        compute: impl FnOnce() -> Result<ResolutionTable, ExecutionError>,
    ) -> Result<Arc<ResolutionTable>, ExecutionError> {
        if !self.protocol_config.enable_ptb_tx_cache() {
            return Ok(Arc::new(compute()?));
        }
        if let Some(cached) = self.borrow()?.type_resolution_cache.get(&key).cloned() {
            return Ok(cached);
        }
        if let Some(rc) = self.runtime_cache
            && let Some(cached) = rc.type_resolution.get(&key)
        {
            self.borrow_mut()?
                .type_resolution_cache
                .insert(key, cached.clone());
            return Ok(cached);
        }
        let table = Arc::new(compute()?);
        self.borrow_mut()?
            .type_resolution_cache
            .insert(key, table.clone());
        Ok(table)
    }

    /// Resolve a `TypeInput` into a `TypeTag` whose addresses are defining IDs (not the raw
    /// user-supplied addresses), consulting (and populating) the per-tx cache. The recursion
    /// also memoizes nested type-input fragments.
    pub(crate) fn type_input_to_defining_tag(
        &self,
        ty: &TypeInput,
        type_arg_idx: usize,
        package_store: &dyn PackageStore,
    ) -> Result<Arc<TypeTag>, ExecutionError> {
        if self.protocol_config.enable_ptb_tx_cache() {
            if let Some(cached) = self.borrow()?.type_input_to_tag.get(ty).cloned() {
                return Ok(cached);
            }
            if let Some(rc) = self.runtime_cache
                && let Some(cached) = rc.type_input_to_tag.get(ty)
            {
                self.borrow_mut()?
                    .type_input_to_tag
                    .insert(ty.clone(), cached.clone());
                return Ok(cached);
            }
        }

        let tag = match ty {
            TypeInput::Bool => TypeTag::Bool,
            TypeInput::U8 => TypeTag::U8,
            TypeInput::U16 => TypeTag::U16,
            TypeInput::U32 => TypeTag::U32,
            TypeInput::U64 => TypeTag::U64,
            TypeInput::U128 => TypeTag::U128,
            TypeInput::U256 => TypeTag::U256,
            TypeInput::Address => TypeTag::Address,
            TypeInput::Signer => TypeTag::Signer,
            TypeInput::Vector(inner) => {
                let inner_tag = self.type_input_to_defining_tag(inner, type_arg_idx, package_store)?;
                TypeTag::Vector(Box::new((*inner_tag).clone()))
            }
            TypeInput::Struct(struct_input) => {
                let StructInput {
                    address,
                    module,
                    name,
                    type_params,
                } = &**struct_input;

                let pkg = package_store
                    .get_package(&ObjectID::from(*address))
                    .ok()
                    .flatten()
                    .ok_or_else(|| {
                        let argument_idx = match checked_as!(type_arg_idx, u16) {
                            Err(e) => return e,
                            Ok(v) => v,
                        };
                        ExecutionError::from_kind(ExecutionErrorKind::TypeArgumentError {
                            argument_idx,
                            kind: TypeArgumentError::TypeNotFound,
                        })
                    })?;
                let module = super::to_identifier(module.clone())?;
                let name = super::to_identifier(name.clone())?;
                let tid = IntraPackageName {
                    module_name: module,
                    type_name: name,
                };
                let Some(resolved_address) = pkg.type_origin_table().get(&tid).cloned() else {
                    return Err(ExecutionError::from_kind(
                        ExecutionErrorKind::TypeArgumentError {
                            argument_idx: checked_as!(type_arg_idx, u16)?,
                            kind: TypeArgumentError::TypeNotFound,
                        },
                    ));
                };

                let type_params = type_params
                    .iter()
                    .map(|tp| {
                        self.type_input_to_defining_tag(tp, type_arg_idx, package_store)
                            .map(|rc| (*rc).clone())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                TypeTag::Struct(Box::new(StructTag {
                    address: resolved_address,
                    module: tid.module_name,
                    name: tid.type_name,
                    type_params,
                }))
            }
        };

        let tag = Arc::new(tag);
        gated!(self.protocol_config, tag);
        self.borrow_mut()?
            .type_input_to_tag
            .insert(ty.clone(), tag.clone());
        Ok(tag)
    }

    pub(super) fn lookup_type_input(
        &self,
        input: &TypeInput,
    ) -> Result<Option<Type>, ExecutionError> {
        gated!(self.protocol_config, None);
        if let Some(cached) = self.borrow()?.type_input_to_type.get(input).cloned() {
            return Ok(Some(cached));
        }
        if let Some(rc) = self.runtime_cache
            && let Some(cached) = rc.type_input_to_type.get(input)
        {
            self.borrow_mut()?
                .type_input_to_type
                .insert(input.clone(), cached.clone());
            return Ok(Some(cached));
        }
        Ok(None)
    }

    pub(super) fn insert_type_input(
        &self,
        input: TypeInput,
        ty: Type,
    ) -> Result<(), ExecutionError> {
        gated!(self.protocol_config, ());
        assert_invariant!(
            !matches!(ty, Type::Reference(_, _)),
            "TypeInput cannot represent a reference type: {:?}",
            ty
        );

        let previous = self.borrow_mut()?.type_input_to_type.insert(input, ty);
        assert_invariant!(
            previous.is_none(),
            "duplicate insert into type_input_to_type"
        );

        Ok(())
    }

    pub(super) fn lookup_type_by_tag(&self, tag: &TypeTag) -> Result<Option<Type>, ExecutionError> {
        gated!(self.protocol_config, None);
        if let Some(cached) = self.borrow()?.tag_to_type.get(tag).cloned() {
            return Ok(Some(cached));
        }
        if let Some(rc) = self.runtime_cache
            && let Some(cached) = rc.tag_to_type.get(tag)
        {
            self.borrow_mut()?
                .tag_to_type
                .insert(Arc::new(tag.clone()), cached.clone());
            return Ok(Some(cached));
        }
        Ok(None)
    }

    pub(super) fn lookup_tag(&self, ty: &Type) -> Result<Option<Arc<TypeTag>>, ExecutionError> {
        gated!(self.protocol_config, None);
        if let Some(cached) = self.borrow()?.type_to_tag.get(ty).cloned() {
            return Ok(Some(cached));
        }
        if let Some(rc) = self.runtime_cache
            && let Some(cached) = rc.type_to_tag.get(ty)
        {
            self.borrow_mut()?
                .type_to_tag
                .insert(ty.clone(), cached.clone());
            return Ok(Some(cached));
        }
        Ok(None)
    }

    pub(super) fn insert_tag_type_pair(
        &self,
        tag: TypeTag,
        ty: Type,
    ) -> Result<(Arc<TypeTag>, Type), ExecutionError> {
        let tag = Arc::new(tag);

        gated!(self.protocol_config, (tag, ty));
        assert_invariant!(
            !matches!(ty, Type::Reference(_, _)),
            "TypeTag cannot represent a reference type: {:?}",
            ty
        );

        let mut c = self.borrow_mut()?;

        let previous_tag = c.tag_to_type.insert(tag.clone(), ty.clone());
        assert_invariant!(previous_tag.is_none(), "duplicate insert into tag_to_type");

        let previous_type = c.type_to_tag.insert(ty.clone(), tag.clone());
        assert_invariant!(previous_type.is_none(), "duplicate insert into type_to_tag");

        Ok((tag, ty))
    }

    pub(super) fn lookup_type_by_vm_type(
        &self,
        vm_type: &vm_runtime::Type,
    ) -> Result<Option<Type>, ExecutionError> {
        gated!(self.protocol_config, None);
        if let Some(cached) = self.borrow()?.vm_to_type.get(vm_type).cloned() {
            return Ok(Some(cached));
        }
        if let Some(rc) = self.runtime_cache
            && let Some(cached) = rc.vm_to_type.get(vm_type)
        {
            self.borrow_mut()?
                .vm_to_type
                .insert(Arc::new(vm_type.clone()), cached.clone());
            return Ok(Some(cached));
        }
        Ok(None)
    }

    pub(super) fn lookup_vm_type(
        &self,
        ty: &Type,
    ) -> Result<Option<Arc<vm_runtime::Type>>, ExecutionError> {
        gated!(self.protocol_config, None);
        if let Some(cached) = self.borrow()?.type_to_vm.get(ty).cloned() {
            return Ok(Some(cached));
        }
        if let Some(rc) = self.runtime_cache
            && let Some(cached) = rc.type_to_vm.get(ty)
        {
            self.borrow_mut()?
                .type_to_vm
                .insert(ty.clone(), cached.clone());
            return Ok(Some(cached));
        }
        Ok(None)
    }

    pub(super) fn insert_vm_type_pair(
        &self,
        vm_type: vm_runtime::Type,
        ty: Type,
    ) -> Result<(Arc<vm_runtime::Type>, Type), ExecutionError> {
        let vm_type = Arc::new(vm_type);

        gated!(self.protocol_config, (vm_type, ty));
        assert_invariant!(
            !matches!(vm_type.as_ref(), vm_runtime::Type::TyParam(_)),
            "cannot cache unresolved TyParam: {:?}",
            vm_type
        );

        let mut c = self.borrow_mut()?;

        let previous_vm_type = c.vm_to_type.insert(vm_type.clone(), ty.clone());
        assert_invariant!(
            previous_vm_type.is_none(),
            "duplicate insert into vm_to_type"
        );

        let previous_type = c.type_to_vm.insert(ty.clone(), vm_type.clone());
        assert_invariant!(previous_type.is_none(), "duplicate insert into type_to_vm");

        Ok((vm_type, ty))
    }

    pub(super) fn lookup_function(
        &self,
        key: &LoadedFunctionKey,
    ) -> Result<Option<Arc<LoadedFunction>>, ExecutionError> {
        gated!(self.protocol_config, None);
        if let Some(cached) = self.borrow()?.function_cache.get(key).cloned() {
            return Ok(Some(cached));
        }
        if let Some(rc) = self.runtime_cache
            && let Some(cached) = rc.function_cache.get(key)
        {
            self.borrow_mut()?
                .function_cache
                .insert(key.clone(), cached.clone());
            return Ok(Some(cached));
        }
        Ok(None)
    }

    pub(super) fn insert_function(
        &self,
        key: LoadedFunctionKey,
        function: Arc<LoadedFunction>,
    ) -> Result<(), ExecutionError> {
        gated!(self.protocol_config, ());
        let previous_function = self.borrow_mut()?.function_cache.insert(key, function);
        assert_invariant!(
            previous_function.is_none(),
            "duplicate insert into function_cache"
        );
        Ok(())
    }

    pub(super) fn insert_type_to_defining_id(
        &self,
        package: ObjectID,
        module: &IdentStr,
        name: &IdentStr,
        defining_id: ObjectID,
    ) -> Result<(), ExecutionError> {
        gated!(self.protocol_config, ());
        let previous = self
            .borrow_mut()?
            .defining_id_map
            .insert((package, module.to_owned(), name.to_owned()), defining_id);
        assert_invariant!(
            previous.is_none(),
            "duplicate insert into defining_id_map: ({}::{}::{})",
            package,
            module,
            name
        );
        Ok(())
    }

    pub(super) fn lookup_type_to_defining_id(
        &self,
        package: ObjectID,
        module: &IdentStr,
        name: &IdentStr,
    ) -> Result<Option<ObjectID>, ExecutionError> {
        gated!(self.protocol_config, None);
        let key = (package, module.to_owned(), name.to_owned());
        if let Some(cached) = self.borrow()?.defining_id_map.get(&key).cloned() {
            return Ok(Some(cached));
        }
        if let Some(rc) = self.runtime_cache
            && let Some(cached) = rc.defining_id_map.get(&key)
        {
            self.borrow_mut()?.defining_id_map.insert(key, cached);
            return Ok(Some(cached));
        }
        Ok(None)
    }

    /// Persist accepted per-tx cache entries into the cross-tx LFU cache. Called once at end
    /// of transaction. Each cache type is gated by its own predicate on `RuntimeCache`; for
    /// now all predicates return `true`. No-op when caching is disabled or no runtime cache
    /// is attached.
    pub(crate) fn flush_to_runtime_cache(&self) -> Result<(), ExecutionError> {
        if !self.protocol_config.enable_ptb_tx_cache() {
            return Ok(());
        }
        let Some(rc) = self.runtime_cache else {
            return Ok(());
        };
        let inner = self.borrow()?;

        for (k, v) in &inner.type_resolution_cache {
            if rc.keep_type_resolution(k, v) {
                rc.type_resolution.insert(k.clone(), v.clone());
            }
        }
        for (k, v) in &inner.type_input_to_type {
            if rc.keep_type_input_to_type(k, v) {
                rc.type_input_to_type.insert(k.clone(), v.clone());
            }
        }
        for (k, v) in &inner.type_input_to_tag {
            if rc.keep_type_input_to_tag(k, v) {
                rc.type_input_to_tag.insert(k.clone(), v.clone());
            }
        }
        for (tag, ty) in &inner.tag_to_type {
            if rc.keep_tag_type_pair(tag, ty) {
                rc.tag_to_type.insert(tag.clone(), ty.clone());
            }
        }
        for (ty, tag) in &inner.type_to_tag {
            if rc.keep_tag_type_pair(tag, ty) {
                rc.type_to_tag.insert(ty.clone(), tag.clone());
            }
        }
        for (vm_ty, ty) in &inner.vm_to_type {
            if rc.keep_vm_type_pair(vm_ty, ty) {
                rc.vm_to_type.insert(vm_ty.clone(), ty.clone());
            }
        }
        for (ty, vm_ty) in &inner.type_to_vm {
            if rc.keep_vm_type_pair(vm_ty, ty) {
                rc.type_to_vm.insert(ty.clone(), vm_ty.clone());
            }
        }
        for (k, v) in &inner.function_cache {
            if rc.keep_function(k, v) {
                rc.function_cache.insert(k.clone(), v.clone());
            }
        }
        for (k, v) in &inner.defining_id_map {
            if rc.keep_defining_id(k, v) {
                rc.defining_id_map.insert(k.clone(), *v);
            }
        }
        Ok(())
    }
}

impl TypeLinkageCacheKey {
    pub(crate) fn new(root_ids: &IndexSet<AccountAddress>) -> Self {
        Self {
            root_ids: root_ids.iter().copied().collect(),
        }
    }
}

impl LoadedFunctionKey {
    pub(super) fn new(
        package: ObjectID,
        module: Identifier,
        function: Identifier,
        type_arguments: Vec<Type>,
    ) -> Self {
        Self {
            package,
            module,
            function,
            type_arguments,
        }
    }
}
