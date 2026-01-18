# Fees Tooling MVP (epoch-binder)

This project is a standalone MVP that fetches DoubleZero economic data from
public Solana RPC and public snapshot buckets, then writes epoch bundles to JSON.

## What it does
- Pulls ProgramConfig, Journal, and Distribution accounts for a DZ epoch.
- Writes JSON files that are easy to inspect manually.
- Builds an optional distributions index across all epochs.
- Generates a human-readable epoch report (optional).
- Enriches epoch bundles with inventory + telemetry aggregates (optional).

## Quick usage

```bash
cargo run -- --latest-finalized --scan-back 120
```

Outputs to `out/epoch_<dz_epoch>/`:
- `onchain/program_config.json`
- `onchain/journal.json`
- `onchain/distribution.json`
- `summary.json`

Full pipeline (on-chain + snapshot + enrich, parallelized):

```bash
cargo run -- pipeline --latest-finalized --scan-back 120
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
