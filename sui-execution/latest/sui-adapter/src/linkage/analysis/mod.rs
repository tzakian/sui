// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    data_store::PackageStore,
    execution_mode::ExecutionMode,
    linkage::analysis::shared::{ConflictResolution, LinkageConfig, ResolutionTable},
};
use move_binary_format::{binary_config::BinaryConfig, file_format::Visibility};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, btree_map::Entry},
    rc::Rc,
};
use sui_protocol_config::ProtocolConfig;
use sui_types::{
    base_types::{ObjectID, SequenceNumber},
    error::{ExecutionError, ExecutionErrorKind},
    execution_config_utils::to_binary_config,
    move_package::MovePackage,
    transaction as P,
    type_input::TypeInput,
};

mod per_command_linkage;
mod per_transaction_linkage;
mod shared;

pub trait LinkageAnalysis {
    fn add_command(
        &self,
        command: &P::Command,
        store: &dyn PackageStore,
    ) -> Result<ResolvedLinkage, ExecutionError>;

    fn resolver(&self) -> &PTBLinkageResolver;
}

pub fn linkage_analysis_for_protocol_config<Mode: ExecutionMode>(
    protocol_config: &ProtocolConfig,
    store: &dyn PackageStore,
) -> Result<Box<dyn LinkageAnalysis>, ExecutionError> {
    Ok(Box::new(per_command_linkage::PerCommandLinkage::new(
        !Mode::packages_are_predefined(),
        to_binary_config(protocol_config),
        store,
    )?))
}

pub type TypeOriginMap = BTreeMap<ObjectID, BTreeMap<(String, String), ObjectID>>;

#[derive(Debug)]
pub struct CachedTypeOriginMap {
    /// Tracker of all packages that we've loaded so far. This is used to determine if the
    /// `TypeOriginMap` needs to be updated when loading a package, or if that package has already
    /// contributed to the `TypeOriginMap`.
    pub cached_type_origins: BTreeSet<ObjectID>,
    /// A mapping of the (original package ID)::<module_name>::<type_name> to the defining ID for
    /// that type.
    pub type_origin_map: TypeOriginMap,
}

impl CachedTypeOriginMap {
    pub fn new() -> Self {
        Self {
            cached_type_origins: BTreeSet::new(),
            type_origin_map: BTreeMap::new(),
        }
    }
}

/// Metadata and shared operations for the PTB linkage analysis.
#[derive(Debug)]
pub struct PTBLinkageResolver {
    /// Config to use for the linkage analysis.
    pub linkage_config: LinkageConfig,
    /// Config to use for the binary analysis (needed for deserialization to determine if a
    /// function is a non-public entry function).
    pub binary_config: BinaryConfig,
    /// A mapping of the (original package ID)::<module_name>::<type_name> to the defining ID for
    /// that type.
    pub type_origin_cache: RefCell<CachedTypeOriginMap>,
}

#[derive(Debug)]
pub struct ResolvedLinkage {
    pub linkage: BTreeMap<ObjectID, ObjectID>,
    // A mapping of every package ID to its runtime ID.
    // Note: Multiple packages can have the same runtime ID in this mapping, and domain of this map
    // is a superset of range of `linkage`.
    pub linkage_resolution: BTreeMap<ObjectID, ObjectID>,
    pub versions: BTreeMap<ObjectID, SequenceNumber>,
}

impl ResolvedLinkage {
    fn from_resolution_table(resolution_table: ResolutionTable) -> Self {
        let mut linkage = BTreeMap::new();
        let mut versions = BTreeMap::new();
        for (runtime_id, resolution) in resolution_table.resolution_table {
            match resolution {
                ConflictResolution::Exact(version, object_id)
                | ConflictResolution::AtLeast(version, object_id) => {
                    linkage.insert(runtime_id, object_id);
                    versions.insert(runtime_id, version);
                }
            }
        }
        Self {
            linkage,
            linkage_resolution: resolution_table.all_versions_resolution_table,
            versions,
        }
    }

    // pub fn linkage_context(&self) -> LinkageContext {
    //     LinkageContext::new(self.linkage.iter().map(|(k, v)| (**k, **v)).collect())
    // }

    pub fn resolve_to_runtime_id(&self, object_id: &ObjectID) -> Option<ObjectID> {
        self.linkage_resolution.get(object_id).copied()
    }

    /// Given a module name and type name, resolve it to the defining package ID.
    /// The `module_address` can be _any_ valid referent of the package in question (i.e., any
    /// valid package ID for the package in question).
    pub fn resolve_type_to_defining_id(
        &self,
        resolver: &PTBLinkageResolver,
        module_address: ObjectID,
        module_name: String,
        type_name: String,
    ) -> Option<ObjectID> {
        let runtime_id = self.resolve_to_runtime_id(&module_address)?;
        resolver
            .type_origin_cache
            .borrow()
            .type_origin_map
            .get(&runtime_id)?
            .get(&(module_name, type_name))
            .copied()
    }
}

impl PTBLinkageResolver {
    pub fn type_linkage(
        &self,
        ids: &[ObjectID],
        store: &dyn PackageStore,
    ) -> Result<ResolvedLinkage, ExecutionError> {
        let mut resolution_table = ResolutionTable::empty();
        for id in ids {
            let type_origin_cache = &mut self.type_origin_cache.borrow_mut();
            let pkg = Self::get_package(type_origin_cache, id, store)?;
            let transitive_deps = pkg
                .linkage_table()
                .values()
                .map(|info| info.upgraded_id)
                .collect::<Vec<_>>();
            let package_id = pkg.id();
            self.add_and_unify(
                &package_id,
                store,
                &mut resolution_table,
                ConflictResolution::at_least,
            )?;
            for object_id in transitive_deps.iter() {
                self.add_and_unify(
                    object_id,
                    store,
                    &mut resolution_table,
                    ConflictResolution::at_least,
                )?;
            }
        }

        Ok(ResolvedLinkage::from_resolution_table(resolution_table))
    }

    // pub fn publication_linkage(
    //     &mut self,
    //     linkage: &LinkageContext,
    //     store: &dyn PackageStore,
    // ) -> Result<ResolvedLinkage, ExecutionError> {
    //     let mut resolution_table = ResolutionTable::empty();
    //     for (runtime_id, package_id) in linkage.linkage_table.iter() {
    //         let package = PTBLinkageResolver::get_package(
    //             &mut self.package_cache,
    //             &mut self.type_origin_cache,
    //             &ObjectID::from(*package_id),
    //             store,
    //         )?;
    //
    //         assert_eq!(*package.id(), *package_id);
    //         assert_eq!(*package.original_package_id(), *runtime_id);
    //
    //         self.add_and_unify(
    //             &ObjectID::from(*runtime_id),
    //             store,
    //             &mut resolution_table,
    //             ConflictResolution::exact,
    //         )?;
    //     }
    //     Ok(ResolvedLinkage::from_resolution_table(resolution_table))
    // }
}

impl PTBLinkageResolver {
    pub fn new(linkage_config: LinkageConfig, binary_config: BinaryConfig) -> Self {
        Self {
            type_origin_cache: RefCell::new(CachedTypeOriginMap {
                cached_type_origins: BTreeSet::new(),
                type_origin_map: BTreeMap::new(),
            }),
            linkage_config,
            binary_config,
        }
    }

    fn add_command(
        &self,
        command: &P::Command,
        store: &dyn PackageStore,
        resolution_table: &mut ResolutionTable,
    ) -> Result<ResolutionTable, ExecutionError> {
        match command {
            P::Command::MoveCall(programmable_move_call) => {
                let type_origin_cache = &mut self.type_origin_cache.borrow_mut();
                let pkg =
                    Self::get_package(type_origin_cache, &programmable_move_call.package, store)?;
                let pkg_id = pkg.id();
                let transitive_deps = pkg
                    .linkage_table()
                    .values()
                    .map(|info| info.upgraded_id)
                    .collect::<Vec<_>>();

                let m = pkg
                    .deserialize_module_by_str(&programmable_move_call.module, &self.binary_config)
                    .map_err(|e| {
                        ExecutionError::new_with_source(
                            ExecutionErrorKind::VMVerificationOrDeserializationError,
                            e,
                        )
                    })?;
                let Some(fdef) = m.function_defs().iter().find(|f| {
                    m.identifier_at(m.function_handle_at(f.function).name)
                        .as_str()
                        == programmable_move_call.function
                }) else {
                    return Err(ExecutionError::new_with_source(
                        ExecutionErrorKind::FunctionNotFound,
                        format!(
                            "Could not resolve function '{}' in module '{}::{}'",
                            programmable_move_call.function,
                            programmable_move_call.package,
                            programmable_move_call.module
                        ),
                    ));
                };

                for ty in &programmable_move_call.type_arguments {
                    self.add_type_input(ty, store, resolution_table)?;
                }

                // Register function entrypoint
                if fdef.is_entry && fdef.visibility != Visibility::Public {
                    self.add_and_unify(
                        &pkg_id,
                        store,
                        resolution_table,
                        ConflictResolution::exact,
                    )?;

                    // transitive closure of entry functions are fixed
                    for object_id in transitive_deps.iter() {
                        self.add_and_unify(
                            object_id,
                            store,
                            resolution_table,
                            self.linkage_config
                                .generate_entry_transitive_dep_constraint(),
                        )?;
                    }
                } else {
                    self.add_and_unify(
                        &pkg_id,
                        store,
                        resolution_table,
                        self.linkage_config.generate_top_level_fn_constraint(),
                    )?;

                    // transitive closure of non-entry functions are at-least
                    for object_id in transitive_deps.iter() {
                        self.add_and_unify(
                            object_id,
                            store,
                            resolution_table,
                            self.linkage_config.generate_transitive_dep_constraint(),
                        )?;
                    }
                }
            }
            P::Command::MakeMoveVec(type_input, _) => {
                if let Some(ty) = type_input {
                    self.add_type_input(ty, store, resolution_table)?;
                }
            }
            P::Command::Upgrade(_, deps, _, _) | P::Command::Publish(_, deps) => {
                let type_origin_cache = &mut self.type_origin_cache.borrow_mut();

                let mut resolution_table = self
                    .linkage_config
                    .resolution_table_with_native_packages(type_origin_cache, store)?;
                for id in deps {
                    let pkg = Self::get_package(type_origin_cache, id, store)?;
                    resolution_table.resolution_table.insert(
                        pkg.original_package_id(),
                        ConflictResolution::Exact(pkg.version(), pkg.id()),
                    );
                    resolution_table
                        .all_versions_resolution_table
                        .insert(pkg.id(), pkg.original_package_id());
                }
                return Ok(resolution_table);
            }
            P::Command::TransferObjects(_, _)
            | P::Command::SplitCoins(_, _)
            | P::Command::MergeCoins(_, _) => (),
        };

        Ok(resolution_table.clone())
    }

    // TODO: Produce loaded `Type` at this point?
    fn add_type_input(
        &self,
        ty: &TypeInput,
        store: &dyn PackageStore,
        unification_table: &mut ResolutionTable,
    ) -> Result<(), ExecutionError> {
        let mut stack = vec![ty];
        while let Some(ty) = stack.pop() {
            match ty {
                TypeInput::Bool
                | TypeInput::U8
                | TypeInput::U64
                | TypeInput::U128
                | TypeInput::Address
                | TypeInput::Signer
                | TypeInput::U16
                | TypeInput::U32
                | TypeInput::U256 => (),
                TypeInput::Vector(type_input) => {
                    stack.push(&**type_input);
                }
                TypeInput::Struct(struct_input) => {
                    let sid = ObjectID::from(struct_input.address);
                    self.add_and_unify(
                        &sid,
                        store,
                        unification_table,
                        self.linkage_config.generate_type_constraint(),
                    )?;
                    let type_origin_cache = &mut self.type_origin_cache.borrow_mut();
                    let pkg = Self::get_package(
                        type_origin_cache,
                        &ObjectID::from(struct_input.address),
                        store,
                    )?;
                    let linkage_table = pkg
                        .linkage_table()
                        .values()
                        .map(|info| info.upgraded_id)
                        .collect::<Vec<_>>();
                    for dep_id in linkage_table {
                        self.add_and_unify(
                            &dep_id,
                            store,
                            unification_table,
                            self.linkage_config.generate_type_constraint(),
                        )?;
                    }
                    for ty in struct_input.type_params.iter() {
                        stack.push(ty);
                    }
                }
            }
        }
        Ok(())
    }

    fn get_package(
        cached_type_origin_map: &mut CachedTypeOriginMap,
        object_id: &ObjectID,
        store: &dyn PackageStore,
    ) -> Result<Rc<MovePackage>, ExecutionError> {
        let package = store
            .get_package(object_id)
            .map_err(|e| {
                ExecutionError::new_with_source(
                    ExecutionErrorKind::PublishUpgradeMissingDependency,
                    e,
                )
            })?
            .ok_or_else(|| ExecutionError::from_kind(ExecutionErrorKind::InvalidLinkage))?;

        if !cached_type_origin_map
            .cached_type_origins
            .contains(object_id)
        {
            cached_type_origin_map
                .cached_type_origins
                .insert(*object_id);
            let original_package_id = package.original_package_id();
            let package_types = cached_type_origin_map
                .type_origin_map
                .entry(original_package_id)
                .or_default();
            for ((module_name, type_name), defining_id) in package.type_origin_map().into_iter() {
                if let Some(other) = package_types.insert(
                    (module_name.to_string(), type_name.to_string()),
                    defining_id,
                ) {
                    assert_eq!(
                        other, defining_id,
                        "type origin map should never have conflicting entries"
                    );
                }
            }
        }

        Ok(package)
    }

    // Add a package to the unification table, unifying it with any existing package in the table.
    // Errors if the packages cannot be unified (e.g., if one is exact and the other is not).
    fn add_and_unify(
        &self,
        object_id: &ObjectID,
        store: &dyn PackageStore,
        resolution_table: &mut ResolutionTable,
        resolution_fn: fn(&MovePackage) -> ConflictResolution,
    ) -> Result<(), ExecutionError> {
        let type_origin_cache = &mut self.type_origin_cache.borrow_mut();
        let package = Self::get_package(type_origin_cache, object_id, store)?;

        let resolution = resolution_fn(&package);
        let original_pkg_id = package.original_package_id();

        if let Entry::Vacant(e) = resolution_table.resolution_table.entry(original_pkg_id) {
            e.insert(resolution);
        } else {
            let existing_unifier = resolution_table
                .resolution_table
                .get_mut(&original_pkg_id)
                .expect("Guaranteed to exist");
            *existing_unifier = existing_unifier.unify(&resolution)?;
        }

        if !resolution_table
            .all_versions_resolution_table
            .contains_key(object_id)
        {
            resolution_table
                .all_versions_resolution_table
                .insert(*object_id, original_pkg_id);
        }

        Ok(())
    }
}
