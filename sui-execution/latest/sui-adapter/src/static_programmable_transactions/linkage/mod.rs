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
use std::collections::BTreeSet;
use sui_protocol_config::ProtocolConfig;
use sui_types::{
    base_types::EpochId,
    error::{ExecutionError, ExecutionErrorTrait, SuiResult},
    package_config,
    storage::{BackingPackageStore, BackingStore},
    transaction::{ProgrammableTransaction, UnifiedLinkageInformation},
};

pub fn collect_unification_information_for_signing(
    protocol_config: &ProtocolConfig,
    pt: &ProgrammableTransaction,
    backing_store: &dyn BackingStore,
    epoch: EpochId,
) -> SuiResult<UnifiedLinkageInformation> {
    let backing_package_store: &dyn BackingPackageStore = backing_store;
    let backing_package_metadata_store =
        BackingPackageMetadataStore::new(protocol_config, backing_package_store);
    let facts = linkage_facts_from_programmable_transaction(pt, &backing_package_metadata_store)?;
    let linkage_analyzer = LinkageAnalyzer::new::<Normal<ExecutionError>>(protocol_config)
        .map_err(|error| sui_types::error::SuiError::from(error.to_string()))?;

    let minversion_resolver = |original_id| {
        package_config::read_minversion(original_id, backing_store, epoch).map_err(|error| {
            ExecutionError::new_with_source(
                sui_types::execution_status::ExecutionErrorKind::InvalidLinkage,
                error,
            )
        })
    };
    let minversion_resolver = protocol_config
        .enable_package_minversion()
        .then_some(&minversion_resolver as &dyn Fn(_) -> _);

    let mut execution_original_ids = BTreeSet::new();
    let linkage = single_linkage::compute_unified_linkage::<ExecutionError, _>(
        facts,
        &linkage_analyzer,
        &backing_package_metadata_store,
        protocol_config,
        Some(&mut execution_original_ids),
        minversion_resolver,
    )
    .map_err(sui_types::error::SuiError::from)?;

    Ok(UnifiedLinkageInformation {
        execution_original_ids,
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
    epoch: EpochId,
) -> Result<loading::Transaction, Mode::Error> {
    if !protocol_config.enable_unified_linkage() {
        return Ok(txn);
    }

    let minversion_resolver = |original_id| {
        execution_state
            .read_minversion(original_id, epoch)
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
    single_linkage::refine_to_single_linkage::<Mode::Error>(
        &mut txn,
        linkage_analysis,
        package_store,
        protocol_config,
        minversion_resolver,
    )?;

    Ok(txn)
}
