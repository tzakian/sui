use std::collections::BTreeMap;

// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    data_store::PackageStore,
    linkage::analysis::{CachedTypeOriginMap, PTBLinkageResolver},
};
use sui_types::{
    MOVE_STDLIB_PACKAGE_ID, SUI_FRAMEWORK_PACKAGE_ID, SUI_SYSTEM_PACKAGE_ID,
    base_types::{ObjectID, SequenceNumber},
    error::{ExecutionError, ExecutionErrorKind},
    move_package::MovePackage,
};

/// These are the set of native packages in Sui -- importantly they can be used implicitly by
/// different parts of the system and are not required to be explicitly imported always.
/// Additionally, there is no versioning concerns around these as they are "stable" for a given
/// epoch, and are the special packages that are always available, and updated in-place.
const NATIVE_PACKAGE_IDS: &[ObjectID] = &[
    SUI_FRAMEWORK_PACKAGE_ID,
    SUI_SYSTEM_PACKAGE_ID,
    MOVE_STDLIB_PACKAGE_ID,
];

/// Configuration for the linkage analysis.
#[derive(Debug)]
pub struct LinkageConfig {
    /// Do we introduce an `Exact(<pkg_id>)` for each top-level function <pkg_id>::mname::fname?
    pub fix_top_level_functions: bool,
    /// Do we introduce an `Exact(<pkg_id>)` for each type <pkg_id>::mname::tname?
    pub fix_types: bool,
    /// Do we introduce an `Exact(<pkg_id>)` for each transitive dependency of a non-public entry function?
    pub exact_entry_transitive_deps: bool,
    /// Do we introduce an `Exact(<pkg>)` for each transitive dependency of a
    /// function?
    pub exact_transitive_deps: bool,
    /// Whether system packages should always be included as a member in the generated linkage.
    /// This is almost always true except for system transactions and genesis transactions.
    pub always_include_system_packages: bool,
}

/// Unifiers. These are used to determine how to unify two packages.
#[derive(Debug, Clone)]
pub enum ConflictResolution {
    /// An exact constraint unifies as follows:
    /// 1. Exact(a) ~ Exact(b) ==> Exact(a), iff a == b
    /// 2. Exact(a) ~ AtLeast(b) ==> Exact(a), iff a >= b
    Exact(SequenceNumber, ObjectID),
    /// An at least constraint unifies as follows:
    /// * AtLeast(a, a_version) ~ AtLeast(b, b_version) ==> AtLeast(x, max(a_version, b_version)),
    ///   where x is the package id of either a or b (the one with the greatest version).
    AtLeast(SequenceNumber, ObjectID),
}

#[derive(Debug, Clone)]
pub(crate) struct ResolutionTable {
    pub(crate) resolution_table: BTreeMap<ObjectID, ConflictResolution>,
    /// For every version of every package that we have seen, a mapping of the ObjectID for that
    /// package to its runtime ID.
    pub(crate) all_versions_resolution_table: BTreeMap<ObjectID, ObjectID>,
}

impl LinkageConfig {
    pub fn unified_linkage_settings(always_include_system_packages: bool) -> Self {
        Self {
            fix_top_level_functions: true,
            fix_types: false,
            exact_entry_transitive_deps: false,
            exact_transitive_deps: false,
            always_include_system_packages,
        }
    }

    pub fn per_command_linkage_settings(always_include_system_packages: bool) -> Self {
        Self {
            fix_top_level_functions: true,
            fix_types: false,
            exact_entry_transitive_deps: true,
            exact_transitive_deps: true,
            always_include_system_packages,
        }
    }

    pub fn generate_top_level_fn_constraint(
        &self,
    ) -> for<'a> fn(&'a MovePackage) -> ConflictResolution {
        if self.fix_top_level_functions {
            ConflictResolution::exact
        } else {
            ConflictResolution::at_least
        }
    }

    pub fn generate_type_constraint(&self) -> for<'a> fn(&'a MovePackage) -> ConflictResolution {
        if self.fix_types {
            ConflictResolution::exact
        } else {
            ConflictResolution::at_least
        }
    }

    pub fn generate_entry_transitive_dep_constraint(
        &self,
    ) -> for<'a> fn(&'a MovePackage) -> ConflictResolution {
        if self.exact_entry_transitive_deps {
            ConflictResolution::exact
        } else {
            ConflictResolution::at_least
        }
    }

    pub fn generate_transitive_dep_constraint(
        &self,
    ) -> for<'a> fn(&'a MovePackage) -> ConflictResolution {
        if self.exact_transitive_deps {
            ConflictResolution::exact
        } else {
            ConflictResolution::at_least
        }
    }

    pub(crate) fn resolution_table_with_native_packages(
        &self,
        type_origin_map: &mut CachedTypeOriginMap,
        store: &dyn PackageStore,
    ) -> Result<ResolutionTable, ExecutionError> {
        let mut resolution_table = ResolutionTable::empty();
        if self.always_include_system_packages {
            for id in NATIVE_PACKAGE_IDS {
                let package = PTBLinkageResolver::get_package(type_origin_map, id, store)?;
                debug_assert_eq!(package.id(), *id);
                debug_assert_eq!(package.original_package_id(), *id);
                resolution_table
                    .resolution_table
                    .insert(*id, ConflictResolution::Exact(package.version(), *id));
                resolution_table
                    .all_versions_resolution_table
                    .insert(*id, *id);
            }
        }

        Ok(resolution_table)
    }
}

impl ResolutionTable {
    pub fn empty() -> Self {
        Self {
            resolution_table: BTreeMap::new(),
            all_versions_resolution_table: BTreeMap::new(),
        }
    }
}

impl ConflictResolution {
    pub fn exact(pkg: &MovePackage) -> ConflictResolution {
        ConflictResolution::Exact(pkg.version(), pkg.id())
    }

    pub fn at_least(pkg: &MovePackage) -> ConflictResolution {
        ConflictResolution::AtLeast(pkg.version(), pkg.id())
    }

    pub fn unify(&self, other: &ConflictResolution) -> Result<ConflictResolution, ExecutionError> {
        match (&self, other) {
            // If we have two exact resolutions, they must be the same.
            (ConflictResolution::Exact(sv, self_id), ConflictResolution::Exact(ov, other_id)) => {
                if self_id != other_id || sv != ov {
                    Err(ExecutionError::new_with_source(
                        ExecutionErrorKind::InvalidLinkage,
                        format!(
                            "exact/exact conflicting resolutions for package: linkage requires the same package \
                                 at different versions. Linkage requires exactly {self_id} (version {sv}) and \
                                 {other_id} (version {ov}) to be used in the same transaction"
                        ),
                    ))
                } else {
                    Ok(ConflictResolution::Exact(*sv, *self_id))
                }
            }
            // Take the max if you have two at least resolutions.
            (
                ConflictResolution::AtLeast(self_version, sid),
                ConflictResolution::AtLeast(other_version, oid),
            ) => {
                let id = if self_version > other_version {
                    *sid
                } else {
                    *oid
                };

                Ok(ConflictResolution::AtLeast(
                    *self_version.max(other_version),
                    id,
                ))
            }
            // If you unify an exact and an at least, the exact must be greater than or equal to
            // the at least. It unifies to an exact.
            (
                ConflictResolution::Exact(exact_version, exact_id),
                ConflictResolution::AtLeast(at_least_version, at_least_id),
            )
            | (
                ConflictResolution::AtLeast(at_least_version, at_least_id),
                ConflictResolution::Exact(exact_version, exact_id),
            ) => {
                if exact_version < at_least_version {
                    return Err(ExecutionError::new_with_source(
                        ExecutionErrorKind::InvalidLinkage,
                        format!(
                            "Exact/AtLeast conflicting resolutions for package: linkage requires exactly this \
                                 package {exact_id} (version {exact_version}) and also at least the following \
                                 version of the package {at_least_id} at version {at_least_version}. However \
                                 {exact_id} is at version {exact_version} which is less than {at_least_version}."
                        ),
                    ));
                }

                Ok(ConflictResolution::Exact(*exact_version, *exact_id))
            }
        }
    }
}
