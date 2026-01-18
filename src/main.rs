use std::{
    env,
    fs,
    path::{Path, PathBuf},
    str::FromStr,
    thread,
};

mod dz;
mod epoch_report;
mod enrich;
mod snapshot_fetch;
mod zero_copy;

use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use bs58::encode as base58_encode;
use dz::{
    BurnRate, Distribution, DoubleZeroEpoch, Journal, ProgramConfig, ValidatorFee,
    REVENUE_DISTRIBUTION_PROGRAM_ID, SolanaValidatorFeeParameters,
};
use hex::encode as hex_encode;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use solana_program::pubkey::Pubkey;
use zero_copy::{checked_from_bytes_with_discriminator, discriminator};

#[derive(Debug)]
struct Args {
    dz_epoch: Option<u64>,
    index_distributions: bool,
    out_dir: Option<PathBuf>,
    snapshot_bucket: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
    result: Option<RpcResult<T>>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcResult<T> {
    context: RpcContext,
    value: Option<T>,
}

#[derive(Debug, Deserialize)]
struct RpcContext {
    slot: u64,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

#[derive(Debug, Deserialize)]
struct RpcResponseValue {
    result: Option<Value>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct AccountValue {
    lamports: u64,
    owner: String,
    executable: bool,
    #[serde(rename = "rentEpoch")]
    rent_epoch: u64,
    data: (String, String),
}

#[derive(Debug, Deserialize)]
struct ProgramAccount {
    pubkey: String,
    account: AccountValue,
}

#[derive(Debug, Deserialize)]
struct ProgramAccountsResult<T> {
    #[serde(default)]
    context: Option<RpcContext>,
    value: Vec<T>,
}

#[derive(Debug, Serialize)]
struct AccountMetadata {
    pubkey: String,
    owner: String,
    lamports: u64,
    executable: bool,
    rent_epoch: u64,
    data_len: usize,
}

#[derive(Debug, Serialize)]
struct AccountDump<T> {
    context_slot: u64,
    account: AccountMetadata,
    remaining_data_len: usize,
    parsed: T,
}

#[derive(Debug, Serialize)]
struct ProgramConfigJson {
    program_id: String,
    program_config_key: String,
    admin_key: String,
    debt_accountant_key: String,
    rewards_accountant_key: String,
    contributor_manager_key: String,
    sol_2z_swap_program_id: String,
    next_completed_dz_epoch: u64,
    is_paused: bool,
    is_migrated: bool,
    distribution_parameters: DistributionParametersJson,
    relay_parameters: RelayParametersJson,
    last_initialized_distribution_timestamp: u32,
    debt_write_off_feature_activation_epoch: u64,
}

#[derive(Debug, Serialize)]
struct DistributionParametersJson {
    calculation_grace_period_minutes: u16,
    initialization_grace_period_minutes: u16,
    minimum_epoch_duration_to_finalize_rewards: u8,
    community_burn_rate_parameters: CommunityBurnRateParametersJson,
    solana_validator_fee_parameters: SolanaValidatorFeeParametersJson,
}

#[derive(Debug, Serialize)]
struct RelayParametersJson {
    distribute_rewards_lamports: u32,
}

#[derive(Debug, Serialize)]
struct CommunityBurnRateParametersJson {
    limit_raw: u64,
    limit_pct: f64,
    dz_epochs_to_increasing: u32,
    dz_epochs_to_limit: u32,
    next_burn_rate_raw: Option<u64>,
    next_burn_rate_pct: Option<f64>,
    mode: String,
}

#[derive(Debug, Serialize)]
struct SolanaValidatorFeeParametersJson {
    base_block_rewards_pct_raw: u64,
    base_block_rewards_pct: f64,
    priority_block_rewards_pct_raw: u64,
    priority_block_rewards_pct: f64,
    inflation_rewards_pct_raw: u64,
    inflation_rewards_pct: f64,
    jito_tips_pct_raw: u64,
    jito_tips_pct: f64,
    fixed_sol_amount: u32,
}

#[derive(Debug, Serialize)]
struct DistributionJson {
    dz_epoch: u64,
    flags: String,
    is_debt_calculation_finalized: bool,
    is_rewards_calculation_finalized: bool,
    has_swept_2z_tokens: bool,
    community_burn_rate_raw: u64,
    community_burn_rate_pct: f64,
    solana_validator_fee_parameters: SolanaValidatorFeeParametersJson,
    solana_validator_debt_merkle_root: String,
    rewards_merkle_root: String,
    total_solana_validators: u32,
    solana_validator_payments_count: u32,
    total_solana_validator_debt: u64,
    collected_solana_validator_payments: u64,
    total_contributors: u32,
    distributed_rewards_count: u32,
    collected_prepaid_2z_payments: u64,
    collected_2z_converted_from_sol: u64,
    uncollectible_sol_debt: u64,
    processed_solana_validator_debt_start_index: u32,
    processed_solana_validator_debt_end_index: u32,
    processed_rewards_start_index: u32,
    processed_rewards_end_index: u32,
    distribute_rewards_relay_lamports: u32,
    calculation_allowed_timestamp: u32,
    distributed_2z_amount: u64,
    burned_2z_amount: u64,
    processed_solana_validator_debt_write_off_start_index: u32,
    processed_solana_validator_debt_write_off_end_index: u32,
    solana_validator_write_off_count: u32,
}

#[derive(Debug, Serialize)]
struct JournalJson {
    total_sol_balance: u64,
    total_2z_balance: u64,
    swap_2z_destination_balance: u64,
    swapped_sol_amount: u64,
    next_dz_epoch_to_sweep_tokens: u64,
    lifetime_swapped_2z_amount: u128,
}

#[derive(Debug, Serialize)]
struct DistributionIndexEntry {
    dz_epoch: u64,
    pubkey: String,
    remaining_data_len: usize,
    is_debt_calculation_finalized: bool,
    is_rewards_calculation_finalized: bool,
    has_swept_2z_tokens: bool,
    total_solana_validator_debt: u64,
    collected_solana_validator_payments: u64,
    distributed_2z_amount: u64,
    burned_2z_amount: u64,
    total_solana_validators: u32,
    total_contributors: u32,
}

#[derive(Debug, Serialize)]
struct DistributionsIndex {
    context_slot: u64,
    total_accounts: usize,
    entries: Vec<DistributionIndexEntry>,
}

#[derive(Debug)]
struct OnchainSummary {
    program_id: String,
    program_config_key: Pubkey,
    journal_key: Pubkey,
    distribution_key: Pubkey,
}

fn main() -> Result<()> {
    let mut raw_args: Vec<String> = env::args().skip(1).collect();
    if matches!(raw_args.first().map(String::as_str), Some("report")) {
        raw_args.remove(0);
        if raw_args.is_empty()
            || raw_args.iter().any(|arg| arg == "-h" || arg == "--help")
        {
            epoch_report::print_usage();
            return Ok(());
        }
        return epoch_report::run_from_args(raw_args.into_iter());
    }

    if matches!(raw_args.first().map(String::as_str), Some("snapshot")) {
        raw_args.remove(0);
        if raw_args.is_empty()
            || raw_args.iter().any(|arg| arg == "-h" || arg == "--help")
        {
            snapshot_fetch::print_usage();
            return Ok(());
        }
        return snapshot_fetch::run_from_args(raw_args.into_iter());
    }

    if matches!(raw_args.first().map(String::as_str), Some("enrich")) {
        raw_args.remove(0);
        if raw_args.is_empty()
            || raw_args.iter().any(|arg| arg == "-h" || arg == "--help")
        {
            enrich::print_usage();
            return Ok(());
        }
        return enrich::run_from_args(raw_args.into_iter());
    }

    if matches!(raw_args.first().map(String::as_str), Some("pipeline")) {
        raw_args.remove(0);
        if raw_args.is_empty()
            || raw_args.iter().any(|arg| arg == "-h" || arg == "--help")
        {
            print_usage();
            return Ok(());
        }
        return run_pipeline(raw_args.into_iter());
    }

    if raw_args.is_empty()
        || raw_args.iter().any(|arg| arg == "-h" || arg == "--help")
    {
        print_usage();
        return Ok(());
    }

    let args = parse_args(raw_args.into_iter())?;
    let dz_epoch = args
        .dz_epoch
        .context("missing dz epoch (use --dz-epoch <n> or pass it as a positional)")?;
    let rpc_url = default_rpc_url();
    let client = Client::new();
    let out_dir =
        args.out_dir.clone().unwrap_or_else(|| PathBuf::from(format!("out/epoch_{dz_epoch}")));
    let onchain_dir = out_dir.join("onchain");

    let onchain = write_onchain_bundle(
        &client,
        &rpc_url,
        dz_epoch,
        &onchain_dir,
        &out_dir,
        args.index_distributions,
    )?;

    let summary = json!({
        "program_id": onchain.program_id,
        "dz_epoch": dz_epoch,
        "program_config_key": onchain.program_config_key.to_string(),
        "journal_key": onchain.journal_key.to_string(),
        "distribution_key": onchain.distribution_key.to_string(),
        "onchain_dir": onchain_dir.display().to_string(),
        "epoch_selection": {
            "mode": "explicit"
        }
    });
    write_json(&out_dir, "summary.json", &summary)?;

    Ok(())
}

fn parse_args<I>(mut args: I) -> Result<Args>
where
    I: Iterator<Item = String>,
{
    let mut dz_epoch: Option<u64> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut index_distributions = false;
    let mut snapshot_bucket: Option<String> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dz-epoch" => {
                let value = args
                    .next()
                    .context("missing value for --dz-epoch")?;
                dz_epoch = Some(value.parse().context("invalid dz epoch")?);
            }
            "--out-dir" => {
                out_dir = Some(PathBuf::from(
                    args.next()
                        .context("missing value for --out-dir")?,
                ));
            }
            "--index" => {
                index_distributions = true;
            }
            "--snapshot-bucket" => {
                snapshot_bucket = Some(
                    args.next().context("missing value for --snapshot-bucket")?,
                );
            }
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            _ => {
                if dz_epoch.is_none() {
                    dz_epoch = Some(arg.parse().context("invalid dz epoch")?);
                } else {
                    return Err(anyhow!("unexpected argument: {arg}"));
                }
            }
        }
    }

    if dz_epoch.is_none() {
        return Err(anyhow!(
            "missing dz epoch (use --dz-epoch <n> or pass it as a positional)"
        ));
    }
    Ok(Args {
        dz_epoch,
        index_distributions,
        out_dir,
        snapshot_bucket,
    })
}

fn default_rpc_url() -> String {
    let _ = dotenvy::dotenv();
    let key = env::var("HELIUS_API_KEY")
        .expect("HELIUS_API_KEY is not set (set it in fees-tooling/.env)");
    format!("https://mainnet.helius-rpc.com/?api-key={key}")
}

fn run_pipeline<I>(args: I) -> Result<()>
where
    I: Iterator<Item = String>,
{
    let args = parse_args(args)?;
    let dz_epoch = args
        .dz_epoch
        .context("missing dz epoch (use --dz-epoch <n> or pass it as a positional)")?;

    let out_dir =
        args.out_dir.clone().unwrap_or_else(|| PathBuf::from(format!("out/epoch_{dz_epoch}")));
    let onchain_dir = out_dir.join("onchain");
    let snapshot_dir = out_dir.join("snapshot");
    let snapshot_bucket = args
        .snapshot_bucket
        .clone()
        .unwrap_or_else(|| snapshot_fetch::DEFAULT_SNAPSHOT_BUCKET.to_string());
    let index_distributions = args.index_distributions;

    let rpc_url = default_rpc_url();
    let onchain_dir_thread = onchain_dir.clone();
    let out_dir_thread = out_dir.clone();
    let onchain_handle = thread::spawn(move || -> Result<OnchainSummary> {
        let client = Client::new();
        write_onchain_bundle(
            &client,
            &rpc_url,
            dz_epoch,
            &onchain_dir_thread,
            &out_dir_thread,
            index_distributions,
        )
    });

    let snapshot_dir_thread = snapshot_dir.clone();
    let snapshot_bucket_thread = snapshot_bucket.clone();
    let snapshot_handle = thread::spawn(move || -> Result<PathBuf> {
        let client = Client::new();
        snapshot_fetch::fetch_snapshot(
            &client,
            dz_epoch,
            &snapshot_dir_thread,
            &snapshot_bucket_thread,
        )
    });

    let onchain = onchain_handle
        .join()
        .map_err(|_| anyhow!("onchain thread panicked"))??;
    let snapshot_path = snapshot_handle
        .join()
        .map_err(|_| anyhow!("snapshot thread panicked"))??;
    let snapshot_path_str = snapshot_path.display().to_string();

    let enriched_path = enrich::run(enrich::EnrichConfig {
        snapshot_path,
        onchain_dir: onchain_dir.clone(),
        out_dir: Some(out_dir.clone()),
    })?;

    let summary = json!({
        "program_id": onchain.program_id,
        "dz_epoch": dz_epoch,
        "program_config_key": onchain.program_config_key.to_string(),
        "journal_key": onchain.journal_key.to_string(),
        "distribution_key": onchain.distribution_key.to_string(),
        "onchain_dir": onchain_dir.display().to_string(),
        "snapshot_dir": snapshot_dir.display().to_string(),
        "snapshot_file": snapshot_path_str,
        "snapshot_bucket": snapshot_bucket,
        "enriched_file": enriched_path.display().to_string(),
        "epoch_selection": {
            "mode": "explicit"
        }
    });
    write_json(&out_dir, "summary.json", &summary)?;

    Ok(())
}

fn write_onchain_bundle(
    client: &Client,
    rpc_url: &str,
    dz_epoch: u64,
    onchain_dir: &Path,
    out_dir: &Path,
    index_distributions: bool,
) -> Result<OnchainSummary> {
    fs::create_dir_all(onchain_dir).context("create output directory")?;

    let program_id = REVENUE_DISTRIBUTION_PROGRAM_ID;
    let program_id_str = program_id.to_string();

    let program_config_key = ProgramConfig::find_address().0;
    let journal_key = Journal::find_address().0;
    let distribution_key =
        Distribution::find_address(DoubleZeroEpoch(dz_epoch)).0;

    let program_config_dump = fetch_and_parse::<ProgramConfigJson, ProgramConfig>(
        client,
        rpc_url,
        &program_config_key,
        program_config_discriminator(),
        |config| program_config_to_json(&program_id_str, &program_config_key, config),
    )?;
    write_json(onchain_dir, "program_config.json", &program_config_dump)?;

    let journal_dump = fetch_and_parse::<JournalJson, Journal>(
        client,
        rpc_url,
        &journal_key,
        journal_discriminator(),
        journal_to_json,
    )?;
    write_json(onchain_dir, "journal.json", &journal_dump)?;

    let distribution_dump = fetch_and_parse::<DistributionJson, Distribution>(
        client,
        rpc_url,
        &distribution_key,
        distribution_discriminator(),
        distribution_to_json,
    )?;
    write_json(onchain_dir, "distribution.json", &distribution_dump)?;

    if index_distributions {
        let index = fetch_distributions_index(client, rpc_url)?;
        write_json(out_dir, "distributions_index.json", &index)?;
    }

    Ok(OnchainSummary {
        program_id: program_id_str,
        program_config_key,
        journal_key,
        distribution_key,
    })
}

fn fetch_and_parse<TOut, TRaw>(
    client: &Client,
    rpc_url: &str,
    pubkey: &Pubkey,
    expected_discriminator: [u8; 8],
    transform: impl Fn(&TRaw) -> TOut,
) -> Result<AccountDump<TOut>>
where
    TRaw: bytemuck::Pod + Copy,
    TOut: Serialize,
{
    let account = fetch_account(client, rpc_url, pubkey)?;
    let (parsed, remaining_len) = parse_account::<TRaw>(&account.data, expected_discriminator)?;
    let parsed = transform(&parsed);

    Ok(AccountDump {
        context_slot: account.context_slot,
        account: AccountMetadata {
            pubkey: pubkey.to_string(),
            owner: account.owner.to_string(),
            lamports: account.lamports,
            executable: account.executable,
            rent_epoch: account.rent_epoch,
            data_len: account.data.len(),
        },
        remaining_data_len: remaining_len,
        parsed,
    })
}

fn fetch_account(client: &Client, rpc_url: &str, pubkey: &Pubkey) -> Result<RawAccount> {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getAccountInfo",
        "params": [
            pubkey.to_string(),
            { "encoding": "base64" }
        ]
    });

    let response: RpcResponse<AccountValue> = client
        .post(rpc_url)
        .json(&body)
        .send()
        .context("rpc request failed")?
        .error_for_status()
        .context("rpc error status")?
        .json()
        .context("rpc response parse failed")?;

    if let Some(err) = response.error {
        return Err(anyhow!("rpc error {}: {}", err.code, err.message));
    }

    let result = response.result.context("missing rpc result")?;
    let value = result.value.context("account not found")?;
    let (data_b64, encoding) = value.data;
    if encoding != "base64" {
        return Err(anyhow!("unexpected encoding: {encoding}"));
    }
    let data = BASE64_STANDARD
        .decode(data_b64.as_bytes())
        .context("base64 decode failed")?;

    let owner = Pubkey::from_str(&value.owner)
        .context("invalid owner pubkey")?;

    Ok(RawAccount {
        context_slot: result.context.slot,
        owner,
        lamports: value.lamports,
        executable: value.executable,
        rent_epoch: value.rent_epoch,
        data,
    })
}

fn fetch_distributions_index(client: &Client, rpc_url: &str) -> Result<DistributionsIndex> {
    let memcmp_bytes = base58_encode(distribution_discriminator()).into_string();

    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getProgramAccounts",
        "params": [
            REVENUE_DISTRIBUTION_PROGRAM_ID.to_string(),
            {
                "encoding": "base64",
                "filters": [
                    { "memcmp": { "offset": 0, "bytes": memcmp_bytes } }
                ]
            }
        ]
    });

    let response: RpcResponseValue = client
        .post(rpc_url)
        .json(&body)
        .send()
        .context("rpc request failed")?
        .error_for_status()
        .context("rpc error status")?
        .json()
        .context("rpc response parse failed")?;

    if let Some(err) = response.error {
        return Err(anyhow!("rpc error {}: {}", err.code, err.message));
    }

    let result = response.result.context("missing rpc result")?;

    let (context_slot, accounts) = if result.is_array() {
        (0u64, serde_json::from_value::<Vec<ProgramAccount>>(result)?)
    } else if result.is_object() {
        let parsed = serde_json::from_value::<ProgramAccountsResult<ProgramAccount>>(result)?;
        (
            parsed.context.map(|ctx| ctx.slot).unwrap_or_default(),
            parsed.value,
        )
    } else {
        return Err(anyhow!("unexpected rpc result shape"));
    };

    let mut entries = Vec::with_capacity(accounts.len());

    for account in accounts {
        let (data_b64, encoding) = account.account.data;
        if encoding != "base64" {
            continue;
        }
        let data = BASE64_STANDARD
            .decode(data_b64.as_bytes())
            .context("base64 decode failed")?;
        let (distribution, remaining) =
            match parse_account::<Distribution>(&data, distribution_discriminator()) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };

        entries.push(DistributionIndexEntry {
            dz_epoch: distribution.dz_epoch.value(),
            pubkey: account.pubkey,
            remaining_data_len: remaining,
            is_debt_calculation_finalized: distribution.is_debt_calculation_finalized(),
            is_rewards_calculation_finalized: distribution.is_rewards_calculation_finalized(),
            has_swept_2z_tokens: distribution.has_swept_2z_tokens(),
            total_solana_validator_debt: distribution.total_solana_validator_debt,
            collected_solana_validator_payments: distribution.collected_solana_validator_payments,
            distributed_2z_amount: distribution.distributed_2z_amount,
            burned_2z_amount: distribution.burned_2z_amount,
            total_solana_validators: distribution.total_solana_validators,
            total_contributors: distribution.total_contributors,
        });
    }

    entries.sort_by_key(|entry| entry.dz_epoch);

    Ok(DistributionsIndex {
        context_slot,
        total_accounts: entries.len(),
        entries,
    })
}

fn parse_account<T>(data: &[u8], expected_discriminator: [u8; 8]) -> Result<(T, usize)>
where
    T: bytemuck::Pod + Copy,
{
    let account = checked_from_bytes_with_discriminator::<T>(data, expected_discriminator)
        .context("account discriminator mismatch")?;
    let remaining = data
        .len()
        .saturating_sub(8 + std::mem::size_of::<T>());
    Ok((*account, remaining))
}

fn program_config_discriminator() -> [u8; 8] {
    discriminator(b"dz::account::program_config")
}

fn journal_discriminator() -> [u8; 8] {
    discriminator(b"dz::account::journal")
}

fn distribution_discriminator() -> [u8; 8] {
    discriminator(b"dz::account::distribution")
}

fn program_config_to_json(
    program_id: &str,
    program_config_key: &Pubkey,
    config: &ProgramConfig,
) -> ProgramConfigJson {
    let distribution_parameters = &config.distribution_parameters;
    let burn_rate_params = &distribution_parameters.community_burn_rate_parameters;
    let fee_params = &distribution_parameters.solana_validator_fee_parameters;

    ProgramConfigJson {
        program_id: program_id.to_string(),
        program_config_key: program_config_key.to_string(),
        admin_key: config.admin_key.to_string(),
        debt_accountant_key: config.debt_accountant_key.to_string(),
        rewards_accountant_key: config.rewards_accountant_key.to_string(),
        contributor_manager_key: config.contributor_manager_key.to_string(),
        sol_2z_swap_program_id: config.sol_2z_swap_program_id.to_string(),
        next_completed_dz_epoch: config.next_completed_dz_epoch.value(),
        is_paused: config.is_paused(),
        is_migrated: config.is_migrated(),
        distribution_parameters: DistributionParametersJson {
            calculation_grace_period_minutes: distribution_parameters
                .calculation_grace_period_minutes,
            initialization_grace_period_minutes: distribution_parameters
                .initialization_grace_period_minutes,
            minimum_epoch_duration_to_finalize_rewards: distribution_parameters
                .minimum_epoch_duration_to_finalize_rewards,
            community_burn_rate_parameters: CommunityBurnRateParametersJson {
                limit_raw: burn_rate_to_raw(burn_rate_params.limit),
                limit_pct: burn_rate_to_pct(burn_rate_params.limit),
                dz_epochs_to_increasing: burn_rate_params.dz_epochs_to_increasing,
                dz_epochs_to_limit: burn_rate_params.dz_epochs_to_limit,
                next_burn_rate_raw: burn_rate_params.next_burn_rate_raw(),
                next_burn_rate_pct: burn_rate_params
                    .next_burn_rate_raw()
                    .map(|raw| raw as f64 / BurnRate::MAX as f64),
                mode: burn_rate_params.mode().to_string(),
            },
            solana_validator_fee_parameters: fee_params_to_json(fee_params),
        },
        relay_parameters: RelayParametersJson {
            distribute_rewards_lamports: config.relay_parameters.distribute_rewards_lamports,
        },
        last_initialized_distribution_timestamp: config.last_initialized_distribution_timestamp,
        debt_write_off_feature_activation_epoch: config
            .debt_write_off_feature_activation_epoch
            .value(),
    }
}

fn distribution_to_json(distribution: &Distribution) -> DistributionJson {
    let fee_params = distribution.solana_validator_fee_parameters;
    DistributionJson {
        dz_epoch: distribution.dz_epoch.value(),
        flags: format!("{:?}", distribution.flags),
        is_debt_calculation_finalized: distribution.is_debt_calculation_finalized(),
        is_rewards_calculation_finalized: distribution.is_rewards_calculation_finalized(),
        has_swept_2z_tokens: distribution.has_swept_2z_tokens(),
        community_burn_rate_raw: burn_rate_to_raw(distribution.community_burn_rate),
        community_burn_rate_pct: burn_rate_to_pct(distribution.community_burn_rate),
        solana_validator_fee_parameters: fee_params_to_json(&fee_params),
        solana_validator_debt_merkle_root: hash_to_hex(&distribution.solana_validator_debt_merkle_root),
        rewards_merkle_root: hash_to_hex(&distribution.rewards_merkle_root),
        total_solana_validators: distribution.total_solana_validators,
        solana_validator_payments_count: distribution.solana_validator_payments_count,
        total_solana_validator_debt: distribution.total_solana_validator_debt,
        collected_solana_validator_payments: distribution.collected_solana_validator_payments,
        total_contributors: distribution.total_contributors,
        distributed_rewards_count: distribution.distributed_rewards_count,
        collected_prepaid_2z_payments: distribution.collected_prepaid_2z_payments,
        collected_2z_converted_from_sol: distribution.collected_2z_converted_from_sol,
        uncollectible_sol_debt: distribution.uncollectible_sol_debt,
        processed_solana_validator_debt_start_index: distribution
            .processed_solana_validator_debt_start_index,
        processed_solana_validator_debt_end_index: distribution
            .processed_solana_validator_debt_end_index,
        processed_rewards_start_index: distribution.processed_rewards_start_index,
        processed_rewards_end_index: distribution.processed_rewards_end_index,
        distribute_rewards_relay_lamports: distribution.distribute_rewards_relay_lamports,
        calculation_allowed_timestamp: distribution.calculation_allowed_timestamp,
        distributed_2z_amount: distribution.distributed_2z_amount,
        burned_2z_amount: distribution.burned_2z_amount,
        processed_solana_validator_debt_write_off_start_index: distribution
            .processed_solana_validator_debt_write_off_start_index,
        processed_solana_validator_debt_write_off_end_index: distribution
            .processed_solana_validator_debt_write_off_end_index,
        solana_validator_write_off_count: distribution.solana_validator_write_off_count,
    }
}

fn journal_to_json(journal: &Journal) -> JournalJson {
    JournalJson {
        total_sol_balance: journal.total_sol_balance,
        total_2z_balance: journal.total_2z_balance,
        swap_2z_destination_balance: journal.swap_2z_destination_balance,
        swapped_sol_amount: journal.swapped_sol_amount,
        next_dz_epoch_to_sweep_tokens: journal.next_dz_epoch_to_sweep_tokens.value(),
        lifetime_swapped_2z_amount: journal.lifetime_swapped_2z_amount(),
    }
}

fn fee_params_to_json(params: &SolanaValidatorFeeParameters) -> SolanaValidatorFeeParametersJson {
    SolanaValidatorFeeParametersJson {
        base_block_rewards_pct_raw: validator_fee_raw(params.base_block_rewards_pct),
        base_block_rewards_pct: validator_fee_pct(params.base_block_rewards_pct),
        priority_block_rewards_pct_raw: validator_fee_raw(params.priority_block_rewards_pct),
        priority_block_rewards_pct: validator_fee_pct(params.priority_block_rewards_pct),
        inflation_rewards_pct_raw: validator_fee_raw(params.inflation_rewards_pct),
        inflation_rewards_pct: validator_fee_pct(params.inflation_rewards_pct),
        jito_tips_pct_raw: validator_fee_raw(params.jito_tips_pct),
        jito_tips_pct: validator_fee_pct(params.jito_tips_pct),
        fixed_sol_amount: params.fixed_sol_amount,
    }
}

fn validator_fee_raw(fee: ValidatorFee) -> u64 {
    fee.0 as u64
}

fn validator_fee_pct(fee: ValidatorFee) -> f64 {
    validator_fee_raw(fee) as f64 / 10_000.0
}

fn burn_rate_to_raw(rate: BurnRate) -> u64 {
    rate.0 as u64
}

fn burn_rate_to_pct(rate: BurnRate) -> f64 {
    burn_rate_to_raw(rate) as f64 / 1_000_000_000.0
}

fn hash_to_hex(hash: &[u8; 32]) -> String {
    hex_encode(hash)
}

fn write_json<T: Serialize>(out_dir: &Path, name: &str, value: &T) -> Result<()> {
    let path = out_dir.join(name);
    let data = serde_json::to_vec_pretty(value).context("serialize json")?;
    fs::write(&path, data).with_context(|| format!("write {name}"))?;
    Ok(())
}

struct RawAccount {
    context_slot: u64,
    owner: Pubkey,
    lamports: u64,
    executable: bool,
    rent_epoch: u64,
    data: Vec<u8>,
}

fn print_usage() {
    eprintln!(
        r#"Usage:
        fees-tooling --dz-epoch <n> [--out-dir <path>] [--index]
        fees-tooling pipeline --dz-epoch <n> [--out-dir <path>] [--index] [--snapshot-bucket <url>]
        fees-tooling report <epoch_dir> [--fees-csv <path>] [--out-dir <path>]
        fees-tooling snapshot --epoch <n> [--out-dir <path>] [--bucket <url>]
        fees-tooling enrich --snapshot <path> --onchain-dir <path> [--out-dir <path>]

        Defaults:
        --out-dir  out/epoch_<dz_epoch>
        
        Notes:
        report expects <epoch_dir> to contain onchain/distribution.json
        
        Flags:
        --index            Write distributions_index.json (summary of all distribution accounts)
        --snapshot-bucket  Override snapshot S3 bucket (pipeline only)
        report             Generate epoch_report.json/.md from an epoch output folder
        snapshot           Download snapshot JSON from the public S3 bucket
        enrich             Merge snapshot + on-chain outputs into enriched_epoch.json
        --fees-csv <path>  Optional fees CSV for reconciliation (report mode)
        "#
    );
}
