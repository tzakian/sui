// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

mod compatibility_tests {
    use move_package::source_package::{
        manifest_parser::parse_move_manifest_from_file, parsed_manifest::SourceManifest,
    };
    use std::collections::BTreeMap;
    use std::path::Path;
    use sui_framework::{compare_system_package, BuiltInFramework};
    use sui_framework_snapshot::{load_bytecode_snapshot, load_bytecode_snapshot_manifest};
    use sui_move_build::published_at_property;
    use sui_protocol_config::{Chain, ProtocolConfig, ProtocolVersion};
    use sui_types::execution_config_utils::to_binary_config;

    #[tokio::test]
    async fn test_framework_compatibility() {
        // This test checks that the current framework is compatible with the previous framework
        // bytecode snapshot.
        let manifest = load_bytecode_snapshot_manifest();
        let versions = manifest.keys().cloned().collect::<Vec<_>>();
        for versions in versions.windows(2) {
            let [version, next_version] = *versions else {
                panic!("Expected two versions in the window");
            };
            let old_framework = load_bytecode_snapshot(version).unwrap();
            let new_framework = load_bytecode_snapshot(next_version).unwrap();
            let config =
                ProtocolConfig::get_for_version(ProtocolVersion::new(version), Chain::Unknown);
            let binary_config = to_binary_config(&config);
            let old_framework_store: BTreeMap<_, _> = old_framework
                .into_iter()
                .map(|package| (package.id, package.genesis_object()))
                .collect();
            for cur_package in new_framework.iter() {
                if compare_system_package(
                    &old_framework_store,
                    &cur_package.id,
                    &cur_package.modules(),
                    cur_package.dependencies.to_vec(),
                    &binary_config,
                )
                .await
                .is_none()
                {
                    // Between protocol versions 63 and 67, there were incompatible upgrades across
                    // snapshot versions, but not incompatible across the snapshot versions as they
                    // were released/live on testnet/mainnet.
                    //
                    // In particular version 65 at commit
                    // [`cf3fee71b907969b9308bfa83802c16b9b29bc70`](https://github.com/MystenLabs/sui/tree/cf3fee71b907969b9308bfa83802c16b9b29bc70/crates/sui-framework/packages/move-stdlib/sources)
                    // was released to testnet (upgrade from version 63).
                    //
                    // This commit was never live on mainnet due to safe mode being triggered and the protocol version was not incremented to 65.
                    //
                    // Mainnet was then upgraded to protocol version 66 at commit
                    // [`86fa6e86b62a6984b741bab9c6440adff9aec669`](https://github.com/MystenLabs/sui/tree/86fa6e86b62a6984b741bab9c6440adff9aec669/crates/sui-framework/packages/move-stdlib/sources)
                    // (upgrade from 64).
                    //
                    // cf3fee71b907969b9308bfa83802c16b9b29bc70 contains `std::uq32_32` as a new
                    // Move module. However, `86fa6e86b62a6984b741bab9c6440adff9aec669` does not
                    // contain that module.
                    //
                    // Later both mainnet and testnet were upgraded to protocol version 67 at
                    // commit
                    // [`3ada97c109cc7ae1b451cb384a1f2cfae49c8d3e`](https://github.com/MystenLabs/sui/tree/3ada97c109cc7ae1b451cb384a1f2cfae49c8d3e/crates/sui-framework/packages/move-stdlib/sources)
                    // which contains the `std::uq32_32` module.
                    //
                    // Because of this we have the following upgrade compatibility check on testnet
                    // (T) and mainnet (M) (where Y means the `std::uq32_32` module is present and N means it is not):
                    //
                    // testnet: 63(N) -> 65(Y) -> 67(Y)
                    // mainnet: 64(N) -> 66(N) -> 67(Y)
                    //
                    // These are valid upgrade paths across each network, but the snapshot for `65` is not compatible with `66` since
                    // `65` contains `std::uq32_32` and `66` does not.
                    //
                    // Because of this we special case the check for protocol version 66 to allow
                    // an incompatibility with the previous version 65.
                    if next_version == 66 && version == 65 {
                        continue;
                    } else {
                        panic!(
                            "The Sui framework {:?} at version {version} is not compatible with the next version {next_version}",
                            cur_package.id,
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn check_framework_change_with_protocol_upgrade() {
        // This test checks that if we ever update the framework, the current protocol version must differ
        // the latest bytecode snapshot in each network.
        let snapshots = load_bytecode_snapshot_manifest();
        let latest_snapshot_version = *snapshots.keys().max().unwrap();
        if latest_snapshot_version != ProtocolVersion::MAX.as_u64() {
            // If we have already incremented the protocol version, then we are fine and we don't
            // care if the framework has changed.
            return;
        }
        let latest_snapshot = load_bytecode_snapshot(*snapshots.keys().max().unwrap()).unwrap();
        // Turn them into BTreeMap for deterministic comparison.
        let latest_snapshot_ref: BTreeMap<_, _> =
            latest_snapshot.iter().map(|p| (&p.id, p)).collect();
        let current_framework: BTreeMap<_, _> = BuiltInFramework::iter_system_packages()
            .map(|p| (&p.id, p))
            .collect();
        assert_eq!(
                latest_snapshot_ref,
                current_framework,
                "The current framework differs the latest bytecode snapshot. Did you forget to upgrade protocol version?"
            );
    }

    /// This test checks that the `SinglePackage` entries in `manifest.json` match the metadata
    /// in the `Move.toml` files in the repo.
    ///
    /// Note that this test currently assumes that no framework packages will be removed or moved
    /// within the repo; we check the historical metadata against the current repository. If
    /// needed, we could be more precise by first checking out the revision of the package listed
    /// in the manifest (this should actually be fairly cheap since the git history is present).
    #[test]
    fn check_manifest_against_tomls() {
        let manifest = load_bytecode_snapshot_manifest();
        for entry in manifest.values() {
            for package in entry.packages.iter() {
                // parse package.path/Move.toml
                let toml_path = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .join("..")
                    .join(&package.path);
                let package_toml: SourceManifest =
                    parse_move_manifest_from_file(&toml_path).expect("Move.toml exists");
                // check manifest name field is package.name
                assert_eq!(package_toml.package.name.to_string(), package.name);
                // check manifest published-at field is package.id
                let published_at_field = published_at_property(&package_toml)
                    .expect("Move.toml file has published-at field");
                assert_eq!(published_at_field, package.id);
            }
        }
    }

    #[test]
    fn check_no_dirty_manifest_commit() {
        let snapshots = load_bytecode_snapshot_manifest();
        for snapshot in snapshots.values() {
            assert!(
                !snapshot.git_revision.contains("dirty"),
                "If you are trying to regenerate the bytecode snapshot after cherry-picking, please do so in a standalone PR after the cherry-pick is merged on the release branch.",
            );
        }
    }
}
