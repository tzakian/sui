// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Publish has no init. Its historical DependencyV1 input selects DependencyV2 after minversion
// activation. DependencyV2 introduces PausedLeaf, so policy linkage collection must reject the
// publish after PausedLeaf is globally paused.

//# init --addresses PausedLeaf=0x0 DependencyV1=0x0 DependencyV2=0x0 Publisher=0x0 UpgradePublisherV1=0x0 UpgradePublisherV2=0x0 --accounts A

//# publish --upgradeable --sender A
module PausedLeaf::leaf;

public fun ping() {}

//# publish --upgradeable --sender A
module DependencyV1::dependency;

public fun ping() {}

//# upgrade --package DependencyV1 --upgrade-capability 2,1 --dependencies PausedLeaf --sender A
module DependencyV2::dependency;
use PausedLeaf::leaf;

public fun ping() { leaf::ping() }

//# programmable --sender A --inputs object(2,1) object(0x426)
//> 0: sui::package::enable_minversion(Input(0));
//> sui::package_config::record_minversion_enrollment(Input(1), Result(0));

//# advance-epoch

// This package has no init. It publishes before the leaf is paused so the following upgrade can
// isolate command-linkage policy checking after the pause.
//# publish --upgradeable --dependencies DependencyV1 --sender A
module UpgradePublisherV1::publisher;
use DependencyV1::dependency;

public fun publish_time_only() { dependency::ping() }

//# run sui::package_config::enable_global_pause --args object(0x426) object(1,1) --sender A

// DependencyV1 itself has no dependency on PausedLeaf. This publish must nevertheless be denied
// because minversion first selects DependencyV2 and collects its newly introduced linkage.
//# publish --dependencies DependencyV1 --sender A
module Publisher::publisher;
use DependencyV1::dependency;

public fun publish_time_only() { dependency::ping() }

//# upgrade --package UpgradePublisherV1 --upgrade-capability 6,1 --dependencies DependencyV1 --sender A
module UpgradePublisherV2::publisher;
use DependencyV1::dependency;

public fun publish_time_only() { dependency::ping() }
