// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    data_store::{ResolvablePackageStore, linked_data_store::LinkedDataStore},
    linkage::{Linkage, analysis::type_linkage},
};
use move_core_types::{account_address::AccountAddress, language_storage::StructTag};
use std::rc::Rc;
use sui_types::{
    base_types::{MoveObjectType, ObjectID},
    error::ExecutionError,
    transaction as P,
};

/// LinkableStore is a thin wrapper around a `PackageStore` that allows us to add some convenience
/// functionality towards building linkages, and to create `LinkedDataStore`s.
pub struct LinkableStore<'a> {
    pub store: Box<dyn ResolvablePackageStore + 'a>,
    // TODO: Linkage cache?
}

impl<'a> LinkableStore<'a> {
    /// Create a new `LinkableStore` with the given `ResolvablePackageStore`.
    pub fn new(store: Box<dyn ResolvablePackageStore + 'a>) -> Self {
        Self { store }
    }

    /// Create a new `LinkedDataStore` for the given `Linkage`.
    pub fn linked_data_store_for_linkage<'b>(&'b self, linkage: &'b Linkage) -> LinkedDataStore<'b>
    where
        'a: 'b,
    {
        LinkedDataStore::new(linkage, self.store.as_ref())
    }

    /// Compute the `Linkage` for a `MoveObjectType`. All `MoveObjectType`s are expected to be
    /// defining-id based.
    pub fn linkage_for_object_type(
        &self,
        object_type: &MoveObjectType,
    ) -> Result<Linkage, ExecutionError> {
        self.linkage_for_struct_tag(&StructTag::from(object_type.clone()))
    }

    /// Compute the `Linkage` for a `StructTag`. All `StructTag`s are expected to be
    /// defining-id based.
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
        let resolved_linkage =
            Rc::new(type_linkage(ids.as_slice(), self.store.as_package_store())?);
        Ok(Linkage {
            link_context,
            resolved_linkage,
        })
    }

    /// Compute the link context for a PTB command. We set the link context to a default value for
    /// all commands other than `MoveCall`.
    ///
    /// FUTURE: This will go away and we will simply use the computed PTB linkage for the command
    /// in the static PTBs.
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
