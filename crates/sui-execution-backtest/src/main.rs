// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Backtest the execution layer against historical mainnet data: re-execute past transactions under
//! the current execution rules and report where the recomputed result diverges from what was
//! recorded on chain. Useful for measuring the behavioral impact of an execution/protocol change
//! before it ships.
//!
//! For each epoch it resolves the checkpoint range + protocol version from a fullnode, then streams
//! the checkpoints through a bounded-concurrency pipeline: checkpoints are prefetched concurrently,
//! and every programmable transaction matching `--status` (success / failed / all) is fully
//! re-executed against reconstructed state via the `sui-execution` Executor on a blocking worker.
//! Any transaction whose recomputed success/failure status disagrees with its on-chain status is a
//! *divergence*: it is written to an NDJSON file tagged with its on-chain status and the recomputed
//! error (if any). `--status success` is the strict baseline (a tx that succeeded on chain now
//! erroring); `all`/`failed` also replay failures, with each record carrying the on-chain status so
//! the differential can be applied downstream.

mod grpc;
mod store;

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context as _, Result};
use clap::{Parser, ValueEnum};
use futures::StreamExt as _;
use move_core_types::language_storage::TypeTag;
use prometheus::Registry;
use sui_execution::Executor;
use sui_indexer_alt_framework::ingestion::ingestion_client::{
    IngestionClient, IngestionClientArgs,
};
use sui_indexer_alt_framework::metrics::IngestionMetrics;
use sui_protocol_config::{Chain, ProtocolConfig, ProtocolVersion};
use sui_types::accumulator_root::AccumulatorValue;
use sui_types::balance::Balance;
use sui_types::base_types::SuiAddress;
use sui_types::coin_reservation::ParsedObjectRefWithdrawal;
use sui_types::digests::ChainIdentifier;
use sui_types::effects::{TransactionEffects, TransactionEffectsAPI};
use sui_types::execution_params::ExecutionOrEarlyError;
use sui_types::execution_status::{ExecutionErrorKind, ExecutionStatus};
use sui_types::gas::SuiGasStatus;
use sui_types::gas_coin::GAS;
use sui_types::metrics::ExecutionMetrics;
use sui_types::object::Object;
use sui_types::storage::ObjectKey;
use sui_types::transaction::{
    CallArg, CheckedInputObjects, Command, FundsWithdrawalArg, ObjectArg, TransactionData,
    TransactionDataAPI, TransactionKind,
};
use tracing::{error, info};
use url::Url;

use crate::grpc::RpcClient;
use crate::store::{resolve_input_objects, PackageCache, ScanStore};

/// Which on-chain transaction statuses to re-execute.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum StatusFilter {
    /// Only transactions that succeeded on-chain (strict differential baseline).
    Success,
    /// Only transactions that failed on-chain.
    Failed,
    /// Both; divergence records carry their on-chain status/failure kind for downstream filtering.
    All,
}

#[derive(Parser)]
#[clap(
    name = "sui-execution-backtest",
    about = "Re-execute historical transactions and report divergences from their on-chain effects."
)]
struct Args {
    #[clap(flatten)]
    ingestion: IngestionClientArgs,

    /// Sui fullnode gRPC url used to resolve epochs and fetch packages. Defaults to `--rpc-api-url`
    /// when that is provided as the checkpoint source.
    #[clap(long)]
    fullnode_url: Option<Url>,

    /// First epoch to scan (inclusive).
    #[clap(long)]
    start_epoch: u64,

    /// Last epoch to scan (inclusive).
    #[clap(long)]
    end_epoch: u64,

    /// Optional cap on the number of checkpoints to process per epoch (for bounded samples).
    #[clap(long)]
    max_checkpoints_per_epoch: Option<u64>,

    /// Which on-chain transaction statuses to re-execute. `success` keeps the strict baseline (only
    /// txns that succeeded on-chain, so any divergence is a clear regression); `failed` runs only
    /// failed txns; `all` runs both and tags each divergence record with its `original_status`/
    /// failure kind so the differential can be applied in the analysis layer.
    #[clap(long, value_enum, default_value_t = StatusFilter::All)]
    status: StatusFilter,

    /// Number of checkpoints processed concurrently (prefetch + execution pipeline width).
    #[clap(long, default_value_t = 16)]
    concurrency: usize,

    /// Optional directory for an on-disk package cache (speeds up re-scans).
    #[clap(long)]
    cache: Option<PathBuf>,

    /// Path to write divergent transactions to, as newline-delimited JSON.
    #[clap(long)]
    output: PathBuf,
}

/// Per-epoch resolved context, shared (by Arc) across all of the epoch's checkpoint workers.
struct EpochCtx {
    epoch: u64,
    protocol_config: Arc<ProtocolConfig>,
    /// Version-correct executor for full execution. Owns its own `MoveRuntime`.
    executor: Arc<dyn Executor + Send + Sync>,
    /// Execution metrics (shared across all epochs).
    metrics: Arc<ExecutionMetrics>,
    /// The epoch's start timestamp (the first checkpoint's `timestamp_ms`), which is the value the
    /// executor expects for `epoch_timestamp_ms` (constant across the epoch, as `TxContext` sees it
    /// on chain) — *not* the per-checkpoint timestamp.
    epoch_start_timestamp_ms: u64,
    /// The epoch's reference gas price, used to meter execution faithfully (the gas accounting and
    /// `TxContext` differ from passing the tx's own price as the RGP).
    reference_gas_price: u64,
}

/// Per-checkpoint tally returned by a worker; merged by the sequential collector.
#[derive(Default)]
struct CheckpointStats {
    checked: u64,
    /// Transactions we could not faithfully reconstruct/replay (input resolution failure, gas-status
    /// build failure, or a panicked pipeline) — counted, not reported as divergences.
    reconstruction_errors: u64,
    coin_reservation_skipped: u64,
    fetch_errors: u64,
    /// Transactions skipped because they can't be faithfully executed (price-0, non-gasless).
    execute_skipped: u64,
    /// Transactions with empty gas payment (gas paid from address balance via the
    /// accumulator/withdrawal mechanism — state we don't reconstruct). Counted, not skipped (yet).
    gas_from_balance: u64,
    /// Transactions that ran to completion (got effects, no panic).
    executed: u64,
    /// Executed txns whose recomputed success/failure status disagrees with the on-chain status —
    /// an execution-divergence signal. Each is written to the output (see `records`).
    divergences: u64,
    /// Txns whose on-chain failure is a consensus-layer cancellation (shared-object congestion /
    /// randomness unavailable) and which therefore "succeed" in single-tx replay. Excluded from
    /// `divergences` since they're not reproducible.
    cancellation_excluded: u64,
    /// Serialized NDJSON lines, one per divergent transaction.
    records: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    // The adapter raises some missing-data conditions via `invariant_violation!`, which panics in
    // this build. Those panics are caught per-transaction (execution runs on a blocking task) and
    // counted as skips, so route the default hook to a debug log rather than letting it spam stderr
    // — the panic is still observable under `RUST_LOG=debug`, just not at the default level.
    std::panic::set_hook(Box::new(|info| {
        tracing::debug!("caught panic (treated as a per-transaction skip): {info}");
    }));

    let args = Args::parse();

    if args.start_epoch > args.end_epoch {
        bail!(
            "start_epoch ({}) must be <= end_epoch ({})",
            args.start_epoch,
            args.end_epoch
        );
    }
    if args.concurrency == 0 {
        bail!("--concurrency must be >= 1");
    }

    let fullnode_url: Url = args
        .fullnode_url
        .clone()
        .or_else(|| args.ingestion.rpc_api_url.clone())
        .context("provide --fullnode-url (or --rpc-api-url) for epoch and package resolution")?;
    let rpc = RpcClient::new(fullnode_url)?;

    let registry = Registry::new();
    let metrics = IngestionMetrics::new(None, &registry);
    let ingestion = Arc::new(IngestionClient::new(args.ingestion, metrics)?);
    // Shared across all epochs; only consulted on the `--execute` path.
    let execution_metrics = Arc::new(ExecutionMetrics::new(&registry));

    let packages = Arc::new(PackageCache::new(
        rpc.clone(),
        tokio::runtime::Handle::current(),
        args.cache.clone(),
    )?);

    let mut output = std::fs::File::create(&args.output)
        .with_context(|| format!("creating output file {}", args.output.display()))?;

    // Learn the chain identifier (needed to build the right ProtocolConfig) from the first
    // checkpoint of the first epoch.
    let first_bounds = rpc
        .epoch_bounds(args.start_epoch)
        .await
        .with_context(|| format!("resolving epoch {}", args.start_epoch))?;
    let chain: Chain = ingestion
        .checkpoint(first_bounds.first_checkpoint)
        .await
        .with_context(|| format!("fetching checkpoint {}", first_bounds.first_checkpoint))?
        .chain_id
        .chain();

    // Resolve each epoch and build the flat list of (checkpoint, epoch-context) work items.
    let mut work: Vec<(u64, Arc<EpochCtx>)> = Vec::new();
    for epoch in args.start_epoch..=args.end_epoch {
        let bounds = if epoch == args.start_epoch {
            first_bounds
        } else {
            rpc.epoch_bounds(epoch)
                .await
                .with_context(|| format!("resolving epoch {epoch}"))?
        };
        let protocol_config =
            ProtocolConfig::get_for_version(ProtocolVersion::new(bounds.protocol_version), chain);
        let executor = sui_execution::executor(&protocol_config, /* silent */ true)
            .map_err(|e| anyhow::anyhow!("building executor for epoch {epoch}: {e}"))?;
        // The executor expects the epoch *start* timestamp (the first checkpoint's), not a
        // per-checkpoint one.
        let epoch_start_timestamp_ms = ingestion
            .checkpoint(bounds.first_checkpoint)
            .await
            .with_context(|| {
                format!(
                    "fetching first checkpoint {} of epoch {epoch}",
                    bounds.first_checkpoint
                )
            })?
            .checkpoint
            .summary
            .timestamp_ms;
        let ctx = Arc::new(EpochCtx {
            epoch,
            protocol_config: Arc::new(protocol_config),
            executor,
            metrics: execution_metrics.clone(),
            epoch_start_timestamp_ms,
            reference_gas_price: bounds.reference_gas_price,
        });

        let last = match args.max_checkpoints_per_epoch {
            Some(cap) => bounds.last_checkpoint.min(
                bounds
                    .first_checkpoint
                    .saturating_add(cap)
                    .saturating_sub(1),
            ),
            None => bounds.last_checkpoint,
        };
        let count = last
            .saturating_sub(bounds.first_checkpoint)
            .saturating_add(1);
        info!(
            epoch,
            first_checkpoint = bounds.first_checkpoint,
            last_checkpoint = last,
            checkpoints = count,
            protocol_version = bounds.protocol_version,
            "queued epoch"
        );
        for cp in bounds.first_checkpoint..=last {
            work.push((cp, ctx.clone()));
        }
    }

    let total_work = work.len() as u64;
    info!(
        total_checkpoints = total_work,
        concurrency = args.concurrency,
        "starting scan"
    );

    // Bounded-concurrency pipeline: prefetch + execute up to `concurrency` checkpoints at once,
    // then merge results sequentially (so the output file and counters need no locking).
    let mut totals = CheckpointStats::default();
    let mut processed: u64 = 0;
    let mut stream = futures::stream::iter(work.into_iter().map(|(cp, ctx)| {
        let ingestion = ingestion.clone();
        let packages = packages.clone();
        let status = args.status;
        async move { process_checkpoint(cp, ctx, ingestion, packages, status).await }
    }))
    .buffer_unordered(args.concurrency);

    while let Some(stats) = stream.next().await {
        totals.checked += stats.checked;
        totals.reconstruction_errors += stats.reconstruction_errors;
        totals.coin_reservation_skipped += stats.coin_reservation_skipped;
        totals.fetch_errors += stats.fetch_errors;
        totals.execute_skipped += stats.execute_skipped;
        totals.gas_from_balance += stats.gas_from_balance;
        totals.executed += stats.executed;
        totals.divergences += stats.divergences;
        totals.cancellation_excluded += stats.cancellation_excluded;
        for record in stats.records {
            writeln!(output, "{record}").context("writing output record")?;
        }
        output.flush().ok();

        processed += 1;
        if processed.is_multiple_of(500) {
            info!(
                processed,
                total_checkpoints = total_work,
                total_checked = totals.checked,
                total_divergences = totals.divergences,
                total_reconstruction_errors = totals.reconstruction_errors,
                total_fetch_errors = totals.fetch_errors,
                "progress"
            );
        }
    }

    info!(
        total_checked = totals.checked,
        total_divergences = totals.divergences,
        total_reconstruction_errors = totals.reconstruction_errors,
        total_coin_reservation_skipped = totals.coin_reservation_skipped,
        total_fetch_errors = totals.fetch_errors,
        total_execute_skipped = totals.execute_skipped,
        total_gas_from_balance = totals.gas_from_balance,
        total_executed = totals.executed,
        total_cancellation_excluded = totals.cancellation_excluded,
        "backtest complete"
    );
    Ok(())
}

/// Recursively collect every struct `TypeTag` appearing in `tt` (itself + nested type params).
fn collect_struct_types(tt: &TypeTag, out: &mut Vec<TypeTag>) {
    if let TypeTag::Struct(s) = tt {
        out.push(tt.clone());
        for p in &s.type_params {
            collect_struct_types(p, out);
        }
    }
}

/// Rewrite coin-reservation inputs (address-balance backward-compat "fake coin" `ImmOrOwnedObject`
/// refs) into `FundsWithdrawal` args, mirroring the validator's rewrite — but *without* reading the
/// accumulator balance field object (which is often deleted/pruned). The only thing the rewrite
/// needs from that object is the coin type; the amount is in the reservation digest and the owner
/// is the sender. The reservation's (unmasked) id is the deterministic dynamic-field id
/// `derive(accumulator_root, sender, Balance<T>)`, so we recover `T` by re-deriving that id for
/// candidate coin types (every MoveCall type argument in the PTB, plus SUI) and matching.
///
/// Returns the per-input "was rewritten" mask (`None` if nothing was rewritten); errors (→ skip) if
/// a reservation's coin type can't be identified from the candidates.
fn rewrite_coin_reservations(
    chain_id: ChainIdentifier,
    sender: SuiAddress,
    kind: &mut TransactionKind,
    objects: &std::collections::BTreeMap<ObjectKey, Object>,
    effects: &TransactionEffects,
) -> anyhow::Result<Option<Vec<bool>>> {
    let TransactionKind::ProgrammableTransaction(pt) = kind else {
        return Ok(None);
    };
    if pt.coin_reservation_obj_refs().next().is_none() {
        return Ok(None);
    }
    // Candidate coin types `T` for `Balance<T>`: SUI, every MoveCall type argument, and the type
    // parameters of the object types this tx touches (these protocols' calls are often monomorphic
    // with no type args, but an input/output `Pool<_, USDC>` / `Coin<USDC>` still reveals the coin
    // type). The reservation's id deterministically encodes `Balance<T>`, so we match to recover T.
    let mut candidates: Vec<TypeTag> = vec![GAS::type_tag()];
    for cmd in &pt.commands {
        if let Command::MoveCall(call) = cmd {
            candidates.extend(
                call.type_arguments
                    .iter()
                    .filter_map(|ti| ti.to_type_tag().ok()),
            );
        }
    }
    let touched = effects.modified_at_versions().into_iter().chain(
        effects
            .all_changed_objects()
            .into_iter()
            .map(|(oref, _, _)| (oref.0, oref.1)),
    );
    for (id, ver) in touched {
        if let Some(mo) = objects
            .get(&ObjectKey(id, ver))
            .and_then(|o| o.data.try_as_move())
        {
            for p in mo.type_().type_params() {
                collect_struct_types(&p, &mut candidates);
            }
        }
    }
    let mut mask = Vec::with_capacity(pt.inputs.len());
    for input in pt.inputs.iter_mut() {
        let parsed = match input {
            CallArg::Object(ObjectArg::ImmOrOwnedObject(oref)) => {
                ParsedObjectRefWithdrawal::parse(oref, chain_id)
            }
            _ => None,
        };
        let Some(parsed) = parsed else {
            mask.push(false);
            continue;
        };
        // Identify the coin type by matching the reservation's derived accumulator-field id.
        let withdrawal = candidates.iter().find_map(|coin_ty| {
            let field_id =
                AccumulatorValue::get_field_id(sender, &Balance::type_tag(coin_ty.clone())).ok()?;
            (*field_id.inner() == parsed.unmasked_object_id).then(|| {
                FundsWithdrawalArg::balance_from_sender(
                    parsed.reservation_amount(),
                    coin_ty.clone(),
                )
            })
        });
        match withdrawal {
            Some(w) => {
                *input = CallArg::FundsWithdrawal(w);
                mask.push(true);
            }
            None => anyhow::bail!(
                "could not identify balance type for coin reservation {}",
                parsed.unmasked_object_id
            ),
        }
    }
    Ok(Some(mask))
}

/// Fetch one checkpoint and fully re-execute each of its programmable transactions whose on-chain
/// status matches `status`, against reconstructed state. Returns a tally; all errors are folded
/// into counters (never aborts the scan).
async fn process_checkpoint(
    cp: u64,
    ctx: Arc<EpochCtx>,
    ingestion: Arc<IngestionClient>,
    packages: Arc<PackageCache>,
    status: StatusFilter,
) -> CheckpointStats {
    let mut stats = CheckpointStats::default();

    let envelope = match ingestion.checkpoint(cp).await {
        Ok(envelope) => envelope,
        Err(e) => {
            stats.fetch_errors += 1;
            error!(checkpoint = cp, "fetch failed: {e:#}");
            return stats;
        }
    };
    let chain_id = envelope.chain_id;
    let checkpoint = envelope.checkpoint;
    let (objects, latest) = ScanStore::index_object_set(&checkpoint.object_set);
    let objects = Arc::new(objects);
    let latest = Arc::new(latest);
    // Tombstones (deleted/wrapped/unwrapped-then-deleted) across all of the checkpoint's
    // transactions, so child reads can tell a dynamic field was *removed* even though the bundled
    // object set still carries a stale pre-deletion version of it.
    let mut tombstones: std::collections::BTreeMap<
        sui_types::base_types::ObjectID,
        std::collections::BTreeSet<sui_types::base_types::SequenceNumber>,
    > = std::collections::BTreeMap::new();
    for executed in &checkpoint.transactions {
        for (id, ver) in executed.effects.all_tombstones() {
            tombstones.entry(id).or_default().insert(ver);
        }
    }
    let tombstones = Arc::new(tombstones);
    // Execution wants the epoch start timestamp (constant across the epoch), not the per-checkpoint
    // one.
    let epoch_timestamp_ms = ctx.epoch_start_timestamp_ms;
    let epoch_id = ctx.epoch;

    for executed in &checkpoint.transactions {
        let txn_data = &executed.transaction;
        // Apply the status filter. `Success` is the strict differential baseline (only txns that
        // succeeded on-chain can "now fail"); `Failed`/`All` also admit failures, whose original
        // status/failure kind is recorded so the differential can be applied downstream.
        let (original_status, original_failure, non_replayable_cancellation) =
            match executed.effects.status() {
                ExecutionStatus::Success => ("success", None, false),
                ExecutionStatus::Failure(f) => (
                    "failure",
                    Some(format!("{:?}", f.error)),
                    // Cancellations made by consensus-layer congestion/randomness control happen
                    // *before* execution, so the transaction never ran on chain. Single-tx replay
                    // can't reconstruct that scheduling decision and will faithfully execute the
                    // (otherwise valid) transaction — not a reconstruction divergence.
                    matches!(
                        f.error,
                        ExecutionErrorKind::ExecutionCancelledDueToSharedObjectCongestion { .. }
                            | ExecutionErrorKind::ExecutionCancelledDueToRandomnessUnavailable
                    ),
                ),
            };
        let is_success = original_failure.is_none();
        match status {
            StatusFilter::Success if !is_success => continue,
            StatusFilter::Failed if is_success => continue,
            _ => {}
        }
        if !matches!(txn_data.kind(), TransactionKind::ProgrammableTransaction(_)) {
            continue;
        }
        let digest = *executed.effects.transaction_digest();
        let store = Arc::new(ScanStore::new(
            objects.clone(),
            latest.clone(),
            tombstones.clone(),
            packages.clone(),
        ));

        // Rewrite coin-reservation (address-balance fake-coin) inputs into FundsWithdrawal args,
        // on a cloned TransactionData, so input resolution and execution both see the rewritten
        // inputs. The mask flows to the executor (`rewritten_inputs`). A reservation we can't
        // resolve is skipped + counted.
        let mut txn_data: TransactionData = executed.transaction.clone();
        let rewritten_inputs = match rewrite_coin_reservations(
            chain_id,
            txn_data.sender(),
            txn_data.kind_mut(),
            &objects,
            &executed.effects,
        ) {
            Ok(mask) => mask,
            Err(e) => {
                stats.coin_reservation_skipped += 1;
                error!(%digest, "coin-reservation rewrite failed: {e:#}");
                continue;
            }
        };
        let TransactionKind::ProgrammableTransaction(pt) = txn_data.kind() else {
            continue; // checked above
        };
        let pt = pt.clone();

        let input_objects = match resolve_input_objects(&txn_data, &executed.effects, &store) {
            Ok(objs) => CheckedInputObjects::new_for_replay(objs),
            Err(e) => {
                stats.reconstruction_errors += 1;
                error!(%digest, "could not resolve inputs: {e:#}");
                continue;
            }
        };

        let gas_data = txn_data.gas_data().clone();
        let signer = txn_data.sender();
        // Execution must be *metered*: unmetered execution routes storage rebate through the 0x5
        // system-state object (see the adapter's `conserve_unmetered_storage_rebate`), which we do
        // not carry for user transactions. We reproduce the tx's own budget/price and use its price
        // as the reference gas price (the RGP check only requires price >= rgp; we reproduce
        // on-chain behavior, not validate it).
        let gas_status = {
            let rgp = ctx.reference_gas_price;
            // Gasless transactions (empty gas payment + price 0; gas paid from the address balance)
            // are metered on chain at the reference gas price with a fixed compute cap — *not* the
            // tx's zero price/budget. Mirror `sui-transaction-checks::check_gas`. A price-0 tx that
            // isn't this gasless pattern we can't meter (the gas model asserts `price > 0`), so skip.
            let gasless = gas_data.price == 0 && gas_data.payment.is_empty();
            if gas_data.price == 0 && !gasless {
                stats.execute_skipped += 1;
                continue;
            }
            if gas_data.payment.is_empty() {
                // Gas (and possibly funds) from the address balance via the accumulator; the
                // executor handles this without us reconstructing accumulator state.
                stats.gas_from_balance += 1;
            }
            let (budget, price) = if gasless {
                let r = rgp.max(1);
                (
                    ctx.protocol_config
                        .gasless_max_computation_units()
                        .saturating_mul(r),
                    r,
                )
            } else {
                (gas_data.budget, gas_data.price)
            };
            match SuiGasStatus::new(budget, price, rgp, &ctx.protocol_config) {
                Ok(gs) => gs,
                Err(e) => {
                    stats.reconstruction_errors += 1;
                    error!(%digest, "building gas status: {e}");
                    continue;
                }
            }
        };

        stats.checked += 1;

        let protocol_config = ctx.protocol_config.clone();
        // Execute the transaction on a blocking worker, yielding `(recomputed_ok, result)` where
        // `result: Result<(), ExecutionError>` carries the recomputed execution error (if any) and
        // `recomputed_ok` is whether the recomputed effects succeeded.
        let executor = ctx.executor.clone();
        let metrics = ctx.metrics.clone();
        let txn_kind = TransactionKind::ProgrammableTransaction(pt);
        let rewritten = rewritten_inputs.clone();
        let join = tokio::task::spawn_blocking(move || {
            let (_inner, _gas, effects, _timing, exec_res) = executor
                .execute_transaction_to_effects_and_execution_error(
                    &*store,
                    &protocol_config,
                    metrics,
                    /* enable_expensive_checks */ false,
                    ExecutionOrEarlyError::ok(None),
                    &epoch_id,
                    epoch_timestamp_ms,
                    input_objects,
                    gas_data,
                    gas_status,
                    txn_kind,
                    rewritten,
                    signer,
                    digest,
                    &mut None,
                );
            (effects.status().is_ok(), exec_res)
        });

        let (recomputed_ok, result) = match join.await {
            Ok(out) => out,
            Err(join_err) => {
                // A panicked pipeline (e.g. an `invariant_violation!` on data we couldn't
                // reconstruct) counts as a reconstruction error rather than aborting the scan.
                stats.reconstruction_errors += 1;
                error!(%digest, "pipeline task panicked: {join_err}");
                continue;
            }
        };

        stats.executed += 1;
        // Divergence: the recomputed success/failure status disagrees with what happened on chain.
        // This captures any execution-behavior change (e.g. a transaction that succeeded on chain
        // now erroring, or vice versa); the recomputed error, if any, is recorded for triage.
        if recomputed_ok != is_success {
            if non_replayable_cancellation {
                stats.cancellation_excluded += 1;
                continue;
            }
            stats.divergences += 1;
            let recomputed_error = result.as_ref().err().map(|e| e.to_string());
            // Diagnostic: is every object version this tx read present in the checkpoint object
            // set? A missing version (a loaded dynamic-field child or consensus input the stream
            // didn't carry) makes the store serve stale/absent state → a protocol-internal abort.
            use sui_types::effects::InputConsensusObject as Ico;
            use sui_types::storage::ObjectKey as OK;
            let missing_modified: Vec<String> = executed
                .effects
                .modified_at_versions()
                .into_iter()
                .filter(|(id, v)| !objects.contains_key(&OK(*id, *v)))
                .map(|(id, v)| format!("{id}@{v}"))
                .collect();
            let missing_loaded: Vec<String> = executed
                .unchanged_loaded_runtime_objects
                .iter()
                .filter(|k| !objects.contains_key(k))
                .map(|k| format!("{}@{}", k.0, k.1))
                .collect();
            let missing_consensus: Vec<String> = executed
                .effects
                .input_consensus_objects()
                .into_iter()
                .filter_map(|ico| match ico {
                    Ico::Mutate((id, v, _)) | Ico::ReadOnly((id, v, _)) => Some((id, v)),
                    _ => None,
                })
                .filter(|(id, v)| !objects.contains_key(&OK(*id, *v)))
                .map(|(id, v)| format!("{id}@{v}"))
                .collect();
            // Content check: for every input object the effects record (id@version@digest),
            // does our object set hold the *same digest*? A mismatch means the stream gave us
            // different bytes for that version (a data inconsistency) rather than a logic gap.
            let digest_mismatches: Vec<String> = executed
                .effects
                .old_object_metadata()
                .into_iter()
                .filter_map(|((id, v, d), _)| {
                    objects.get(&OK(id, v)).and_then(|o| {
                        let ours = o.compute_object_reference().2;
                        (ours != d).then(|| format!("{id}@{v}"))
                    })
                })
                .collect();
            error!(%digest, on_chain_success = is_success, recomputed_error = ?recomputed_error,
                    ?missing_modified, ?missing_loaded, ?missing_consensus,
                    ?digest_mismatches,
                    loaded_children = executed.unchanged_loaded_runtime_objects.len(),
                    "execution diverges from on-chain");
            let record = serde_json::json!({
                "digest": digest.to_string(),
                "checkpoint": cp,
                "epoch": epoch_id,
                "original_status": original_status,
                "original_failure": original_failure,
                "recomputed_error": recomputed_error,
            });
            stats.records.push(record.to_string());
        }
    }

    stats
}
