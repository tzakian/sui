// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::static_programmable_transactions::{
    env::Env, metering::transaction_meter::TransactionMeter,
};
use sui_types::{error::ExecutionError, transaction::ProgrammableTransaction};

pub fn meter(
    meter: &mut TransactionMeter,
    env: &Env,
    transaction: &ProgrammableTransaction,
) -> Result<(), ExecutionError> {
    todo!()
}
