// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use sui_types::error::ExecutionError;

use crate::static_programmable_transactions::{
    env::Env, metering::transaction_meter::TransactionMeter, typing::ast as T,
};

pub fn meter(
    meter: &mut TransactionMeter,
    env: &Env,
    transaction: &T::Transaction,
) -> Result<(), ExecutionError> {
    todo!()
}
