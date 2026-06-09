<!--
Copyright (c) Mysten Labs, Inc.
SPDX-License-Identifier: Apache-2.0
-->

# sui-execution-backtest

Backtests the execution layer against historical mainnet data: it re-executes past transactions
under the **current** execution rules and reports where the recomputed result diverges from what was
recorded on chain. Useful for measuring the behavioral impact of an execution/protocol change before
it ships (e.g. a VM, gas, or linkage change).

For a range of epochs it:

1. Resolves each epoch's checkpoint range + protocol version from a fullnode (gRPC `GetEpoch`).
2. Streams the epoch's checkpoints (via `sui-indexer-alt-framework`'s ingestion client), prefetching
   and executing up to `--concurrency` checkpoints at once.
3. For every programmable transaction whose on-chain status matches `--status` (default `all`),
   fully re-executes it against reconstructed checkpoint state via the `sui-execution` Executor.
4. Records a **divergence** for any transaction whose recomputed success/failure status disagrees
   with its on-chain status, writing it to an NDJSON file as it is found (flushed per line, so
   partial results survive an interrupted run).

The strict baseline — "succeeded on chain, now errors" — is `--status success`, so any divergence is
a clear regression. With `--status all`/`failed` the on-chain status is recorded per record so the
differential can be applied downstream.

Divergence direction is recoverable from each record: a recomputed error (on-chain succeeded) has a
non-null `recomputed_error`; a recomputed success (on-chain failed) has `recomputed_error: null`.

## Run

```bash
cargo run --release -p sui-execution-backtest -- \
  --rpc-api-url https://fullnode.mainnet.sui.io:443 \
  --start-epoch 1146 \
  --end-epoch 1147 \
  --max-checkpoints-per-epoch 50000 \
  --concurrency 4 \
  --cache ./.package-cache \
  --output ./divergences.ndjson
```

- `--max-checkpoints-per-epoch N` caps each epoch at its first `N` checkpoints (for bounded
  samples). Omit it to backtest whole epochs — note a mainnet epoch is ~390k checkpoints.
- `--status {success,failed,all}` (default `all`) selects which on-chain statuses to re-execute.
- `--concurrency` is the pipeline width (prefetch + parallel execution). See the rate-limit caveat
  below before raising it against a shared/public node.
- Checkpoints may instead come from a remote store (`--remote-store-url …`), in which case a
  fullnode for epoch/package resolution must be supplied separately with `--fullnode-url`.

Each output line looks like:

```json
{"digest":"…","checkpoint":282446861,"epoch":1147,"original_status":"success","original_failure":null,"recomputed_error":"ExecutionError { … kind: InvalidLinkage, source: Some(\"…conflicting resolutions…\") }"}
```

The `backtest complete` log line reports the run totals: `total_checked`, `total_divergences`,
`total_reconstruction_errors`, `total_executed`, `total_cancellation_excluded`, and the
skip/count categories below.

## How execution context is reconstructed

Per transaction the tool builds a read-only `BackingStore` from:

- **Objects**: the checkpoint's object set (input + output + unchanged-loaded-runtime objects).
  Shared objects are served at their per-transaction version (from the effects' input consensus
  objects), and dynamic-field child reads are tombstone-aware (within-checkpoint deletions are
  honored).
- **Packages**: looked up in that object set first, then a process-wide cache (in-memory + optional
  on-disk `--cache` dir), then a gRPC fetch from the fullnode (with retry + exponential backoff on
  rate-limit / transient errors).

Execution is **metered** with the transaction's own budget/price (gasless txns are metered at the
epoch RGP with the gasless compute cap, mirroring `sui-transaction-checks`). Because `BackingStore`
is synchronous, each transaction runs on a blocking worker (`spawn_blocking`).

Coin-reservation (address-balance) inputs — synthetic "fake coin" object refs that encode a
withdrawal in their digest — are rewritten back into `FundsWithdrawal` args by re-deriving the
balance type from the reservation id (no extra fetch). Reservations whose type can't be identified
are counted in `coin_reservation_skipped` and skipped.

## Caveats

- **Single-transaction replay can't model scheduling.** Transactions cancelled before execution by
  consensus-layer congestion / randomness control (`ExecutionCancelledDueToSharedObjectCongestion`,
  `ExecutionCancelledDueToRandomnessUnavailable`) never ran on chain, so they "succeed" here. These
  are detected from the on-chain effects and counted under `cancellation_excluded` rather than
  reported as divergences.
- **Public-node rate limiting.** `fullnode.mainnet.sui.io` returns HTTP 429 under concurrent load.
  The package fetcher retries with backoff so this does not corrupt results, but high `--concurrency`
  against a shared node mostly sleeps in backoff. For wide/parallel backtests use a dedicated or
  archival fullnode; against the public node keep `--concurrency` low (≈4). Public nodes also
  **prune old epochs** — only recent epochs are available there.
- Only `ProgrammableTransaction`s are considered; system/consensus transactions are out of scope.

## Analysis

The backtest itself is change-agnostic — it just emits divergence records. Analysis specific to a
particular change lives outside this crate.
