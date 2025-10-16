// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use sui_types::error::ExecutionError;

use crate::static_programmable_transactions::{
    env::Env, loading::ast as L, metering::transaction_meter::TransactionMeter,
};

pub fn meter(
    meter: &mut TransactionMeter,
    env: &Env,
    transaction: &L::Transaction,
) -> Result<(), ExecutionError> {
    todo!()
}
