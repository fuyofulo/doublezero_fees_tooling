use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug)]
struct Args {
    input_dir: PathBuf,
    fees_csv: Option<PathBuf>,
    out_dir: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct DistributionFile {
    context_slot: u64,
    account: AccountMetadata,
    remaining_data_len: usize,
    parsed: DistributionParsed,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct AccountMetadata {
    pubkey: String,
    owner: String,
    lamports: u64,
    executable: bool,
    rent_epoch: u64,
    data_len: usize,
}

#[derive(Debug, Deserialize)]
struct DistributionParsed {
    dz_epoch: u64,
    is_debt_calculation_finalized: bool,
    is_rewards_calculation_finalized: bool,
    has_swept_2z_tokens: bool,
    total_solana_validator_debt: u64,
    collected_solana_validator_payments: u64,
    collected_prepaid_2z_payments: u64,
    collected_2z_converted_from_sol: u64,
    uncollectible_sol_debt: u64,
    total_solana_validators: u32,
    total_contributors: u32,
    distributed_2z_amount: u64,
    burned_2z_amount: u64,
    rewards_merkle_root: String,
    solana_validator_debt_merkle_root: String,
}

#[derive(Debug, Serialize)]
struct FeesSummary {
    rows: usize,
    total_dz_fee_lamports: u64,
    total_dz_fee_sol: f64,
    source_file: String,
}

#[derive(Debug, Serialize)]
struct EpochReport {
    dz_epoch: u64,
    publishable: bool,
    flags: FlagsSummary,
    totals: TotalsSummary,
    merkle_roots: MerkleRootsSummary,
    fees_summary: Option<FeesSummary>,
    reconciliation: Option<ReconciliationSummary>,
}

#[derive(Debug, Serialize)]
struct FlagsSummary {
    debt_finalized: bool,
    rewards_finalized: bool,
    has_swept_2z_tokens: bool,
}

#[derive(Debug, Serialize)]
struct TotalsSummary {
    total_solana_validator_debt_lamports: u64,
    total_solana_validator_debt_sol: f64,
    collected_solana_validator_payments: u64,
    collected_prepaid_2z_payments: u64,
    collected_2z_converted_from_sol: u64,
    uncollectible_sol_debt: u64,
    distributed_2z_amount: u64,
    burned_2z_amount: u64,
    total_solana_validators: u32,
    total_contributors: u32,
}

#[derive(Debug, Serialize)]
struct MerkleRootsSummary {
    rewards_merkle_root: String,
    solana_validator_debt_merkle_root: String,
}

#[derive(Debug, Serialize)]
struct ReconciliationSummary {
    fees_total_lamports: u64,
    onchain_debt_lamports: u64,
    difference_lamports: i128,
    difference_sol: f64,
}

pub fn run_from_args<I>(args: I) -> Result<()>
where
    I: Iterator<Item = String>,
{
    let args = parse_args(args)?;
    let input_dir = args.input_dir;
    let out_dir = args.out_dir.clone().unwrap_or_else(|| input_dir.clone());

    let distribution_path = find_distribution_file(&input_dir)?;
    let distribution = read_distribution(&distribution_path)?;

    let publishable = distribution.parsed.is_debt_calculation_finalized
        && distribution.parsed.is_rewards_calculation_finalized
        && distribution.parsed.has_swept_2z_tokens;

    let fees_summary = if let Some(path) = args.fees_csv.as_ref() {
        Some(summarize_fees_csv(path)?)
    } else {
        None
    };

    let reconciliation = fees_summary.as_ref().map(|fees| {
        let debt = distribution.parsed.total_solana_validator_debt;
        let diff = debt as i128 - fees.total_dz_fee_lamports as i128;
        ReconciliationSummary {
            fees_total_lamports: fees.total_dz_fee_lamports,
            onchain_debt_lamports: debt,
            difference_lamports: diff,
            difference_sol: diff as f64 / 1_000_000_000.0,
        }
    });

    let report = EpochReport {
        dz_epoch: distribution.parsed.dz_epoch,
        publishable,
        flags: FlagsSummary {
            debt_finalized: distribution.parsed.is_debt_calculation_finalized,
            rewards_finalized: distribution.parsed.is_rewards_calculation_finalized,
            has_swept_2z_tokens: distribution.parsed.has_swept_2z_tokens,
        },
        totals: TotalsSummary {
            total_solana_validator_debt_lamports: distribution.parsed.total_solana_validator_debt,
            total_solana_validator_debt_sol: distribution.parsed.total_solana_validator_debt as f64
                / 1_000_000_000.0,
            collected_solana_validator_payments: distribution.parsed.collected_solana_validator_payments,
            collected_prepaid_2z_payments: distribution.parsed.collected_prepaid_2z_payments,
            collected_2z_converted_from_sol: distribution.parsed.collected_2z_converted_from_sol,
            uncollectible_sol_debt: distribution.parsed.uncollectible_sol_debt,
            distributed_2z_amount: distribution.parsed.distributed_2z_amount,
            burned_2z_amount: distribution.parsed.burned_2z_amount,
            total_solana_validators: distribution.parsed.total_solana_validators,
            total_contributors: distribution.parsed.total_contributors,
        },
        merkle_roots: MerkleRootsSummary {
            rewards_merkle_root: distribution.parsed.rewards_merkle_root.clone(),
            solana_validator_debt_merkle_root: distribution
                .parsed
                .solana_validator_debt_merkle_root
                .clone(),
        },
        fees_summary,
        reconciliation,
    };

    fs::create_dir_all(&out_dir).context("create output directory")?;
    write_json(&out_dir, "epoch_report.json", &report)?;
    write_markdown(&out_dir, "epoch_report.md", &report)?;

    Ok(())
}

fn parse_args<I>(mut args: I) -> Result<Args>
where
    I: Iterator<Item = String>,
{
    let mut input_dir: Option<PathBuf> = None;
    let mut fees_csv: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input-dir" => {
                input_dir = Some(PathBuf::from(
                    args.next().context("missing value for --input-dir")?,
                ));
            }
            "--fees-csv" => {
                fees_csv = Some(PathBuf::from(
                    args.next().context("missing value for --fees-csv")?,
                ));
            }
            "--out-dir" => {
                out_dir = Some(PathBuf::from(
                    args.next().context("missing value for --out-dir")?,
                ));
            }
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            _ => {
                if input_dir.is_none() {
                    input_dir = Some(PathBuf::from(arg));
                } else {
                    return Err(anyhow!("unexpected argument: {arg}"));
                }
            }
        }
    }

    let input_dir = input_dir.context("missing input directory")?;

    Ok(Args {
        input_dir,
        fees_csv,
        out_dir,
    })
}

fn find_distribution_file(input_dir: &Path) -> Result<PathBuf> {
    let mut matches = Vec::new();
    for entry in fs::read_dir(input_dir).context("read input dir")? {
        let entry = entry.context("read entry")?;
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with("distribution_") && name.ends_with(".json") {
                matches.push(path);
            }
        }
    }

    match matches.len() {
        0 => Err(anyhow!("no distribution_*.json found in {input_dir:?}")),
        1 => Ok(matches.remove(0)),
        _ => Err(anyhow!(
            "multiple distribution_*.json files found; specify a single input dir"
        )),
    }
}

fn read_distribution(path: &Path) -> Result<DistributionFile> {
    let data = fs::read(path).with_context(|| format!("read {path:?}"))?;
    serde_json::from_slice(&data).context("parse distribution json")
}

fn summarize_fees_csv(path: &Path) -> Result<FeesSummary> {
    let content = fs::read_to_string(path).with_context(|| format!("read fees csv {path:?}"))?;
    let mut lines = content.lines();
    let _header = lines.next().context("fees csv missing header")?;

    let mut rows = 0usize;
    let mut total: u64 = 0;

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 3 {
            return Err(anyhow!("invalid fees csv line: {line}"));
        }
        let lamports_str = parts[2].trim().trim_matches('"');
        let value: u64 = lamports_str.parse().context("parse dz_fee_lamports")?;
        total = total.saturating_add(value);
        rows += 1;
    }

    Ok(FeesSummary {
        rows,
        total_dz_fee_lamports: total,
        total_dz_fee_sol: total as f64 / 1_000_000_000.0,
        source_file: path.to_string_lossy().to_string(),
    })
}

fn write_json<T: Serialize>(out_dir: &Path, name: &str, value: &T) -> Result<()> {
    let path = out_dir.join(name);
    let data = serde_json::to_vec_pretty(value).context("serialize json")?;
    fs::write(&path, data).with_context(|| format!("write {name}"))?;
    Ok(())
}

fn write_markdown(out_dir: &Path, name: &str, report: &EpochReport) -> Result<()> {
    let path = out_dir.join(name);
    let fees_line = if let Some(fees) = report.fees_summary.as_ref() {
        format!(
            "- Fees CSV total: {} lamports ({:.9} SOL) from `{}`",
            fees.total_dz_fee_lamports, fees.total_dz_fee_sol, fees.source_file
        )
    } else {
        "- Fees CSV total: (not provided)".to_string()
    };

    let reconciliation_line = if let Some(rec) = report.reconciliation.as_ref() {
        format!(
            "- Debt minus fees: {} lamports ({:.9} SOL)",
            rec.difference_lamports, rec.difference_sol
        )
    } else {
        "- Debt minus fees: (not computed)".to_string()
    };

    let md = format!(
        "# DZ Epoch {epoch} Report\n\n\
## Flags\n\
- Debt finalized: {debt}\n\
- Rewards finalized: {rewards}\n\
- 2Z swept: {swept}\n\
- Publishable: {publishable}\n\n\
## Totals\n\
- Total SOL debt: {debt_lamports} lamports ({debt_sol:.9} SOL)\n\
- Distributed 2Z: {distributed}\n\
- Burned 2Z: {burned}\n\
- Total validators: {validators}\n\
- Total contributors: {contributors}\n\n\
## Merkle Roots\n\
- Rewards: `{rewards_root}`\n\
- Debt: `{debt_root}`\n\n\
## Reconciliation\n\
{fees_line}\n\
{reconciliation_line}\n",
        epoch = report.dz_epoch,
        debt = report.flags.debt_finalized,
        rewards = report.flags.rewards_finalized,
        swept = report.flags.has_swept_2z_tokens,
        publishable = report.publishable,
        debt_lamports = report.totals.total_solana_validator_debt_lamports,
        debt_sol = report.totals.total_solana_validator_debt_sol,
        distributed = report.totals.distributed_2z_amount,
        burned = report.totals.burned_2z_amount,
        validators = report.totals.total_solana_validators,
        contributors = report.totals.total_contributors,
        rewards_root = report.merkle_roots.rewards_merkle_root,
        debt_root = report.merkle_roots.solana_validator_debt_merkle_root,
        fees_line = fees_line,
        reconciliation_line = reconciliation_line,
    );

    fs::write(&path, md).with_context(|| format!("write {name}"))?;
    Ok(())
}

pub fn print_usage() {
    eprintln!(
        "Usage: fees-tooling report <input_dir> [--fees-csv <path>] [--out-dir <path>]\\n\\n\
Defaults:\\n\
  --out-dir  <input_dir>\\n"
    );
}
