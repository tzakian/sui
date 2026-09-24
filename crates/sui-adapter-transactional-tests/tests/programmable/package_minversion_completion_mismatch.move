// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// The completion path must match the cap's minversion state. Both failures are atomic.

//# init --addresses EnrolledV1=0x0 EnrolledV2=0x0 OrdinaryV1=0x0 OrdinaryV2=0x0 --accounts A

//# publish --upgradeable --sender A
module EnrolledV1::enrolled;
public fun ping() {}

//# programmable --sender A --inputs object(1,1) object(0x426)
//> 0: sui::package::enable_minversion(Input(0));
//> 1: sui::package_config::record_minversion_enrollment(Input(1), Result(0));

// An enrolled cap rejects the ordinary completion emitted without --minversion.
//# upgrade --package EnrolledV1 --upgrade-capability 1,1 --sender A
module EnrolledV2::enrolled;
public fun ping() {}

//# publish --upgradeable --sender A
module OrdinaryV1::ordinary;
public fun ping() {}

// An unenrolled cap rejects the minversion completion emitted by --minversion.
//# upgrade --package OrdinaryV1 --upgrade-capability 4,1 --sender A --minversion
module OrdinaryV2::ordinary;
public fun ping() {}
