// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

pub mod analysis;
pub mod config;
pub(crate) mod facts;
pub(crate) mod raw_facts;
pub mod resolution;
pub mod resolved_linkage;
pub mod single_linkage;

use crate::{
    data_store::{
        VerifiedPackageStore, backing_package_metadata_store::BackingPackageMetadataStore,
    },
    execution_mode::{ExecutionMode, Normal},
    execution_value::ExecutionState,
    static_programmable_transactions::{
        linkage::{
            analysis::LinkageAnalyzer, raw_facts::linkage_facts_from_programmable_transaction,
        },
        loading::ast as loading,
    },
};
use sui_protocol_config::ProtocolConfig;
use sui_types::{
    error::{ExecutionError, ExecutionErrorTrait, SuiResult},
    package_config,
    storage::{BackingPackageStore, BackingStore},
    transaction::{ProgrammableTransaction, UnifiedLinkageInformation},
};

pub fn collect_unification_information_for_signing(
    protocol_config: &ProtocolConfig,
    pt: &ProgrammableTransaction,
    backing_store: &dyn BackingStore,
) -> SuiResult<UnifiedLinkageInformation> {
    let backing_package_store: &dyn BackingPackageStore = backing_store;
    let backing_package_metadata_store =
        BackingPackageMetadataStore::new(protocol_config, backing_package_store);
    let facts = linkage_facts_from_programmable_transaction(pt, &backing_package_metadata_store)?;
    let linkage_analyzer = LinkageAnalyzer::new::<Normal<ExecutionError>>(protocol_config)
        .map_err(|error| sui_types::error::SuiError::from(error.to_string()))?;

    let package_config_root_version = backing_store
        .get_object(&sui_types::SUI_PACKAGE_CONFIG_OBJECT_ID)
        .map(|object| object.version());
    let minversion_resolver = |original_id| {
        let root_version = package_config_root_version.ok_or_else(|| {
            ExecutionError::new_with_source(
                sui_types::execution_status::ExecutionErrorKind::InvalidLinkage,
                "package config root not found",
            )
        })?;
        package_config::read_minversion(original_id, root_version, backing_store).map_err(|error| {
            ExecutionError::new_with_source(
                sui_types::execution_status::ExecutionErrorKind::InvalidLinkage,
                error,
            )
        })
    };
    let minversion_resolver = protocol_config
        .enable_package_minversion()
        .then_some(&minversion_resolver as &dyn Fn(_) -> _);

    let mut policy_targets = single_linkage::PackagePolicyTargets::default();
    let linkage = single_linkage::compute_unified_linkage::<ExecutionError, _>(
        facts,
        &linkage_analyzer,
        &backing_package_metadata_store,
        protocol_config,
        Some(&mut policy_targets),
        minversion_resolver,
    )
    .map_err(sui_types::error::SuiError::from)?;

    Ok(UnifiedLinkageInformation {
        execution_original_ids: policy_targets.execution_original_ids,
        publication_versions: policy_targets.publication_versions,
        resolved_packages: linkage
            .resolution_table
            .iter()
            .map(|(original_id, resolution)| {
                (*original_id, (resolution.object_id(), resolution.version()))
            })
            .collect(),
    })
}

/// Refine the transaction's per-call linkages into a single, unified linkage for the whole
/// transaction (when enabled by the protocol config).
pub fn refine_linkage<Mode: ExecutionMode>(
    mut txn: loading::Transaction,
    linkage_analysis: &LinkageAnalyzer,
    package_store: &VerifiedPackageStore<'_>,
    protocol_config: &ProtocolConfig,
    execution_state: &dyn ExecutionState,
) -> Result<loading::Transaction, Mode::Error> {
    if !protocol_config.enable_unified_linkage() {
        return Ok(txn);
    }

    let minversion_resolver = |original_id| {
        execution_state
            .read_minversion(original_id)
            .map_err(|error| {
                Mode::Error::new_with_source(
                    sui_types::execution_status::ExecutionErrorKind::InvalidLinkage,
                    error,
                )
            })
    };
    let minversion_resolver = protocol_config
        .enable_package_minversion()
        .then_some(&minversion_resolver as &dyn Fn(_) -> _);
    let policy_targets = single_linkage::refine_to_single_linkage::<Mode::Error>(
        &mut txn,
        linkage_analysis,
        package_store,
        protocol_config,
        minversion_resolver,
    )?;

    if protocol_config.enable_package_version_forbid_list() {
        let linkage = txn.unified_linkage.as_ref().ok_or_else(|| {
            Mode::Error::from_kind(sui_types::execution_status::ExecutionErrorKind::InvalidLinkage)
        })?;
        for original_id in policy_targets.execution_original_ids {
            let Some(package_id) = linkage.0.linkage.get(&original_id) else {
                return Err(Mode::Error::from_kind(
                    sui_types::execution_status::ExecutionErrorKind::InvalidLinkage,
                ));
            };
            let Some(version) = linkage.0.resolved_version(package_id) else {
                // Packages published or upgraded by this PTB are late-bound and have no
                // on-chain version to check yet.
                continue;
            };
            if execution_state
                .is_package_version_forbidden(original_id, version)
                .map_err(|error| {
                    Mode::Error::new_with_source(
                        sui_types::execution_status::ExecutionErrorKind::InvalidLinkage,
                        error,
                    )
                })?
            {
                return Err(Mode::Error::from_kind(
                    sui_types::execution_status::ExecutionErrorKind::InvalidLinkage,
                ));
            }
        }
        for (original_id, version) in &policy_targets.publication_versions {
            if execution_state
                .is_package_version_forbidden(*original_id, *version)
                .map_err(|error| {
                    Mode::Error::new_with_source(
                        sui_types::execution_status::ExecutionErrorKind::InvalidLinkage,
                        error,
                    )
                })?
            {
                return Err(Mode::Error::from_kind(
                    sui_types::execution_status::ExecutionErrorKind::InvalidLinkage,
                ));
            }
        }
    }

    if protocol_config.enable_package_minversion() {
        for (original_id, version) in &policy_targets.publication_versions {
            let minversion = execution_state
                .read_minversion(*original_id)
                .map_err(|error| {
                    Mode::Error::new_with_source(
                        sui_types::execution_status::ExecutionErrorKind::InvalidLinkage,
                        error,
                    )
                })?;
            if minversion.is_some_and(|minversion| *version < minversion.version) {
                return Err(Mode::Error::from_kind(
                    sui_types::execution_status::ExecutionErrorKind::InvalidLinkage,
                ));
            }
        }
    }

    Ok(txn)
}
