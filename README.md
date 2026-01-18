# DoubleZero: Fees Tooling (RFP-2) Grant Project

This project is a standalone MVP that fetches DoubleZero economic data from
public Solana RPC and public snapshot buckets, then writes epoch bundles to JSON.

## What it does
- Pulls ProgramConfig, Journal, and Distribution accounts for a DZ epoch.
- Writes JSON files that are easy to inspect manually.
- Builds an optional distributions index across all epochs.
- Generates a human-readable epoch report (optional).
- Enriches epoch bundles with inventory + telemetry aggregates (optional).

## Prereqs
- Rust toolchain installed.
- set `HELIUS_API_KEY` in .env file

## Quick usage

```bash
cargo run -- --dz-epoch <dz_epoch>
```

Outputs to `out/epoch_<dz_epoch>/`:
- `onchain/program_config.json`
- `onchain/journal.json`
- `onchain/distribution.json`
- `summary.json`

Full pipeline (on-chain + snapshot + enrich, parallelized):

```bash
cargo run -- pipeline --dz-epoch <dz_epoch>
```

Additional outputs:
- `snapshot/mn-epoch-<dz_epoch>-snapshot.json`
- `enriched_epoch.json`

Optional report:

```bash
cargo run -- report out/epoch_<dz_epoch>
```

Fetch snapshot from public S3 (standalone):

```bash
cargo run -- snapshot --epoch <dz_epoch>
```

Enrich on-chain data with snapshot telemetry:

```bash
cargo run -- enrich \
  --snapshot out/epoch_<dz_epoch>/snapshot \
  --onchain-dir out/epoch_<dz_epoch>/onchain
```

## How to read the output
- `onchain/distribution.json` is the canonical per-epoch totals and merkle roots.
- `onchain/program_config.json` contains fee and burn parameters used for that epoch.
- `onchain/journal.json` shows global balances and swap context.
- `enriched_epoch.json` merges on-chain totals with telemetry + inventory aggregates.

Publishable epochs must have all flags set:
- debt finalized
- rewards finalized
- swept

The report file summarizes these flags and totals in one place.
