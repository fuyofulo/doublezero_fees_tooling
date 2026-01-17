# Fees Tooling MVP (epoch-binder)

This project is a standalone MVP that fetches DoubleZero economic data from
public Solana RPC and writes epoch bundles to JSON.

## What it does
- Pulls ProgramConfig, Journal, and Distribution accounts for a DZ epoch.
- Writes JSON files that are easy to inspect manually.
- Builds an optional distributions index across all epochs.
- Generates a human-readable epoch report (optional).

## Quick usage
From `fees-tooling/`:

```bash
cargo run -- --latest-finalized --scan-back 120
```

Outputs to `out/epoch_<dz_epoch>/`:
- `program_config.json`
- `journal.json`
- `distribution_<dz_epoch>.json`

Optional report:

```bash
cargo run -- report out/epoch_<dz_epoch>
```

## How to read the output
- `distribution_*.json` is the canonical per-epoch totals and merkle roots.
- `program_config.json` contains fee and burn parameters used for that epoch.
- `journal.json` shows global balances and swap context.

Publishable epochs must have all flags set:
- debt finalized
- rewards finalized
- swept

The report file summarizes these flags and totals in one place.
