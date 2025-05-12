// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    data_store::{PackageStore, linked_data_store::LinkedDataStore},
    linkage::{Linkage, analysis::LinkageAnalysis},
};
use move_core_types::{account_address::AccountAddress, language_storage::StructTag};
use std::rc::Rc;
use sui_types::{
    base_types::{MoveObjectType, ObjectID},
    error::ExecutionError,
    transaction as P,
};

// LinkableStore is a wrapper around a `PackageStore` and holds a `LinkageAnalysis` object -- this
// is all the state needed such that if we are given e.g., a `MoveObjectType` or some other
// "linkable" data we can compute and produce a `LinkedDataStore` for it.
pub struct LinkableStore<'a> {
    pub linkage_analyzer: &'a mut dyn LinkageAnalysis,
    pub store: &'a dyn PackageStore,
}

impl<'a> LinkableStore<'a> {
    pub fn new(linkage_analyzer: &'a mut dyn LinkageAnalysis, store: &'a dyn PackageStore) -> Self {
        Self {
            linkage_analyzer,
            store,
        }
    }

    pub fn linked_data_store_for_linkage<'b>(&'b self, linkage: &'b Linkage) -> LinkedDataStore<'b>
    where
        'a: 'b,
    {
        let resolver = self.linkage_analyzer.resolver();
        LinkedDataStore::new(linkage, &resolver, self.store)
    }

    pub fn linkage_for_object_type(
        &self,
        object_type: &MoveObjectType,
    ) -> Result<Linkage, ExecutionError> {
        self.linkage_for_struct_tag(&StructTag::from(object_type.clone()))
    }

    pub fn linkage_for_struct_tag(
        &self,
        struct_tag: &StructTag,
    ) -> Result<Linkage, ExecutionError> {
        let link_context = struct_tag.address;
        let ids: Vec<_> = struct_tag
            .all_addresses()
            .into_iter()
            .map(ObjectID::from)
            .collect();
        let resolved_linkage = Rc::new(
            self.linkage_analyzer
                .resolver()
                .type_linkage(ids.as_slice(), self.store)?,
        );
        Ok(Linkage {
            link_context,
            resolved_linkage,
        })
    }

    pub fn link_context_for_command(&self, command: &P::Command) -> AccountAddress {
        match command {
            P::Command::MoveCall(programmable_move_call) => *programmable_move_call.package,
            P::Command::MakeMoveVec(_, _)
            | P::Command::SplitCoins(_, _)
            | P::Command::MergeCoins(_, _)
            | P::Command::Publish(_, _)
            | P::Command::TransferObjects(_, _)
            | P::Command::Upgrade(_, _, _, _) => Linkage::DEFAULT_LINK_CTX,
        }
    }
}
