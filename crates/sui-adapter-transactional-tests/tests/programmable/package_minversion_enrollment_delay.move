// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Enrollment takes effect only at an epoch boundary. A historical v1 reference remains v1 in the
// enrollment epoch and selects the enrolled v2 package in the following epoch.

//# init --addresses BaseV1=0x0 BaseV2=0x0 --accounts A

//# publish --upgradeable --sender A
module BaseV1::base;
public fun ping() { abort 1 }

//# upgrade --package BaseV1 --upgrade-capability 1,1 --sender A
module BaseV2::base;
public fun ping() {}

//# programmable --sender A --inputs object(1,1) object(0x426)
//> 0: sui::package::enable_minversion(Input(0));
//> 1: sui::package_config::record_minversion_enrollment(Input(1), Result(0));

// The pending enrollment is not yet stable, so v1 still aborts.
//# run BaseV1::base::ping --sender A

// An explicit newer root is never downgraded to the pending/stable package.
//# run BaseV2::base::ping --sender A

//# advance-epoch

// The stable v2 selection now redirects v1.
//# run BaseV1::base::ping --sender A

// An equal root remains v2 after selection activates.
//# run BaseV2::base::ping --sender A
