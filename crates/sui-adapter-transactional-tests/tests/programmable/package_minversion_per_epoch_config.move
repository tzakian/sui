// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// The package-config object is an implicit per-epoch-config effect only when execution uses a
// stable minversion selection. Missing and pending settings do not add it; an explicit input is
// not additionally tracked as an implicit read.

//# init --addresses BaseV1=0x0 BaseV2=0x0 --accounts A

//# publish --upgradeable --sender A
module BaseV1::base;

public fun ping() {}

//# upgrade --package BaseV1 --upgrade-capability 1,1 --sender A
module BaseV2::base;

public fun ping() {}

// There is no setting. This succeeds without an unchanged package-config effect.
//# run BaseV2::base::ping --sender A

//# programmable --sender A --inputs object(1,1) object(0x426)
//> 0: sui::package::enable_minversion(Input(0));
//> sui::package_config::record_minversion_enrollment(Input(1), Result(0));

// The setting is pending in this epoch, so execution does not use or track package config.
//# run BaseV2::base::ping --sender A

//# advance-epoch

// The stable setting redirects BaseV1 and records package config as an unchanged consensus read.
//# run BaseV1::base::ping --sender A

// Supplying package config explicitly still permits the selection, but it is an input rather than
// an additional implicit per-epoch-config read.
//# programmable --sender A --inputs object(0x426)
//> BaseV1::base::ping();

// The same holds for a non-mutable shared input: it is a read-only-root effect, not an additional
// implicit PerEpochConfig effect.
//# programmable --sender A --inputs immshared(0x426)
//> BaseV1::base::ping();
