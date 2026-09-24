// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    data_store::{PackageMetadata, PackageStore, VerifiedPackageStore},
    static_programmable_transactions::{
        linkage::{
            analysis::LinkageAnalyzer,
            facts::{LinkageCommandFacts, LinkageFacts, ModuleInitFacts},
            resolution::{
                ConstraintKind, LinkageStoreResolver, ResolutionTable, VersionConstraint,
                get_package,
            },
            resolved_linkage::{ExecutableLinkage, ResolvedLinkage},
        },
        loading::ast::{
            Argument, Command, DeserializedPackage, InputArg, InputType, Inputs, PackagePayload,
            Transaction,
        },
    },
};
use move_binary_format::{CompiledModule, file_format::Visibility};
use std::collections::{BTreeMap, BTreeSet};
use sui_protocol_config::ProtocolConfig;
use sui_types::{
    Identifier,
    base_types::ObjectID,
    error::ExecutionErrorTrait,
    execution_status::{ExecutionErrorKind, PackageUpgradeError},
    package_config::MinVersion,
};
use sui_verifier::INIT_FN_NAME;

#[derive(Default)]
pub(crate) struct PackagePolicyTargets {
    // The set of original package IDs that were resolved by a `MoveCall` in the transaction and
    // that may participate in the runtime linkage.
    pub execution_original_ids: BTreeSet<ObjectID>,
    // The set of `(original_id, version_id)` pairs that were declared by a publish or upgrade
    // command, regardless of whether that command runs an `init` and contributes to the runtime
    // linkage.
    pub publication_versions: BTreeSet<(ObjectID, u64)>,
}

/// Replace each command's per-call linkage with a single linkage shared by the whole transaction.
///
/// Done in two passes:
///   1. Fold every command's package and type-argument constraints into one `ResolutionTable`,
///      unifying as we go (an error here means the commands cannot agree on a single set of
///      package versions).
///      - Top level functions are pinned `exact`, while their dependencies are
///        pinned `exact` or `at_least` based on the visibility of the top-level function.
///        Type-argument packages are always `at_least`.
///      - Publishes and upgrades introduce their own constraints to the linkage, but only if
///        they have an `init` function (otherwise they do not contribute to the linkage). See
///        comments on each of the command arms for details on this.
///   2. Write the resulting unified linkage back into every `MoveCall`.
///
/// Because all calls end up sharing one linkage, every package version selection is consistent
/// across the transaction.
pub(crate) fn refine_to_single_linkage<E: ExecutionErrorTrait>(
    txn: &mut Transaction,
    linkage_analysis: &LinkageAnalyzer,
    package_store: &VerifiedPackageStore<'_>,
    protocol_config: &ProtocolConfig,
    minversion_resolver: Option<&dyn Fn(ObjectID) -> Result<Option<MinVersion>, E>>,
) -> Result<PackagePolicyTargets, E> {
    let facts = txn
        .commands
        .iter()
        .enumerate()
        .map(|(i, command)| {
            loaded_command::<E>(command, package_store).map_err(|e| e.with_command_index(i))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut policy_targets = PackagePolicyTargets::default();
    let mut linkage = compute_unified_linkage::<E, _>(
        facts.iter().cloned(),
        linkage_analysis,
        package_store,
        protocol_config,
        Some(&mut policy_targets),
        minversion_resolver,
    )?;

    let mut resolver = LinkageStoreResolver::new(package_store, minversion_resolver);
    for (i, command) in txn.commands.iter().enumerate() {
        add_used_input_linkage::<E, _>(
            command.arguments(),
            &txn.inputs,
            &mut linkage,
            &mut resolver,
            protocol_config,
        )
        .map_err(|e| e.with_command_index(i))?;
    }
    add_withdrawal_compatibility_input_linkage::<E, _>(
        &txn.inputs,
        &mut linkage,
        &mut resolver,
        protocol_config,
    )?;

    if protocol_config.harden_linkage_consistency() {
        for (i, fact) in facts.iter().enumerate() {
            validate_init_linkage_pinning::<E>(
                fact,
                &linkage,
                protocol_config.enable_package_minversion(),
            )
            .map_err(|e| e.with_command_index(i))?;
        }
    }

    let resolved_linkage = ExecutableLinkage::new(ResolvedLinkage::from_resolution_table(linkage));

    assert_invariant!(
        !protocol_config.harden_linkage_consistency()
            || resolved_linkage
                .0
                .linkage_resolution
                .iter()
                .all(|(_, resolution)| resolution.version.is_some()),
        "Unified linkage must resolve every package to a specific version, but found: {:?}",
        resolved_linkage
    );

    for (i, command) in txn.commands.iter_mut().enumerate() {
        write_back_linkage::<E>(command, &resolved_linkage).map_err(|e| e.with_command_index(i))?;
    }

    txn.unified_linkage = Some(resolved_linkage);

    Ok(policy_targets)
}

fn add_used_input_linkage<'a, E: ExecutionErrorTrait, S: PackageStore + ?Sized>(
    arguments: impl IntoIterator<Item = &'a Argument>,
    inputs: &Inputs,
    resolution_table: &mut ResolutionTable,
    resolver: &mut LinkageStoreResolver<'_, S, E>,
    protocol_config: &ProtocolConfig,
) -> Result<(), E> {
    if !protocol_config.harden_linkage_consistency() {
        return Ok(());
    }

    let type_defining_ids = arguments.into_iter().filter_map(|argument| {
        let Argument::Input(i) = argument else {
            return None;
        };
        let Some((_, InputType::Fixed(ty))) = inputs.get(*i as usize) else {
            return None;
        };
        Some(ty.all_addresses().into_iter().map(ObjectID::from))
    });
    add_type_package_ids::<E, _>(resolution_table, type_defining_ids.flatten(), resolver)
}

fn add_withdrawal_compatibility_input_linkage<E: ExecutionErrorTrait, S: PackageStore + ?Sized>(
    inputs: &Inputs,
    resolution_table: &mut ResolutionTable,
    resolver: &mut LinkageStoreResolver<'_, S, E>,
    protocol_config: &ProtocolConfig,
) -> Result<(), E> {
    if !protocol_config.harden_linkage_consistency() {
        return Ok(());
    }

    let type_defining_ids = inputs.iter().filter_map(|(input_arg, input_ty)| {
        if let InputArg::FundsWithdrawal(withdrawal) = input_arg
            && withdrawal.from_compatibility_object
            && let InputType::Fixed(ty) = input_ty
        {
            Some(ty.all_addresses().into_iter().map(ObjectID::from))
        } else {
            None
        }
    });
    add_type_package_ids::<E, _>(resolution_table, type_defining_ids.flatten(), resolver)
}

/// A publish or upgrade that runs an `init` executes it under the linkage declared by that command
/// so every dependency the command declares must be pinned `exact`ly to the version it declared in
/// the larger unified linkage.
fn validate_init_linkage_pinning<E: ExecutionErrorTrait>(
    facts: &LinkageCommandFacts,
    resolution_table: &ResolutionTable,
    minversion_enabled: bool,
) -> Result<(), E> {
    let validate_package_init_linkage = |linkage: &LinkageFacts, err_context| {
        for (original_id, version_id) in linkage {
            match resolution_table.resolution_table.get(original_id) {
                Some(VersionConstraint::Exact(_, pinned_id)) if pinned_id == version_id => (),
                other if minversion_enabled => {
                    return Err(E::new_with_source(
                        ExecutionErrorKind::InvalidLinkage,
                        format!(
                            "{err_context} runs an `init` that requires package {original_id} at \
                            {version_id}, but the transaction linkage pins it to {other:?}"
                        ),
                    ));
                }
                other => invariant_violation!(
                    "{err_context} runs an `init` that requires package {original_id} at \
                    {version_id}, but the transaction linkage pins it to {other:?}"
                ),
            }
        }
        Ok(())
    };

    match facts {
        LinkageCommandFacts::Publish { has_init, linkage } if *has_init => {
            validate_package_init_linkage(linkage, "publish")
        }
        LinkageCommandFacts::Upgrade {
            current_module_inits,
            upgrade_modules,
            linkage,
            ..
        } if has_new_module_init(current_module_inits, upgrade_modules) => {
            validate_package_init_linkage(linkage, "upgrade")
        }
        _ => Ok(()),
    }
}

fn loaded_command<E: ExecutionErrorTrait>(
    command: &Command,
    package_store: &VerifiedPackageStore<'_>,
) -> Result<LinkageCommandFacts, E> {
    match command {
        Command::MoveCall(move_call) => Ok(LinkageCommandFacts::MoveCall {
            package: (*move_call.function.version_mid.address()).into(),
            visibility: move_call.function.visibility,
            type_defining_ids: move_call
                .function
                .type_arguments
                .iter()
                .flat_map(|ty| ty.all_addresses())
                .map(ObjectID::from)
                .collect(),
        }),
        Command::Publish(PackagePayload::Serialized(_), ..) => {
            invariant_violation!("Unexpected serialized package payload in linkage analysis")
        }
        Command::Publish(
            PackagePayload::Deserialized(DeserializedPackage {
                deserialized_modules,
                ..
            }),
            _,
            resolved_linkage,
        ) => Ok(LinkageCommandFacts::Publish {
            has_init: deserialized_modules.iter().any(module_has_init),
            linkage: resolved_linkage.linkage.clone(),
        }),
        Command::Upgrade(payload, _, current_package_id, _, resolved_linkage) => {
            let current_pkg = get_package::<E, _>(current_package_id, package_store)?;
            // Whether each module already present in the current package defines an `init`.
            let current_module_inits = current_pkg
                .modules()
                .iter()
                .map(|(module_id, module)| {
                    (
                        module_id.name().as_str().to_owned(),
                        module_has_init(module.compiled_module()),
                    )
                })
                .collect::<BTreeMap<_, _>>();

            let upgrade_modules = match payload {
                PackagePayload::Serialized(_) => {
                    invariant_violation!(
                        "Unexpected serialized package payload in linkage analysis"
                    )
                }
                PackagePayload::Deserialized(DeserializedPackage {
                    deserialized_modules,
                    ..
                }) => deserialized_modules.clone(),
            };

            Ok(LinkageCommandFacts::Upgrade {
                current_package_id: *current_package_id,
                current_module_inits,
                upgrade_modules,
                linkage: resolved_linkage.linkage.clone(),
            })
        }
        Command::MakeMoveVec(Some(ty), _) => Ok(LinkageCommandFacts::MakeMoveVec {
            type_defining_ids: ty.all_addresses().into_iter().map(ObjectID::from).collect(),
        }),
        Command::MakeMoveVec(None, _)
        | Command::TransferObjects(_, _)
        | Command::SplitCoins(_, _)
        | Command::MergeCoins(_, _) => Ok(LinkageCommandFacts::Noop),
    }
}

pub(crate) fn compute_unified_linkage<E: ExecutionErrorTrait, S: PackageStore + ?Sized>(
    facts: impl IntoIterator<Item = LinkageCommandFacts>,
    linkage_analysis: &LinkageAnalyzer,
    package_store: &S,
    protocol_config: &ProtocolConfig,
    mut policy_targets: Option<&mut PackagePolicyTargets>,
    minversion_resolver: Option<&dyn Fn(ObjectID) -> Result<Option<MinVersion>, E>>,
) -> Result<ResolutionTable, E> {
    let mut base_linkage = linkage_analysis
        .config()
        .resolution_table_with_native_packages::<E, _>(package_store)?;
    // Share selected-package and minversion-setting cache across all transaction commands.
    let mut resolver = LinkageStoreResolver::new(package_store, minversion_resolver);

    let (deferred_upgrades, facts): (Vec<_>, Vec<_>) =
        facts.into_iter().enumerate().partition(|(_, facts)| {
            protocol_config.enable_order_independent_upgrade_init_linkage()
                && matches!(facts, LinkageCommandFacts::Upgrade { .. })
        });

    for (i, facts) in facts.into_iter().chain(deferred_upgrades) {
        if let Some(policy_targets) = policy_targets.as_deref_mut() {
            collect_package_policy_targets::<E, S>(
                &facts,
                &base_linkage,
                protocol_config,
                policy_targets,
                &mut resolver,
            )
            .map_err(|e| e.with_command_index(i))?;
        }
        analyze_command::<E, S>(facts, &mut base_linkage, protocol_config, &mut resolver)
            .map_err(|e| e.with_command_index(i))?;
    }

    Ok(base_linkage)
}

pub(crate) fn collect_package_policy_targets<E: ExecutionErrorTrait, S: PackageStore + ?Sized>(
    facts: &LinkageCommandFacts,
    resolution_table: &ResolutionTable,
    protocol_config: &ProtocolConfig,
    policy_targets: &mut PackagePolicyTargets,
    resolver: &mut LinkageStoreResolver<'_, S, E>,
) -> Result<(), E> {
    let linkage = match facts {
        LinkageCommandFacts::MoveCall { package, .. } => {
            resolver.collect_original_ids(
                resolution_table,
                *package,
                &mut policy_targets.execution_original_ids,
            )?;
            return Ok(());
        }
        LinkageCommandFacts::Publish {
            has_init: true,
            linkage,
        } => {
            resolver.collect_declared_package_versions(
                resolution_table,
                linkage.values().copied(),
                &mut policy_targets.publication_versions,
            )?;
            linkage
        }
        LinkageCommandFacts::Upgrade {
            current_module_inits,
            upgrade_modules,
            linkage,
            ..
        } if protocol_config.enable_init_on_upgrade()
            && has_new_module_init(current_module_inits, upgrade_modules) =>
        {
            resolver.collect_declared_package_versions(
                resolution_table,
                linkage.values().copied(),
                &mut policy_targets.publication_versions,
            )?;
            linkage
        }
        // A publication without an executing init does not enter the runtime unified linkage,
        // but it persists its exact declared dependency IDs. Check those IDs against package
        // policy without applying minversion selection.
        LinkageCommandFacts::Publish { linkage, .. }
        | LinkageCommandFacts::Upgrade { linkage, .. } => {
            resolver.collect_declared_package_versions(
                resolution_table,
                linkage.values().copied(),
                &mut policy_targets.publication_versions,
            )?;
            return Ok(());
        }
        LinkageCommandFacts::MakeMoveVec { .. } | LinkageCommandFacts::Noop => return Ok(()),
    };

    for package_id in linkage.values() {
        resolver.collect_original_ids(
            resolution_table,
            ObjectID::from(*package_id),
            &mut policy_targets.execution_original_ids,
        )?;
    }
    Ok(())
}

/// Fold a command's linkage facts into the shared `resolution_table` (pass 1). Only commands
/// that pull packages into the runtime linkage contribute; the rest are no-ops.
fn analyze_command<E: ExecutionErrorTrait, S: PackageStore + ?Sized>(
    facts: LinkageCommandFacts,
    resolution_table: &mut ResolutionTable,
    protocol_config: &ProtocolConfig,
    resolver: &mut LinkageStoreResolver<'_, S, E>,
) -> Result<(), E> {
    match facts {
        LinkageCommandFacts::MoveCall {
            package,
            visibility,
            type_defining_ids,
        } => {
            add_call_to_table::<E, S>(
                resolution_table,
                &package,
                visibility,
                type_defining_ids,
                resolver,
            )?;
        }
        LinkageCommandFacts::Publish { has_init, linkage } => {
            // A publish only affects the transaction's linkage if the package has an `init`
            // function: `init` runs as part of the publish, so its dependencies must be resolvable
            // in this transaction. Without an `init` the freshly published package is not called
            // and contributes nothing.
            //
            // NB: We presuppose here that if a published module has a function named `init`, then
            // it is the package's init function. If it does not conform to the required `init`
            // signature, entry-point verification rejects the transaction later.
            //
            // Published modules are guaranteed non-empty by package deserialization.
            if has_init {
                add_exact_linkage_to_table::<E, S>(resolution_table, &linkage, resolver)?;
            }
        }
        LinkageCommandFacts::Upgrade {
            current_package_id,
            current_module_inits,
            upgrade_modules: new_modules,
            linkage,
        } => {
            if !protocol_config.enable_init_on_upgrade() {
                return Ok(());
            }

            assert_invariant!(
                protocol_config.enable_unified_linkage(),
                "Unified linkage must be enabled before init on upgrade is supported"
            );

            // Reject upgrades where an existing module adds an `init`.
            reject_existing_module_added_init::<E>(&current_module_inits, &new_modules)?;

            // Only newly-introduced modules with an `init` contribute to the linkage.
            if has_new_module_init(&current_module_inits, &new_modules) {
                add_upgrade_init_linkage_to_table::<E, S>(
                    resolution_table,
                    &current_package_id,
                    &linkage,
                    resolver,
                    protocol_config,
                )?;
            }
        }
        LinkageCommandFacts::MakeMoveVec { type_defining_ids } => {
            add_type_package_ids::<E, S>(resolution_table, type_defining_ids, resolver)?;
        }
        LinkageCommandFacts::Noop => (),
    }
    Ok(())
}

fn module_has_init(module: &CompiledModule) -> bool {
    module.function_defs().iter().any(|func_def| {
        let handle = module.function_handle_at(func_def.function);
        module.identifier_at(handle.name) == INIT_FN_NAME
    })
}

fn add_exact_linkage_to_table<E: ExecutionErrorTrait, S: PackageStore + ?Sized>(
    resolution_table: &mut ResolutionTable,
    linkage: &LinkageFacts,
    resolver: &mut LinkageStoreResolver<'_, S, E>,
) -> Result<(), E> {
    for resolved in linkage.values() {
        resolver.resolve_package(
            resolution_table,
            *resolved,
            ConstraintKind::Exact,
            ConstraintKind::Exact,
        )?;
    }
    Ok(())
}

/// Reject an upgrade in which a module that already exists in the current package (and did not
/// previously define an `init`) introduces one.
fn reject_existing_module_added_init<E: ExecutionErrorTrait>(
    current_module_inits: &ModuleInitFacts,
    new_modules: &[CompiledModule],
) -> Result<(), E> {
    for new_module in new_modules {
        let module_name = new_module
            .identifier_at(new_module.self_handle().name)
            .as_str();
        if current_module_inits.get(module_name) == Some(&false) && module_has_init(new_module) {
            return Err(<E>::from_kind(ExecutionErrorKind::PackageUpgradeError {
                upgrade_error: PackageUpgradeError::IncompatibleUpgrade,
            }));
        }
    }
    Ok(())
}

/// Return true if the upgrade introduces at least one new module (absent from the current package)
/// that defines an `init` function. Existing modules never count because adding an `init` to an
/// existing module is rejected by `reject_existing_module_added_init`.
fn has_new_module_init(
    current_module_inits: &ModuleInitFacts,
    new_modules: &[CompiledModule],
) -> bool {
    new_modules.iter().any(|new_module| {
        let module_name = new_module
            .identifier_at(new_module.self_handle().name)
            .as_str();
        current_module_inits.get(module_name).is_none() && module_has_init(new_module)
    })
}

/// Whether this upgrade introduces a module that is absent from the current package and defines an
/// `init` -- i.e. whether this upgrade will run an `init`.
pub(crate) fn upgrade_introduces_new_init<E: ExecutionErrorTrait>(
    current_package_id: &ObjectID,
    upgrade_modules_with_init: &BTreeSet<Identifier>,
    store: &VerifiedPackageStore<'_>,
) -> Result<bool, E> {
    let current_pkg = get_package(current_package_id, store)?;
    Ok(upgrade_modules_with_init.iter().any(|module_name| {
        !current_pkg
            .modules()
            .keys()
            .any(|module_id| module_id.name().as_str() == module_name.as_str())
    }))
}

/// Add the linkage constraints introduced by an upgrade with a new-module `init`.
///
/// There are two cases based on whether the upgraded package already participates in the
/// transaction-wide (Lumpy) linkage:
///
/// - If the upgraded package's original ID is not already in the resolution table, the upgrade is
///   treated like a fresh publish-with-init: every entry of its linkage is added as an `exact`
///   constraint.
/// - If the upgraded package's original ID is in the resolution table, then for every
///   `(original_id, version_id)` in the upgrade linkage either:
///   a. `original_id` is not in the existing Lumpy linkage, so an
///   `original_id -> exact(version_id)` constraint is introduced; or
///   b. it is in the existing Lumpy linkage, in which case the resolved package ID must equal
///   `version_id`.
fn add_upgrade_init_linkage_to_table<E: ExecutionErrorTrait, S: PackageStore + ?Sized>(
    resolution_table: &mut ResolutionTable,
    current_package_id: &ObjectID,
    linkage: &LinkageFacts,
    resolver: &mut LinkageStoreResolver<'_, S, E>,
    protocol_config: &ProtocolConfig,
) -> Result<(), E> {
    let current_pkg = resolver.load_package(current_package_id)?;
    let pkg_original_id = current_pkg.original_id();

    if !resolution_table
        .resolution_table
        .contains_key(&pkg_original_id)
    {
        return add_exact_linkage_to_table::<E, S>(resolution_table, linkage, resolver);
    }

    for (original_id, version_id) in linkage {
        let package = resolver.load_package(&ObjectID::from(*version_id))?;
        let selected_id = package.version_id();
        match resolution_table.resolution_table.get(original_id) {
            None => resolver.resolve_package(
                resolution_table,
                selected_id,
                ConstraintKind::Exact,
                ConstraintKind::Exact,
            )?,
            Some(existing) if existing.object_id() == selected_id => {
                if protocol_config.harden_linkage_consistency() {
                    resolver.resolve_package(
                        resolution_table,
                        selected_id,
                        ConstraintKind::Exact,
                        ConstraintKind::Exact,
                    )?;
                }
            }
            Some(existing) => {
                return Err(E::new_with_source(
                    ExecutionErrorKind::InvalidLinkage,
                    format!(
                        "upgrade init linkage conflicts with transaction linkage: package \
                         {original_id} resolves to {} in transaction linkage, but upgrade \
                         linkage requires {selected_id}",
                        existing.object_id(),
                    ),
                ));
            }
        }
    }

    Ok(())
}

/// Add a `MoveCall`'s target package and type-argument packages to the resolution table.
///
/// The called package is pinned `exact`. Its dependencies are `at_least` for a public entrypoint,
/// but `exact` for a private or friend entrypoint. Type-argument packages are always `at_least`,
/// since types resolve upwards to later versions. This mirrors
/// `LinkageAnalyzer::compute_call_linkage_`.
fn add_call_to_table<E: ExecutionErrorTrait, S: PackageStore + ?Sized>(
    resolution_table: &mut ResolutionTable,
    package: &ObjectID,
    visibility: Visibility,
    type_defining_ids: Vec<ObjectID>,
    resolver: &mut LinkageStoreResolver<'_, S, E>,
) -> Result<(), E> {
    let dependency_constraint = match visibility {
        Visibility::Public => ConstraintKind::AtLeast,
        Visibility::Private | Visibility::Friend => ConstraintKind::Exact,
    };
    resolver.resolve_package(
        resolution_table,
        *package,
        ConstraintKind::Exact,
        dependency_constraint,
    )?;
    add_type_package_ids(resolution_table, type_defining_ids, resolver)
}

/// Add every type-defining package to the resolution table. Types resolve upwards to later
/// versions, so the package and its dependencies are both `at_least`.
fn add_type_package_ids<E: ExecutionErrorTrait, S: PackageStore + ?Sized>(
    resolution_table: &mut ResolutionTable,
    type_defining_ids: impl IntoIterator<Item = ObjectID>,
    resolver: &mut LinkageStoreResolver<'_, S, E>,
) -> Result<(), E> {
    for type_defining_id in type_defining_ids {
        resolver.resolve_package(
            resolution_table,
            type_defining_id,
            ConstraintKind::AtLeast,
            ConstraintKind::AtLeast,
        )?;
    }
    Ok(())
}

/// Overwrite each `MoveCall`'s per-call linkage with the unified transaction-wide linkage (pass 2).
/// Only `MoveCall`s carry an executable linkage; the other commands need no write-back.
fn write_back_linkage<E: ExecutionErrorTrait>(
    command: &mut Command,
    ptb_linkage: &ExecutableLinkage,
) -> Result<(), E> {
    match command {
        Command::MoveCall(move_call) => {
            let previous_linkage = &move_call.function.linkage;
            // Stronger than the length check above: every package the per-call linkage resolved
            // must still be present in the per-component linkage. Unification only ever adds
            // packages (the key set is a union across member calls), so a dropped key signals a
            // bug in how component constraints were folded together.
            //
            // Since `linkage`'s keys are a set, this check also implies that
            // `previous_linkage.0.linkage.len() <= ptb_linkage.0.linkage.len()`.
            assert_invariant!(
                previous_linkage
                    .0
                    .linkage
                    .keys()
                    .all(|k| ptb_linkage.0.linkage.contains_key(k)),
                "single linkage drops a package that the per-call linkage of MoveCall had resolved"
            );
            debug_assert!(
                previous_linkage.0.linkage.len() <= ptb_linkage.0.linkage.len(),
                "single linkage has fewer candidates than the per-call linkage of MoveCall"
            );
            move_call.function.linkage = ptb_linkage.clone();
        }
        Command::TransferObjects(_, _)
        | Command::SplitCoins(_, _)
        | Command::MergeCoins(_, _)
        | Command::MakeMoveVec(_, _)
        | Command::Publish(_, _, _)
        | Command::Upgrade(_, _, _, _, _) => (),
    };
    Ok(())
}
