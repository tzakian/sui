// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::mock_account::Account;
use crate::tx_generator::TxGenerator;
use move_core_types::identifier::Identifier;
use std::sync::atomic::{AtomicU32, Ordering};
use sui_test_transaction_builder::TestTransactionBuilder;
use sui_types::SUI_FRAMEWORK_PACKAGE_ID;
use sui_types::base_types::SuiAddress;
use sui_types::digests::ChainIdentifier;
use sui_types::gas_coin::GAS;
use sui_types::transaction::{DEFAULT_VALIDATOR_GAS_PRICE, FundsWithdrawalArg, Transaction};

pub struct SendFundsTxGenerator {
    num_fanouts: u64,
    send_amount: u64,
    recipients: Vec<SuiAddress>,
    gas_coin_payment: bool,
    chain_identifier: ChainIdentifier,
    current_epoch: u64,
    nonce: AtomicU32,
}

impl SendFundsTxGenerator {
    pub fn new(
        num_fanouts: u64,
        send_amount: u64,
        recipients: Vec<SuiAddress>,
        gas_coin_payment: bool,
        chain_identifier: ChainIdentifier,
        current_epoch: u64,
    ) -> Self {
        assert_eq!(
            recipients.len() as u64,
            num_fanouts,
            "recipients pool size must match num_fanouts"
        );
        Self {
            num_fanouts,
            send_amount,
            recipients,
            gas_coin_payment,
            chain_identifier,
            current_epoch,
            nonce: AtomicU32::new(0),
        }
    }
}

impl TxGenerator for SendFundsTxGenerator {
    fn generate_tx(&self, account: Account) -> Transaction {
        // `FundsWithdrawalArg::balance_from_sender` wraps the inner T into
        // `WithdrawalTypeArg::Balance(T)`, i.e. a reservation against
        // `Balance<T>`. So we pass `SUI` here, not `Balance<SUI>`.
        let sui_inner = GAS::type_tag();

        let mut tx_builder = if self.gas_coin_payment {
            TestTransactionBuilder::new(
                account.sender,
                account.gas_objects[0],
                DEFAULT_VALIDATOR_GAS_PRICE,
            )
        } else {
            let nonce = self.nonce.fetch_add(1, Ordering::Relaxed);
            TestTransactionBuilder::new_with_address_balance_gas(
                account.sender,
                DEFAULT_VALIDATOR_GAS_PRICE,
                self.chain_identifier,
                self.current_epoch,
                nonce,
            )
        };

        {
            let builder = tx_builder.ptb_builder_mut();

            // N FundsWithdrawal reservation inputs + N recipient pure inputs.
            let mut reservations = Vec::with_capacity(self.num_fanouts as usize);
            for _ in 0..self.num_fanouts {
                let arg = builder
                    .funds_withdrawal(FundsWithdrawalArg::balance_from_sender(
                        self.send_amount,
                        sui_inner.clone(),
                    ))
                    .unwrap();
                reservations.push(arg);
            }
            let mut recipient_args = Vec::with_capacity(self.num_fanouts as usize);
            for recipient in &self.recipients {
                recipient_args.push(builder.pure(*recipient).unwrap());
            }

            // Commands: N `redeem_funds<SUI>(reservation)` producing Balance<SUI>,
            // then N `send_funds<SUI>(balance, recipient)`.
            let balance_module = Identifier::new("balance").unwrap();
            let redeem_fn = Identifier::new("redeem_funds").unwrap();
            let send_fn = Identifier::new("send_funds").unwrap();

            let mut balances = Vec::with_capacity(self.num_fanouts as usize);
            for r in reservations {
                let balance = builder.programmable_move_call(
                    SUI_FRAMEWORK_PACKAGE_ID,
                    balance_module.clone(),
                    redeem_fn.clone(),
                    vec![GAS::type_tag()],
                    vec![r],
                );
                balances.push(balance);
            }
            assert_eq!(balances.len(), recipient_args.len());
            for i in 0..balances.len() {
                builder.programmable_move_call(
                    SUI_FRAMEWORK_PACKAGE_ID,
                    balance_module.clone(),
                    send_fn.clone(),
                    vec![GAS::type_tag()],
                    vec![balances[i], recipient_args[i]],
                );
            }
        }

        tx_builder.build_and_sign(account.keypair.as_ref())
    }

    fn name(&self) -> &'static str {
        "Send Funds Transaction Generator"
    }
}
