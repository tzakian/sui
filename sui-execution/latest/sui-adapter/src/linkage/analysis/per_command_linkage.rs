// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    data_store::PackageStore,
    linkage::analysis::{
        CachedTypeOriginMap, LinkageAnalysis, PTBLinkageResolver, ResolvedLinkage,
        shared::{LinkageConfig, ResolutionTable},
    },
};
use move_binary_format::binary_config::BinaryConfig;
use std::{cell::RefCell, collections::BTreeMap};
use sui_types::{error::ExecutionError, transaction as P};

#[derive(Debug)]
pub struct PerCommandLinkage {
    internal: PTBLinkageResolver,
}

impl LinkageAnalysis for PerCommandLinkage {
    fn add_command(
        &self,
        command: &P::Command,
        store: &dyn PackageStore,
    ) -> Result<ResolvedLinkage, ExecutionError> {
        self.add_command(command, store)
    }

    fn resolver(&self) -> &PTBLinkageResolver {
        &self.internal
    }
}

impl PerCommandLinkage {
    pub fn new(
        always_include_system_packages: bool,
        binary_config: BinaryConfig,
        _store: &dyn PackageStore,
    ) -> Result<Self, ExecutionError> {
        let linkage_config =
            LinkageConfig::per_command_linkage_settings(always_include_system_packages);
        Ok(Self {
            internal: PTBLinkageResolver {
                type_origin_cache: RefCell::new(CachedTypeOriginMap::new()),
                linkage_config,
                binary_config,
            },
        })
    }

    pub fn add_command(
        &self,
        command: &P::Command,
        store: &dyn PackageStore,
    ) -> Result<ResolvedLinkage, ExecutionError> {
        let mut unification_table = ResolutionTable {
            resolution_table: BTreeMap::new(),
            all_versions_resolution_table: BTreeMap::new(),
        };
        Ok(ResolvedLinkage::from_resolution_table(
            self.internal
                .add_command(command, store, &mut unification_table)?,
        ))
    }
}
